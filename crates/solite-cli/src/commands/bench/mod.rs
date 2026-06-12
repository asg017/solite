//! SQL benchmark execution and reporting.
//!
//! This module provides functionality to benchmark SQL queries and report
//! timing statistics including mean, standard deviation, and min/max times.
//!
//! # Features
//!
//! - Benchmark SQL statements or SQL files
//! - Support for custom databases and extensions
//! - Progress bar during benchmark execution
//! - Bytecode step visualization
//! - Statistical summary (mean ± σ, min … max)
//!
//! # Example Usage
//!
//! ```bash
//! # Benchmark a SQL statement
//! solite bench "SELECT * FROM users"
//!
//! # Benchmark with a database
//! solite bench --database mydb.db "SELECT count(*) FROM orders"
//!
//! # Benchmark a SQL file
//! solite bench query.sql
//! ```

mod format;
mod stats;

use crossterm::style::Stylize;
use indicatif::ProgressBar;
use solite_core::{
    dot::bench::render_steps,
    sqlite::{bytecode_steps, Connection},
    Runtime,
};

use crate::cli::BenchArgs;

use format::format_runtime;
use stats::{average, max, min, stddev};

/// Error type for benchmark operations. Open/prepare/execute errors are
/// reported through `anyhow` in `bench_impl`.
#[derive(Debug)]
pub enum BenchError {
    /// Failed to load an extension.
    ExtensionLoad(String),
    /// Failed to read a SQL file.
    FileRead(String),
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::ExtensionLoad(msg) => write!(f, "Failed to load extension: {}", msg),
            BenchError::FileRead(msg) => write!(f, "Failed to read file: {}", msg),
        }
    }
}

impl std::error::Error for BenchError {}

/// Load extensions into a connection.
fn load_extensions(
    conn: &Connection,
    extensions: &[std::path::PathBuf],
) -> Result<(), BenchError> {
    for extension in extensions {
        conn.load_extension(&extension.as_os_str().to_string_lossy(), &None)
            .map_err(|err| {
                BenchError::ExtensionLoad(format!("{}: {}", extension.display(), err))
            })?;
    }
    Ok(())
}

/// Read SQL from a file or return the string as-is.
///
/// An argument ending in `.sql` is always treated as a file path — a typo'd
/// path is a file-not-found error, never benched as literal SQL.
fn resolve_sql(sql: &str) -> Result<String, BenchError> {
    if sql.ends_with(".sql") {
        std::fs::read_to_string(sql).map_err(|e| BenchError::FileRead(format!("{sql}: {e}")))
    } else {
        Ok(sql.to_owned())
    }
}

/// Returns true if `sql` contains at least one executable statement.
///
/// Used to peek ahead at the tail that follows a prepared statement: a tail
/// holding only comments, whitespace, or bare semicolons has no statement, so
/// the *current* statement is the bench target rather than setup. A prepare
/// error counts as "has a statement" — the tail may reference objects the
/// setup statements haven't created yet; any real error surfaces when the
/// statement is prepared for execution.
fn has_statement(conn: &Connection, sql: &str) -> bool {
    let mut slice = sql;
    loop {
        match conn.prepare(slice) {
            Ok((_, Some(_))) => return true,
            Ok((Some(offset), None)) if offset > 0 => slice = &slice[offset..],
            Ok((_, None)) => return false,
            Err(_) => return true,
        }
    }
}

/// Create progress bar with standard styling.
fn create_progress_bar() -> ProgressBar {
    let pb = ProgressBar::new(1);
    if let Ok(style) = indicatif::ProgressStyle::with_template(
        " {spinner} {msg:<30} {wide_bar} ETA {eta_precise} ",
    ) {
        pb.set_style(style);
    }
    pb
}

/// Print benchmark results for a single SQL query.
fn print_results(sql: &str, times: &[jiff::Span], steps: Vec<solite_core::sqlite::BytecodeStep>) {
    let avg = average(times)
        .map(format_runtime)
        .unwrap_or_else(|| "N/A".to_string());
    let std = stddev(times)
        .map(format_runtime)
        .unwrap_or_else(|| "N/A".to_string());
    let mn = min(times)
        .map(format_runtime)
        .unwrap_or_else(|| "N/A".to_string());
    let mx = max(times)
        .map(format_runtime)
        .unwrap_or_else(|| "N/A".to_string());

    println!("{sql}:");
    println!(
        "  Time  ({} ± {}):  {} ± {}",
        "mean"
            .with(crate::themes::ctp_mocha_colors::GREEN.clone().into())
            .bold(),
        "σ".with(crate::themes::ctp_mocha_colors::GREEN.clone().into()),
        avg.as_str()
            .with(crate::themes::ctp_mocha_colors::GREEN.clone().into())
            .bold(),
        std.as_str()
            .with(crate::themes::ctp_mocha_colors::GREEN.clone().into()),
    );
    println!(
        "  Range ({} … {}):  {} … {}",
        "min".with(crate::themes::ctp_mocha_colors::SKY.clone().into()),
        "max".with(crate::themes::ctp_mocha_colors::MAUVE.clone().into()),
        mn.as_str()
            .with(crate::themes::ctp_mocha_colors::SKY.clone().into()),
        mx.as_str()
            .with(crate::themes::ctp_mocha_colors::MAUVE.clone().into()),
    );
    println!("{}", render_steps(steps));
}

/// Entry point for the bench command.
pub fn bench(args: BenchArgs) -> Result<(), ()> {
    bench_impl(args).map_err(|e| eprintln!("Error: {e:?}"))
}

fn bench_impl(args: BenchArgs) -> anyhow::Result<()> {
    let mut runtime = Runtime::new(None)?;

    // Load extensions for the runtime connection
    if let Some(ref extensions) = args.load_extension {
        load_extensions(&runtime.connection, extensions)?;
    }

    let pb = create_progress_bar();

    for (idx, sql_arg) in args.sql.iter().enumerate() {
        // Set up connection for this query
        if let Some(databases) = &args.database {
            let db_path = databases.get(idx)
                .ok_or_else(|| anyhow::anyhow!("No database specified for SQL argument {}", idx))?;

            let path_str = db_path.as_os_str().to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid database path: {}", db_path.display()))?;

            let conn = Connection::open(path_str)?;

            if let Some(ref extensions) = args.load_extension {
                load_extensions(&conn, extensions)?;
            }

            runtime.connection = conn;
        } else {
            pb.set_message("Using in-memory database");
        }

        // Resolve SQL (from file or direct)
        let sql = resolve_sql(sql_arg)?;
        if sql_arg.ends_with(".sql") {
            pb.set_message(format!("Reading SQL file: {}", sql_arg));
        } else {
            pb.set_message(format!("SQL: {}", sql_arg));
        }

        // Prepare the statement. Multi-statement input (e.g. a .sql file
        // with setup + query) runs the leading statements once, untimed,
        // and benches the last one.
        let mut setup_count = 0usize;
        let mut sql_slice: &str = &sql;
        let stmt = loop {
            let (remaining, stmt) = runtime.connection.prepare(sql_slice)?;
            match (remaining, stmt) {
                (Some(offset), Some(stmt)) => {
                    let rest = &sql_slice[offset..];
                    // Peek ahead: only run this statement as untimed setup
                    // if another statement actually follows. A tail that is
                    // only comments/whitespace (e.g. `SELECT ...;\n-- done`)
                    // means this statement is the bench target.
                    if has_statement(&runtime.connection, rest) {
                        stmt.execute()?;
                        setup_count += 1;
                        sql_slice = rest;
                    } else {
                        break stmt;
                    }
                }
                // consumed chunk held no statement (e.g. a bare `;`)
                (Some(offset), None) => sql_slice = &sql_slice[offset..],
                (None, Some(stmt)) => break stmt,
                (None, None) => {
                    return Err(anyhow::anyhow!(
                        "no SQL statement to benchmark in '{}'",
                        sql_arg
                    ))
                }
            }
        };
        if setup_count > 0 {
            println!(
                "ran {setup_count} setup statement{}",
                if setup_count == 1 { "" } else { "s" }
            );
        }

        // Run benchmark iterations
        let mut times = vec![];
        let mut steps = vec![];
        pb.reset();
        pb.set_length(10);

        for _ in 0..10 {
            pb.inc(1);
            let tn = jiff::Timestamp::now();
            stmt.execute()?;
            times.push(jiff::Timestamp::now() - tn);

            stmt.reset();

            if let Some(avg) = average(&times) {
                pb.set_message(format!(
                    "Current estimate: {}",
                    format_runtime(avg)
                        .as_str()
                        .with(crate::themes::ctp_mocha_colors::GREEN.clone().into())
                ));
            }

            steps = unsafe { bytecode_steps(stmt.pointer()) }?;
        }

        pb.finish_and_clear();
        // Label results with the statement actually benched, not the whole
        // input (which may include setup statements).
        let benched_sql = stmt.sql();
        print_results(benched_sql.trim(), &times, steps);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_sql_direct() {
        let result = resolve_sql("SELECT 1").unwrap();
        assert_eq!(result, "SELECT 1");
    }

    #[test]
    fn test_resolve_sql_nonexistent_file() {
        // A .sql argument is always a file path: a missing file is an
        // error naming the path, never benched as literal SQL.
        let err = resolve_sql("nonexistent.sql").unwrap_err();
        assert!(
            err.to_string().contains("nonexistent.sql"),
            "got: {err}"
        );
    }

    #[test]
    fn test_resolve_sql_directory_named_dot_sql() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queries.sql");
        std::fs::create_dir(&path).unwrap();
        let err = resolve_sql(path.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("queries.sql"), "got: {err}");
    }

    #[test]
    fn test_has_statement_comment_only_tail() {
        let conn = Connection::open(":memory:").unwrap();
        assert!(!has_statement(&conn, "-- done"));
        assert!(!has_statement(&conn, "  \n\t"));
        assert!(!has_statement(&conn, "/* block comment */"));
        assert!(!has_statement(&conn, "; -- done"));
        assert!(!has_statement(&conn, ""));
    }

    #[test]
    fn test_has_statement_real_statement() {
        let conn = Connection::open(":memory:").unwrap();
        assert!(has_statement(&conn, "SELECT 1;"));
        assert!(has_statement(&conn, "-- comment\nSELECT 1;"));
        assert!(has_statement(&conn, "; SELECT 1; -- done"));
    }

    #[test]
    fn test_has_statement_prepare_error_counts_as_statement() {
        // A statement that can't compile yet (e.g. it depends on a table a
        // setup statement will create) still counts as a statement.
        let conn = Connection::open(":memory:").unwrap();
        assert!(has_statement(&conn, "INSERT INTO not_yet_created VALUES (1);"));
    }

    #[test]
    fn test_resolve_sql_reads_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("query.sql");
        std::fs::write(&path, "SELECT 1;").unwrap();
        let result = resolve_sql(path.to_str().unwrap()).unwrap();
        assert_eq!(result, "SELECT 1;");
    }
}
