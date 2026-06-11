//! SQL query execution command.
//!
//! This module provides functionality to execute SQL queries and statements
//! from the command line, with support for various output formats.

use solite_core::{exporter::ExportFormat, replacement_scans::replacement_scan, Runtime};
use solite_table::TableConfig;
use std::{
    fmt,
    fs,
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
    ReplacementScanFailed,
    /// Statement execution failed.
    ExecutionFailed(String),
    /// SQL syntax or preparation error (already reported).
    SqlError,
    /// Failed to read SQL file.
    FileReadError(PathBuf, std::io::Error),
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
            QueryError::ReplacementScanFailed => write!(f, "Replacement scan failed"),
            QueryError::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            QueryError::SqlError => write!(f, "SQL error"),
            QueryError::FileReadError(path, err) => {
                write!(f, "Failed to read SQL file '{}': {}", path.display(), err)
            }
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
    if path.extension().is_some_and(|ext| ext == "sql") {
        if !path.exists() {
            return Err(QueryError::FileReadError(
                path.to_path_buf(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
            ));
        }
        let contents = fs::read_to_string(path)
            .map_err(|e| QueryError::FileReadError(path.to_path_buf(), e))?;
        Ok(contents.trim().to_string())
    } else {
        Ok(sql)
    }
}

/// Internal implementation of the query command.
fn query_impl(args: QueryArgs) -> Result<(), QueryError> {
    let (db_path, sql) = parse_arguments(&args)?;
    let sql = resolve_sql(sql)?;

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

    // Set parameters
    for chunk in args.parameters.chunks(2) {
        if chunk.len() == 2 {
            runtime
                .define_parameter(chunk[0].clone(), chunk[1].clone())
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

    // Set up output
    let output: Box<dyn Write> = match &args.output {
        Some(output) => solite_core::exporter::output_from_path(output)
            .map_err(|e| QueryError::ExecutionFailed(e.to_string()))?,
        None => Box::new(stdout()),
    };

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

        // Write output
        let mut stmt = stmt;
        solite_core::exporter::write_output(&mut stmt, output, format)
            .map_err(|e| QueryError::ExecutionFailed(e.to_string()))?;
    }

    Ok(())
}

/// Parse command line arguments to determine database path and SQL.
fn is_remote_url(s: &str) -> bool {
    solite_core::sqlite::is_remote_path(s)
}

fn is_sql_file(p: &Path) -> bool {
    p.extension().is_some_and(|ext| ext == "sql")
}

fn parse_arguments(args: &QueryArgs) -> Result<(Option<PathBuf>, String), QueryError> {
    match &args.database {
        None => {
            // Check if the statement arg is actually an ssh:// URL (user put db first)
            if is_remote_url(&args.statement) {
                return Err(QueryError::ExecutionFailed(
                    "Usage: solite query <sql> <database>".to_string(),
                ));
            }
            Ok((None, args.statement.clone()))
        }
        Some(arg1) => {
            let arg0 = &args.statement;
            let arg1_str = arg1.to_string_lossy();

            // If either arg looks like an ssh:// URL, treat it as the database
            if is_remote_url(&arg1_str) {
                Ok((Some(arg1.clone()), arg0.clone()))
            } else if is_remote_url(arg0) {
                let sql = arg1_str.to_string();
                Ok((Some(PathBuf::from(arg0)), sql))
            } else if is_sql_file(arg1) {
                // .sql file as second arg is SQL, first arg is database
                let p = PathBuf::from(arg0);
                Ok((Some(p), arg1_str.to_string()))
            } else if is_sql_file(Path::new(arg0)) {
                // .sql file as first arg is SQL, second arg is database
                Ok((Some(arg1.clone()), arg0.clone()))
            } else if arg1.exists() {
                Ok((Some(arg1.clone()), arg0.clone()))
            } else {
                let p = PathBuf::from(arg0);
                if !p.exists() {
                    return Err(QueryError::DatabaseNotFound(p));
                }
                let sql = arg1
                    .to_str()
                    .ok_or_else(|| QueryError::InvalidPath(arg1.clone()))?
                    .to_string();
                Ok((Some(p), sql))
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
            Ok((_, Some(stmt))) => return Ok(stmt),
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
                    Some(Err(_)) => return Err(QueryError::ReplacementScanFailed),
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
            statement: "SELECT 1".to_string(),
            database: None,
            format: Some(crate::cli::QueryFormat::Csv),
            output: None,
            load_extension: None,
            parameters: vec![],
            remote: Default::default(),
        };
        let format = determine_format(&args);
        assert!(matches!(format, ExportFormat::Csv));
    }

    #[test]
    fn test_determine_format_from_path() {
        let args = QueryArgs {
            statement: "SELECT 1".to_string(),
            database: None,
            format: None,
            output: Some(PathBuf::from("output.csv")),
            load_extension: None,
            parameters: vec![],
            remote: Default::default(),
        };
        let format = determine_format(&args);
        assert!(matches!(format, ExportFormat::Csv));
    }

    #[test]
    fn test_determine_format_default() {
        let args = QueryArgs {
            statement: "SELECT 1".to_string(),
            database: None,
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
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
            statement: "test.sql".to_string(),
            database: None,
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args).unwrap();
        assert!(db.is_none());
        assert_eq!(sql, "test.sql");
    }

    #[test]
    fn test_parse_arguments_sql_file_then_db() {
        let args = QueryArgs {
            statement: "query.sql".to_string(),
            database: Some(PathBuf::from("data.db")),
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from("data.db"));
        assert_eq!(sql, "query.sql");
    }

    #[test]
    fn test_parse_arguments_db_then_sql_file() {
        let args = QueryArgs {
            statement: ":memory:".to_string(),
            database: Some(PathBuf::from("query.sql")),
            format: None,
            output: None,
            load_extension: None,
            parameters: vec![],
            remote: Default::default(),
        };
        let (db, sql) = parse_arguments(&args).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql, "query.sql");
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
