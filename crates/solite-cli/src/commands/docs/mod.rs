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

use markdown::mdast::{Node, Text};
use mdast_util_to_markdown::to_markdown;
use solite_core::Runtime;

use crate::cli::{DocsCommand, DocsInlineArgs, DocsNamespace};
use crate::commands::test::snap::{copy, snapshot_value};
use crate::errors::{report_error, report_error_string};

use sql::{
    BASE_FUNCTIONS_CREATE, BASE_MODULES_CREATE, LOADED_FUNCTIONS_CREATE, LOADED_MODULES_CREATE,
};
use table::render_table;

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
    /// Undocumented functions found.
    UndocumentedFunctions(Vec<String>),
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
            DocsError::UndocumentedFunctions(funcs) => {
                write!(f, "Undocumented functions: {}", funcs.join(", "))
            }
        }
    }
}

impl std::error::Error for DocsError {}

/// Process inline documentation.
fn inline(args: DocsInlineArgs) -> Result<(), DocsError> {
    let rt = Runtime::new(None);

    // Attach in-memory database for tracking
    if let Err(e) = rt
        .connection
        .execute("ATTACH DATABASE ':memory:' AS solite_docs")
    {
        return Err(DocsError::DatabaseAttach(format!("{:?}", e)));
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

    let mut ast = markdown::to_mdast(&docs_in, &options)
        .map_err(|e| DocsError::MarkdownParse(e.to_string()))?;

    // Process code blocks
    if let Some(children) = ast.children_mut() {
        for node in children.iter_mut() {
            if let Node::Code(code) = node {
                process_code_block(&rt, code, &args)?;
            }
        }
    }

    // Extract documented functions from headings
    let documented_funcs = extract_documented_functions(&mut ast);

    // Get loaded functions from extension
    let loaded_funcs = get_loaded_functions(&rt)?;

    // Find undocumented functions
    let mut undocumented_funcs: Vec<String> = loaded_funcs
        .iter()
        .filter(|f| !documented_funcs.contains(f))
        .cloned()
        .collect();

    // Convert AST back to markdown
    let out_md = to_markdown(&ast)
        .map_err(|e| DocsError::MarkdownParse(format!("Failed to serialize: {}", e)))?
        .replace("ю", "_");

    // Write output
    write_output(&args, &out_md)?;

    // Report undocumented functions
    if !undocumented_funcs.is_empty() {
        undocumented_funcs.sort();
        eprintln!("The following functions are not documented:");
        for func in &undocumented_funcs {
            eprintln!("  - {}", func);
        }
        return Err(DocsError::UndocumentedFunctions(undocumented_funcs));
    }

    Ok(())
}

/// Set up extension tracking tables and load extension.
fn setup_extension_tracking(rt: &Runtime, ext: &str) -> Result<(), DocsError> {
    if let Err(e) = rt.connection.execute(BASE_FUNCTIONS_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create base functions table: {:?}",
            e
        )));
    }

    if let Err(e) = rt.connection.execute(BASE_MODULES_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create base modules table: {:?}",
            e
        )));
    }

    if let Err(e) = rt.connection.load_extension(ext, &None) {
        return Err(DocsError::ExtensionLoad(format!("{}: {:?}", ext, e)));
    }

    if let Err(e) = rt.connection.execute(LOADED_FUNCTIONS_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create loaded functions table: {:?}",
            e
        )));
    }

    if let Err(e) = rt.connection.execute(LOADED_MODULES_CREATE) {
        return Err(DocsError::SqlError(format!(
            "Failed to create loaded modules table: {:?}",
            e
        )));
    }

    Ok(())
}

/// Process a SQL code block, executing queries and inlining results.
fn process_code_block(
    rt: &Runtime,
    code: &mut markdown::mdast::Code,
    args: &DocsInlineArgs,
) -> Result<(), DocsError> {
    let sql = code.value.clone();
    let mut new_value = String::new();
    let mut curr = sql.as_str();

    loop {
        match rt.prepare_with_parameters(curr) {
            Ok((rest, Some(stmt))) => {
                new_value.push_str(&stmt.sql());
                new_value.push('\n');

                let columns = stmt.column_names().unwrap_or_default();

                if columns.is_empty() {
                    // No columns - just execute
                    if let Err(e) = stmt.execute() {
                        return Err(DocsError::SqlError(format!("Execute failed: {:?}", e)));
                    }
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
                                return Err(DocsError::SqlError(error.message));
                            }
                        }
                    }

                    // Format results
                    match results.len() {
                        0 => new_value.push_str("No results\n"),
                        1 => {
                            new_value.push_str(&format!(
                                "-- {}",
                                snapshot_value(&results[0][0])
                            ));
                        }
                        _ => {
                            new_value.push_str("/*\n");
                            new_value.push_str(&render_table(&columns, &results));
                            new_value.push_str("*/");
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
                let error_msg = report_error_string("TODO", &sql, &error, None);
                eprintln!("{}", error_msg);
                return Err(DocsError::SqlError(format!("Prepare failed: {:?}", error)));
            }
        }
    }

    code.value = new_value;
    Ok(())
}

/// Extract function names from documentation headings.
fn extract_documented_functions(ast: &mut Node) -> Vec<String> {
    let children = match ast.children_mut() {
        Some(c) => c,
        None => return vec![],
    };

    children
        .iter_mut()
        .filter_map(|node| {
            if let Node::Heading(heading) = node {
                if heading.depth == 3 || heading.depth == 4 {
                    let children = node.children()?;
                    let first = children.first()?;

                    let function = if let Node::InlineCode(c) = first {
                        match c.value.split_once('(') {
                            Some((f, _)) => Some(f.to_owned()),
                            None => Some(c.value.clone()),
                        }
                    } else {
                        None
                    };

                    // Add anchor link
                    if let Some(ref f) = function {
                        if let Some(children) = node.children_mut() {
                            children.push(Node::Text(Text {
                                value: format!(" {{#{}}}", f.replace('_', "ю")),
                                position: None,
                            }));
                        }
                    }

                    return function;
                }
            }
            None
        })
        .collect()
}

/// Get list of loaded functions from extension.
fn get_loaded_functions(rt: &Runtime) -> Result<Vec<String>, DocsError> {
    let stmt = match rt
        .connection
        .prepare("SELECT name FROM solite_docs.solite_docs_loaded_functions")
    {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => return Ok(vec![]),
        Err(e) => {
            return Err(DocsError::SqlError(format!(
                "Failed to query loaded functions: {:?}",
                e
            )))
        }
    };

    let mut funcs = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                if let Some(val) = row.first() {
                    funcs.push(val.as_str().to_owned());
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(DocsError::SqlError(format!(
                    "Failed to read loaded functions: {:?}",
                    e
                )))
            }
        }
    }

    Ok(funcs)
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
            Err(e) => {
                eprintln!("Error: {}", e);
                Err(())
            }
        },
    }
}
