//! SQL test runner for assertion-based and snapshot testing.
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
//! - `-- @snap <name>`: Snapshot assertion (captures full output to a .snap file)
//!
//! # Example Test File
//!
//! ```sql
//! -- Setup (no annotation = just execute)
//! CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT);
//! INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob');
//!
//! -- Inline assertions
//! SELECT COUNT(*) FROM users; -- 2
//! SELECT name FROM users WHERE id = 1; -- 'Alice'
//!
//! -- Snapshot assertions
//! SELECT * FROM users ORDER BY id; -- @snap all-users
//! ```

mod parser;
mod report;
pub(crate) mod snap;
mod value;

use console::Style;
use solite_core::dot::DotCommand;
use solite_core::{BlockSource, Runtime, StepResult};
use std::fs::read_to_string;
use std::io::Write as _;

use crate::cli::TestArgs;

use parser::{
    compute_offset_from_reference, parse_epilogue_comment, parse_ref_file_line_col,
    parse_snap_directive,
};
use report::{report_mismatch, TestStats};
use snap::{handle_orphans, handle_snap_assertion, SnapMode, SnapState};
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

    let snap_mode = if args.update {
        SnapMode::Update
    } else if args.review {
        SnapMode::Review
    } else {
        SnapMode::Default
    };
    let mut snap_state = SnapState::new(&source_path, snap_mode);

    let filestem = source_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "test".to_string());

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
                StepResult::ProcedureDefinition(_) => { /* already registered in runtime */ }
                StepResult::SqlStatement { stmt, .. } => {
                    let epilogue = match &step.epilogue {
                        Some(s) => parse_epilogue_comment(s),
                        None => {
                            // No epilogue = setup statement, just execute silently
                            let _ = stmt.execute();
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

                    // Handle @snap directives
                    match parse_snap_directive(&epilogue) {
                        Ok(Some(snap_dir)) => {
                            handle_snap_assertion(
                                &mut snap_state,
                                &stmt,
                                &snap_dir.name,
                                &filestem,
                                &source_path,
                            );
                            let _ = handle.flush();
                            continue;
                        }
                        Ok(None) => {
                            // Not a snap directive, fall through to inline assertion
                        }
                        Err(e) => {
                            stats.record_failure();
                            eprintln!("\n{}", e);
                            print!("{}", Style::new().red().apply_to("x"));
                            let _ = handle.flush();
                            continue;
                        }
                    }

                    // Inline assertion: execute and compare
                    match stmt.next() {
                        Err(err) => {
                            let ref_display = format!("{}", step.reference);
                            let maybe_offset =
                                compute_offset_from_reference(&content, &ref_display);

                            if let Some(expected) = epilogue.strip_prefix("error:").map(str::trim) {
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

    // Handle orphaned snapshots
    handle_orphans(&mut snap_state, &filestem);

    // Print results
    stats.print_summary();
    snap_state.print_summary();

    let has_failures =
        stats.has_failures() || snap_state.has_failures();

    if has_failures {
        if !stats.todos.is_empty() {
            eprintln!(
                "\nThere are {} TODO(s). Treating as failure per policy.",
                stats.todos.len()
            );
        }
        Err(TestError::TestsFailed {
            failures: stats.failures + snap_state.rejected,
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
        DotCommand::Parameter(param_cmd) => {
            if let solite_core::dot::ParameterCommand::Set { key, value } = param_cmd {
                if let Err(e) = rt.define_parameter(key.clone(), value.to_owned()) {
                    eprintln!("Warning: Failed to set parameter {}: {}", key, e);
                }
            }
        }
        DotCommand::Call(_) => { /* resolved to SqlStatement in next_stepx() */ }
        DotCommand::Run(run_cmd) => {
            if let Some(ref proc_name) = run_cmd.procedure {
                for (key, value) in &run_cmd.parameters {
                    if let Err(e) = rt.define_parameter(key.clone(), value.clone()) {
                        eprintln!("Warning: Failed to set parameter {}: {}", key, e);
                    }
                }
                if let Err(e) = rt.load_file(&run_cmd.file) {
                    eprintln!("Warning: Failed to load file '{}': {}", run_cmd.file, e);
                    return;
                }
                let proc = match rt.get_procedure(proc_name) {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("Warning: Unknown procedure: '{}'", proc_name);
                        return;
                    }
                };
                match rt.prepare_with_parameters(&proc.sql) {
                    Ok((_, Some(stmt))) => { let _ = stmt.execute(); }
                    Ok((_, None)) => {
                        eprintln!("Warning: Procedure '{}' prepared to empty statement", proc_name);
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to prepare procedure '{}': {:?}", proc_name, e);
                    }
                }
            } else {
                let saved = match rt.run_file_begin(&run_cmd.file, &run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Warning: {}", e);
                        return;
                    }
                };
                loop {
                    match rt.next_stepx() {
                        None => break,
                        Some(Ok(step)) => match step.result {
                            solite_core::StepResult::SqlStatement { stmt, .. } => {
                                let _ = stmt.execute();
                            }
                            solite_core::StepResult::DotCommand(ref cmd) => {
                                handle_dot_command(cmd, rt);
                            }
                            solite_core::StepResult::ProcedureDefinition(_) => {}
                        },
                        Some(Err(e)) => {
                            eprintln!("Warning: Error in .run file: {}", e);
                            break;
                        }
                    }
                }
                rt.run_file_end(saved);
            }
        }
        #[cfg(feature = "ritestream")]
        DotCommand::Stream(_) => {
            eprintln!("Warning: .stream command not supported in test mode");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "solite-test-impl-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn write_sql(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    fn make_args(file: PathBuf, update: bool, review: bool) -> TestArgs {
        TestArgs {
            file,
            database: None,
            verbose: false,
            update,
            review,
        }
    }

    fn default_args(file: PathBuf) -> TestArgs {
        make_args(file, false, false)
    }

    fn update_args(file: PathBuf) -> TestArgs {
        make_args(file, true, false)
    }

    // ===== Setup-only tests =====

    #[test]
    fn test_setup_only_file_succeeds() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "setup.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (1);
INSERT INTO t VALUES (2);
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_empty_file_succeeds() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "empty.sql", "");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_comments_only_file_succeeds() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "comments.sql", "\
-- This is a comment
-- Another comment
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    // ===== Inline assertion tests =====

    #[test]
    fn test_inline_assertion_passes() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "pass.sql", "SELECT 1 + 1; -- 2\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_inline_assertion_fails() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "fail.sql", "SELECT 1 + 1; -- 3\n");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    #[test]
    fn test_inline_assertion_string_value() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "str.sql", "SELECT 'hello'; -- 'hello'\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_inline_assertion_null_value() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "null.sql", "SELECT NULL; -- NULL\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_inline_assertion_float_value() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "float.sql", "SELECT 3.14; -- 3.14\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_inline_assertion_negative() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "neg.sql", "SELECT -42; -- -42\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_multiple_inline_assertions_all_pass() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "multi.sql", "\
SELECT 1; -- 1
SELECT 2; -- 2
SELECT 3; -- 3
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_multiple_inline_one_fails() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "multi_fail.sql", "\
SELECT 1; -- 1
SELECT 2; -- 999
SELECT 3; -- 3
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    // ===== No results assertion =====

    #[test]
    fn test_no_results_passes() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "noresult.sql", "\
CREATE TABLE t(x);
SELECT * FROM t; -- [no results]
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_no_results_fails_when_rows_exist() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "noresult_fail.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (1);
SELECT * FROM t; -- [no results]
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    // ===== Error assertion =====

    #[test]
    fn test_error_assertion_passes() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "error.sql", "\
CREATE TABLE t(x UNIQUE);
INSERT INTO t VALUES (1);
INSERT INTO t VALUES (1); -- error: UNIQUE constraint failed: t.x
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn test_error_assertion_wrong_message() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "error_wrong.sql", "\
CREATE TABLE t(x UNIQUE);
INSERT INTO t VALUES (1);
INSERT INTO t VALUES (1); -- error: wrong message
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    #[test]
    fn test_unexpected_error_fails() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "unex_err.sql", "\
CREATE TABLE t(x UNIQUE);
INSERT INTO t VALUES (1);
INSERT INTO t VALUES (1); -- 42
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    // ===== TODO annotation =====

    #[test]
    fn test_todo_causes_failure() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "todo.sql", "\
SELECT 1; -- TODO fix this later
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    #[test]
    fn test_todo_case_insensitive() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "todo_case.sql", "\
SELECT 1; -- todo fix this later
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    // ===== Setup before assertion =====

    #[test]
    fn test_setup_then_assertion() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "setup_assert.sql", "\
CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT);
INSERT INTO users VALUES (1, 'Alice');
INSERT INTO users VALUES (2, 'Bob');
SELECT COUNT(*) FROM users; -- 2
SELECT name FROM users WHERE id = 1; -- 'Alice'
");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    // ===== Snapshot: @snap =====

    #[test]
    fn test_snap_update_creates_snapshot() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "snap_new.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (42);
SELECT * FROM t; -- @snap my-snap
");
        let result = test_impl(update_args(file));
        assert!(result.is_ok());

        let snap_path = tmp.join("__snapshots__").join("snap_new-my-snap.snap");
        assert!(snap_path.exists());
        let contents = fs::read_to_string(&snap_path).unwrap();
        assert!(contents.contains("42"));
        assert!(contents.contains("---"));

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_default_mode_fails_on_new() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "snap_new_fail.sql", "\
SELECT 1; -- @snap new-one
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    #[test]
    fn test_snap_matches_existing() {
        let tmp = temp_dir();
        // First run: create snapshot
        let file = write_sql(&tmp, "snap_match.sql", "\
SELECT 1; -- @snap the-value
");
        let result = test_impl(update_args(file.clone()));
        assert!(result.is_ok());

        // Second run: should match (default mode)
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_mismatch_default_mode_fails() {
        let tmp = temp_dir();

        // Create initial snapshot with value 1
        let file = write_sql(&tmp, "snap_mis.sql", "SELECT 1; -- @snap val\n");
        test_impl(update_args(file)).unwrap();

        // Change the query to return 2
        let file = write_sql(&tmp, "snap_mis.sql", "SELECT 2; -- @snap val\n");
        let result = test_impl(default_args(file));
        assert!(result.is_err());

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_mismatch_update_mode_overwrites() {
        let tmp = temp_dir();

        // Create initial snapshot with value 1
        let file = write_sql(&tmp, "snap_upd.sql", "SELECT 1; -- @snap val\n");
        test_impl(update_args(file)).unwrap();

        // Change query to return 2, update mode
        let file = write_sql(&tmp, "snap_upd.sql", "SELECT 2; -- @snap val\n");
        let result = test_impl(update_args(file));
        assert!(result.is_ok());

        // Verify snapshot was updated
        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("snap_upd-val.snap")
        ).unwrap();
        assert!(snap.contains("2"));

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_multiple_in_one_file() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "multi_snap.sql", "\
SELECT 1; -- @snap first
SELECT 2; -- @snap second
SELECT 3; -- @snap third
");
        let result = test_impl(update_args(file.clone()));
        assert!(result.is_ok());

        let snap_dir = tmp.join("__snapshots__");
        assert!(snap_dir.join("multi_snap-first.snap").exists());
        assert!(snap_dir.join("multi_snap-second.snap").exists());
        assert!(snap_dir.join("multi_snap-third.snap").exists());

        // Re-run in default mode should match
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_multi_row_result() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "rows.sql", "\
CREATE TABLE t(id INTEGER, name TEXT);
INSERT INTO t VALUES (1, 'Alice');
INSERT INTO t VALUES (2, 'Bob');
SELECT * FROM t ORDER BY id; -- @snap all-rows
");
        test_impl(update_args(file.clone())).unwrap();

        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("rows-all-rows.snap")
        ).unwrap();
        assert!(snap.contains("Alice"));
        assert!(snap.contains("Bob"));
        assert!(snap.contains("id:"));
        assert!(snap.contains("name:"));

        // Re-run should match
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_error_result() {
        let tmp = temp_dir();
        // Use a CHECK constraint violation which fails at execution time, not prepare time
        let file = write_sql(&tmp, "snap_err.sql", "\
CREATE TABLE t(x INTEGER CHECK(x > 0));
INSERT INTO t VALUES (-1); -- @snap error-case
");
        test_impl(update_args(file.clone())).unwrap();

        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("snap_err-error-case.snap")
        ).unwrap();
        assert!(snap.contains("ERROR"));
        assert!(snap.contains("CHECK constraint"));

        // Re-run should match
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_no_results() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "snap_empty.sql", "\
CREATE TABLE t(x);
SELECT * FROM t; -- @snap empty-table
");
        test_impl(update_args(file.clone())).unwrap();

        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("snap_empty-empty-table.snap")
        ).unwrap();
        assert!(snap.contains("[no results]"));

        // Re-run matches
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    // ===== @snap naming validation =====

    #[test]
    fn test_snap_without_name_fails() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "no_name.sql", "SELECT 1; -- @snap\n");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    #[test]
    fn test_snap_invalid_name_fails() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "bad_name.sql", "SELECT 1; -- @snap foo.bar\n");
        let result = test_impl(default_args(file));
        assert!(result.is_err());
        cleanup(&tmp);
    }

    // ===== Mixed inline + snapshot =====

    #[test]
    fn test_mixed_inline_and_snap() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "mixed.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (1);
INSERT INTO t VALUES (2);
SELECT COUNT(*) FROM t; -- 2
SELECT * FROM t ORDER BY x; -- @snap all-rows
SELECT 1 + 1; -- 2
");
        let result = test_impl(update_args(file.clone()));
        assert!(result.is_ok());

        // Re-run in default mode
        let result = test_impl(default_args(file));
        assert!(result.is_ok());

        cleanup(&tmp);
    }

    #[test]
    fn test_mixed_inline_fails_snap_passes() {
        let tmp = temp_dir();
        // First create the snapshot
        let file = write_sql(&tmp, "mixed_fail.sql", "\
SELECT 1; -- @snap val
SELECT 1; -- 1
");
        test_impl(update_args(file)).unwrap();

        // Now change inline assertion to fail
        let file = write_sql(&tmp, "mixed_fail.sql", "\
SELECT 1; -- @snap val
SELECT 1; -- 999
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());

        cleanup(&tmp);
    }

    #[test]
    fn test_mixed_snap_fails_inline_passes() {
        let tmp = temp_dir();
        // Create snapshot
        let file = write_sql(&tmp, "mixed_snap_fail.sql", "\
SELECT 1; -- 1
SELECT 1; -- @snap val
");
        test_impl(update_args(file)).unwrap();

        // Change snap query
        let file = write_sql(&tmp, "mixed_snap_fail.sql", "\
SELECT 1; -- 1
SELECT 999; -- @snap val
");
        let result = test_impl(default_args(file));
        assert!(result.is_err());

        cleanup(&tmp);
    }

    // ===== Orphan detection in integration =====

    #[test]
    fn test_orphan_deleted_in_update_mode() {
        let tmp = temp_dir();

        // Create two snapshots
        let file = write_sql(&tmp, "orphan.sql", "\
SELECT 1; -- @snap keep
SELECT 2; -- @snap remove-me
");
        test_impl(update_args(file)).unwrap();

        let snap_dir = tmp.join("__snapshots__");
        assert!(snap_dir.join("orphan-keep.snap").exists());
        assert!(snap_dir.join("orphan-remove-me.snap").exists());

        // Remove one snap from the file
        let file = write_sql(&tmp, "orphan.sql", "\
SELECT 1; -- @snap keep
");
        test_impl(update_args(file)).unwrap();

        assert!(snap_dir.join("orphan-keep.snap").exists());
        assert!(!snap_dir.join("orphan-remove-me.snap").exists());

        cleanup(&tmp);
    }

    #[test]
    fn test_orphan_not_deleted_in_default_mode() {
        let tmp = temp_dir();

        // Create two snapshots
        let file = write_sql(&tmp, "orphan_def.sql", "\
SELECT 1; -- @snap keep
SELECT 2; -- @snap to-orphan
");
        test_impl(update_args(file)).unwrap();

        // Remove one snap from the file
        let file = write_sql(&tmp, "orphan_def.sql", "\
SELECT 1; -- @snap keep
");
        test_impl(default_args(file)).unwrap();

        // Orphan should still exist in default mode
        let snap_dir = tmp.join("__snapshots__");
        assert!(snap_dir.join("orphan_def-to-orphan.snap").exists());

        cleanup(&tmp);
    }

    // ===== File not found =====

    #[test]
    fn test_nonexistent_file_error() {
        let result = test_impl(default_args(PathBuf::from("/nonexistent/test.sql")));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestError::FileRead(msg) => assert!(msg.contains("nonexistent")),
            other => panic!("Expected FileRead, got {:?}", other),
        }
    }

    // ===== Snapshot directory creation =====

    #[test]
    fn test_snap_creates_snapshots_dir() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "dir_create.sql", "SELECT 1; -- @snap val\n");

        let snap_dir = tmp.join("__snapshots__");
        assert!(!snap_dir.exists());

        test_impl(update_args(file)).unwrap();
        assert!(snap_dir.exists());
        assert!(snap_dir.is_dir());

        cleanup(&tmp);
    }

    // ===== Snapshot file format =====

    #[test]
    fn test_snap_file_format_has_source_header() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "fmt.sql", "SELECT 42; -- @snap val\n");
        test_impl(update_args(file)).unwrap();

        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("fmt-val.snap")
        ).unwrap();

        assert!(snap.starts_with("Source: "));
        assert!(snap.contains("fmt.sql"));
        assert!(snap.contains("---"));
        assert!(snap.contains("42"));

        cleanup(&tmp);
    }

    #[test]
    fn test_snap_file_naming_convention() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "my-test-file.sql", "SELECT 1; -- @snap foo\n");
        test_impl(update_args(file)).unwrap();

        // Should be {filestem}-{name}.snap
        let expected = tmp.join("__snapshots__").join("my-test-file-foo.snap");
        assert!(expected.exists());

        cleanup(&tmp);
    }

    // ===== Block comment syntax =====

    #[test]
    fn test_inline_assertion_block_comment() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "block.sql", "SELECT 42; /* 42 */\n");
        let result = test_impl(default_args(file));
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    // ===== Multiple passes =====

    #[test]
    fn test_snap_stable_across_reruns() {
        let tmp = temp_dir();
        let sql = "\
CREATE TABLE t(x);
INSERT INTO t VALUES (1);
SELECT * FROM t; -- @snap stable
";
        let file = write_sql(&tmp, "stable.sql", sql);

        // Create
        test_impl(update_args(file.clone())).unwrap();

        // Verify 3 consecutive default runs all pass
        for _ in 0..3 {
            let file = write_sql(&tmp, "stable.sql", sql);
            assert!(test_impl(default_args(file)).is_ok());
        }

        cleanup(&tmp);
    }

    // ===== Snapshot with setup statements =====

    #[test]
    fn test_snap_with_complex_setup() {
        let tmp = temp_dir();
        let file = write_sql(&tmp, "complex.sql", "\
CREATE TABLE categories(id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE products(id INTEGER PRIMARY KEY, name TEXT, category_id INTEGER);
INSERT INTO categories VALUES (1, 'Electronics');
INSERT INTO categories VALUES (2, 'Books');
INSERT INTO products VALUES (1, 'Laptop', 1);
INSERT INTO products VALUES (2, 'Phone', 1);
INSERT INTO products VALUES (3, 'Novel', 2);

SELECT c.name, COUNT(*) as cnt
FROM products p
JOIN categories c ON p.category_id = c.id
GROUP BY c.name
ORDER BY c.name; -- @snap category-counts
");
        test_impl(update_args(file.clone())).unwrap();

        let snap = fs::read_to_string(
            tmp.join("__snapshots__").join("complex-category-counts.snap")
        ).unwrap();
        assert!(snap.contains("Books"));
        assert!(snap.contains("Electronics"));

        // Re-run should match
        assert!(test_impl(default_args(file)).is_ok());

        cleanup(&tmp);
    }

    // ===== Snapshot update changes content =====

    #[test]
    fn test_snap_update_reflects_data_change() {
        let tmp = temp_dir();

        // Version 1
        let file = write_sql(&tmp, "evolve.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (1);
SELECT * FROM t; -- @snap data
");
        test_impl(update_args(file)).unwrap();
        let snap1 = fs::read_to_string(
            tmp.join("__snapshots__").join("evolve-data.snap")
        ).unwrap();

        // Version 2 - different data
        let file = write_sql(&tmp, "evolve.sql", "\
CREATE TABLE t(x);
INSERT INTO t VALUES (99);
SELECT * FROM t; -- @snap data
");
        test_impl(update_args(file)).unwrap();
        let snap2 = fs::read_to_string(
            tmp.join("__snapshots__").join("evolve-data.snap")
        ).unwrap();

        assert_ne!(snap1, snap2);
        assert!(snap1.contains("1"));
        assert!(snap2.contains("99"));

        cleanup(&tmp);
    }
}
