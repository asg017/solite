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
    let (db_path, sql) = parse_arguments(&args);

    let mut runtime = Runtime::new(db_path.map(|p| p.to_string_lossy().to_string()));

    // Set parameters
    for chunk in args.parameters.chunks(2) {
        if chunk.len() == 2 {
            runtime
                .define_parameter(chunk[0].clone(), chunk[1].clone())
                .map_err(|e| anyhow::anyhow!("Failed to set parameter: {e}"))?;
        }
    }

    match runtime.prepare_with_parameters(&sql) {
        Ok((_, Some(stmt))) => {
            stmt.execute()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok((_, None)) => {
            bail!("No SQL statement to execute");
        }
        Err(err) => {
            crate::errors::report_error("[input]", &sql, &err, None);
            bail!("SQL error");
        }
    }

    println!("✔︎");
    Ok(())
}

/// Parse arguments to determine database path and SQL string.
///
/// Accepts 1 or 2 positional args in any order: if an arg exists as a file
/// it's the database, the other is SQL.
fn parse_arguments(args: &ExecuteArgs) -> (Option<PathBuf>, String) {
    match args.args.as_slice() {
        [only] => {
            let p = PathBuf::from(only);
            if p.exists() {
                (Some(p), String::new())
            } else {
                (None, only.clone())
            }
        }
        [first, second] => {
            let p0 = PathBuf::from(first);
            let p1 = PathBuf::from(second);
            if p0.exists() {
                (Some(p0), second.clone())
            } else if p1.exists() {
                (Some(p1), first.clone())
            } else {
                // Neither is a file — treat first as SQL, ignore second?
                // More likely: the db doesn't exist yet, treat the one that
                // looks like a path as the db. Fall back to first=sql.
                (None, first.clone())
            }
        }
        _ => (None, String::new()),
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
        let stmt = stmt.unwrap();
        let row = stmt.next().unwrap().unwrap();
        row.first().unwrap().as_str().to_string()
    }

    // --- parse_arguments tests ---

    #[test]
    fn parse_args_sql_only() {
        let args = make_args(vec!["INSERT INTO t VALUES (1)"]);
        let (db, sql) = parse_arguments(&args);
        assert!(db.is_none());
        assert_eq!(sql, "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn parse_args_db_first() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec![db_path.to_str().unwrap(), "INSERT INTO t VALUES (1)"]);
        let (db, sql) = parse_arguments(&args);
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(sql, "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn parse_args_db_second() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, "").unwrap();

        let args = make_args(vec!["INSERT INTO t VALUES (1)", db_path.to_str().unwrap()]);
        let (db, sql) = parse_arguments(&args);
        assert_eq!(db.unwrap(), db_path);
        assert_eq!(sql, "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn parse_args_neither_is_file() {
        let args = make_args(vec!["CREATE TABLE t(a)", "INSERT INTO t VALUES (1)"]);
        let (db, sql) = parse_arguments(&args);
        assert!(db.is_none());
        assert_eq!(sql, "CREATE TABLE t(a)");
    }

    // --- exec_impl tests ---

    #[test]
    fn exec_single_statement() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        {
            let rt = Runtime::new(Some(db_str.to_string()));
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

        let rt = Runtime::new(Some(db_str.to_string()));
        assert_eq!(query_val(&rt, "SELECT a FROM t"), "42");
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
