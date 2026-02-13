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
pub(crate) mod status;
#[cfg(test)]
mod test_status;

use std::ffi::OsStr;
use std::fs::read_to_string;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use nbformat::{parse_notebook, Notebook};
use solite_core::{BlockSource, Runtime, StepError, StepResult};

use crate::cli::{ReplArgs, RunArgs};

use dot::handle_dot_command;
pub use format::format_duration;
use sql::handle_sql;

/// Entry point for the run command.
pub(crate) fn run(flags: RunArgs) -> Result<(), ()> {
    match run_impl(flags) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e:?}");
            Err(())
        }
    }
}

/// Classification of a positional argument.
enum InputType {
    Script(PathBuf),
    Database(PathBuf),
    Procedure(String),
}

/// Classify a positional argument by file extension.
fn classify_arg(s: &str) -> InputType {
    let path = std::path::Path::new(s);
    match path.extension().and_then(OsStr::to_str) {
        Some("sql") | Some("ipynb") => InputType::Script(PathBuf::from(s)),
        Some("db") | Some("sqlite") | Some("sqlite3") => InputType::Database(PathBuf::from(s)),
        _ => InputType::Procedure(s.to_string()),
    }
}

/// Parsed positional arguments for the run command.
#[derive(Debug)]
struct ParsedArgs {
    database: Option<PathBuf>,
    script: Option<PathBuf>,
    procedure: Option<String>,
}

/// Parse positional arguments into database, script, and procedure.
///
/// Procedure name must follow immediately after the script file.
/// Valid forms:
///   solite run script.sql
///   solite run script.sql procedureName
///   solite run db.db script.sql
///   solite run db.db script.sql procedureName
///   solite run script.sql procedureName db.db
///   solite run script.sql db.db
fn parse_args(args: &[String]) -> Result<ParsedArgs> {
    let mut database: Option<PathBuf> = None;
    let mut script: Option<PathBuf> = None;
    let mut procedure: Option<String> = None;

    let classified: Vec<InputType> = args.iter().map(|a| classify_arg(a)).collect();

    for (i, input) in classified.into_iter().enumerate() {
        match input {
            InputType::Script(path) => {
                if script.is_some() {
                    bail!("Multiple script files provided");
                }
                script = Some(path);
            }
            InputType::Database(path) => {
                if database.is_some() {
                    bail!("Multiple database files provided");
                }
                database = Some(path);
            }
            InputType::Procedure(name) => {
                // Procedure name must follow immediately after a script file
                let prev_was_script =
                    i > 0 && matches!(classify_arg(&args[i - 1]), InputType::Script(_));
                if !prev_was_script {
                    bail!("Procedure name '{name}' must follow a .sql file");
                }
                if procedure.is_some() {
                    bail!("Multiple procedure names provided");
                }
                procedure = Some(name);
            }
        }
    }

    Ok(ParsedArgs {
        database,
        script,
        procedure,
    })
}

/// Internal implementation of the run command.
fn run_impl(flags: RunArgs) -> Result<()> {
    let parsed = parse_args(&flags.args)?;
    let ParsedArgs {
        database,
        script,
        procedure,
    } = parsed;

    // -c flag: treat the string as inline SQL, no scripts/procedures allowed
    if let Some(ref command) = flags.command {
        if script.is_some() {
            bail!("-c/--command cannot be combined with a .sql file");
        }
        if procedure.is_some() {
            bail!("-c/--command cannot be combined with a procedure name");
        }

        let mut rt = if flags.readonly {
            match &database {
                Some(db) => Runtime::new_readonly(&db.to_string_lossy()),
                None => bail!("--readonly requires a database path"),
            }
        } else {
            Runtime::new(database.as_ref().map(|p| p.to_string_lossy().to_string()))
        };

        if flags.trace.is_some() {
            setup_tracing(&rt)?;
        }

        for chunk in flags.parameters.chunks(2) {
            if chunk.len() == 2 {
                rt.define_parameter(chunk[0].clone(), chunk[1].clone())
                    .map_err(|e| anyhow::anyhow!("Failed to set parameter: {e}"))?;
            }
        }

        rt.enqueue("<command>", command, BlockSource::CommandFlag);

        let mut timer = true;
        execute_steps(&mut rt, flags.trace.is_some(), &mut timer);

        if let Some(trace_path) = flags.trace {
            write_trace_output(&rt, &trace_path)?;
        }

        return Ok(());
    }

    // No args → REPL; only a database → REPL on that db
    if script.is_none() && procedure.is_none() {
        crate::commands::repl::repl(ReplArgs { database })
            .map_err(|_| anyhow::anyhow!("Failed to open REPL"))?;
        return Ok(());
    }

    let script = match script {
        Some(s) => s,
        None => bail!("No SQL script provided"),
    };

    // Create runtime
    let mut rt = if flags.readonly {
        match &database {
            Some(db) => Runtime::new_readonly(&db.to_string_lossy()),
            None => {
                bail!("--readonly requires a database path");
            }
        }
    } else {
        Runtime::new(database.as_ref().map(|p| p.to_string_lossy().to_string()))
    };

    // Set up tracing if requested
    if flags.trace.is_some() {
        setup_tracing(&rt)?;
    }

    // Set parameters
    for chunk in flags.parameters.chunks(2) {
        if chunk.len() == 2 {
            rt.define_parameter(chunk[0].clone(), chunk[1].clone())
                .map_err(|e| anyhow::anyhow!("Failed to set parameter: {e}"))?;
        }
    }

    match procedure {
        Some(proc_name) => {
            // Load the file (execute setup, register procedures), then call one
            let script_str = script.to_string_lossy().to_string();
            rt.load_file(&script_str)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let proc = rt
                .get_procedure(&proc_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown procedure: '{proc_name}'"))?
                .clone();

            match rt.prepare_with_parameters(&proc.sql) {
                Ok((_, Some(stmt))) => {
                    let config = solite_table::TableConfig::terminal();
                    solite_table::print_statement(&stmt, &config)
                        .map_err(|e| anyhow::anyhow!("Error executing procedure: {e}"))?;
                }
                Ok((_, None)) => bail!("Procedure '{proc_name}' prepared to empty statement"),
                Err(e) => bail!("Error preparing procedure '{proc_name}': {e:?}"),
            }
        }
        None => {
            // Normal script execution
            enqueue_script(&mut rt, &script)?;

            let mut timer = true;
            execute_steps(&mut rt, flags.trace.is_some(), &mut timer);

            // Write trace output if requested
            if let Some(trace_path) = flags.trace {
                write_trace_output(&rt, &trace_path)?;
            }
        }
    }

    Ok(())
}

/// Set up tracing database.
fn setup_tracing(rt: &Runtime) -> Result<()> {
    rt.connection
        .execute("ATTACH DATABASE ':memory:' AS solite_trace;")
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .context("Failed to attach trace database")?;

    rt.connection
        .execute(
            "CREATE TABLE solite_trace.statements(id INTEGER PRIMARY KEY AUTOINCREMENT, sql TEXT)",
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .context("Failed to create trace statements table")?;

    rt.connection
        .execute(
            r#"CREATE TABLE solite_trace.steps(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                statement_id INTEGER REFERENCES statements(id),
                addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
            )"#,
        )
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .context("Failed to create trace steps table")?;

    Ok(())
}

/// Load and enqueue a script for execution.
fn enqueue_script(rt: &mut Runtime, script: &std::path::Path) -> Result<()> {
    match script.extension().and_then(OsStr::to_str) {
        Some("sql") => {
            let sql = read_to_string(script)
                .with_context(|| format!("Failed to read {}", script.display()))?;

            rt.enqueue(
                &script.to_string_lossy(),
                &sql,
                BlockSource::File(script.to_path_buf()),
            );
        }
        Some("ipynb") => {
            let content = read_to_string(script)
                .with_context(|| format!("Failed to read {}", script.display()))?;

            let nb: Notebook = parse_notebook(&content)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .with_context(|| format!("Failed to parse notebook {}", script.display()))?;

            let cells = extract_notebook_cells(&nb);
            for (idx, code) in cells {
                rt.enqueue(
                    &format!("{}:{}", script.to_string_lossy(), idx),
                    &code,
                    BlockSource::JupyerCell,
                );
            }
        }
        Some(ext) => bail!("Unknown file type: {ext}"),
        None => bail!("Unknown file type: no extension"),
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
                StepResult::ProcedureDefinition(_) => { /* already registered in runtime */ }
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
fn write_trace_output(rt: &Runtime, trace_path: &std::path::Path) -> Result<()> {
    let stmt = match rt.connection.prepare("VACUUM solite_trace INTO ?;") {
        Ok((_, Some(s))) => s,
        _ => bail!("Failed to prepare vacuum statement"),
    };

    let path_str = trace_path.to_string_lossy();
    stmt.bind_text(1, &path_str);

    stmt.execute()
        .map_err(|e| anyhow::anyhow!("{e:?}"))
        .context("Failed to write trace")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // classify_arg

    #[test]
    fn classify_sql() {
        assert!(matches!(classify_arg("script.sql"), InputType::Script(_)));
    }

    #[test]
    fn classify_ipynb() {
        assert!(matches!(
            classify_arg("notebook.ipynb"),
            InputType::Script(_)
        ));
    }

    #[test]
    fn classify_db() {
        assert!(matches!(classify_arg("my.db"), InputType::Database(_)));
    }

    #[test]
    fn classify_sqlite() {
        assert!(matches!(
            classify_arg("my.sqlite"),
            InputType::Database(_)
        ));
    }

    #[test]
    fn classify_sqlite3() {
        assert!(matches!(
            classify_arg("my.sqlite3"),
            InputType::Database(_)
        ));
    }

    #[test]
    fn classify_procedure() {
        assert!(matches!(
            classify_arg("listUsers"),
            InputType::Procedure(_)
        ));
    }

    // parse_args – valid forms

    #[test]
    fn parse_script_only() {
        let p = parse_args(&args(&["script.sql"])).unwrap();
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert!(p.database.is_none());
        assert!(p.procedure.is_none());
    }

    #[test]
    fn parse_script_procedure() {
        let p = parse_args(&args(&["script.sql", "listUsers"])).unwrap();
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert_eq!(p.procedure.unwrap(), "listUsers");
        assert!(p.database.is_none());
    }

    #[test]
    fn parse_db_script() {
        let p = parse_args(&args(&["my.db", "script.sql"])).unwrap();
        assert_eq!(p.database.unwrap(), PathBuf::from("my.db"));
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert!(p.procedure.is_none());
    }

    #[test]
    fn parse_db_script_procedure() {
        let p = parse_args(&args(&["my.db", "script.sql", "listUsers"])).unwrap();
        assert_eq!(p.database.unwrap(), PathBuf::from("my.db"));
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert_eq!(p.procedure.unwrap(), "listUsers");
    }

    #[test]
    fn parse_script_procedure_db() {
        let p = parse_args(&args(&["script.sql", "listUsers", "my.db"])).unwrap();
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert_eq!(p.procedure.unwrap(), "listUsers");
        assert_eq!(p.database.unwrap(), PathBuf::from("my.db"));
    }

    #[test]
    fn parse_script_db() {
        let p = parse_args(&args(&["script.sql", "my.db"])).unwrap();
        assert_eq!(p.script.unwrap(), PathBuf::from("script.sql"));
        assert_eq!(p.database.unwrap(), PathBuf::from("my.db"));
        assert!(p.procedure.is_none());
    }

    #[test]
    fn parse_no_args() {
        let p = parse_args(&args(&[])).unwrap();
        assert!(p.script.is_none());
        assert!(p.database.is_none());
        assert!(p.procedure.is_none());
    }

    #[test]
    fn parse_db_only() {
        let p = parse_args(&args(&["my.db"])).unwrap();
        assert!(p.script.is_none());
        assert_eq!(p.database.unwrap(), PathBuf::from("my.db"));
        assert!(p.procedure.is_none());
    }

    // parse_args – error cases

    #[test]
    fn error_procedure_before_script() {
        let err = parse_args(&args(&["listUsers", "script.sql"])).unwrap_err();
        assert!(err.to_string().contains("must follow a .sql file"));
    }

    #[test]
    fn error_bare_procedure() {
        let err = parse_args(&args(&["listUsers"])).unwrap_err();
        assert!(err.to_string().contains("must follow a .sql file"));
    }

    #[test]
    fn error_procedure_after_db() {
        let err = parse_args(&args(&["my.db", "listUsers"])).unwrap_err();
        assert!(err.to_string().contains("must follow a .sql file"));
    }

    #[test]
    fn error_multiple_scripts() {
        let err = parse_args(&args(&["a.sql", "b.sql"])).unwrap_err();
        assert!(err.to_string().contains("Multiple script files"));
    }

    #[test]
    fn error_multiple_databases() {
        let err = parse_args(&args(&["a.db", "b.db", "script.sql"])).unwrap_err();
        assert!(err.to_string().contains("Multiple database files"));
    }

    #[test]
    fn error_multiple_procedures() {
        let err = parse_args(&args(&["script.sql", "proc1", "proc2"])).unwrap_err();
        // proc2 doesn't follow a script, so it fails with "must follow"
        assert!(err.to_string().contains("must follow a .sql file"));
    }
}
