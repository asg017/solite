//! Report generation from SQL files.

use anyhow::{anyhow, Result};
use solite_core::sqlite::{ColumnMeta, Connection};
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    let mut rt = Runtime::new(None)?;
    let conn = create_connection(&base_db_type)?;
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
fn create_connection(base_db_type: &BaseDatabaseType) -> Result<Connection> {
    match base_db_type {
        BaseDatabaseType::None => Connection::open_in_memory()
            .map_err(|e| anyhow!("Failed to open database: {:?}", e)),
        BaseDatabaseType::Database(path) => copy_schema_from_database(path),
        BaseDatabaseType::SqlFile(path) => setup_from_sql_file(path),
    }
}

/// Copy schema from an existing database.
fn copy_schema_from_database(path: &Path) -> Result<Connection> {
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

/// Set up the in-memory validation database from a SQL schema file.
///
/// The schema is only used for query validation; it is not part of the
/// report's `setup`, which holds non-annotated statements from the input file.
fn setup_from_sql_file(path: &PathBuf) -> Result<Connection> {
    let db =
        Connection::open_in_memory().map_err(|e| anyhow!("Failed to open database: {:?}", e))?;

    let sql = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read file {}: {}", path.display(), e))?;

    db.execute_script(&sql)
        .map_err(|e| anyhow!("Failed to execute schema: {:?}", e))?;

    Ok(db)
}

/// Process all steps from the runtime.
fn process_steps(rt: &mut Runtime, report: &mut Report) -> Result<()> {
    // class name -> (first query that declared it, its column shape)
    let mut declared_classes: HashMap<String, (String, Vec<ColumnMeta>)> = HashMap::new();

    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(error)) => {
                return Err(handle_step_error(error));
            }
            Some(Ok(ref step)) => match &step.result {
                StepResult::SqlStatement { stmt, raw_sql: _ } => {
                    // Not an export, treat as setup
                    report.setup.push(stmt.sql());
                    if let Err(e) = stmt.execute() {
                        return Err(anyhow!("Failed to execute setup: {:?}", e));
                    }
                }
                StepResult::ProcedureDefinition(proc) => {
                    if let Some(class_name) = &proc.result_class {
                        match declared_classes.get(class_name) {
                            None => {
                                declared_classes.insert(
                                    class_name.clone(),
                                    (proc.name.clone(), proc.columns.clone()),
                                );
                            }
                            Some((first_query, first_shape)) => {
                                if let Err(msg) =
                                    check_shape_match(first_shape, &proc.columns)
                                {
                                    return Err(anyhow!(
                                        "Result class `{}` shape mismatch between `{}` and `{}`: {}",
                                        class_name,
                                        first_query,
                                        proc.name,
                                        msg
                                    ));
                                }
                            }
                        }
                    }

                    report.exports.push(Export {
                        name: proc.name.clone(),
                        parameters: proc.parameters.clone(),
                        columns: proc.columns.clone(),
                        sql: proc.sql.clone(),
                        result_type: proc.result_type.clone(),
                        result_class: proc.result_class.clone(),
                    });
                }
                StepResult::DotCommand(cmd) => {
                    return Err(anyhow!("Unsupported dot command: {:?}", cmd));
                }
            },
        }
    }
    Ok(())
}

/// Compare two result-set column shapes for codegen purposes.
///
/// Returns Ok(()) when the shapes match: same column count, and for each
/// position the name, decltype (case-insensitive), and nullability agree.
/// `origin_database` / `origin_table` / `origin_column` are intentionally
/// ignored so queries that produce the same shape from different sources
/// (e.g. a view and a base table) can share a class.
fn check_shape_match(
    first: &[ColumnMeta],
    other: &[ColumnMeta],
) -> std::result::Result<(), String> {
    if first.len() != other.len() {
        return Err(format!(
            "column count differs ({} vs {})",
            first.len(),
            other.len()
        ));
    }

    for (idx, (a, b)) in first.iter().zip(other.iter()).enumerate() {
        if a.name != b.name {
            return Err(format!(
                "column {} name differs: `{}` vs `{}`",
                idx, a.name, b.name
            ));
        }
        let a_decl = a.decltype.as_deref().unwrap_or("");
        let b_decl = b.decltype.as_deref().unwrap_or("");
        if !a_decl.eq_ignore_ascii_case(b_decl) {
            return Err(format!(
                "column {} (`{}`) decltype differs: `{}` vs `{}`",
                idx, a.name, a_decl, b_decl
            ));
        }
        if a.nullable != b.nullable {
            return Err(format!(
                "column {} (`{}`) nullability differs: {:?} vs {:?}",
                idx, a.name, a.nullable, b.nullable
            ));
        }
    }

    Ok(())
}

fn handle_step_error(error: StepError) -> anyhow::Error {
    match error {
        StepError::ParseDot(e) => anyhow!("Dot command parse error: {:?}", e),
        StepError::Prepare {
            file_name,
            src,
            offset,
            error,
        } => {
            // block.offset points at the start of the failing statement; if SQLite
            // reported its own offset into that statement, combine them to land
            // on the exact token.
            let abs_offset = offset.saturating_add(error.offset.unwrap_or(0));
            let (line, col) = offset_to_line_col(&src, abs_offset);
            let line_text = first_line_at(&src, abs_offset);
            if line_text.is_empty() {
                anyhow!(
                    "Failed to prepare statement at {}:{}:{}: {}",
                    file_name, line, col, error.message,
                )
            } else {
                anyhow!(
                    "Failed to prepare statement at {}:{}:{}: {}\n  > {}",
                    file_name, line, col, error.message, line_text,
                )
            }
        }
    }
}

/// Return the 1-based (line, column) of `offset` within `src`.
///
/// Byte offsets are counted; the column is also in bytes. Offsets past the end
/// of `src` are clamped to the final byte.
fn offset_to_line_col(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let prefix = &src[..offset];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let last_newline = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = offset - last_newline + 1;
    (line, col)
}

/// Return the line of `src` that contains byte `offset`, trimmed of trailing
/// whitespace. Returns empty string if `src` is empty.
fn first_line_at(src: &str, offset: usize) -> &str {
    if src.is_empty() {
        return "";
    }
    let offset = offset.min(src.len().saturating_sub(1));
    let start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    src[start..end].trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_line_col_first_line() {
        assert_eq!(offset_to_line_col("select 1", 0), (1, 1));
        assert_eq!(offset_to_line_col("select 1", 7), (1, 8));
    }

    #[test]
    fn test_offset_to_line_col_mid_file() {
        let src = "line one\nline two\nline three";
        // offset of 'l' in "line two"
        let off = src.find("line two").unwrap();
        assert_eq!(offset_to_line_col(src, off), (2, 1));
        // offset of 't' in "three"
        let off = src.find("three").unwrap();
        assert_eq!(offset_to_line_col(src, off), (3, 6));
    }

    #[test]
    fn test_offset_to_line_col_past_end() {
        let src = "short";
        assert_eq!(offset_to_line_col(src, 999), (1, 6));
    }

    #[test]
    fn test_first_line_at_start() {
        let src = "create table t(a);\nselect * from t;\n";
        let off = src.find("select").unwrap();
        assert_eq!(first_line_at(src, off), "select * from t;");
    }

    #[test]
    fn test_first_line_at_no_trailing_newline() {
        let src = "one\ntwo";
        let off = src.find("two").unwrap();
        assert_eq!(first_line_at(src, off), "two");
    }

    #[test]
    fn test_first_line_at_empty_src() {
        assert_eq!(first_line_at("", 0), "");
    }

    #[test]
    fn test_handle_step_error_formats_line_col() {
        use solite_core::sqlite::SQLiteError;
        let err = StepError::Prepare {
            file_name: "queries.sql".to_string(),
            src: "\ncreate table t(a);\n\nselect * from missing;\n".to_string(),
            offset: "\ncreate table t(a);\n\n".len(),
            error: SQLiteError {
                result_code: 1,
                code_description: "SQL logic error".to_string(),
                message: "no such table: missing".to_string(),
                offset: None,
            },
        };
        let msg = handle_step_error(err).to_string();
        assert!(msg.contains("queries.sql:4:1"), "msg = {msg}");
        assert!(msg.contains("no such table: missing"), "msg = {msg}");
        assert!(msg.contains("> select * from missing;"), "msg = {msg}");
    }
}
