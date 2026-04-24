//! Code generation from annotated SQL files.
//!
//! This module provides functionality to parse SQL files with special annotations
//! and generate a structured report of queries that can be used for code generation
//! in various languages.
//!
//! # Annotation Format
//!
//! Queries are annotated with special comments:
//!
//! ```sql
//! -- name: getUserById :row
//! SELECT * FROM users WHERE id = $id::int;
//! ```
//!
//! The annotation format is:
//! - `-- name: <name>` - Required. Names the query for code generation.
//! - `:rows` - Returns multiple rows (default if columns exist)
//! - `:row` - Returns exactly one row
//! - `:value` - Returns a single value
//! - `:list` - Returns a list of single values
//! - `-> ClassName` - Optional. Attaches a shared result-class name. Multiple
//!   queries may share a class when their column shapes match; codegen rejects
//!   mismatched shapes with an explicit error. The arrow must come last on the
//!   line; trailing annotations are rejected.
//!
//! ```sql
//! -- name: listWorkbooks :rows -> Workbook
//! SELECT * FROM workbooks;
//!
//! -- name: getWorkbook :row -> Workbook
//! SELECT * FROM workbooks WHERE id = $id::int;
//! ```
//!
//! # Parameter Types and Nullability
//!
//! Parameters can be annotated with a type using `::type` and marked
//! not-required/nullable with a trailing `::`. The two markers are
//! orthogonal:
//!
//! | Syntax            | Type     | Nullable |
//! |-------------------|----------|----------|
//! | `$name`           | none     | no       |
//! | `$name::`         | none     | yes      |
//! | `$name::text`     | `text`   | no       |
//! | `$name::text::`   | `text`   | yes      |
//!
//! ```sql
//! -- name: insertUser :row
//! INSERT INTO users (name, email, nickname)
//! VALUES ($name::text, $email::text, $nickname::text::)
//! RETURNING *;
//! ```
//!
//! Both `$` and `:` prefixes are supported. `solite codegen` only surfaces
//! the `nullable` flag in the IR; enforcement (raise if missing, default to
//! NULL, etc.) is left to downstream generators.
//!
//! # Bind-key gotcha
//!
//! SQLite treats the full string after the prefix as the bind name, so
//! `$name::text::` binds to the key `name::text::` — not `name`. Generators
//! that construct parameter dicts for sqlite3 should use the IR's `full_name`
//! (minus the leading `$`/`:`) as the key.
//!
//! # Schema Support
//!
//! The codegen command supports loading a schema from:
//! - An existing SQLite database (`.db` file)
//! - A SQL file with CREATE statements (`.sql` file)
//!
//! This allows the codegen to validate queries and extract column types.
//!
//! # Example Usage
//!
//! ```bash
//! solite codegen queries.sql --schema schema.sql
//! ```

mod report;
mod types;

pub use report::{report_from_file, BaseDatabaseType};
use crate::cli::CodegenArgs;

/// Entry point for the codegen command.
pub(crate) fn codegen(cmd: CodegenArgs) -> Result<(), ()> {
    let db_type = determine_db_type(&cmd.schema);
    let db_type = match db_type {
        Ok(t) => t,
        Err(msg) => {
            eprintln!("{}", msg);
            return Err(());
        }
    };

    let src = match std::fs::read_to_string(&cmd.file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", cmd.file.display(), e);
            return Err(());
        }
    };

    let report = match report_from_file(&src, &cmd.file, db_type) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            return Err(());
        }
    };

    let json = match serde_json::to_string_pretty(&report) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Failed to serialize report: {}", e);
            return Err(());
        }
    };

    match cmd.output {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, &json) {
                eprintln!("Failed to write {}: {}", path.display(), e);
                return Err(());
            }
            Ok(())
        }
        None => {
            println!("{}", json);
            Ok(())
        }
    }
}

/// Determine the database type from the schema path.
fn determine_db_type(
    schema: &Option<std::path::PathBuf>,
) -> Result<BaseDatabaseType, String> {
    match schema {
        Some(path) if path.extension().is_some_and(|ext| ext == "db") => {
            Ok(BaseDatabaseType::Database(path.clone()))
        }
        Some(path) if path.extension().is_some_and(|ext| ext == "sql") => {
            Ok(BaseDatabaseType::SqlFile(path.clone()))
        }
        Some(path) => Err(format!(
            "Unsupported schema file type: {}. Use .db or .sql",
            path.display()
        )),
        None => Ok(BaseDatabaseType::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::types::Report;
    use insta::assert_yaml_snapshot;
    use std::path::PathBuf;

    fn report(src: &str) -> Report {
        report_from_file(src, &PathBuf::from("[test]"), BaseDatabaseType::None)
            .expect("report should succeed")
    }

    #[test]
    fn test_simple_export() {
        assert_yaml_snapshot!(report("-- name: xxx\nselect 1, 2, 3;"));
    }

    #[test]
    fn test_multiple_exports() {
        assert_yaml_snapshot!(report(
            r#"
            create table t(a, b text, c int);

            -- name: getA
            select a from t;

            -- name: getB
            select b from t;

            -- name: getC
            select c from t;
            "#
        ));
    }

    #[test]
    fn test_with_parameters() {
        assert_yaml_snapshot!(report(
            r#"
            create table t(a, b text, c int);

            -- name: withParams :list
            select c from t where a = $a::text and b = $b::text;
            "#
        ));
    }

    #[test]
    fn test_result_type_row() {
        assert_yaml_snapshot!(report(
            r#"
            create table users(id int, name text);

            -- name: getUserById :row
            select * from users where id = $id;
            "#
        ));
    }

    #[test]
    fn test_result_type_value() {
        assert_yaml_snapshot!(report(
            r#"
            create table users(id int, name text);

            -- name: countUsers :value
            select count(*) from users;
            "#
        ));
    }

    #[test]
    fn test_void_result() {
        assert_yaml_snapshot!(report(
            r#"
            create table users(id int, name text);

            -- name: deleteUser
            delete from users where id = $id;
            "#
        ));
    }

    #[test]
    fn test_setup_statements() {
        let r = report(
            r#"
            create table foo(a, b);
            create table bar(x, y);

            -- name: query
            select * from foo, bar;
            "#,
        );
        assert_eq!(r.setup.len(), 2);
        assert!(r.setup[0].contains("foo"));
        assert!(r.setup[1].contains("bar"));
    }

    #[test]
    fn test_parameter_types() {
        let r = report(
            r#"
            create table t(a text, b int);

            -- name: insert
            insert into t values ($a::text, $b::int);
            "#,
        );
        assert_eq!(r.exports.len(), 1);
        let params = &r.exports[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].annotated_type, Some("text".to_string()));
        assert!(!params[0].nullable);
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].annotated_type, Some("int".to_string()));
        assert!(!params[1].nullable);
    }

    #[test]
    fn test_nullable_parameters() {
        assert_yaml_snapshot!(report(
            r#"
            create table t(a text, b text, c text, d text);

            -- name: mixedParams
            insert into t values ($a, $b::, $c::text, $d::text::);
            "#
        ));
    }

    #[test]
    fn test_result_class_matching_shapes() {
        assert_yaml_snapshot!(report(
            r#"
            create table workbooks(id integer primary key, name text not null);

            -- name: listWorkbooks :rows -> Workbook
            select id, name from workbooks;

            -- name: getWorkbook :row -> Workbook
            select id, name from workbooks where id = $id::int;
            "#
        ));
    }

    #[test]
    fn test_result_class_mismatched_shapes_errors() {
        let err = report_from_file(
            r#"
            create table workbooks(id integer primary key, name text not null, deleted_at text);

            -- name: listWorkbooks :rows -> Workbook
            select id, name, deleted_at from workbooks;

            -- name: getWorkbook :row -> Workbook
            select id, name from workbooks where id = $id::int;
            "#,
            &PathBuf::from("[test]"),
            BaseDatabaseType::None,
        )
        .expect_err("should error on shape mismatch");

        let msg = err.to_string();
        assert!(msg.contains("Workbook"), "error mentions class: {msg}");
        assert!(msg.contains("listWorkbooks"), "error mentions first query: {msg}");
        assert!(msg.contains("getWorkbook"), "error mentions second query: {msg}");
        assert!(msg.contains("column count differs"), "error describes the diff: {msg}");
    }

    #[test]
    fn test_result_class_without_hint_no_validation() {
        // Two queries with identical shapes but no -> hint; should coexist fine.
        let r = report(
            r#"
            create table workbooks(id integer primary key, name text not null);

            -- name: listWorkbooks :rows
            select id, name from workbooks;

            -- name: listWorkbooksAgain :rows
            select id, name from workbooks;
            "#,
        );
        assert_eq!(r.exports.len(), 2);
        assert!(r.exports.iter().all(|e| e.result_class.is_none()));
    }

    #[test]
    fn test_nullable_parameter_fields() {
        let r = report(
            r#"
            create table t(a text, b text, c text, d text);

            -- name: mixed
            insert into t values ($a, $b::, $c::text, $d::text::);
            "#,
        );
        let params = &r.exports[0].parameters;
        assert_eq!(params.len(), 4);

        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].annotated_type, None);
        assert!(!params[0].nullable);

        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].annotated_type, None);
        assert!(params[1].nullable);

        assert_eq!(params[2].name, "c");
        assert_eq!(params[2].annotated_type, Some("text".to_string()));
        assert!(!params[2].nullable);

        assert_eq!(params[3].name, "d");
        assert_eq!(params[3].annotated_type, Some("text".to_string()));
        assert!(params[3].nullable);
    }

    #[test]
    fn test_colon_prefix_parameter() {
        let r = report(
            r#"
            create table t(a text);

            -- name: query
            select * from t where a = :name::text;
            "#,
        );
        let params = &r.exports[0].parameters;
        assert_eq!(params[0].name, "name");
        assert_eq!(params[0].annotated_type, Some("text".to_string()));
    }
}
