use anyhow::{bail, Result};
use solite_core::Runtime;
use std::path::PathBuf;

use crate::cli::ExecuteArgs;

pub(crate) fn exec(args: ExecuteArgs) -> Result<(), ()> {
    match exec_impl(args) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e:?}");
            Err(())
        }
    }
}

fn exec_impl(args: ExecuteArgs) -> Result<()> {
    let (db_path, statements) = parse_arguments(&args);

    if statements.is_empty() {
        bail!("No SQL statements provided");
    }

    let mut runtime = Runtime::new(db_path.map(|p| p.to_string_lossy().to_string()));

    // Set parameters
    for chunk in args.parameters.chunks(2) {
        if chunk.len() == 2 {
            runtime
                .define_parameter(chunk[0].clone(), chunk[1].clone())
                .map_err(|e| anyhow::anyhow!("Failed to set parameter: {e}"))?;
        }
    }

    // Wrap all statements in a transaction
    runtime
        .connection
        .execute_script("BEGIN")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    for sql in &statements {
        if let Err(err) = execute_sql(&mut runtime, sql) {
            let _ = runtime.connection.execute_script("ROLLBACK");
            return Err(err);
        }
    }

    runtime
        .connection
        .execute_script("COMMIT")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("✔︎");
    Ok(())
}

/// Parse arguments to determine database path and SQL statements.
///
/// Uses the same logic as query's parse_arguments: if the first positional arg
/// exists as a file, it's the database. Otherwise, check if the first statement
/// arg exists as a file (swap). If neither exists, treat everything as SQL.
fn parse_arguments(args: &ExecuteArgs) -> (Option<PathBuf>, Vec<String>) {
    match &args.database {
        None => (None, args.statement.clone()),
        Some(arg0) => {
            if arg0.exists() {
                // First arg is a database path
                (Some(arg0.clone()), args.statement.clone())
            } else if let Some(first_stmt) = args.statement.first() {
                let p = PathBuf::from(first_stmt);
                if p.exists() {
                    // First statement is actually the database, arg0 is SQL
                    let mut stmts = vec![arg0.to_string_lossy().to_string()];
                    stmts.extend(args.statement[1..].iter().cloned());
                    (Some(p), stmts)
                } else {
                    // Neither exists as file — all SQL, no database
                    let mut stmts = vec![arg0.to_string_lossy().to_string()];
                    stmts.extend(args.statement.clone());
                    (None, stmts)
                }
            } else {
                // Only one arg, doesn't exist as file — it's SQL
                (None, vec![arg0.to_string_lossy().to_string()])
            }
        }
    }
}

/// Execute one or more SQL statements from a single string.
/// Handles strings containing multiple semicolon-separated statements.
fn execute_sql(runtime: &mut Runtime, sql: &str) -> Result<()> {
    let mut offset = 0;
    let full_sql = sql;

    loop {
        let remaining = &full_sql[offset..];
        if remaining.trim().is_empty() {
            break;
        }

        match runtime.prepare_with_parameters(remaining) {
            Ok((tail, Some(stmt))) => {
                stmt.execute()
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                match tail {
                    Some(tail_offset) => offset += tail_offset,
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ExecuteArgs;

    fn make_args(database: Option<&str>, statements: Vec<&str>) -> ExecuteArgs {
        ExecuteArgs {
            database: database.map(PathBuf::from),
            statement: statements.into_iter().map(String::from).collect(),
            output: None,
            format: None,
            parameters: vec![],
        }
    }

    fn query_val(rt: &solite_core::Runtime, sql: &str) -> String {
        let (_, stmt) = rt.connection.prepare(sql).unwrap();
        let stmt = stmt.unwrap();
        let row = stmt.next().unwrap().unwrap();
        row.first().unwrap().as_str().to_string()
    }

    // --- parse_arguments tests ---

    #[test]
    fn parse_args_sql_only() {
        let args = make_args(None, vec!["INSERT INTO t VALUES (1)"]);
        let (db, stmts) = parse_arguments(&args);
        assert!(db.is_none());
        assert_eq!(stmts, vec!["INSERT INTO t VALUES (1)"]);
    }

    #[test]
    fn parse_args_all_sql_when_no_files_exist() {
        let args = make_args(
            Some("CREATE TABLE t(a)"),
            vec!["INSERT INTO t VALUES (1)"],
        );
        let (db, stmts) = parse_arguments(&args);
        assert!(db.is_none());
        assert_eq!(stmts, vec!["CREATE TABLE t(a)", "INSERT INTO t VALUES (1)"]);
    }

    #[test]
    fn parse_args_first_arg_is_db_when_exists() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(
            Some(db_path.to_str().unwrap()),
            vec!["INSERT INTO t VALUES (1)"],
        );
        let (db, stmts) = parse_arguments(&args);
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(stmts, vec!["INSERT INTO t VALUES (1)"]);
    }

    #[test]
    fn parse_args_swap_when_second_arg_is_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(
            Some("INSERT INTO t VALUES (1)"),
            vec![db_path.to_str().unwrap()],
        );
        let (db, stmts) = parse_arguments(&args);
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(stmts, vec!["INSERT INTO t VALUES (1)"]);
    }

    #[test]
    fn parse_args_single_sql_no_file() {
        let args = make_args(Some("CREATE TABLE t(a)"), vec![]);
        let (db, stmts) = parse_arguments(&args);
        assert!(db.is_none());
        assert_eq!(stmts, vec!["CREATE TABLE t(a)"]);
    }

    // --- execute_sql tests ---

    #[test]
    fn execute_single_statement() {
        let mut rt = Runtime::new(None);
        rt.connection
            .execute_script("CREATE TABLE t(a INTEGER)")
            .unwrap();
        execute_sql(&mut rt, "INSERT INTO t VALUES (42)").unwrap();

        assert_eq!(query_val(&rt, "SELECT a FROM t"), "42");
    }

    #[test]
    fn execute_multiple_semicolon_separated() {
        let mut rt = Runtime::new(None);
        rt.connection
            .execute_script("CREATE TABLE t(a INTEGER)")
            .unwrap();
        execute_sql(
            &mut rt,
            "INSERT INTO t VALUES (1); INSERT INTO t VALUES (2);",
        )
        .unwrap();

        assert_eq!(query_val(&rt, "SELECT count(*) FROM t"), "2");
    }

    #[test]
    fn execute_sql_error_reported() {
        let mut rt = Runtime::new(None);
        let result = execute_sql(&mut rt, "INSERT INTO nonexistent VALUES (1)");
        assert!(result.is_err());
    }

    // --- exec_impl transaction tests ---

    #[test]
    fn exec_commits_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("commit.db");
        let db_str = db_path.to_str().unwrap();

        // Create the database file so parse_arguments recognizes it
        { Runtime::new(Some(db_str.to_string())); }

        let args = ExecuteArgs {
            database: Some(db_path.clone()),
            statement: vec![
                "CREATE TABLE t(a INTEGER)".into(),
                "INSERT INTO t VALUES (1)".into(),
                "INSERT INTO t VALUES (2)".into(),
            ],
            output: None,
            format: None,
            parameters: vec![],
        };
        exec_impl(args).unwrap();

        let rt = Runtime::new(Some(db_str.to_string()));
        assert_eq!(query_val(&rt, "SELECT count(*) FROM t"), "2");
    }

    #[test]
    fn exec_rolls_back_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("rollback.db");
        let db_str = db_path.to_str().unwrap();

        // Seed with a table
        {
            let rt = Runtime::new(Some(db_str.to_string()));
            rt.connection
                .execute_script("CREATE TABLE t(a INTEGER PRIMARY KEY)")
                .unwrap();
        }

        // Try to insert two rows where the second is a duplicate — should rollback both
        let args = ExecuteArgs {
            database: Some(db_path.clone()),
            statement: vec![
                "INSERT INTO t VALUES (1)".into(),
                "INSERT INTO t VALUES (1)".into(), // duplicate PK
            ],
            output: None,
            format: None,
            parameters: vec![],
        };
        let result = exec_impl(args);
        assert!(result.is_err());

        // Verify nothing was committed
        let rt = Runtime::new(Some(db_str.to_string()));
        assert_eq!(query_val(&rt, "SELECT count(*) FROM t"), "0");
    }

    #[test]
    fn exec_multiple_args_all_in_one_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("multi.db");
        let db_str = db_path.to_str().unwrap();

        // Create the database file so parse_arguments recognizes it
        { Runtime::new(Some(db_str.to_string())); }

        let args = ExecuteArgs {
            database: Some(db_path.clone()),
            statement: vec![
                "CREATE TABLE a(x); CREATE TABLE b(y);".into(),
                "INSERT INTO a VALUES ('hello')".into(),
                "INSERT INTO b VALUES ('world')".into(),
            ],
            output: None,
            format: None,
            parameters: vec![],
        };
        exec_impl(args).unwrap();

        let rt = Runtime::new(Some(db_str.to_string()));
        assert_eq!(query_val(&rt, "SELECT x FROM a"), "hello");
        assert_eq!(query_val(&rt, "SELECT y FROM b"), "world");
    }
}
