//! SQL script execution command.
//!
//! This module provides functionality to execute SQL scripts and Jupyter
//! notebooks, with support for progress tracking, tracing, and dot commands.
//!
//! # Features
//!
//! - Execute `.sql` files
//! - Execute `.ipynb` Jupyter notebooks (SQL cells only)
//! - Progress tracking for long-running statements
//! - Optional execution tracing to SQLite database
//! - Timer display for statement execution times
//! - Dot command support (.load, .open, .timer, etc.)
//!
//! # Example Usage
//!
//! ```bash
//! # Run a SQL script
//! solite run script.sql
//!
//! # Run against a database
//! solite run mydb.db script.sql
//!
//! # Run with tracing
//! solite run script.sql --trace trace.db
//!
//! # Run a Jupyter notebook
//! solite run notebook.ipynb
//! ```

mod dot;
mod format;
mod sql;
mod status;

use std::ffi::OsStr;
use std::fs::read_to_string;

use nbformat::{parse_notebook, Notebook};
use solite_core::{BlockSource, Runtime, StepError, StepResult};

use crate::cli::{ReplArgs, RunArgs};

use dot::handle_dot_command;
pub use format::format_duration;
use sql::handle_sql;

/// Errors that can occur during script execution.
#[derive(Debug)]
pub enum RunError {
    /// Failed to read input file.
    FileRead(String),
    /// Failed to parse notebook.
    NotebookParse(String),
    /// Unknown file type.
    UnknownFileType(String),
    /// Failed to set up tracing.
    TraceSetup(String),
    /// Failed to set parameter.
    ParameterSet(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::FileRead(msg) => write!(f, "Failed to read file: {}", msg),
            RunError::NotebookParse(msg) => write!(f, "Failed to parse notebook: {}", msg),
            RunError::UnknownFileType(msg) => write!(f, "Unknown file type: {}", msg),
            RunError::TraceSetup(msg) => write!(f, "Failed to set up tracing: {}", msg),
            RunError::ParameterSet(msg) => write!(f, "Failed to set parameter: {}", msg),
        }
    }
}

impl std::error::Error for RunError {}

/// Entry point for the run command.
pub(crate) fn run(flags: RunArgs) -> Result<(), ()> {
    match run_impl(flags) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(())
        }
    }
}

/// Internal implementation of the run command.
fn run_impl(flags: RunArgs) -> Result<(), RunError> {
    // Determine database and script from arguments
    let (database, script) = match (flags.database, flags.script) {
        (Some(a), Some(b)) => {
            // If first arg is .sql, swap them
            if a.extension().and_then(OsStr::to_str) == Some("sql") {
                (Some(b), a)
            } else {
                (Some(a), b)
            }
        }
        (Some(input), None) | (None, Some(input)) => {
            let ext = input.extension().and_then(OsStr::to_str);
            match ext {
                Some("sql") | Some("ipynb") => (None, input),
                Some("db") | Some("sqlite") | Some("sqlite3") => {
                    // Open REPL for database files
                    return crate::commands::repl::repl(ReplArgs {
                        database: Some(input),
                    })
                    .map_err(|_| RunError::FileRead("Failed to open REPL".to_string()));
                }
                Some(ext) => {
                    return Err(RunError::UnknownFileType(ext.to_string()));
                }
                None => {
                    return Err(RunError::UnknownFileType("no extension".to_string()));
                }
            }
        }
        (None, None) => {
            // No arguments - open REPL
            return crate::commands::repl::repl(ReplArgs { database: None })
                .map_err(|_| RunError::FileRead("Failed to open REPL".to_string()));
        }
    };

    // Create runtime
    let mut rt = Runtime::new(database.as_ref().map(|p| p.to_string_lossy().to_string()));

    // Set up tracing if requested
    if flags.trace.is_some() {
        setup_tracing(&rt)?;
    }

    // Set parameters
    for chunk in flags.parameters.chunks(2) {
        if chunk.len() == 2 {
            rt.define_parameter(chunk[0].clone(), chunk[1].clone())
                .map_err(|e| RunError::ParameterSet(e.to_string()))?;
        }
    }

    // Load and enqueue script
    enqueue_script(&mut rt, &script)?;

    // Execute
    let mut timer = true;
    execute_steps(&mut rt, flags.trace.is_some(), &mut timer);

    // Write trace output if requested
    if let Some(trace_path) = flags.trace {
        write_trace_output(&rt, &trace_path)?;
    }

    Ok(())
}

/// Set up tracing database.
fn setup_tracing(rt: &Runtime) -> Result<(), RunError> {
    rt.connection
        .execute("ATTACH DATABASE ':memory:' AS solite_trace;")
        .map_err(|e| RunError::TraceSetup(format!("{:?}", e)))?;

    rt.connection
        .execute(
            "CREATE TABLE solite_trace.statements(id INTEGER PRIMARY KEY AUTOINCREMENT, sql TEXT)",
        )
        .map_err(|e| RunError::TraceSetup(format!("{:?}", e)))?;

    rt.connection
        .execute(
            r#"CREATE TABLE solite_trace.steps(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                statement_id INTEGER REFERENCES statements(id),
                addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
            )"#,
        )
        .map_err(|e| RunError::TraceSetup(format!("{:?}", e)))?;

    Ok(())
}

/// Load and enqueue a script for execution.
fn enqueue_script(rt: &mut Runtime, script: &std::path::Path) -> Result<(), RunError> {
    match script.extension().and_then(OsStr::to_str) {
        Some("sql") => {
            let sql = read_to_string(script)
                .map_err(|e| RunError::FileRead(format!("{}: {}", script.display(), e)))?;

            rt.enqueue(
                &script.to_string_lossy(),
                &sql,
                BlockSource::File(script.to_path_buf()),
            );
        }
        Some("ipynb") => {
            let content = read_to_string(script)
                .map_err(|e| RunError::FileRead(format!("{}: {}", script.display(), e)))?;

            let nb: Notebook = parse_notebook(&content)
                .map_err(|e| RunError::NotebookParse(format!("{}: {}", script.display(), e)))?;

            let cells = extract_notebook_cells(&nb);
            for (idx, code) in cells {
                rt.enqueue(
                    &format!("{}:{}", script.to_string_lossy(), idx),
                    &code,
                    BlockSource::JupyerCell,
                );
            }
        }
        Some(ext) => {
            return Err(RunError::UnknownFileType(ext.to_string()));
        }
        None => {
            return Err(RunError::UnknownFileType("no extension".to_string()));
        }
    }

    Ok(())
}

/// Extract code cells from a Jupyter notebook.
fn extract_notebook_cells(nb: &Notebook) -> Vec<(usize, String)> {
    match nb {
        Notebook::V4(nb) => nb
            .cells
            .iter()
            .enumerate()
            .filter_map(|(idx, cell)| match cell {
                nbformat::v4::Cell::Code { source, .. } => Some((idx, source.join("\n"))),
                _ => None,
            })
            .collect(),
        Notebook::Legacy(nb) => nb
            .cells
            .iter()
            .enumerate()
            .filter_map(|(idx, cell)| match cell {
                nbformat::legacy::Cell::Code { source, .. } => Some((idx, source.join("\n"))),
                _ => None,
            })
            .collect(),
    }
}

/// Execute all queued steps.
fn execute_steps(rt: &mut Runtime, is_trace: bool, timer: &mut bool) {
    loop {
        match rt.next_stepx() {
            None => break,
            Some(Ok(mut step)) => match step.result {
                StepResult::SqlStatement { ref stmt, .. } => {
                    handle_sql(rt, stmt, &step.reference.to_string(), is_trace, *timer);
                }
                StepResult::DotCommand(ref mut cmd) => {
                    handle_dot_command(rt, cmd, timer);
                }
            },
            Some(Err(step_error)) => {
                handle_step_error(&step_error);
            }
        }
    }
}

/// Handle a step error.
fn handle_step_error(error: &StepError) {
    match error {
        StepError::Prepare {
            error,
            file_name,
            src,
            offset,
        } => {
            crate::errors::report_error(file_name, src, error, Some(*offset));
        }
        StepError::ParseDot(err) => {
            eprintln!("Parse dot error: {}", err);
        }
    }
}

/// Write trace output to file.
fn write_trace_output(rt: &Runtime, trace_path: &std::path::Path) -> Result<(), RunError> {
    let stmt = match rt.connection.prepare("VACUUM solite_trace INTO ?;") {
        Ok((_, Some(s))) => s,
        _ => {
            return Err(RunError::TraceSetup(
                "Failed to prepare vacuum statement".to_string(),
            ));
        }
    };

    let path_str = trace_path.to_string_lossy();
    stmt.bind_text(1, &path_str);

    if let Err(e) = stmt.execute() {
        return Err(RunError::TraceSetup(format!("Failed to write trace: {:?}", e)));
    }

    Ok(())
}
