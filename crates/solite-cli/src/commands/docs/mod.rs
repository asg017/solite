//! Documentation generation from markdown with embedded SQL.
//!
//! This module provides functionality to process markdown files containing
//! SQL code blocks, execute the SQL, and inline the results back into
//! the documentation.
//!
//! # Features
//!
//! - Execute SQL code blocks in markdown files
//! - Inline query results as comments or tables
//! - Track extension functions and flag undocumented ones
//! - Support for GFM (GitHub Flavored Markdown)
//!
//! # Example
//!
//! Input markdown:
//!
//! ````markdown
//! # My Extension
//!
//! ```sql
//! SELECT my_function(1, 2);
//! ```
//! ````
//!
//! Output markdown (with `--extension my_ext.so`):
//!
//! ````markdown
//! # My Extension
//!
//! ```sql
//! SELECT my_function(1, 2);
//! -- 3
//! ```
//! ````

mod sql;
mod table;
mod value;

use std::fs::OpenOptions;
use std::io::{stdout, Write};

use markdown::mdast::{Code, Heading, Node};
use solite_core::Runtime;

use crate::cli::{DocsCommand, DocsInlineArgs, DocsNamespace};
use crate::commands::test::snap::copy;
use crate::errors::{report_error, report_error_string};

use sql::{
    BASE_FUNCTIONS_CREATE, BASE_MODULES_CREATE, LOADED_FUNCTIONS_CREATE, LOADED_MODULES_CREATE,
};
use table::render_table;
use value::display_value;

/// Errors that can occur during documentation generation.
#[derive(Debug)]
pub enum DocsError {
    /// Failed to attach database.
    DatabaseAttach(String),
    /// Failed to execute SQL.
    SqlError(String),
    /// Failed to load extension.
    ExtensionLoad(String),
    /// Failed to read input file.
    FileRead(String),
    /// Failed to parse markdown.
    MarkdownParse(String),
    /// Failed to write output file.
    FileWrite(String),
    /// Undocumented functions and/or virtual-table modules found.
    Undocumented {
        functions: Vec<String>,
        modules: Vec<String>,
    },
    /// Error already reported to stderr (e.g. a codespan report); the
    /// caller should not print it again.
    AlreadyReported,
}

impl std::fmt::Display for DocsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocsError::DatabaseAttach(msg) => write!(f, "Failed to attach database: {}", msg),
            DocsError::SqlError(msg) => write!(f, "SQL error: {}", msg),
            DocsError::ExtensionLoad(msg) => write!(f, "Failed to load extension: {}", msg),
            DocsError::FileRead(msg) => write!(f, "Failed to read file: {}", msg),
            DocsError::MarkdownParse(msg) => write!(f, "Failed to parse markdown: {}", msg),
            DocsError::FileWrite(msg) => write!(f, "Failed to write file: {}", msg),
            DocsError::Undocumented { functions, modules } => {
                let mut first = true;
                for (label, names) in [("functions", functions), ("modules", modules)] {
                    if names.is_empty() {
                        continue;
                    }
                    if !first {
                        writeln!(f)?;
                    }
                    write!(f, "The following {} are not documented:", label)?;
                    for name in names {
                        write!(f, "\n  - {}", name)?;
                    }
                    first = false;
                }
                Ok(())
            }
            DocsError::AlreadyReported => write!(f, "SQL error in code block"),
        }
    }
}

impl std::error::Error for DocsError {}

/// Process inline documentation.
fn inline(args: DocsInlineArgs) -> Result<(), DocsError> {
    let rt = Runtime::new(None).map_err(|e| DocsError::SqlError(e.to_string()))?;

    // Attach in-memory database for tracking
    if let Err(e) = rt
        .connection
        .execute("ATTACH DATABASE ':memory:' AS solite_docs")
    {
        return Err(DocsError::DatabaseAttach(e.message));
    }

    // Load extension if provided
    if let Some(ref ext) = args.extension {
        setup_extension_tracking(&rt, ext)?;
    }

    // Read and parse markdown
    let docs_in = std::fs::read_to_string(&args.input)
        .map_err(|e| DocsError::FileRead(format!("{}: {}", args.input.display(), e)))?;

    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;

    let ast = markdown::to_mdast(&docs_in, &options)
        .map_err(|e| DocsError::MarkdownParse(e.to_string()))?;

    // Walk the AST collecting span edits (code block results, heading
    // anchors) against the original source. Splicing by byte span instead
    // of re-serializing the whole AST preserves every construct the
    // serializer doesn't understand (GFM tables, strikethrough,
    // frontmatter, footnotes) and avoids reformatting churn.
    let mut edits: Vec<Edit> = Vec::new();
    let mut documented_funcs: Vec<String> = Vec::new();
    collect_edits(
        &rt,
        &ast,
        &docs_in,
        &args,
        &mut edits,
        &mut documented_funcs,
        false,
    )?;

    // Get loaded functions/modules from extension; the tracking tables
    // only exist when an extension was loaded
    let (loaded_funcs, loaded_modules) = if args.extension.is_some() {
        (
            query_names(
                &rt,
                "SELECT name FROM solite_docs.solite_docs_loaded_functions",
            )?,
            query_names(
                &rt,
                "SELECT name FROM solite_docs.solite_docs_loaded_modules",
            )?,
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // Find undocumented functions and modules; module headings use the
    // same inline-code convention (### `vtab_foo`) as function headings
    let mut undocumented_funcs: Vec<String> = loaded_funcs
        .iter()
        .filter(|f| !documented_funcs.contains(f))
        .cloned()
        .collect();
    let mut undocumented_modules: Vec<String> = loaded_modules
        .iter()
        .filter(|m| !documented_funcs.contains(m))
        .cloned()
        .collect();

    // Apply edits back-to-front so earlier offsets stay valid
    let mut out_md = docs_in;
    for edit in edits.iter().rev() {
        out_md.replace_range(edit.start..edit.end, &edit.replacement);
    }

    // Write output
    write_output(&args, &out_md)?;

    // Report undocumented functions/modules; printing is left to the
    // Display impl so the list shows up exactly once
    if !undocumented_funcs.is_empty() || !undocumented_modules.is_empty() {
        undocumented_funcs.sort();
        undocumented_modules.sort();
        return Err(DocsError::Undocumented {
            functions: undocumented_funcs,
            modules: undocumented_modules,
        });
    }

    Ok(())
}

/// Set up extension tracking tables and load extension.
fn setup_extension_tracking(rt: &Runtime, ext: &str) -> Result<(), DocsError> {
    if let Err(e) = rt.connection.execute(BASE_FUNCTIONS_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create base functions table: {}",
            e.message
        )));
    }

    if let Err(e) = rt.connection.execute(BASE_MODULES_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create base modules table: {}",
            e.message
        )));
    }

    if let Err(e) = rt.connection.load_extension(ext, &None) {
        return Err(DocsError::ExtensionLoad(format!("{}: {}", ext, e)));
    }

    if let Err(e) = rt.connection.execute(LOADED_FUNCTIONS_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create loaded functions table: {}",
            e.message
        )));
    }

    if let Err(e) = rt.connection.execute(LOADED_MODULES_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create loaded modules table: {}",
            e.message
        )));
    }

    Ok(())
}

/// A byte-span replacement against the original markdown source.
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Walk the AST in document order, executing ```sql code blocks and
/// collecting span edits for their results and for heading anchors.
/// Recurses into container nodes so nested code blocks (in lists, ...) are
/// processed too. Code blocks inside blockquotes are passed through
/// untouched: every line carries a `> ` prefix in the source, which breaks
/// both closing-fence detection and the whitespace-only re-indentation,
/// so editing them would corrupt the document.
fn collect_edits(
    rt: &Runtime,
    node: &Node,
    src: &str,
    args: &DocsInlineArgs,
    edits: &mut Vec<Edit>,
    documented: &mut Vec<String>,
    in_blockquote: bool,
) -> Result<(), DocsError> {
    match node {
        // Only ```sql blocks are executed — other languages (and untagged
        // blocks) are left untouched, as are blocks inside blockquotes
        Node::Code(code)
            if !in_blockquote
                && matches!(code.lang.as_deref(), Some("sql") | Some("sqlite")) =>
        {
            let new_value = process_code_block(rt, &code.value, args)?;
            if let Some(edit) = code_block_edit(code, src, &new_value) {
                edits.push(edit);
            }
        }
        Node::Heading(heading) if heading.depth == 3 || heading.depth == 4 => {
            if let Some(function) = heading_function_name(heading) {
                if let Some(edit) = heading_anchor_edit(heading, src, &function) {
                    edits.push(edit);
                }
                documented.push(function);
            }
        }
        _ => {
            let in_blockquote = in_blockquote || matches!(node, Node::Blockquote(_));
            if let Some(children) = node.children() {
                for child in children {
                    collect_edits(rt, child, src, args, edits, documented, in_blockquote)?;
                }
            }
        }
    }
    Ok(())
}

/// Build the edit replacing a code block's contents (the bytes between the
/// fence lines, which are preserved byte-for-byte) with the new SQL+results.
fn code_block_edit(code: &Code, src: &str, new_value: &str) -> Option<Edit> {
    let pos = code.position.as_ref()?;
    let (start, end) = (pos.start.offset, pos.end.offset);
    let block = src.get(start..end)?;

    // Interior spans from just after the opening fence line to the start
    // of the closing fence line (or to the end when the fence is unclosed)
    let first_newline = block.find('\n')?;
    let interior_start = first_newline + 1;
    let interior_end = match block.rfind('\n') {
        // `idx + 1 >= interior_start` (not `idx >= interior_start`): in an
        // empty block the only newline is the opening fence's own, so the
        // line after it is the closing fence and must still be recognized
        // (otherwise the fence would be swallowed into the replacement)
        Some(idx) if idx + 1 >= interior_start => {
            let last_line = block[idx + 1..].trim_start();
            if last_line.starts_with("```") || last_line.starts_with("~~~") {
                // Clamp so an empty interior yields an empty span instead
                // of one that ends before it starts
                (idx + 1).max(interior_start)
            } else {
                block.len()
            }
        }
        _ => block.len(),
    };

    // Re-indent content to the fence's column (e.g. blocks in list items)
    let indent = " ".repeat(pos.start.column.saturating_sub(1));
    let mut replacement = String::new();
    for line in new_value.lines() {
        if !line.is_empty() {
            replacement.push_str(&indent);
            replacement.push_str(line);
        }
        replacement.push('\n');
    }

    Some(Edit {
        start: start + interior_start,
        end: start + interior_end,
        replacement,
    })
}

/// Extract the documented function name from a heading whose first child is
/// inline code, e.g. ``### `my_func(a, b)` `` → `my_func`.
fn heading_function_name(heading: &Heading) -> Option<String> {
    match heading.children.first()? {
        Node::InlineCode(c) => match c.value.split_once('(') {
            Some((f, _)) => Some(f.to_owned()),
            None => Some(c.value.clone()),
        },
        _ => None,
    }
}

/// Build the edit appending a fresh `{#name}` anchor to a heading,
/// replacing any anchor a previous run left there so reruns are idempotent
/// and stale anchors from renamed headings self-heal.
fn heading_anchor_edit(heading: &Heading, src: &str, function: &str) -> Option<Edit> {
    let pos = heading.position.as_ref()?;
    let (start, end) = (pos.start.offset, pos.end.offset);
    let heading_src = src.get(start..end)?;
    let kept = strip_trailing_anchors(heading_src);
    Some(Edit {
        start: start + kept.len(),
        end,
        replacement: format!(" {{#{}}}", function),
    })
}

/// Process a SQL code block, executing queries and returning the new block
/// contents with results inlined.
fn process_code_block(
    rt: &Runtime,
    sql: &str,
    args: &DocsInlineArgs,
) -> Result<String, DocsError> {
    let mut new_value = String::new();
    let mut curr = sql;
    // Result text generated for the previous statement. When regenerating a
    // previously inlined document, the prior run's result comments show up
    // as leading trivia of the *next* statement — strip them (they are
    // byte-identical for deterministic queries) so reruns are stable.
    let mut last_result: Option<String> = None;

    loop {
        match rt.prepare_with_parameters(curr) {
            Ok((rest, Some(mut stmt))) => {
                let stmt_sql = stmt.sql();
                let mut text = stmt_sql.trim_start();
                if let Some(prev) = &last_result {
                    let prev = prev.trim_end();
                    while let Some(stripped) = text.strip_prefix(prev) {
                        // Only strip on a line boundary, never mid-line
                        if stripped.is_empty() || stripped.starts_with(['\n', '\r']) {
                            text = stripped.trim_start();
                        } else {
                            break;
                        }
                    }
                }
                new_value.push_str(text);
                new_value.push('\n');

                let columns = stmt.column_names().unwrap_or_default();

                if columns.is_empty() {
                    // No columns - just execute
                    if let Err(e) = stmt.execute() {
                        return Err(DocsError::SqlError(format!(
                            "Execute failed: {}",
                            e.message
                        )));
                    }
                    last_result = None;
                } else {
                    // Has columns - collect results
                    let mut results: Vec<Vec<crate::commands::test::snap::ValueCopy>> = vec![];
                    loop {
                        match stmt.next() {
                            Ok(Some(row)) => {
                                let row = row.iter().map(copy).collect();
                                results.push(row);
                            }
                            Ok(None) => break,
                            Err(error) => {
                                report_error(
                                    args.input.to_string_lossy().as_ref(),
                                    &stmt.sql(),
                                    &error,
                                    None,
                                );
                                return Err(DocsError::AlreadyReported);
                            }
                        }
                    }

                    // Format results; every branch ends with exactly one
                    // newline so a following statement starts on its own
                    // line instead of being swallowed into the comment
                    let mut result_text = String::new();
                    match results.len() {
                        0 => result_text.push_str("-- No results\n"),
                        1 => {
                            let value = display_value(&results[0][0]);
                            if value.contains('\n') {
                                // A value containing a newline would break
                                // out of the `-- ` comment, leaving raw SQL
                                // fragments on unprefixed lines; prefix
                                // every line to keep the block valid SQL
                                for line in value.lines() {
                                    result_text.push_str("-- ");
                                    result_text.push_str(line);
                                    result_text.push('\n');
                                }
                            } else {
                                result_text.push_str(&format!("-- {}\n", value));
                            }
                        }
                        _ => {
                            let table = render_table(&columns, &results);
                            if table.contains("*/") {
                                // A cell containing `*/` would terminate the
                                // block comment early; fall back to
                                // line-comment prefixes to keep the block
                                // valid SQL
                                for line in table.lines() {
                                    result_text.push_str("-- ");
                                    result_text.push_str(line);
                                    result_text.push('\n');
                                }
                            } else {
                                result_text.push_str("/*\n");
                                result_text.push_str(&table);
                                result_text.push_str("*/\n");
                            }
                        }
                    }
                    new_value.push_str(&result_text);
                    last_result = Some(result_text);
                }

                // Move to rest of SQL
                match rest {
                    Some(offset) => {
                        if let Some(remaining) = curr.get(offset..) {
                            curr = remaining;
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            Ok((_, None)) => break,
            Err(error) => {
                let error_msg = report_error_string(
                    args.input.to_string_lossy().as_ref(),
                    sql,
                    &error,
                    None,
                );
                eprintln!("{}", error_msg);
                return Err(DocsError::AlreadyReported);
            }
        }
    }

    // Drop the trailing newline so the closing fence sits directly under
    // the last line instead of after a blank line
    Ok(new_value.trim_end().to_string())
}

/// Remove `{#anchor}` text (possibly several, possibly escaped as
/// `{#my\_func}` by older serializer-based runs) trailing a heading's
/// source text, so re-running `docs inline` on its own output replaces the
/// anchor instead of appending another copy. Stripping (rather than
/// skipping the push) also self-heals stale anchors when a heading's
/// function name changed.
fn strip_trailing_anchors(heading_src: &str) -> &str {
    let mut text = heading_src.trim_end();
    loop {
        let stripped = match (text.rfind("{#"), text.ends_with('}')) {
            (Some(idx), true) => {
                let inner = &text[idx + 2..text.len() - 1];
                if !inner.is_empty() && !inner.contains(['{', '}']) {
                    Some(text[..idx].trim_end())
                } else {
                    None
                }
            }
            _ => None,
        };
        match stripped {
            Some(value) => text = value,
            None => return text,
        }
    }
}

/// Collect the first column of every row of a query as strings. Serves
/// both the loaded-functions and loaded-modules tracking tables.
fn query_names(rt: &Runtime, sql: &str) -> Result<Vec<String>, DocsError> {
    let mut stmt = match rt.connection.prepare(sql) {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => return Ok(vec![]),
        Err(e) => {
            return Err(DocsError::SqlError(format!(
                "Failed to query names ({}): {}",
                sql, e.message
            )))
        }
    };

    let mut names = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                if let Some(val) = row.first() {
                    names.push(val.as_str().to_owned());
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(DocsError::SqlError(format!(
                    "Failed to read names ({}): {}",
                    sql, e.message
                )))
            }
        }
    }

    Ok(names)
}

/// Write output to file or stdout.
fn write_output(args: &DocsInlineArgs, content: &str) -> Result<(), DocsError> {
    match &args.output {
        Some(output) => {
            let mut f = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(output)
                .map_err(|e| DocsError::FileWrite(format!("{}: {}", output.display(), e)))?;

            f.write_all(content.as_bytes())
                .map_err(|e| DocsError::FileWrite(format!("{}: {}", output.display(), e)))?;

            println!("Wrote docs to {}", output.display());
        }
        None => {
            writeln!(stdout(), "{}", content)
                .map_err(|e| DocsError::FileWrite(format!("stdout: {}", e)))?;
        }
    }
    Ok(())
}

/// Entry point for the docs command.
pub(crate) fn docs(cmd: DocsNamespace) -> Result<(), ()> {
    match cmd.command {
        DocsCommand::Inline(args) => match inline(args) {
            Ok(()) => Ok(()),
            // Already printed (codespan report on stderr) — don't repeat it
            Err(DocsError::AlreadyReported) => Err(()),
            Err(e) => {
                eprintln!("Error: {}", e);
                Err(())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undocumented_functions_display_lists_each_once() {
        let err = DocsError::Undocumented {
            functions: vec!["a".into(), "b".into()],
            modules: vec![],
        };
        let s = err.to_string();
        assert_eq!(
            s,
            "The following functions are not documented:\n  - a\n  - b"
        );
        assert_eq!(s.matches("- a").count(), 1);
    }

    #[test]
    fn test_undocumented_display_labels_functions_and_modules() {
        let err = DocsError::Undocumented {
            functions: vec!["my_func".into()],
            modules: vec!["vtab_foo".into()],
        };
        assert_eq!(
            err.to_string(),
            "The following functions are not documented:\n  - my_func\n\
             The following modules are not documented:\n  - vtab_foo"
        );
    }

    #[test]
    fn test_undocumented_display_modules_only() {
        let err = DocsError::Undocumented {
            functions: vec![],
            modules: vec!["vtab_foo".into()],
        };
        assert_eq!(
            err.to_string(),
            "The following modules are not documented:\n  - vtab_foo"
        );
    }
}
