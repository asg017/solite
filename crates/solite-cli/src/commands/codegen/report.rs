//! Report generation from SQL files.

use anyhow::{anyhow, Result};
use solite_core::sqlite::Connection;
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use std::path::PathBuf;

use super::parser::{determine_result_type, parse_name_line, parse_parameter};
use super::types::{Export, Report};

/// The type of base database to use for schema validation.
#[derive(Debug)]
pub enum BaseDatabaseType {
    /// No base database, use empty in-memory database.
    None,
    /// Use an existing SQLite database file.
    Database(PathBuf),
    /// Execute a SQL file to set up the schema.
    SqlFile(PathBuf),
}

/// Generate a report from a SQL source file.
///
/// # Arguments
///
/// * `source` - The SQL source code to process
/// * `filename` - The filename for error reporting
/// * `base_db_type` - The type of base database for schema validation
///
/// # Returns
///
/// A `Report` containing setup statements and exported queries, or an error.
pub fn report_from_file(
    source: &str,
    filename: &PathBuf,
    base_db_type: BaseDatabaseType,
) -> Result<Report> {
    let mut report = Report::new();

    let mut rt = Runtime::new(None);
    let conn = create_connection(&base_db_type, &mut report)?;
    rt.connection = conn;

    rt.enqueue(
        &filename.to_string_lossy(),
        source,
        BlockSource::File(filename.to_owned()),
    );

    process_steps(&mut rt, &mut report)?;

    Ok(report)
}

/// Create a connection based on the database type.
fn create_connection(
    base_db_type: &BaseDatabaseType,
    report: &mut Report,
) -> Result<Connection> {
    match base_db_type {
        BaseDatabaseType::None => Connection::open_in_memory()
            .map_err(|e| anyhow!("Failed to open database: {:?}", e)),
        BaseDatabaseType::Database(path) => copy_schema_from_database(path),
        BaseDatabaseType::SqlFile(path) => setup_from_sql_file(path, report),
    }
}

/// Copy schema from an existing database.
fn copy_schema_from_database(path: &PathBuf) -> Result<Connection> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid path: {}", path.display()))?;

    let base_db =
        Connection::open(path_str).map_err(|e| anyhow!("Failed to open database: {:?}", e))?;

    let db =
        Connection::open_in_memory().map_err(|e| anyhow!("Failed to open database: {:?}", e))?;

    // Query for all tables and views
    let stmt = match base_db.prepare(
        r#"
        WITH t AS (
            SELECT name
            FROM pragma_table_list
            WHERE type IN ('table', 'view', 'virtual')
              AND name NOT LIKE 'sqlite_%'
        )
        SELECT t.name, sqlite_master.sql
        FROM t
        LEFT JOIN sqlite_master ON sqlite_master.name = t.name
        "#,
    ) {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => return Err(anyhow!("Failed to prepare schema query")),
        Err(e) => return Err(anyhow!("SQL error: {:?}", e)),
    };

    loop {
        match stmt.nextx() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let sql = row.value_at(1);
                let sql_str = sql.as_str();
                if !sql_str.is_empty() {
                    if let Err(e) = db.execute(sql_str) {
                        return Err(anyhow!("Failed to copy schema: {:?}", e));
                    }
                }
            }
            Err(e) => {
                return Err(anyhow!("Failed to read schema: {:?}", e));
            }
        }
    }

    Ok(db)
}

/// Set up database from a SQL file.
fn setup_from_sql_file(path: &PathBuf, report: &mut Report) -> Result<Connection> {
    let db =
        Connection::open_in_memory().map_err(|e| anyhow!("Failed to open database: {:?}", e))?;

    let sql = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read file {}: {}", path.display(), e))?;

    db.execute_script(&sql)
        .map_err(|e| anyhow!("Failed to execute schema: {:?}", e))?;

    report.setup.push(sql);
    Ok(db)
}

/// Process all steps from the runtime.
fn process_steps(rt: &mut Runtime, report: &mut Report) -> Result<()> {
    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(error)) => {
                return Err(handle_step_error(error));
            }
            Some(Ok(ref step)) => match &step.result {
                StepResult::SqlStatement { stmt, raw_sql: _ } => {
                    if let Some(preamble) = &step.preamble {
                        let trimmed = preamble.trim();
                        if trimmed.starts_with("-- name:") {
                            let (name, annotations) = parse_name_line(trimmed)
                                .ok_or_else(|| anyhow!("Invalid name line"))?;

                            let columns = stmt.column_meta();
                            let parameters: Vec<_> = stmt
                                .parameter_info()
                                .iter()
                                .map(|p| parse_parameter(p))
                                .collect();

                            let result_type = determine_result_type(&annotations, columns.len());

                            report.exports.push(Export {
                                name,
                                parameters,
                                columns,
                                sql: stmt.sql(),
                                result_type,
                            });
                            continue;
                        }
                    }

                    // Not an export, treat as setup
                    report.setup.push(stmt.sql());
                    if let Err(e) = stmt.execute() {
                        return Err(anyhow!("Failed to execute setup: {:?}", e));
                    }
                }
                StepResult::DotCommand(cmd) => {
                    return Err(anyhow!("Unsupported dot command: {:?}", cmd));
                }
            },
        }
    }
    Ok(())
}

fn handle_step_error(error: StepError) -> anyhow::Error {
    match error {
        StepError::ParseDot(e) => anyhow!("Dot command parse error: {:?}", e),
        StepError::Prepare {
            file_name,
            offset,
            error,
            ..
        } => anyhow!("Failed to prepare statement at {}:{}: {:?}", file_name, offset, error),
    }
}
