//! Report generation from SQL files.

use anyhow::{anyhow, Result};
use regex::Regex;
use solite_core::sqlite::{ColumnMeta, Connection};
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Loose matcher for lines that look like a `-- name:` annotation.
///
/// Intentionally broader than the strict parser regex in
/// `solite_core::procedure` (it also matches `--name:` and `-- name :`):
/// any preamble line matching this that did NOT parse into a procedure
/// definition is a malformed annotation, and codegen errors instead of
/// silently demoting the query to an executed `setup` statement.
static LOOSE_NAME_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--\s*name\s*:").expect("valid regex"));

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
        BaseDatabaseType::None => open_validation_db(),
        BaseDatabaseType::Database(path) => copy_schema_from_database(path),
        BaseDatabaseType::SqlFile(path) => setup_from_sql_file(path),
    }
}

/// Open the in-memory validation database with the solite stdlib initialized,
/// mirroring `Runtime::new_with_options` so annotated queries can use stdlib
/// functions (e.g. `ulid()`) and virtual tables (e.g. `vec0`).
fn open_validation_db() -> Result<Connection> {
    let db =
        Connection::open_in_memory().map_err(|e| anyhow!("Failed to open in-memory database: {}", e))?;
    unsafe {
        solite_stdlib::solite_stdlib_init(db.db(), std::ptr::null_mut(), std::ptr::null_mut());
    }
    Ok(db)
}

/// Copy schema from an existing database.
fn copy_schema_from_database(path: &Path) -> Result<Connection> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid path: {}", path.display()))?;

    let base_db = Connection::open(path_str)
        .map_err(|e| anyhow!("Failed to open database {}: {}", path.display(), e))?;

    let db = open_validation_db()?;

    // Query for all tables and views
    let mut stmt = match base_db.prepare(
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
        Err(e) => return Err(anyhow!("Failed to prepare schema query: {}", e)),
    };

    // Virtual tables must be replayed before everything else: their shadow
    // tables can show up as plain tables in `pragma_table_list`, and creating
    // a shadow table first makes the later CREATE VIRTUAL TABLE fail.
    let mut virtual_tables: Vec<String> = vec![];
    let mut others: Vec<String> = vec![];
    loop {
        match stmt.nextx() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let sql = row.value_at(1);
                let sql_str = sql.as_str();
                if sql_str.is_empty() {
                    continue;
                }
                let is_virtual = sql_str
                    .trim_start()
                    .get(..20)
                    .is_some_and(|p| p.eq_ignore_ascii_case("create virtual table"));
                if is_virtual {
                    virtual_tables.push(sql_str.to_string());
                } else {
                    others.push(sql_str.to_string());
                }
            }
            Err(e) => {
                return Err(anyhow!("Failed to read schema from {}: {}", path.display(), e));
            }
        }
    }

    for sql in &virtual_tables {
        if let Err(e) = db.execute(sql) {
            return Err(anyhow!("Failed to copy schema statement `{}`: {}", first_line(sql), e));
        }
    }
    for sql in &others {
        if let Err(e) = db.execute(sql) {
            // Shadow tables of a just-created virtual table already exist in
            // the new database; skip their replayed CREATE statements.
            if e.message.contains("already exists") {
                continue;
            }
            return Err(anyhow!("Failed to copy schema statement `{}`: {}", first_line(sql), e));
        }
    }

    Ok(db)
}

/// Set up the in-memory validation database from a SQL schema file.
///
/// The schema is only used for query validation; it is not part of the
/// report's `setup`, which holds non-annotated statements from the input file.
fn setup_from_sql_file(path: &PathBuf) -> Result<Connection> {
    let db = open_validation_db()?;

    let sql = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read file {}: {}", path.display(), e))?;

    db.execute_script(&sql)
        .map_err(|e| anyhow!("Failed to execute schema {}: {}", path.display(), e))?;

    Ok(db)
}

/// Process all steps from the runtime.
fn process_steps(rt: &mut Runtime, report: &mut Report) -> Result<()> {
    // class name -> (first query that declared it, its column shape)
    let mut declared_classes: HashMap<String, (String, Vec<ColumnMeta>)> = HashMap::new();
    // export name -> location (file:line:col) of its first definition
    let mut seen_exports: HashMap<String, String> = HashMap::new();

    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(error)) => {
                return Err(handle_step_error(error));
            }
            Some(Ok(ref step)) => match &step.result {
                StepResult::SqlStatement { stmt, raw_sql: _ } => {
                    // A statement whose preamble contains something that looks
                    // like a `-- name:` annotation but didn't parse is an
                    // authoring error, not a setup statement.
                    if let Some(preamble) = &step.preamble {
                        if let Some(bad) = preamble
                            .lines()
                            .map(str::trim)
                            .find(|l| LOOSE_NAME_LINE_RE.is_match(l))
                        {
                            return Err(anyhow!(
                                "Malformed `-- name:` annotation at {}: `{}`\n\
                                 Expected `-- name: <name> [:rows|:row|:value|:list] [-> ClassName]`",
                                step.reference,
                                bad
                            ));
                        }
                    }

                    // Not an export, treat as setup
                    report.setup.push(stmt.sql());
                    if let Err(e) = stmt.execute() {
                        return Err(anyhow!(
                            "Failed to execute setup statement at {}: {}",
                            step.reference,
                            e
                        ));
                    }
                }
                StepResult::ProcedureDefinition(proc) => {
                    validate_annotations(proc, &step.reference)?;

                    if let Some(first_at) =
                        seen_exports.insert(proc.name.clone(), step.reference.to_string())
                    {
                        return Err(anyhow!(
                            "Duplicate export name `{}` at {} (first defined at {})",
                            proc.name,
                            step.reference,
                            first_at
                        ));
                    }

                    // Bare `?` parameters have no name (`sqlite3_bind_parameter_name`
                    // returns NULL), so downstream generators cannot construct an
                    // argument or bind key for them. A `?N` numbering gap leaves the
                    // skipped index equally anonymous. Positional order is invisible
                    // in the IR, so error instead of guessing.
                    if proc.parameters.iter().any(|p| p.full_name.is_empty()) {
                        return Err(anyhow!(
                            "Query `{}` at {} uses an anonymous positional parameter (a bare `?`, or a `?N` numbering gap); use named ($x, :x) or contiguous numbered (?1, ?2, ...) parameters in annotated queries",
                            proc.name,
                            step.reference
                        ));
                    }

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
                    return Err(anyhow!(
                        "Dot commands are not supported in codegen input (found .{} at {})",
                        dot_command_name(cmd),
                        step.reference
                    ));
                }
            },
        }
    }
    Ok(())
}

/// Return the first line of a (possibly multi-line) SQL statement, trimmed,
/// for use in error messages.
fn first_line(sql: &str) -> &str {
    sql.lines().next().unwrap_or("").trim()
}

/// The dot command's name as the user typed it (without the leading `.`),
/// for error messages. The parsed enum doesn't retain the original text, so
/// map each variant back to its command name.
fn dot_command_name(cmd: &solite_core::dot::DotCommand) -> &'static str {
    use solite_core::dot::DotCommand::*;
    match cmd {
        Tables(_) => "tables",
        Schema(_) => "schema",
        Graphviz(_) => "graphviz",
        Open(_) => "open",
        Load(_) => "load",
        Tui(_) => "tui",
        Clear(_) => "clear",
        Print(_) => "print",
        Ask(_) => "ask",
        Help(_) => "help",
        Shell(_) => "sh",
        Parameter(_) => "param",
        Env(_) => "env",
        Timer(_) => "timer",
        Export(_) => "export",
        Vegalite(_) => "vegalite",
        Bench(_) => "bench",
        Dotenv(_) => "dotenv",
        Call(_) => "call",
        Run(_) => "run",
        #[cfg(feature = "ritestream")]
        Stream(_) => "stream",
    }
}

/// The annotation tokens accepted on a `-- name:` line.
const KNOWN_ANNOTATIONS: [&str; 4] = ["rows", "row", "value", "list"];

/// Validate the raw annotation tokens of a procedure definition.
///
/// Core silently ignores unknown tokens and resolves conflicting result
/// types by a fixed priority order (REPL/run tolerate sloppiness there);
/// codegen is the authoring gate, so a typo like `:vaule` or an ambiguous
/// `:row :value` is a hard error here.
fn validate_annotations(
    proc: &solite_core::procedure::Procedure,
    reference: &solite_core::StepReference,
) -> Result<()> {
    let mut result_types: Vec<&str> = vec![];
    for annotation in &proc.annotations {
        if KNOWN_ANNOTATIONS.contains(&annotation.as_str()) {
            result_types.push(annotation.as_str());
        } else {
            return Err(anyhow!(
                "Unknown annotation `:{}` on query `{}` at {}; accepted annotations are :rows, :row, :value, :list",
                annotation,
                proc.name,
                reference
            ));
        }
    }
    if result_types.len() > 1 {
        return Err(anyhow!(
            "Conflicting result-type annotations on query `{}` at {}: {}",
            proc.name,
            reference,
            result_types
                .iter()
                .map(|a| format!(":{a}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
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
        StepError::ParseDot { error: e, .. } => anyhow!("Dot command parse error: {}", e),
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
