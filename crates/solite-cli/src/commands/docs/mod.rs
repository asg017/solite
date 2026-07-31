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
//! - Intentional-error examples: a `-- @expect-error` line before a
//!   statement marks it as expected to fail. The directive is stripped
//!   from the output and the actual error message is inlined as an
//!   `-- error: <message>` comment, which doubles as the marker on
//!   reruns. A marked statement that *succeeds* is a hard failure, as is
//!   (unchanged) an error on an unmarked statement.
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
    /// A statement marked as expected-to-fail succeeded.
    ExpectedErrorSucceeded(String),
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
            DocsError::ExpectedErrorSucceeded(stmt) => write!(
                f,
                "Statement is expected to fail (`-- @expect-error` directive \
                 or trailing `-- error:` comment) but succeeded:\n{}",
                stmt
            ),
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
            // Absolute offset of the block's interior (first byte after the
            // opening fence line), so SQL errors report file-accurate lines
            let block_offset = code
                .position
                .as_ref()
                .and_then(|pos| {
                    let block = src.get(pos.start.offset..pos.end.offset)?;
                    Some(pos.start.offset + block.find('\n')? + 1)
                })
                .unwrap_or(0);
            let new_value = process_code_block(rt, &code.value, args, src, block_offset)?;
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

/// The prologue directive marking a statement as expected to fail. The
/// directive is stripped from the output; the regenerated `-- error:`
/// comment below the statement doubles as the marker on reruns, so
/// inlining in place (`-o` onto the input) stays stable.
const EXPECT_ERROR_DIRECTIVE: &str = "-- @expect-error";

/// Process a SQL code block, executing queries and returning the new block
/// contents with results inlined.
///
/// `src` is the full markdown source and `block_offset` the absolute byte
/// offset of the block's interior, so error reports carry file-accurate
/// positions. (For blocks indented inside list items the mapping drifts,
/// since `code.value` has the indentation stripped.)
fn process_code_block(
    rt: &Runtime,
    sql: &str,
    args: &DocsInlineArgs,
    src: &str,
    block_offset: usize,
) -> Result<String, DocsError> {
    let mut new_value = String::new();
    let mut curr = sql;
    // Result text generated for the previous statement. When regenerating a
    // previously inlined document, the prior run's result comments show up
    // as leading trivia of the *next* statement — strip them (they are
    // byte-identical for deterministic queries) so reruns are stable.
    let mut last_result: Option<String> = None;

    loop {
        let curr_offset = block_offset + (sql.len() - curr.len());
        match rt.prepare_with_parameters(curr) {
            Ok((rest, Some(mut stmt))) => {
                let stmt_sql = stmt.sql();
                let text = strip_stale_result(stmt_sql.trim_start(), &last_result);
                let (text, directive) = strip_expect_error_directive(text);
                // An `-- error:` comment right after the statement is the
                // previous run's regenerated output acting as the marker
                let stale_error = rest
                    .and_then(|offset| curr.get(offset..))
                    .and_then(trailing_error_line);
                let expect_error = directive || stale_error.is_some();

                new_value.push_str(&text);
                new_value.push('\n');

                let columns = stmt.column_names().unwrap_or_default();

                // Run the statement: result text on success (None for
                // column-less statements), or the SQLite error
                let outcome = if columns.is_empty() {
                    stmt.execute().map(|_| None)
                } else {
                    let mut results: Vec<Vec<crate::commands::test::snap::ValueCopy>> = vec![];
                    loop {
                        match stmt.next() {
                            Ok(Some(row)) => results.push(row.iter().map(copy).collect()),
                            Ok(None) => break Ok(Some(format_results(&columns, &results))),
                            Err(error) => break Err(error),
                        }
                    }
                };

                match outcome {
                    Ok(result_text) => {
                        if expect_error {
                            // A fixed error example must not silently flip
                            // into a result example. (A genuine result can
                            // never impersonate the marker: strings render
                            // quoted, so no result line starts `-- error:`.)
                            return Err(DocsError::ExpectedErrorSucceeded(
                                text.trim().to_string(),
                            ));
                        }
                        match result_text {
                            Some(result_text) => {
                                new_value.push_str(&result_text);
                                last_result = Some(result_text);
                            }
                            None => last_result = None,
                        }
                    }
                    Err(error) => {
                        if expect_error {
                            // Regenerate like a result comment: the actual
                            // message replaces whatever the marker said, so
                            // drift shows up as a diff instead of a failure.
                            // Strip the *stale* line from the next
                            // statement's trivia (it differs from the fresh
                            // one exactly when the message drifted)
                            let line = format!("-- error: {}\n", error.message);
                            new_value.push_str(&line);
                            last_result = Some(match stale_error {
                                Some(stale) => stale.to_string(),
                                None => line,
                            });
                        } else {
                            report_error(
                                args.input.to_string_lossy().as_ref(),
                                src,
                                &error,
                                Some(error_caret_offset(&error, curr, curr_offset)),
                            );
                            return Err(DocsError::AlreadyReported);
                        }
                    }
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
                // The statement never prepared, so recover its span with the
                // same scan-to-`;` heuristic the test runner uses
                // (commands/test/parser.rs). A `;` inside a string literal
                // can fool it, but the statement already failed to prepare,
                // so a missed marker only means the block fails as it would
                // have anyway.
                let trivia_len = leading_trivia_len(curr);
                let stmt_end = curr[trivia_len..].find(';').map(|idx| trivia_len + idx + 1);
                let stmt_text = &curr[..stmt_end.unwrap_or(curr.len())];
                let text = strip_stale_result(stmt_text.trim_start(), &last_result);
                let (text, directive) = strip_expect_error_directive(text);
                let stale_error = stmt_end
                    .and_then(|end| curr.get(end..))
                    .and_then(trailing_error_line);
                if directive || stale_error.is_some() {
                    new_value.push_str(text.trim_end());
                    new_value.push('\n');
                    let line = format!("-- error: {}\n", error.message);
                    new_value.push_str(&line);
                    last_result = Some(match stale_error {
                        Some(stale) => stale.to_string(),
                        None => line,
                    });
                    match stmt_end.and_then(|end| curr.get(end..)) {
                        Some(remaining) => {
                            curr = remaining;
                            continue;
                        }
                        None => break,
                    }
                }
                let error_msg = report_error_string(
                    args.input.to_string_lossy().as_ref(),
                    src,
                    &error,
                    Some(error_caret_offset(&error, curr, curr_offset)),
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

/// Format collected rows as the comment block inlined under a statement.
/// Every branch ends with exactly one newline so a following statement
/// starts on its own line instead of being swallowed into the comment.
fn format_results(
    columns: &[String],
    results: &[Vec<crate::commands::test::snap::ValueCopy>],
) -> String {
    let mut result_text = String::new();
    match results.len() {
        0 => result_text.push_str("-- No results\n"),
        1 => {
            let value = display_value(&results[0][0]);
            if value.contains('\n') {
                // A value containing a newline would break out of the
                // `-- ` comment, leaving raw SQL fragments on unprefixed
                // lines; prefix every line to keep the block valid SQL
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
            let table = render_table(columns, results);
            if table.contains("*/") {
                // A cell containing `*/` would terminate the block comment
                // early; fall back to line-comment prefixes to keep the
                // block valid SQL
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
    result_text
}

/// Strip the previous statement's regenerated result comment from the
/// leading trivia of `text` (only on line boundaries, never mid-line).
fn strip_stale_result<'a>(mut text: &'a str, last_result: &Option<String>) -> &'a str {
    if let Some(prev) = last_result {
        let prev = prev.trim_end();
        while let Some(stripped) = text.strip_prefix(prev) {
            if stripped.is_empty() || stripped.starts_with(['\n', '\r']) {
                text = stripped.trim_start();
            } else {
                break;
            }
        }
    }
    text
}

/// Byte length of the leading trivia (whitespace, `--` line comments, and
/// `/* ... */` block comments) at the start of `s`.
fn leading_trivia_len(s: &str) -> usize {
    let mut idx = 0;
    loop {
        let rest = &s[idx..];
        idx += rest.len() - rest.trim_start().len();
        let rest = &s[idx..];
        if rest.starts_with("--") {
            match rest.find('\n') {
                Some(n) => idx += n + 1,
                None => return s.len(),
            }
        } else if rest.starts_with("/*") {
            match rest.find("*/") {
                Some(n) => idx += n + 2,
                None => return s.len(),
            }
        } else {
            return idx;
        }
    }
}

/// Remove a `-- @expect-error` directive line from the leading trivia of
/// `text`, returning the stripped text and whether it was present.
fn strip_expect_error_directive(text: &str) -> (String, bool) {
    let trivia_len = leading_trivia_len(text);
    let mut out = String::with_capacity(text.len());
    let mut found = false;
    for line in text[..trivia_len].split_inclusive('\n') {
        if !found && line.trim() == EXPECT_ERROR_DIRECTIVE {
            found = true;
        } else {
            out.push_str(line);
        }
    }
    if !found {
        return (text.to_string(), false);
    }
    out.push_str(&text[trivia_len..]);
    (out.trim_start().to_string(), true)
}

/// The first non-blank line after a statement, when it is an `-- error:`
/// comment left by a previous run (the rerun form of the expected-failure
/// marker).
fn trailing_error_line(after: &str) -> Option<&str> {
    let line = after.trim_start().lines().next()?.trim_end();
    line.starts_with("-- error:").then_some(line)
}

/// Absolute caret offset for an error report: sqlite's own error offset is
/// relative to `curr` and added by the diagnostic range builder, so pass
/// just `curr`'s base; without one, point at the statement text past its
/// leading trivia instead of at a comment line.
fn error_caret_offset(
    error: &solite_core::sqlite::SQLiteError,
    curr: &str,
    curr_offset: usize,
) -> usize {
    match error.offset {
        Some(_) => curr_offset,
        None => curr_offset + leading_trivia_len(curr),
    }
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
    use std::path::PathBuf;

    fn run_block(sql: &str) -> Result<String, DocsError> {
        let rt = Runtime::new(None).unwrap();
        let args = DocsInlineArgs {
            input: PathBuf::from("test.md"),
            extension: None,
            output: None,
        };
        process_code_block(&rt, sql, &args, sql, 0)
    }

    #[test]
    fn test_expect_error_directive_inlines_prepare_error() {
        let out = run_block("-- @expect-error\nselect * from missing;").unwrap();
        assert_eq!(out, "select * from missing;\n-- error: no such table: missing");
    }

    #[test]
    fn test_expect_error_directive_inlines_runtime_error() {
        let out = run_block("-- @expect-error\nselect json_extract('x', '$.a');").unwrap();
        assert_eq!(
            out,
            "select json_extract('x', '$.a');\n-- error: malformed JSON"
        );
    }

    #[test]
    fn test_expect_error_rerun_is_byte_stable() {
        for src in [
            "-- @expect-error\nselect * from missing;",
            "-- @expect-error\nselect json_extract('x', '$.a');",
            "-- @expect-error\nselect * from missing;\nselect 'after';",
        ] {
            let once = run_block(src).unwrap();
            let twice = run_block(&once).unwrap();
            assert_eq!(once, twice, "rerun not byte-stable for {src:?}");
        }
    }

    #[test]
    fn test_expect_error_execute_error_on_columnless_statement() {
        let out = run_block(
            "create table t(a int unique);\ninsert into t values (1);\n\
             -- @expect-error\ninsert into t values (1);",
        )
        .unwrap();
        assert_eq!(
            out,
            "create table t(a int unique);\ninsert into t values (1);\n\
             insert into t values (1);\n-- error: UNIQUE constraint failed: t.a"
        );
        assert_eq!(run_block(&out).unwrap(), out);
    }

    #[test]
    fn test_statements_after_expected_error_still_run() {
        let out =
            run_block("-- @expect-error\nselect * from missing;\nselect 'after';").unwrap();
        assert_eq!(
            out,
            "select * from missing;\n-- error: no such table: missing\n\
             select 'after';\n-- 'after'"
        );
    }

    #[test]
    fn test_stale_error_message_is_replaced_not_duplicated() {
        let out = run_block(
            "select * from missing;\n-- error: old message\nselect 'after';",
        )
        .unwrap();
        assert_eq!(
            out,
            "select * from missing;\n-- error: no such table: missing\n\
             select 'after';\n-- 'after'"
        );
    }

    #[test]
    fn test_expect_error_on_succeeding_statement_fails() {
        let err = run_block("-- @expect-error\nselect 1;").unwrap_err();
        assert!(matches!(err, DocsError::ExpectedErrorSucceeded(_)));
        // rerun form: a stale `-- error:` marker on a now-succeeding statement
        let err = run_block("select 1;\n-- error: whatever").unwrap_err();
        assert!(matches!(err, DocsError::ExpectedErrorSucceeded(_)));
    }

    #[test]
    fn test_error_without_marker_is_still_fatal() {
        let err = run_block("select * from missing;").unwrap_err();
        assert!(matches!(err, DocsError::AlreadyReported));
    }

    #[test]
    fn test_stale_result_comment_is_not_an_error_marker() {
        // A regenerated *result* comment after a succeeding statement must
        // not read as an expected-failure marker
        let src = "select 1;\n-- 1\nselect 2;";
        let out = run_block(src).unwrap();
        assert_eq!(out, "select 1;\n-- 1\nselect 2;\n-- 2");
    }

    #[test]
    fn test_consecutive_expect_error_statements() {
        let out = run_block(
            "-- @expect-error\nselect * from a_missing;\n\
             -- @expect-error\nselect * from b_missing;",
        )
        .unwrap();
        assert_eq!(
            out,
            "select * from a_missing;\n-- error: no such table: a_missing\n\
             select * from b_missing;\n-- error: no such table: b_missing"
        );
        assert_eq!(run_block(&out).unwrap(), out);
    }

    #[test]
    fn test_leading_trivia_len() {
        assert_eq!(leading_trivia_len("select 1;"), 0);
        assert_eq!(leading_trivia_len("  select 1;"), 2);
        assert_eq!(leading_trivia_len("-- c\nselect 1;"), 5);
        assert_eq!(leading_trivia_len("/* c */ select 1;"), 8);
        assert_eq!(leading_trivia_len("\n-- a\n/* b */\n-- c\nx"), 19);
        // unterminated trivia swallows the rest
        assert_eq!(leading_trivia_len("-- only a comment"), 17);
        assert_eq!(leading_trivia_len("/* unterminated"), 15);
    }

    #[test]
    fn test_strip_expect_error_directive() {
        // directive alone
        assert_eq!(
            strip_expect_error_directive("-- @expect-error\nselect 1;"),
            ("select 1;".to_string(), true)
        );
        // directive between other leading comments
        assert_eq!(
            strip_expect_error_directive("-- a\n-- @expect-error\n-- b\nselect 1;"),
            ("-- a\n-- b\nselect 1;".to_string(), true)
        );
        // not in leading trivia: untouched
        assert_eq!(
            strip_expect_error_directive("select 1;\n-- @expect-error"),
            ("select 1;\n-- @expect-error".to_string(), false)
        );
        // similar comments don't match
        assert_eq!(
            strip_expect_error_directive("-- @expect-errors\nselect 1;").1,
            false
        );
    }

    #[test]
    fn test_trailing_error_line() {
        assert_eq!(
            trailing_error_line("\n-- error: boom\nselect 1;"),
            Some("-- error: boom")
        );
        assert_eq!(trailing_error_line("\n-- 1\nselect 1;"), None);
        assert_eq!(trailing_error_line("\nselect 1;"), None);
        assert_eq!(trailing_error_line(""), None);
    }

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

    #[test]
    fn test_strip_trailing_anchors() {
        // no anchor: unchanged
        assert_eq!(strip_trailing_anchors("### `f(a)`"), "### `f(a)`");
        // one anchor
        assert_eq!(strip_trailing_anchors("### `f(a)` {#f}"), "### `f(a)`");
        // accumulated + escaped anchors from older runs
        assert_eq!(
            strip_trailing_anchors("### `my_func(a)` {#my\\_func} {#my_func}"),
            "### `my_func(a)`"
        );
        // braces mid-heading are not anchors
        assert_eq!(strip_trailing_anchors("### a {b} c"), "### a {b} c");
    }

    fn first_heading(md: &str) -> Heading {
        let ast = markdown::to_mdast(md, &markdown::ParseOptions::gfm()).unwrap();
        for child in ast.children().unwrap() {
            if let Node::Heading(h) = child {
                return h.clone();
            }
        }
        panic!("no heading in {md:?}");
    }

    #[test]
    fn test_heading_function_name() {
        assert_eq!(
            heading_function_name(&first_heading("### `my_func(a, b)`")),
            Some("my_func".to_string())
        );
        // no parens: whole inline code is the name
        assert_eq!(
            heading_function_name(&first_heading("#### `vtab_foo`")),
            Some("vtab_foo".to_string())
        );
        // plain-text headings are not documented functions
        assert_eq!(heading_function_name(&first_heading("### Usage")), None);
    }
}
