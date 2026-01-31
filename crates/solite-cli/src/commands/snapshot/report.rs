//! Report generation for snapshot test results.

use super::state::{SnapshotResult, SnapshotState};

const SNAPSHOT_FUNCTIONS_REPORT_SQL: &str = include_str!("snapshot-functions-report.sql");
const SNAPSHOT_MODULES_REPORT_SQL: &str = include_str!("snapshot-modules-report.sql");

/// Report on extension loading (functions and modules).
pub struct ExtensionsReport {
    pub num_functions_loaded: usize,
    pub missing_functions: Vec<String>,
    pub num_modules_loaded: usize,
    pub missing_modules: Vec<String>,
}

/// Summary report of snapshot test results.
pub struct Report {
    pub num_matches: usize,
    pub num_updated: usize,
    pub num_rejected: usize,
    pub num_removed: usize,
    pub extensions_report: Option<ExtensionsReport>,
}

impl Report {
    /// Print the report to stdout.
    pub fn print(&self) {
        println!(
            "{:>4} passing snapshot{}",
            self.num_matches,
            if self.num_matches == 1 { "" } else { "s" }
        );

        if self.num_updated > 0 {
            println!(
                "{:>4} updated snapshot{}",
                self.num_updated,
                if self.num_updated == 1 { "" } else { "s" }
            );
        }

        if self.num_rejected > 0 {
            println!(
                "{:>4} rejected snapshot{}",
                self.num_rejected,
                if self.num_rejected == 1 { "" } else { "s" }
            );
        }

        if self.num_removed > 0 {
            println!(
                "{:>4} removed snapshot{}",
                self.num_removed,
                if self.num_removed == 1 { "" } else { "s" }
            );
        }

        if let Some(report) = &self.extensions_report {
            let total_functions = report.num_functions_loaded + report.missing_functions.len();
            println!(
                "{}/{} functions loaded from extension",
                report.num_functions_loaded, total_functions
            );

            if !report.missing_functions.is_empty() {
                println!(
                    "{} function{} missing from extension",
                    report.missing_functions.len(),
                    if report.missing_functions.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
                for missing in &report.missing_functions {
                    println!("  - {}", missing);
                }
            }

            let total_modules = report.num_modules_loaded + report.missing_modules.len();
            println!(
                "{}/{} modules tested{}",
                report.num_modules_loaded,
                total_modules,
                if report.missing_modules.is_empty() {
                    ""
                } else {
                    ", missing:"
                }
            );
            for missing in &report.missing_modules {
                println!("  - {}", missing);
            }
        }
    }
}

/// Generate a report from the current snapshot state.
pub fn generate_report(state: &SnapshotState) -> Report {
    let num_matches = state
        .results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Matches))
        .count();
    let num_updated = state
        .results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Accepted))
        .count();
    let num_rejected = state
        .results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Rejected))
        .count();
    let num_removed = state
        .results
        .iter()
        .filter(|v| matches!(v, SnapshotResult::Removed))
        .count();

    let extensions_report = if state.loaded_extension {
        build_extensions_report(state)
    } else {
        None
    };

    Report {
        num_matches,
        num_updated,
        num_rejected,
        num_removed,
        extensions_report,
    }
}

fn build_extensions_report(state: &SnapshotState) -> Option<ExtensionsReport> {
    // Get functions report
    let stmt = match state.runtime.connection.prepare(SNAPSHOT_FUNCTIONS_REPORT_SQL) {
        Ok((_, Some(stmt))) => stmt,
        _ => {
            eprintln!("Warning: Failed to prepare functions report query");
            return None;
        }
    };

    let row = match stmt.nextx() {
        Ok(Some(row)) => row,
        _ => {
            eprintln!("Warning: Failed to get functions report");
            return None;
        }
    };

    let num_functions_loaded = row.value_at(0).as_int64() as usize;
    let missing_functions: Vec<String> =
        serde_json::from_str(row.value_at(1).as_str()).unwrap_or_default();
    drop(stmt);

    // Get modules report
    let stmt = match state.runtime.connection.prepare(SNAPSHOT_MODULES_REPORT_SQL) {
        Ok((_, Some(stmt))) => stmt,
        _ => {
            eprintln!("Warning: Failed to prepare modules report query");
            return None;
        }
    };

    let row = match stmt.nextx() {
        Ok(Some(row)) => row,
        _ => {
            eprintln!("Warning: Failed to get modules report");
            return None;
        }
    };

    let num_modules_loaded = row.value_at(0).as_int64() as usize;
    let missing_modules: Vec<String> =
        serde_json::from_str(row.value_at(1).as_str()).unwrap_or_default();

    Some(ExtensionsReport {
        num_functions_loaded,
        missing_functions,
        num_modules_loaded,
        missing_modules,
    })
}
