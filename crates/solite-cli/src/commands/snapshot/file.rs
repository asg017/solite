//! File-level snapshot operations.

use console::{Key, Term};
use solite_core::sqlite::Statement;
use solite_core::{advance_through_ignorable, BlockSource, StepError, StepResult};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::read_to_string;
use std::io::Write as _;
use std::path::PathBuf;

use super::diff::{print_decision, print_diff};
use super::state::{
    register_statement, register_stmt_bytecode, SnapshotResult, SnapshotState,
    BASE_FUNCTIONS_CREATE, BASE_MODULES_CREATE, LOADED_FUNCTIONS_CREATE, LOADED_MODULES_CREATE,
};
use super::value::{copy, snapshot_value, ValueCopy};

/// Dedent a string by removing common leading whitespace.
pub fn dedent(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();

    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line
            }
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Generate snapshot contents from a SQL statement execution.
fn generate_snapshot_contents(source: String, stmt: &Statement) -> Option<String> {
    let mut snapshot_contents = String::new();
    let sql = stmt.sql();
    if write!(
        &mut snapshot_contents,
        "Source: {}\n{}\n---\n",
        source,
        dedent(advance_through_ignorable(&sql))
    )
    .is_err()
    {
        eprintln!("Warning: Failed to write snapshot header");
        return None;
    }

    let columns = match stmt.column_names() {
        Ok(cols) => cols,
        Err(e) => {
            eprintln!("Warning: Failed to get column names: {}", e);
            return None;
        }
    };

    let mut results: Vec<Vec<ValueCopy>> = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let row = row.iter().map(copy).collect();
                results.push(row);
            }
            Ok(None) => break,
            Err(err) => {
                let _ = writeln!(
                    &mut snapshot_contents,
                    "ERROR[{}] {}\n{}",
                    err.result_code, err.code_description, err.message
                );
                return Some(snapshot_contents);
            }
        }
    }

    // single value result (ex `select 1`)
    if columns.len() == 1 && results.len() == 1 {
        let _ = write!(&mut snapshot_contents, "{}", snapshot_value(&results[0][0]));
    }
    // no columns and no results (ex `create table foo`)
    else if columns.is_empty() && results.is_empty() {
        return None;
    }
    // no row results (but still had columns)
    else if results.is_empty() {
        let _ = write!(&mut snapshot_contents, "[no results]");
    }
    // multiple rows
    else {
        for row in results {
            let _ = write!(&mut snapshot_contents, "{{\n");
            for (value, column_name) in row.iter().zip(&columns) {
                let _ = writeln!(
                    &mut snapshot_contents,
                    "\t {}: {}",
                    column_name,
                    snapshot_value(value)
                );
            }
            let _ = write!(&mut snapshot_contents, "}}\n");
        }
    }
    let _ = writeln!(&mut snapshot_contents);
    Some(snapshot_contents)
}

/// Read a key from the terminal, returning None on error.
fn read_key(term: &Term) -> Option<Key> {
    match term.read_key() {
        Ok(key) => Some(key),
        Err(e) => {
            eprintln!("Warning: Failed to read key: {}", e);
            None
        }
    }
}

/// Write snapshot contents to a file.
fn write_snapshot(path: &PathBuf, contents: &str) -> Result<(), ()> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path);

    match file {
        Ok(mut f) => {
            if let Err(e) = f.write_all(contents.as_bytes()) {
                eprintln!("Failed to write snapshot {}: {}", path.display(), e);
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to open snapshot {}: {}", path.display(), e);
            Err(())
        }
    }
}

/// Create a new snapshot file.
fn create_snapshot(path: &PathBuf, contents: &str) -> Result<(), ()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create_new(true)
        .open(path);

    match file {
        Ok(mut f) => {
            if let Err(e) = f.write_all(contents.as_bytes()) {
                eprintln!("Failed to write new snapshot {}: {}", path.display(), e);
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to create snapshot {}: {}", path.display(), e);
            Err(())
        }
    }
}

/// Process a snapshot file and generate/compare snapshots.
pub fn snapshot_file(state: &mut SnapshotState, script: &PathBuf) -> Result<(), ()> {
    let basename = script
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let sql = match read_to_string(script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", script.display(), e);
            return Err(());
        }
    };

    state.runtime.enqueue(
        &script.to_string_lossy(),
        sql.as_str(),
        BlockSource::File(script.into()),
    );

    let mut snapshot_idx_map: HashMap<String, usize> = HashMap::new();
    loop {
        match state.runtime.next_stepx() {
            Some(Ok(ref step)) => match &step.result {
                StepResult::SqlStatement { stmt, raw_sql: _ } => {
                    let region_section = step.reference.region.join("-");
                    let snapshot_idx = snapshot_idx_map.entry(region_section.clone()).or_insert(0);
                    let snapshot_path = state.snapshots_dir.join(format!(
                        "{}-{}{:02}.snap",
                        basename,
                        if region_section.is_empty() {
                            String::new()
                        } else {
                            format!("{region_section}-")
                        },
                        snapshot_idx
                    ));
                    *snapshot_idx += 1;

                    let statement_id = register_statement(&mut state.runtime, stmt, step);

                    if state.verbose {
                        println!("{}", stmt.sql());
                    }

                    let parent_path = snapshot_path.as_path().parent();
                    let source = match parent_path {
                        Some(parent) => pathdiff::diff_paths(script, parent)
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_else(|| script.to_string_lossy().to_string()),
                        None => script.to_string_lossy().to_string(),
                    };

                    let snapshot_contents = generate_snapshot_contents(source, stmt);

                    if let Some(statement_id) = statement_id {
                        register_stmt_bytecode(&mut state.runtime, stmt, statement_id);
                    }

                    let snapshot_contents = match snapshot_contents {
                        Some(x) => x,
                        None => continue,
                    };

                    if snapshot_path.exists() {
                        let filename = snapshot_path
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        state.generated_snapshots.push(filename);

                        let original_snapshot = match std::fs::read_to_string(&snapshot_path) {
                            Ok(s) => s.replace("\r\n", "\n"),
                            Err(e) => {
                                eprintln!(
                                    "Failed to read snapshot {}: {}",
                                    snapshot_path.display(),
                                    e
                                );
                                continue;
                            }
                        };

                        if original_snapshot == snapshot_contents {
                            state.results.push(SnapshotResult::Matches);
                        } else {
                            println!(
                                "{} changed at {}",
                                step.reference,
                                &snapshot_path.display()
                            );
                            let result = if state.is_review {
                                print_diff(&original_snapshot, &snapshot_contents);
                                print_decision();

                                let term = Term::stdout();
                                match read_key(&term) {
                                    Some(Key::Char('a') | Key::Char('A') | Key::Enter) => {
                                        if write_snapshot(&snapshot_path, &snapshot_contents)
                                            .is_ok()
                                        {
                                            SnapshotResult::Accepted
                                        } else {
                                            SnapshotResult::Rejected
                                        }
                                    }
                                    Some(Key::Char('r') | Key::Char('R') | Key::Escape) => {
                                        SnapshotResult::Rejected
                                    }
                                    _ => {
                                        // Unknown key or error, skip
                                        eprintln!("Skipping snapshot (unknown input)");
                                        SnapshotResult::Rejected
                                    }
                                }
                            } else {
                                SnapshotResult::Rejected
                            };
                            state.results.push(result);
                        }
                    } else {
                        println!(
                            "Reviewing {} from {}",
                            &snapshot_path.display(),
                            step.reference
                        );
                        let result = if state.is_review {
                            print_diff("", &snapshot_contents);
                            print_decision();

                            let term = Term::stdout();
                            match read_key(&term) {
                                Some(Key::Char('a') | Key::Enter) => {
                                    if create_snapshot(&snapshot_path, &snapshot_contents).is_ok() {
                                        println!("created {}", &snapshot_path.display());
                                        let filename = snapshot_path
                                            .file_name()
                                            .map(|f| f.to_string_lossy().to_string())
                                            .unwrap_or_default();
                                        state.generated_snapshots.push(filename);
                                        SnapshotResult::Accepted
                                    } else {
                                        SnapshotResult::Rejected
                                    }
                                }
                                Some(Key::Char('r') | Key::Escape) => SnapshotResult::Rejected,
                                _ => {
                                    eprintln!("Skipping snapshot (unknown input)");
                                    SnapshotResult::Rejected
                                }
                            }
                        } else {
                            SnapshotResult::Rejected
                        };
                        state.results.push(result);
                    }
                }
                StepResult::ProcedureDefinition(_) => { /* already registered in runtime */ }
                StepResult::DotCommand(cmd) => match cmd {
                    solite_core::dot::DotCommand::Load(load_command) => {
                        if let Err(e) = state.runtime.connection.execute(BASE_FUNCTIONS_CREATE) {
                            eprintln!("Warning: Failed to create base functions table: {:?}", e);
                        }
                        if let Err(e) = state.runtime.connection.execute(BASE_MODULES_CREATE) {
                            eprintln!("Warning: Failed to create base modules table: {:?}", e);
                        }
                        if let Err(e) = load_command.execute(&mut state.runtime.connection) {
                            eprintln!("Warning: Failed to load extension: {:?}", e);
                        }
                        if let Err(e) = state.runtime.connection.execute(LOADED_FUNCTIONS_CREATE) {
                            eprintln!("Warning: Failed to create loaded functions table: {:?}", e);
                        }
                        if let Err(e) = state.runtime.connection.execute(LOADED_MODULES_CREATE) {
                            eprintln!("Warning: Failed to create loaded modules table: {:?}", e);
                        }
                        state.loaded_extension = true;
                    }
                    solite_core::dot::DotCommand::Open(open_command) => {
                        open_command.execute(&mut state.runtime);
                    }
                    solite_core::dot::DotCommand::Print(print_command) => print_command.execute(),
                    solite_core::dot::DotCommand::Call(_) => { /* resolved to SqlStatement in next_stepx() */ }
                    solite_core::dot::DotCommand::Parameter(param_cmd) => {
                        if let solite_core::dot::ParameterCommand::Set { key, value } = param_cmd {
                            if let Err(e) = state.runtime.define_parameter(key.clone(), value.to_owned()) {
                                eprintln!("Warning: Failed to set parameter {}: {}", key, e);
                            }
                        }
                    }
                    other => {
                        eprintln!("Warning: Unhandled dot command: {:?}", other);
                    }
                },
            },
            None => break,
            Some(Err(step_error)) => match step_error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(file_name.as_str(), &src, &error, Some(offset));
                    return Err(());
                }
                StepError::ParseDot(error) => {
                    eprintln!("Failed to parse dot command: {:?}", error);
                    return Err(());
                }
            },
        }
    }
    Ok(())
}
