use anyhow::{bail, Context, Result};
use solite_core::Runtime;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use super::{is_sql_file, read_sql_file};
use crate::cli::{is_database_path, ExecuteArgs};

pub(crate) fn exec(args: ExecuteArgs) -> Result<(), ()> {
    match exec_impl(args) {
        Ok(()) => Ok(()),
        Err(e) => {
            // "SQL error" means the diagnostic was already reported with
            // source context by report_error(); don't print it twice.
            if e.to_string() != "SQL error" {
                eprintln!("Error: {e:#}");
            }
            Err(())
        }
    }
}

/// The cumulative number of rows inserted/updated/deleted on this connection.
fn total_changes(runtime: &Runtime) -> Result<i64> {
    let (_, stmt) = runtime
        .connection
        .prepare("select total_changes()")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut stmt = stmt.ok_or_else(|| anyhow::anyhow!("failed to query total_changes()"))?;
    let row = stmt
        .next()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .ok_or_else(|| anyhow::anyhow!("failed to query total_changes()"))?;
    Ok(row.first().map(|v| v.as_int64()).unwrap_or(0))
}

fn exec_impl(args: ExecuteArgs) -> Result<()> {
    let stdin_piped = !std::io::stdin().is_terminal();
    let (db_path, sql) = parse_arguments(&args, stdin_piped)?;
    let sql = match sql {
        Some(sql) => sql,
        None => super::read_sql_from_stdin().map_err(|e| anyhow::anyhow!(e))?,
    };

    let mut runtime = Runtime::new(db_path.map(|p| p.to_string_lossy().to_string()))?;

    // Set parameters, inferring integer/real types from the value
    for chunk in args.parameters.chunks(2) {
        if chunk.len() == 2 {
            runtime
                .define_parameter_value(
                    chunk[0].clone(),
                    solite_core::infer_parameter_value(&chunk[1]),
                )
                .map_err(|e| anyhow::anyhow!("Failed to set parameter: {e}"))?;
        }
    }

    let changes_before = total_changes(&runtime)?;

    // Execute every statement in the input, not just the first: keep
    // preparing the remaining SQL until it is exhausted (comment-only or
    // whitespace-only remainders prepare to `(_, None)` and stop the loop).
    let mut remaining: &str = &sql;
    let mut executed_any = false;
    loop {
        match runtime.prepare_with_parameters(remaining) {
            Ok((rest, Some(stmt))) => {
                let mut stmt = stmt;
                // Statements with result columns (RETURNING clauses, or bare
                // SELECTs) have their rows rendered instead of swallowed.
                let has_columns = stmt.column_names().map(|c| !c.is_empty()).unwrap_or(false);
                if has_columns {
                    if std::io::stdout().is_terminal() {
                        let config = solite_table::TableConfig::terminal();
                        solite_table::print_statement(&mut stmt, &config)
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                    } else {
                        solite_core::exporter::write_output(
                            &mut stmt,
                            Box::new(std::io::stdout()),
                            solite_core::exporter::ExportFormat::Json,
                        )
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    }
                } else {
                    stmt.execute().map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                executed_any = true;
                match rest {
                    Some(offset) => remaining = &remaining[offset..],
                    None => break,
                }
            }
            Ok((_, None)) => break,
            Err(err) => {
                crate::errors::report_error("[input]", remaining, &err, None);
                bail!("SQL error");
            }
        }
    }
    if !executed_any {
        bail!("No SQL statement to execute");
    }

    let affected = total_changes(&runtime)? - changes_before;
    let row_word = if affected == 1 { "row" } else { "rows" };
    if std::io::stdout().is_terminal() {
        println!("✔︎ {affected} {row_word} affected");
    } else {
        println!("{affected} {row_word} affected");
    }
    Ok(())
}

/// Whether the arg should be classified as a database by extension alone
/// (so a not-yet-existing database can be created by `execute`).
fn is_database_arg(s: &str) -> bool {
    s == ":memory:" || is_database_path(Path::new(s))
}

/// Resolve a SQL argument: read `.sql` files, otherwise use the string as-is.
fn resolve_sql_arg(arg: &str) -> Result<String> {
    let path = Path::new(arg);
    if is_sql_file(path) {
        read_sql_file(path).with_context(|| format!("Failed to read SQL file '{arg}'"))
    } else {
        Ok(arg.to_string())
    }
}

/// Parse arguments to determine database path and SQL string.
///
/// Accepts 0-2 positional args in any order, classified by extension
/// first: `.db`/`.sqlite`/`.sqlite3` (or `:memory:`) is the database —
/// whether or not it exists on disk, so new databases can be created —
/// and `.sql` is a file whose contents are the SQL. Unclassified args fall
/// back to the existence check (the arg that exists is the database).
///
/// A returned SQL of `None` means "read the SQL from stdin" (`-`
/// placeholder, no args at all, or a lone database arg with piped stdin).
fn parse_arguments(args: &ExecuteArgs, stdin_piped: bool) -> Result<(Option<PathBuf>, Option<String>)> {
    match args.args.as_slice() {
        [] => Ok((None, None)),
        [only] => {
            if only == "-" {
                return Ok((None, None));
            }
            let p = PathBuf::from(only);
            if is_sql_file(&p) {
                Ok((None, Some(resolve_sql_arg(only)?)))
            } else if is_database_arg(only) || p.exists() {
                // A lone database arg: the SQL comes from stdin when piped
                let sql = if stdin_piped { None } else { Some(String::new()) };
                Ok((Some(p), sql))
            } else {
                Ok((None, Some(only.clone())))
            }
        }
        [first, second] => {
            // `-` marks SQL-from-stdin; the other positional is the database
            if first == "-" || second == "-" {
                if first == "-" && second == "-" {
                    bail!("only one `-` stdin placeholder is allowed");
                }
                let db_arg = if first == "-" { second } else { first };
                let p = PathBuf::from(db_arg);
                if is_sql_file(&p) {
                    bail!("cannot combine a .sql file with `-` stdin input");
                }
                if !(is_database_arg(db_arg) || p.exists()) {
                    bail!(
                        "database '{db_arg}' does not exist. To create a new database, \
                         give it a .db/.sqlite/.sqlite3 extension"
                    );
                }
                return Ok((Some(p), None));
            }
            let db0 = is_database_arg(first);
            let db1 = is_database_arg(second);
            let sql0 = is_sql_file(Path::new(first));
            let sql1 = is_sql_file(Path::new(second));

            if db0 && db1 {
                bail!(
                    "two database arguments given ('{first}' and '{second}'); \
                     expected one database and one SQL argument"
                );
            }
            if sql0 && sql1 {
                bail!(
                    "two .sql file arguments given ('{first}' and '{second}'); \
                     expected at most one SQL argument"
                );
            }
            if db0 {
                return Ok((Some(PathBuf::from(first)), Some(resolve_sql_arg(second)?)));
            }
            if db1 {
                return Ok((Some(PathBuf::from(second)), Some(resolve_sql_arg(first)?)));
            }

            let p0 = PathBuf::from(first);
            let p1 = PathBuf::from(second);
            if sql0 || sql1 {
                // One arg is a .sql file; the other must be the database.
                let (sql_arg, db, db_arg) =
                    if sql0 { (first, p1, second) } else { (second, p0, first) };
                if !db.exists() {
                    bail!(
                        "database '{db_arg}' does not exist. To create a new database, \
                         give it a .db/.sqlite/.sqlite3 extension"
                    );
                }
                return Ok((Some(db), Some(resolve_sql_arg(sql_arg)?)));
            }

            // Neither arg is classified by extension: the one that exists on
            // disk is the database.
            if p0.exists() {
                Ok((Some(p0), Some(second.clone())))
            } else if p1.exists() {
                Ok((Some(p1), Some(first.clone())))
            } else {
                bail!(
                    "cannot tell which argument is the database: neither '{first}' nor \
                     '{second}' exists on disk. To create a new database, give it a \
                     .db/.sqlite/.sqlite3 extension; to run multiple statements, pass \
                     them as a single argument"
                );
            }
        }
        _ => bail!("expected at most 2 arguments: a SQL statement and an optional database"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ExecuteArgs;

    fn make_args(args: Vec<&str>) -> ExecuteArgs {
        ExecuteArgs {
            args: args.into_iter().map(String::from).collect(),
            output: None,
            format: None,
            parameters: vec![],
        }
    }

    fn query_val(rt: &solite_core::Runtime, sql: &str) -> String {
        let (_, stmt) = rt.connection.prepare(sql).unwrap();
        let mut stmt = stmt.unwrap();
        let row = stmt.next().unwrap().unwrap();
        row.first().unwrap().as_str().to_string()
    }

    // --- parse_arguments tests ---

    #[test]
    fn parse_args_sql_only() {
        let args = make_args(vec!["INSERT INTO t VALUES (1)"]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert!(db.is_none());
        assert_eq!(sql.as_deref(), Some("INSERT INTO t VALUES (1)"));
    }

    #[test]
    fn parse_args_db_first() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec![db_path.to_str().unwrap(), "INSERT INTO t VALUES (1)"]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(sql.as_deref(), Some("INSERT INTO t VALUES (1)"));
    }

    #[test]
    fn parse_args_db_second() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec!["INSERT INTO t VALUES (1)", db_path.to_str().unwrap()]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(sql.as_deref(), Some("INSERT INTO t VALUES (1)"));
    }

    #[test]
    fn parse_args_neither_is_file_errors() {
        // Nothing is silently ignored: two unclassifiable args are an error
        let args = make_args(vec!["CREATE TABLE t(a)", "INSERT INTO t VALUES (1)"]);
        let err = parse_arguments(&args, false).unwrap_err();
        assert!(err.to_string().contains("cannot tell"), "{err}");
    }

    #[test]
    fn parse_args_nonexistent_db_extension_is_database() {
        // A .db arg is the database even if it doesn't exist yet
        let args = make_args(vec!["/tmp/solite_brand_new_db_does_not_exist.db", "CREATE TABLE t(a)"]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(
            db.unwrap(),
            PathBuf::from("/tmp/solite_brand_new_db_does_not_exist.db")
        );
        assert_eq!(sql.as_deref(), Some("CREATE TABLE t(a)"));
    }

    #[test]
    fn parse_args_memory_is_database() {
        let args = make_args(vec![":memory:", "CREATE TABLE t(a)"]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert_eq!(sql.as_deref(), Some("CREATE TABLE t(a)"));
    }

    #[test]
    fn parse_args_sql_file_is_sql() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("script.sql");
        std::fs::write(&script, "CREATE TABLE t(a);\n").unwrap();
        let db_path = dir.path().join("data.db");

        // .sql file + .db database (db doesn't exist yet)
        let args = make_args(vec![script.to_str().unwrap(), db_path.to_str().unwrap()]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(sql.as_deref(), Some("CREATE TABLE t(a);"));

        // single .sql file arg: contents are the SQL, in-memory database
        let args = make_args(vec![script.to_str().unwrap()]);
        let (db, sql) = parse_arguments(&args, false).unwrap();
        assert!(db.is_none());
        assert_eq!(sql.as_deref(), Some("CREATE TABLE t(a);"));
    }

    #[test]
    fn parse_args_sql_file_with_unrecognized_missing_db_errors() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("script.sql");
        std::fs::write(&script, "CREATE TABLE t(a);\n").unwrap();

        let args = make_args(vec![script.to_str().unwrap(), "newdb_without_extension"]);
        let err = parse_arguments(&args, false).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn parse_args_two_databases_errors() {
        let args = make_args(vec!["a.db", "b.db"]);
        let err = parse_arguments(&args, false).unwrap_err();
        assert!(err.to_string().contains("two database arguments"), "{err}");
    }

    #[test]
    fn parse_args_two_sql_files_errors() {
        let args = make_args(vec!["a.sql", "b.sql"]);
        let err = parse_arguments(&args, false).unwrap_err();
        assert!(err.to_string().contains("two .sql file arguments"), "{err}");
    }

    #[test]
    fn parse_args_dash_reads_stdin() {
        let (db, sql) = parse_arguments(&make_args(vec!["-"]), true).unwrap();
        assert!(db.is_none());
        assert!(sql.is_none());

        let (db, sql) = parse_arguments(&make_args(vec!["-", "new.db"]), true).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from("new.db"));
        assert!(sql.is_none());

        let (db, sql) = parse_arguments(&make_args(vec![":memory:", "-"]), true).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from(":memory:"));
        assert!(sql.is_none());
    }

    #[test]
    fn parse_args_no_args_reads_stdin() {
        let (db, sql) = parse_arguments(&make_args(vec![]), true).unwrap();
        assert!(db.is_none());
        assert!(sql.is_none());
    }

    #[test]
    fn parse_args_lone_db_with_piped_stdin() {
        let (db, sql) = parse_arguments(&make_args(vec!["app.db"]), true).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from("app.db"));
        assert!(sql.is_none());

        // without piped stdin, a lone db arg means "no SQL" (errors later)
        let (db, sql) = parse_arguments(&make_args(vec!["app.db"]), false).unwrap();
        assert_eq!(db.unwrap(), PathBuf::from("app.db"));
        assert_eq!(sql.as_deref(), Some(""));
    }

    #[test]
    fn parse_args_two_dashes_errors() {
        assert!(parse_arguments(&make_args(vec!["-", "-"]), true).is_err());
    }

    #[test]
    fn exec_creates_new_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("brand_new.db");
        assert!(!db_path.exists());

        let args = make_args(vec![
            db_path.to_str().unwrap(),
            "CREATE TABLE t(a); INSERT INTO t VALUES (7)",
        ]);
        exec_impl(args).unwrap();

        assert!(db_path.exists());
        let rt = Runtime::new(Some(db_path.to_str().unwrap().to_string())).unwrap();
        assert_eq!(query_val(&rt, "SELECT a FROM t"), "7");
    }

    // --- exec_impl tests ---

    #[test]
    fn exec_single_statement() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        {
            let rt = Runtime::new(Some(db_str.to_string())).unwrap();
            rt.connection
                .execute_script("CREATE TABLE t(a INTEGER)")
                .unwrap();
        }

        let args = ExecuteArgs {
            args: vec![
                db_str.to_string(),
                "INSERT INTO t VALUES (42)".into(),
            ],
            output: None,
            format: None,
            parameters: vec![],
        };
        exec_impl(args).unwrap();

        let rt = Runtime::new(Some(db_str.to_string())).unwrap();
        assert_eq!(query_val(&rt, "SELECT a FROM t"), "42");
    }

    #[test]
    fn exec_multi_statement() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec![
            db_str,
            "CREATE TABLE t(a INTEGER); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2)",
        ]);
        exec_impl(args).unwrap();

        let rt = Runtime::new(Some(db_str.to_string())).unwrap();
        assert_eq!(query_val(&rt, "SELECT count(*) FROM t"), "2");
    }

    #[test]
    fn exec_multi_statement_trailing_comment() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec![
            db_str,
            "CREATE TABLE t(a); INSERT INTO t VALUES (1); -- done",
        ]);
        exec_impl(args).unwrap();

        let rt = Runtime::new(Some(db_str.to_string())).unwrap();
        assert_eq!(query_val(&rt, "SELECT count(*) FROM t"), "1");
    }

    #[test]
    fn exec_multi_statement_error_in_second() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec![
            db_str,
            "CREATE TABLE t(a); INSERT INTO nonexistent VALUES (1)",
        ]);
        assert!(exec_impl(args).is_err());
    }

    #[test]
    fn total_changes_counts_writes() {
        let rt = Runtime::new(None).unwrap();
        assert_eq!(total_changes(&rt).unwrap(), 0);
        rt.connection
            .execute_script("CREATE TABLE t(a); INSERT INTO t VALUES (1),(2),(3)")
            .unwrap();
        assert_eq!(total_changes(&rt).unwrap(), 3);
        rt.connection.execute_script("DELETE FROM t WHERE a = 1").unwrap();
        assert_eq!(total_changes(&rt).unwrap(), 4);
    }

    #[test]
    fn exec_error_reported() {
        let args = ExecuteArgs {
            args: vec!["INSERT INTO nonexistent VALUES (1)".into()],
            output: None,
            format: None,
            parameters: vec![],
        };
        assert!(exec_impl(args).is_err());
    }
}
