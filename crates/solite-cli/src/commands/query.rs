//! SQL query execution command.
//!
//! This module provides functionality to execute SQL queries and statements
//! from the command line, with support for various output formats.

use solite_core::{exporter::ExportFormat, replacement_scans::replacement_scan, Runtime};
use solite_table::TableConfig;
use std::{
    fmt,
    io::{stdout, IsTerminal, Write},
    path::{Path, PathBuf},
};

use crate::cli::QueryArgs;

/// Errors that can occur during query execution.
#[derive(Debug)]
pub enum QueryError {
    /// Database path does not exist.
    DatabaseNotFound(PathBuf),
    /// Failed to convert path to string.
    InvalidPath(PathBuf),
    /// Failed to set parameter.
    ParameterSet(String),
    /// Statement preparation returned no statement.
    EmptyPrepare,
    /// Replacement scan failed.
    ReplacementScanFailed(String),
    /// Statement execution failed.
    ExecutionFailed(String),
    /// SQL syntax or preparation error (already reported).
    SqlError,
    /// More than one SQL statement was provided.
    TrailingSql,
    /// Failed to read SQL file.
    FileReadError(PathBuf, std::io::Error),
    /// No SQL provided or stdin could not be read.
    Stdin(String),
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::DatabaseNotFound(path) => {
                write!(f, "Database not found: {}", path.display())
            }
            QueryError::InvalidPath(path) => {
                write!(f, "Invalid path (not valid UTF-8): {}", path.display())
            }
            QueryError::ParameterSet(msg) => write!(f, "Failed to set parameter: {}", msg),
            QueryError::EmptyPrepare => write!(f, "Statement preparation returned no statement"),
            QueryError::ReplacementScanFailed(msg) => write!(f, "Replacement scan failed: {}", msg),
            QueryError::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            QueryError::SqlError => write!(f, "SQL error"),
            QueryError::TrailingSql => write!(
                f,
                "Only a single SQL statement is allowed in `solite query`. \
                 Use `solite run` for multi-statement scripts."
            ),
            QueryError::FileReadError(path, err) => {
                write!(f, "Failed to read SQL file '{}': {}", path.display(), err)
            }
            QueryError::Stdin(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for QueryError {}

/// Entry point for the query command.
pub(crate) fn query(args: QueryArgs) -> Result<(), ()> {
    match query_impl(args) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Don't print SqlError - it's already been reported
            if !matches!(err, QueryError::SqlError) {
                eprintln!("Error: {}", err);
            }
            Err(())
        }
    }
}

/// If the string looks like a path to a `.sql` file, read and return its
/// contents. Otherwise return the string as-is.
fn resolve_sql(sql: String) -> Result<String, QueryError> {
    let path = Path::new(&sql);
    if super::is_sql_file(path) {
        super::read_sql_file(path).map_err(|e| QueryError::FileReadError(path.to_path_buf(), e))
    } else {
        Ok(sql)
    }
}

/// Where the SQL text comes from.
#[derive(Debug, PartialEq)]
enum SqlSource {
    /// SQL given inline (or as a .sql file path).
    Inline(String),
    /// SQL is read from piped stdin (`-` placeholder or no statement arg).
    Stdin,
}

/// Internal implementation of the query command.
fn query_impl(args: QueryArgs) -> Result<(), QueryError> {
    let stdin_piped = !std::io::stdin().is_terminal();
    let (db_path, source) = parse_arguments(&args, stdin_piped)?;
    let sql = match source {
        SqlSource::Inline(sql) => resolve_sql(sql)?,
        SqlSource::Stdin => super::read_sql_from_stdin().map_err(QueryError::Stdin)?,
    };

    let mut runtime = Runtime::new_with_options(
        db_path.map(|p| p.to_string_lossy().to_string()),
        args.remote.remote_bin.as_deref(),
        args.remote.transport.as_deref(),
        args.remote.allow_ssh,
    ).map_err(|e| QueryError::ExecutionFailed(e.to_string()))?;

    // Load extensions if specified
    if let Some(exts) = &args.load_extension {
        for ext in exts {
            if let Err(e) = runtime.connection.load_extension(&ext.to_string_lossy(), &None) {
                return Err(QueryError::ExecutionFailed(format!(
                    "Failed to load extension: {:?}",
                    e
                )));
            }
        }
    }

    // Set parameters, inferring integer/real types from the value
    for chunk in args.parameters.chunks(2) {
        if chunk.len() == 2 {
            runtime
                .define_parameter_value(
                    chunk[0].clone(),
                    solite_core::infer_parameter_value(&chunk[1]),
                )
                .map_err(|e| QueryError::ParameterSet(e.to_string()))?;
        }
    }

    // Prepare statement with replacement scan fallback
    let stmt = prepare_statement(&mut runtime, &sql)?;

    // Only allow read-only statements
    if !stmt.readonly() {
        return Err(QueryError::ExecutionFailed(
            "Only read-only statements are allowed in `solite query`. \
             Use `solite exec` instead to modify the database."
                .to_string(),
        ));
    }

    // If stdout is a TTY and no explicit format/output specified, use pretty table
    let use_table = args.format.is_none() && args.output.is_none() && stdout().is_terminal();

    if use_table {
        let config = TableConfig::terminal();
        let mut stmt = stmt;
        solite_table::print_statement(&mut stmt, &config)
            .map_err(|e| QueryError::ExecutionFailed(e.to_string()))?;
    } else {
        // Determine output format
        let format = determine_format(&args);

        // The clipboard is its own destination; combining it with `-o`
        // would silently produce an empty file
        if matches!(format, ExportFormat::Clipboard) && args.output.is_some() {
            return Err(QueryError::ExecutionFailed(
                "-f clipboard cannot be combined with -o: the clipboard is the output \
                 destination"
                    .to_string(),
            ));
        }

        // Set up output (created only once the format is known to use it)
        let output: Box<dyn Write> = match &args.output {
            Some(output) => solite_core::exporter::output_from_path(output)
                .map_err(|e| QueryError::ExecutionFailed(e.to_string()))?,
            None => Box::new(stdout()),
        };

        // Write output
        let mut stmt = stmt;
        let blob_limit = args.blob_limit.unwrap_or_default();
        let clipboard_rows =
            solite_core::exporter::write_output(&mut stmt, output, format, blob_limit)
                .map_err(|e| QueryError::ExecutionFailed(e.to_string()))?;
        if let Some(num_rows) = clipboard_rows {
            let row_word = if num_rows == 1 { "row" } else { "rows" };
            println!("\u{2713} Wrote {} {} to clipboard", num_rows, row_word);
        }
    }

    Ok(())
}

/// Parse command line arguments to determine database path and SQL.
fn is_remote_url(s: &str) -> bool {
    solite_core::sqlite::is_remote_path(s)
}

/// Validate a positional that the stdin paths classify as the database.
/// `query` is read-only and must never create a database file (the
/// underlying open uses SQLITE_OPEN_CREATE), so anything that isn't
/// `:memory:`, a remote URL, or an existing file is rejected.
fn validate_stdin_database(arg: &str) -> Result<PathBuf, QueryError> {
    let p = PathBuf::from(arg);
    if arg == ":memory:" || is_remote_url(arg) || p.exists() {
        Ok(p)
    } else {
        Err(QueryError::DatabaseNotFound(p))
    }
}

use super::is_sql_file;

fn parse_arguments(
    args: &QueryArgs,
    stdin_piped: bool,
) -> Result<(Option<PathBuf>, SqlSource), QueryError> {
    match (&args.statement, &args.database) {
        // No positionals at all: SQL must come from piped stdin
        (None, _) => Ok((args.database.clone(), SqlSource::Stdin)),
        (Some(arg0), None) => {
            if arg0 == "-" {
                return Ok((None, SqlSource::Stdin));
            }
            // A lone database-looking positional with piped stdin: it's the
            // database, and the SQL comes from stdin. A db-extension arg
            // that doesn't exist is an error, not a file to create.
            let p = Path::new(arg0);
            if stdin_piped
                && (arg0 == ":memory:"
                    || is_remote_url(arg0)
                    || crate::cli::is_database_path(p)
                    || (p.exists() && !is_sql_file(p)))
            {
                return validate_stdin_database(arg0).map(|db| (Some(db), SqlSource::Stdin));
            }
            // Check if the statement arg is actually an ssh:// URL (user put db first)
            if is_remote_url(arg0) {
                return Err(QueryError::ExecutionFailed(
                    "Usage: solite query <sql> <database>".to_string(),
                ));
            }
            Ok((None, SqlSource::Inline(arg0.clone())))
        }
        (Some(arg0), Some(arg1)) => {
            let arg1_str = arg1.to_string_lossy();

            // `-` marks SQL-from-stdin; the other positional is the database
            if arg0 == "-" && arg1_str == "-" {
                return Err(QueryError::Stdin(
                    "only one `-` stdin placeholder is allowed".to_string(),
                ));
            }
            if arg0 == "-" {
                return validate_stdin_database(&arg1_str).map(|db| (Some(db), SqlSource::Stdin));
            }
            if arg1_str == "-" {
                return validate_stdin_database(arg0).map(|db| (Some(db), SqlSource::Stdin));
            }

            // If either arg looks like an ssh:// URL, treat it as the database
            if is_remote_url(&arg1_str) {
                Ok((Some(arg1.clone()), SqlSource::Inline(arg0.clone())))
            } else if is_remote_url(arg0) {
                let sql = arg1_str.to_string();
                Ok((Some(PathBuf::from(arg0)), SqlSource::Inline(sql)))
            } else if is_sql_file(arg1) {
                // .sql file as second arg is SQL, first arg is database
                let p = PathBuf::from(arg0);
                Ok((Some(p), SqlSource::Inline(arg1_str.to_string())))
            } else if is_sql_file(Path::new(arg0)) {
                // .sql file as first arg is SQL, second arg is database
                Ok((Some(arg1.clone()), SqlSource::Inline(arg0.clone())))
            } else if arg1_str == ":memory:" {
                // SQLite opens `:memory:` natively as an in-memory database;
                // it never exists on disk, so check before Path::exists()
                Ok((Some(arg1.clone()), SqlSource::Inline(arg0.clone())))
            } else if arg0 == ":memory:" {
                Ok((
                    Some(PathBuf::from(arg0)),
                    SqlSource::Inline(arg1_str.to_string()),
                ))
            } else if arg1.exists() {
                Ok((Some(arg1.clone()), SqlSource::Inline(arg0.clone())))
            } else {
                let p = PathBuf::from(arg0);
                if !p.exists() {
                    // Neither arg exists on disk: blame the one that looks
                    // like a database. The database is the documented second
                    // positional, so default to blaming it unless only the
                    // first arg has a database-ish extension.
                    let blamed = if crate::cli::is_database_path(&p)
                        && !crate::cli::is_database_path(arg1)
                    {
                        p
                    } else {
                        arg1.clone()
                    };
                    return Err(QueryError::DatabaseNotFound(blamed));
                }
                let sql = arg1
                    .to_str()
                    .ok_or_else(|| QueryError::InvalidPath(arg1.clone()))?
                    .to_string();
                Ok((Some(p), SqlSource::Inline(sql)))
            }
        }
    }
}

/// Prepare a statement, using replacement scan as fallback.
fn prepare_statement(
    runtime: &mut Runtime,
    sql: &str,
) -> Result<solite_core::sqlite::Statement, QueryError> {
    loop {
        match runtime.prepare_with_parameters(sql) {
            Ok((rest, Some(stmt))) => {
                // `query` can only print a single result set; reject input
                // that contains a second statement instead of silently
                // dropping it. Trailing comments/whitespace are fine: they
                // re-prepare to `(_, None)`.
                if let Some(offset) = rest {
                    match runtime.prepare_with_parameters(&sql[offset..]) {
                        Ok((_, None)) => {}
                        _ => return Err(QueryError::TrailingSql),
                    }
                }
                return Ok(stmt);
            }
            Ok((_, None)) => return Err(QueryError::EmptyPrepare),
            Err(err) => {
                // Try replacement scan
                match replacement_scan(&err, &runtime.connection) {
                    Some(Ok(stmt)) => {
                        stmt.execute()
                            .map_err(|e| QueryError::ExecutionFailed(format!("{:?}", e)))?;
                        // Continue loop to re-prepare
                        continue;
                    }
                    Some(Err(e)) => return Err(QueryError::ReplacementScanFailed(e.message)),
                    None => {
                        crate::errors::report_error("[input]", sql, &err, None);
                        return Err(QueryError::SqlError);
                    }
                }
            }
        }
    }
}

/// Determine the output format from arguments.
fn determine_format(args: &QueryArgs) -> ExportFormat {
    match &args.format {
        Some(format) => (*format).into(),
        None => match &args.output {
            Some(p) => solite_core::exporter::format_from_path(p).unwrap_or(ExportFormat::Json),
            None => ExportFormat::Json,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_query_error_display() {
        let err = QueryError::DatabaseNotFound(PathBuf::from("/tmp/test.db"));
        assert!(err.to_string().contains("/tmp/test.db"));

        let err = QueryError::ParameterSet("invalid value".to_string());
        assert!(err.to_string().contains("invalid value"));

        let err = QueryError::EmptyPrepare;
        assert!(err.to_string().contains("no statement"));
    }

    #[test]
    fn test_determine_format_explicit() {
        let args = QueryArgs {
            statement: Some("SELECT 1".to_string()),
            database: None,
            format: Some(crate::cli::QueryFormat::Csv),
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let format = determine_format(&args);
        assert!(matches!(format, ExportFormat::Csv));
    }

    #[test]
    fn test_determine_format_from_path() {
        let args = QueryArgs {
            statement: Some("SELECT 1".to_string()),
            database: None,
            format: None,
            output: Some(PathBuf::from("output.csv")),
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let format = determine_format(&args);
        assert!(matches!(format, ExportFormat::Csv));
    }

    #[test]
    fn test_determine_format_default() {
        let args = QueryArgs {
            statement: Some("SELECT 1".to_string()),
            database: None,
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let format = determine_format(&args);
        assert!(matches!(format, ExportFormat::Json));
    }

    #[test]
    fn test_resolve_sql_inline() {
        let result = resolve_sql("SELECT 1".to_string()).unwrap();
        assert_eq!(result, "SELECT 1");
    }

    #[test]
    fn test_resolve_sql_from_file() {
        let dir = std::env::temp_dir().join("solite_test_resolve_sql");
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("query.sql");
        fs::write(&file, "  SELECT 42;\n").unwrap();

        let result = resolve_sql(file.to_string_lossy().to_string()).unwrap();
        assert_eq!(result, "SELECT 42;");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_sql_nonexistent_file() {
        let result = resolve_sql("/tmp/solite_does_not_exist.sql".to_string());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, QueryError::FileReadError(..)));
        assert!(err.to_string().contains("solite_does_not_exist.sql"));
    }

    #[test]
    fn test_resolve_sql_non_sql_extension_not_read() {
        // A string ending in .db should not be treated as a SQL file,
        // even if it happens to exist on disk
        let result = resolve_sql(":memory:".to_string()).unwrap();
        assert_eq!(result, ":memory:");
    }

    #[test]
    fn test_parse_arguments_sql_file_only() {
        let args = QueryArgs {
            statement: Some("test.sql".to_string()),
            database: None,
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert!(db.is_none());
        assert_eq!(sql, SqlSource::Inline("test.sql".to_string()));
    }

    #[test]
    fn test_parse_arguments_sql_file_then_db() {
        let args = QueryArgs {
            statement: Some("query.sql".to_string()),
            database: Some(PathBuf::from("data.db")),
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from("data.db"));
        assert_eq!(sql, SqlSource::Inline("query.sql".to_string()));
    }

    #[test]
    fn test_parse_arguments_db_then_sql_file() {
        let args = QueryArgs {
            statement: Some(":memory:".to_string()),
            database: Some(PathBuf::from("query.sql")),
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, SqlSource::Inline("query.sql".to_string()));
    }

    fn query_args(statement: &str, database: Option<&str>) -> QueryArgs {
        QueryArgs {
            statement: Some(statement.to_string()),
            database: database.map(PathBuf::from),
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        }
    }

    /// parse_arguments with stdin treated as a terminal (not piped).
    fn parse_arguments_no_stdin(
        args: &QueryArgs,
    ) -> Result<(Option<PathBuf>, SqlSource), QueryError> {
        parse_arguments(args, false)
    }

    #[test]
    fn test_parse_arguments_memory_as_second_arg() {
        let (db, sql) = parse_arguments_no_stdin(&query_args("select 1", Some(":memory:"))).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, SqlSource::Inline("select 1".to_string()));
    }

    #[test]
    fn test_parse_arguments_memory_as_first_arg() {
        let (db, sql) = parse_arguments_no_stdin(&query_args(":memory:", Some("select 1"))).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, SqlSource::Inline("select 1".to_string()));
    }

    #[test]
    fn test_parse_arguments_missing_db_blames_db_arg() {
        // The error must name the database-looking argument, not the SQL text
        let err = parse_arguments_no_stdin(&query_args("select 1", Some("/tmp/solite_definitely_nonexistent.db")))
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("solite_definitely_nonexistent.db"), "{msg}");
        assert!(!msg.contains("select 1"), "{msg}");
    }

    #[test]
    fn test_parse_arguments_missing_db_first_arg_blamed() {
        // db-ish extension on the first arg, plain SQL second: blame the db arg
        let err = parse_arguments_no_stdin(&query_args(
            "/tmp/solite_definitely_nonexistent.db",
            Some("select 1"),
        ))
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("solite_definitely_nonexistent.db"), "{msg}");
        assert!(!msg.contains("select 1"), "{msg}");
    }

    #[test]
    fn test_parse_arguments_dash_reads_stdin() {
        let (db, sql) = parse_arguments(&query_args("-", None), true).unwrap();
        assert!(db.is_none());
        assert_eq!(sql, SqlSource::Stdin);
    }

    #[test]
    fn test_parse_arguments_dash_with_database() {
        let dir = std::env::temp_dir().join("solite_test_dash_db");
        let _ = fs::create_dir_all(&dir);
        let db_file = dir.join("data.db");
        fs::write(&db_file, "").unwrap();
        let db_str = db_file.to_str().unwrap();

        let (db, sql) = parse_arguments(&query_args("-", Some(db_str)), true).unwrap();
        assert_eq!(db.unwrap(), db_file);
        assert_eq!(sql, SqlSource::Stdin);

        // database-first order also works
        let (db, sql) = parse_arguments(&query_args(db_str, Some("-")), true).unwrap();
        assert_eq!(db.unwrap(), db_file);
        assert_eq!(sql, SqlSource::Stdin);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_arguments_dash_with_missing_database_errors() {
        // query is read-only: `-` must never silently create a database
        // file from the other positional
        let err =
            parse_arguments(&query_args("-", Some("nope_does_not_exist.db")), true).unwrap_err();
        assert!(matches!(err, QueryError::DatabaseNotFound(_)), "{err}");

        // ...even when the other positional doesn't look like a database
        // (e.g. `solite q "select 1" -` with the args swapped by mistake)
        let err = parse_arguments(&query_args("select 1", Some("-")), true).unwrap_err();
        assert!(matches!(err, QueryError::DatabaseNotFound(_)), "{err}");

        // :memory: needs no existence check
        let (db, sql) = parse_arguments(&query_args(":memory:", Some("-")), true).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, SqlSource::Stdin);
    }

    #[test]
    fn test_parse_arguments_no_statement_uses_stdin() {
        let args = QueryArgs {
            statement: None,
            database: None,
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            blob_limit: None,
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args, true).unwrap();
        assert!(db.is_none());
        assert_eq!(sql, SqlSource::Stdin);
    }

    #[test]
    fn test_parse_arguments_lone_database_with_piped_stdin() {
        // With piped stdin, a lone database-looking positional is the database
        let (db, sql) = parse_arguments(&query_args(":memory:", None), true).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, SqlSource::Stdin);

        let dir = std::env::temp_dir().join("solite_test_lone_db");
        let _ = fs::create_dir_all(&dir);
        let db_file = dir.join("app.db");
        fs::write(&db_file, "").unwrap();
        let (db, sql) =
            parse_arguments(&query_args(db_file.to_str().unwrap(), None), true).unwrap();
        assert_eq!(db.unwrap(), db_file);
        assert_eq!(sql, SqlSource::Stdin);
        let _ = fs::remove_dir_all(&dir);

        // A db-extension positional that doesn't exist errors instead of
        // being silently created (query is read-only)
        let err = parse_arguments(&query_args("nope_does_not_exist.db", None), true).unwrap_err();
        assert!(matches!(err, QueryError::DatabaseNotFound(_)), "{err}");

        // A plain SQL positional stays SQL even when stdin is piped
        let (db, sql) = parse_arguments(&query_args("select 1", None), true).unwrap();
        assert!(db.is_none());
        assert_eq!(sql, SqlSource::Inline("select 1".to_string()));
    }

    #[test]
    fn test_parse_arguments_two_dashes_error() {
        assert!(parse_arguments(&query_args("-", Some("-")), true).is_err());
    }

    #[test]
    fn test_prepare_statement_rejects_trailing_sql() {
        let mut runtime = Runtime::new(None).unwrap();
        let result = prepare_statement(&mut runtime, "select 1; select 2");
        assert!(matches!(result, Err(QueryError::TrailingSql)));
    }

    #[test]
    fn test_prepare_statement_allows_trailing_comment() {
        let mut runtime = Runtime::new(None).unwrap();
        assert!(prepare_statement(&mut runtime, "select 1; -- done").is_ok());
        assert!(prepare_statement(&mut runtime, "select 1;   ").is_ok());
        assert!(prepare_statement(&mut runtime, "select 1").is_ok());
    }

    #[test]
    fn test_trailing_sql_display_mentions_run() {
        let msg = QueryError::TrailingSql.to_string();
        assert!(msg.contains("solite run"));
    }

    #[test]
    fn test_file_read_error_display() {
        let err = QueryError::FileReadError(
            PathBuf::from("/tmp/test.sql"),
            std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        );
        let msg = err.to_string();
        assert!(msg.contains("/tmp/test.sql"));
        assert!(msg.contains("file not found"));
    }
}
