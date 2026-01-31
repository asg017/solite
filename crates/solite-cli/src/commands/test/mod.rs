//! SQL test runner for assertion-based testing.
//!
//! This module provides functionality to run SQL test files that contain
//! assertions in the form of comments following SQL statements.
//!
//! # Test File Format
//!
//! Each SQL statement can be followed by an expected result comment:
//!
//! ```sql
//! SELECT 1 + 1; -- 2
//! SELECT 'hello'; -- 'hello'
//! SELECT * FROM empty_table; -- [no results]
//! ```
//!
//! # Special Annotations
//!
//! - `-- [no results]`: Expect no rows returned
//! - `-- error: <message>`: Expect a specific error message
//! - `-- TODO ...`: Skip this test (marked as pending)
//!
//! # Example Test File
//!
//! ```sql
//! -- Setup
//! CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT);
//! INSERT INTO users VALUES (1, 'Alice');
//!
//! -- Tests
//! SELECT COUNT(*) FROM users; -- 1
//! SELECT name FROM users WHERE id = 1; -- 'Alice'
//! SELECT * FROM users WHERE id = 999; -- [no results]
//! SELECT * FROM nonexistent; -- error: no such table: nonexistent
//! ```

mod parser;
mod report;
mod value;

use console::Style;
use solite_core::dot::DotCommand;
use solite_core::{BlockSource, Runtime, StepResult};
use std::fs::read_to_string;
use std::io::Write as _;

use crate::cli::TestArgs;

use parser::{compute_offset_from_reference, parse_epilogue_comment, parse_ref_file_line_col};
use report::{report_mismatch, TestStats};
use value::value_to_string;

/// Run SQL tests from a file.
fn test_impl(args: TestArgs) -> Result<(), TestError> {
    let source_path = args.file;
    let content = read_to_string(&source_path)
        .map_err(|e| TestError::FileRead(format!("{}: {}", source_path.display(), e)))?;

    let mut rt = Runtime::new(None);
    rt.enqueue(
        &source_path.to_string_lossy(),
        &content,
        BlockSource::File(source_path.clone()),
    );

    let mut stats = TestStats::new();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(e)) => {
                stats.record_failure();
                eprintln!("Error preparing step: {}", e);
                print!("{}", Style::new().red().apply_to("x"));
            }
            Some(Ok(step)) => match step.result {
                StepResult::DotCommand(cmd) => {
                    handle_dot_command(&cmd, &mut rt);
                }
                StepResult::SqlStatement { stmt, .. } => {
                    let epilogue = match &step.epilogue {
                        Some(s) => parse_epilogue_comment(s),
                        None => {
                            stats.record_failure();
                            print!("{}", Style::new().red().apply_to("x"));
                            continue;
                        }
                    };

                    // Handle TODO annotations
                    if epilogue.to_uppercase().starts_with("TODO") {
                        let ref_display = format!("{}", step.reference);
                        if let Some((file, line, col)) = parse_ref_file_line_col(&ref_display) {
                            stats.record_todo(file, line, col, epilogue.clone());
                        } else {
                            stats.record_todo(
                                source_path.to_string_lossy().to_string(),
                                0,
                                0,
                                epilogue.clone(),
                            );
                        }
                        print!("{}", Style::new().yellow().apply_to("-"));
                        continue;
                    }

                    // Execute and compare
                    match stmt.next() {
                        Err(err) => {
                            let ref_display = format!("{}", step.reference);
                            let maybe_offset =
                                compute_offset_from_reference(&content, &ref_display);

                            if epilogue.starts_with("error:") {
                                let expected = epilogue["error:".len()..].trim();
                                if expected == err.message {
                                    stats.record_success();
                                    print!("{}", Style::new().green().apply_to("."));
                                } else {
                                    stats.record_failure();
                                    print!("{}", Style::new().red().apply_to("x"));
                                    crate::errors::report_error(
                                        &source_path.to_string_lossy(),
                                        &content,
                                        &err,
                                        maybe_offset,
                                    );
                                    if args.verbose {
                                        eprintln!(
                                            "\nExpected error: '{}' got: '{}'",
                                            expected, err.message
                                        );
                                    }
                                }
                            } else {
                                stats.record_failure();
                                print!("{}", Style::new().red().apply_to("x"));
                                crate::errors::report_error(
                                    &source_path.to_string_lossy(),
                                    &content,
                                    &err,
                                    maybe_offset,
                                );
                                if args.verbose {
                                    eprintln!("\nExecution error: {}", err.message);
                                }
                            }
                        }
                        Ok(None) => {
                            if epilogue == "[no results]" {
                                stats.record_success();
                                print!("{}", Style::new().green().apply_to("."));
                            } else {
                                stats.record_failure();
                                print!("{}", Style::new().red().apply_to("x"));
                            }
                        }
                        Ok(Some(row)) => {
                            let v = match row.first() {
                                Some(v) => v,
                                None => {
                                    stats.record_failure();
                                    print!("{}", Style::new().red().apply_to("x"));
                                    continue;
                                }
                            };

                            let actual = value_to_string(v);
                            if actual == epilogue {
                                stats.record_success();
                                print!("{}", Style::new().green().apply_to("."));
                            } else {
                                stats.record_failure();
                                print!("{}", Style::new().red().apply_to("x"));

                                let ref_display = format!("{}", step.reference);
                                if let Some((_, line, col)) =
                                    parse_ref_file_line_col(&ref_display)
                                {
                                    report_mismatch(
                                        &source_path.to_string_lossy(),
                                        &content,
                                        line,
                                        col,
                                        &epilogue,
                                        &actual,
                                    );
                                } else if args.verbose {
                                    eprintln!("\nExpected: '{}' Got: '{}'", epilogue, actual);
                                }
                            }
                        }
                    }
                }
            },
        }

        let _ = handle.flush();
    }

    stats.print_summary();

    if stats.has_failures() {
        if !stats.todos.is_empty() {
            eprintln!(
                "\nThere are {} TODO(s). Treating as failure per policy.",
                stats.todos.len()
            );
        }
        Err(TestError::TestsFailed {
            failures: stats.failures,
            todos: stats.todos.len(),
        })
    } else {
        Ok(())
    }
}

/// Handle a dot command during test execution.
fn handle_dot_command(cmd: &DotCommand, rt: &mut Runtime) {
    match cmd {
        DotCommand::Load(load_cmd) => {
            if let Err(e) = load_cmd.execute(&mut rt.connection) {
                eprintln!("Warning: Failed to load extension: {:?}", e);
            }
        }
        other => {
            eprintln!("Warning: Unhandled dot command in test: {:?}", other);
        }
    }
}

/// Errors that can occur during test execution.
#[derive(Debug)]
pub enum TestError {
    /// Failed to read the test file.
    FileRead(String),
    /// Tests failed.
    TestsFailed { failures: usize, todos: usize },
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::FileRead(msg) => write!(f, "Failed to read file: {}", msg),
            TestError::TestsFailed { failures, todos } => {
                write!(f, "{} failures; {} todos", failures, todos)
            }
        }
    }
}

impl std::error::Error for TestError {}

/// Entry point for the test command.
pub fn test(args: TestArgs) -> Result<(), ()> {
    match test_impl(args) {
        Ok(()) => Ok(()),
        Err(_) => Err(()),
    }
}
