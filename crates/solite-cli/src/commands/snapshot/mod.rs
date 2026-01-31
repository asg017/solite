//! Snapshot testing for SQL scripts.

mod diff;
mod file;
mod report;
mod state;
mod value;

// Re-export for use by docs.rs
pub use value::{copy, snapshot_value, ValueCopy, ValueCopyValue};

use crate::cli::SnapNamespace;
use console::{Key, Term};
use indicatif::HumanBytes;
use solite_core::Runtime;
use std::collections::HashSet;
use std::env;
use std::path::Path;

use diff::print_diff;
use file::snapshot_file;
use report::generate_report;
use state::{
    SnapshotResult, SnapshotState, SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE,
    SNAPPED_STATEMENT_CREATE,
};

/// Entry point for snapshot command.
pub(crate) fn snapshot(cmd: SnapNamespace) -> Result<(), ()> {
    let is_review = matches!(cmd.command, crate::cli::SnapCommand::Review(_));
    let args = match cmd.command {
        crate::cli::SnapCommand::Test(args) => args,
        crate::cli::SnapCommand::Review(args) => args,
    };

    let rt = Runtime::new(None);

    // Attach in-memory database for snapshot tracking
    if let Err(e) = rt
        .connection
        .execute("ATTACH DATABASE ':memory:' AS solite_snapshot")
    {
        eprintln!("Failed to attach snapshot database: {:?}", e);
        return Err(());
    }

    if let Err(e) = rt.connection.execute(SNAPPED_STATEMENT_CREATE) {
        eprintln!("Failed to create snapped statement table: {:?}", e);
        return Err(());
    }

    if let Err(e) = rt.connection.execute(SNAPPED_STATEMENT_BYTECODE_STEPS_CREATE) {
        eprintln!("Failed to create bytecode steps table: {:?}", e);
        return Err(());
    }

    let snapshots_dir = match env::var("SOLITE_SNAPSHOT_DIRECTORY") {
        Ok(v) => Path::new(&v).to_path_buf(),
        Err(_) => {
            if args.file.is_dir() {
                args.file.join("__snapshots__")
            } else {
                args.file
                    .parent()
                    .map(|p| p.join("__snapshots__"))
                    .unwrap_or_else(|| Path::new("__snapshots__").to_path_buf())
            }
        }
    };

    if !snapshots_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&snapshots_dir) {
            eprintln!(
                "Failed to create snapshots directory {}: {}",
                snapshots_dir.display(),
                e
            );
            return Err(());
        }
    }

    let preexisting_snapshots: Vec<String> = match std::fs::read_dir(&snapshots_dir) {
        Ok(entries) => entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.file_type().ok()?.is_file() {
                    entry.file_name().to_str().map(|s| s.to_owned())
                } else {
                    None
                }
            })
            .collect(),
        Err(e) => {
            eprintln!(
                "Failed to read snapshots directory {}: {}",
                snapshots_dir.display(),
                e
            );
            return Err(());
        }
    };

    let scripts = if args.file.is_dir() {
        let mut scripts = vec![];
        let entries = match std::fs::read_dir(&args.file) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to read directory {}: {}", args.file.display(), e);
                return Err(());
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Warning: Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    eprintln!("Warning: Failed to get file type: {}", e);
                    continue;
                }
            };

            if file_type.is_file() {
                let path = entry.path();
                if path.extension().is_some_and(|s| s == "sql") {
                    scripts.push(path);
                }
            }
        }

        // Sort with _init.sql first
        scripts.sort_by(|a, b| {
            let a_is_init = a
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|s| s == "_init.sql" || s.ends_with("/_init.sql"));
            let b_is_init = b
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|s| s == "_init.sql" || s.ends_with("/_init.sql"));

            match (a_is_init, b_is_init) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });
        scripts
    } else {
        vec![args.file.clone()]
    };

    let mut state = SnapshotState {
        runtime: rt,
        snapshots_dir,
        generated_snapshots: vec![],
        results: vec![],
        is_review,
        verbose: args.verbose,
        loaded_extension: false,
    };

    for script in scripts {
        snapshot_file(&mut state, &script)?;
    }

    // Check for removed snapshots
    let generated_set: HashSet<String> = state.generated_snapshots.iter().cloned().collect();
    let preexisting_set: HashSet<String> = preexisting_snapshots.into_iter().collect();
    let removed = preexisting_set.difference(&generated_set);

    if is_review {
        for snapshot_name in removed {
            let snapshot_path = state.snapshots_dir.join(snapshot_name);
            let contents = match std::fs::read_to_string(&snapshot_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to read {}: {}", snapshot_path.display(), e);
                    continue;
                }
            };

            print_diff(&contents, "");
            println!("Remove {}? [y/n]", snapshot_path.display());

            let term = Term::stdout();
            let key = match term.read_key() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("Failed to read key: {}", e);
                    continue;
                }
            };

            match key {
                Key::Char('y') | Key::Char('Y') => {
                    if let Err(e) = std::fs::remove_file(&snapshot_path) {
                        eprintln!("Failed to remove {}: {}", snapshot_path.display(), e);
                    } else {
                        state.results.push(SnapshotResult::Removed);
                    }
                }
                Key::Char('n') | Key::Char('N') | Key::Escape => {
                    println!("Keeping {}", snapshot_path.display());
                }
                _ => {
                    eprintln!("Unknown input, keeping snapshot");
                }
            }
        }
    }

    let report = generate_report(&state);
    report.print();

    // Write trace output if requested
    if let Some(output) = &args.trace {
        if output.exists() {
            if let Err(e) = std::fs::remove_file(output) {
                eprintln!("Warning: Failed to remove existing trace file: {}", e);
            }
        }

        let stmt = match state
            .runtime
            .connection
            .prepare("vacuum solite_snapshot into ?")
        {
            Ok((_, Some(stmt))) => stmt,
            Ok((_, None)) => {
                eprintln!("Failed to prepare vacuum statement");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Failed to prepare vacuum: {:?}", e);
                return Ok(());
            }
        };

        let output_str = output.to_string_lossy();
        stmt.bind_text(1, &output_str);

        if let Err(e) = stmt.execute() {
            eprintln!("Failed to write trace output: {:?}", e);
            return Ok(());
        }

        match output.metadata() {
            Ok(meta) => {
                println!(
                    "Wrote tracing output to {} ({})",
                    output.display(),
                    HumanBytes(meta.len())
                );
            }
            Err(e) => {
                eprintln!("Warning: Failed to get trace file size: {}", e);
                println!("Wrote tracing output to {}", output.display());
            }
        }
    }

    Ok(())
}
