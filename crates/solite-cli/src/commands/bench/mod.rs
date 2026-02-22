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

use std::path::Path;

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

/// Error type for benchmark operations.
#[derive(Debug)]
#[allow(dead_code)]
pub enum BenchError {
    /// Failed to load an extension.
    ExtensionLoad(String),
    /// Failed to open a database.
    DatabaseOpen(String),
    /// Failed to read a SQL file.
    FileRead(String),
    /// Failed to prepare a SQL statement.
    PrepareStatement(String),
    /// Failed to execute a SQL statement.
    ExecuteStatement(String),
}

impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchError::ExtensionLoad(msg) => write!(f, "Failed to load extension: {}", msg),
            BenchError::DatabaseOpen(msg) => write!(f, "Failed to open database: {}", msg),
            BenchError::FileRead(msg) => write!(f, "Failed to read file: {}", msg),
            BenchError::PrepareStatement(msg) => write!(f, "Failed to prepare statement: {}", msg),
            BenchError::ExecuteStatement(msg) => write!(f, "Failed to execute statement: {}", msg),
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
fn resolve_sql(sql: &str) -> Result<String, BenchError> {
    if sql.ends_with(".sql") && Path::new(sql).exists() {
        std::fs::read_to_string(sql).map_err(|e| BenchError::FileRead(e.to_string()))
    } else {
        Ok(sql.to_owned())
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
    let mut runtime = Runtime::new(None);

    // Load extensions for the runtime connection
    if let Some(ref extensions) = args.load_extension {
        if let Err(e) = load_extensions(&runtime.connection, extensions) {
            eprintln!("{}", e);
            return Err(());
        }
    }

    let pb = create_progress_bar();

    for (idx, sql_arg) in args.sql.iter().enumerate() {
        // Set up connection for this query
        if let Some(databases) = &args.database {
            let db_path = match databases.get(idx) {
                Some(p) => p,
                None => {
                    eprintln!("No database specified for SQL argument {}", idx);
                    return Err(());
                }
            };

            let path_str = match db_path.as_os_str().to_str() {
                Some(s) => s,
                None => {
                    eprintln!("Invalid database path: {}", db_path.display());
                    return Err(());
                }
            };

            let conn = match Connection::open(path_str) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to open database {}: {:?}", db_path.display(), e);
                    return Err(());
                }
            };

            if let Some(ref extensions) = args.load_extension {
                if let Err(e) = load_extensions(&conn, extensions) {
                    eprintln!("{}", e);
                    return Err(());
                }
            }

            runtime.connection = conn;
        } else {
            pb.set_message("Using in-memory database");
        }

        // Resolve SQL (from file or direct)
        let sql = match resolve_sql(sql_arg) {
            Ok(s) => {
                if sql_arg.ends_with(".sql") {
                    pb.set_message(format!("Reading SQL file: {}", sql_arg));
                } else {
                    pb.set_message(format!("SQL: {}", sql_arg));
                }
                s
            }
            Err(e) => {
                eprintln!("{}", e);
                return Err(());
            }
        };

        // Prepare the statement
        let stmt = match runtime.connection.prepare(&sql) {
            Ok((_, Some(stmt))) => stmt,
            Ok((_, None)) => {
                eprintln!("Failed to prepare statement: no statement returned");
                return Err(());
            }
            Err(e) => {
                eprintln!("Failed to prepare statement: {:?}", e);
                return Err(());
            }
        };

        // Run benchmark iterations
        let mut times = vec![];
        let mut steps = vec![];
        pb.reset();
        pb.set_length(10);

        for _ in 0..10 {
            pb.inc(1);
            let tn = jiff::Timestamp::now();

            if let Err(e) = stmt.execute() {
                eprintln!("Failed to execute statement: {:?}", e);
                return Err(());
            }
            stmt.reset();

            times.push(jiff::Timestamp::now() - tn);

            if let Some(avg) = average(&times) {
                pb.set_message(format!(
                    "Current estimate: {}",
                    format_runtime(avg)
                        .as_str()
                        .with(crate::themes::ctp_mocha_colors::GREEN.clone().into())
                ));
            }

            steps = unsafe { bytecode_steps(stmt.pointer()) };
        }

        pb.finish_and_clear();
        print_results(&sql, &times, steps);
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
        // Should return the string as-is since file doesn't exist
        let result = resolve_sql("nonexistent.sql").unwrap();
        assert_eq!(result, "nonexistent.sql");
    }
}
