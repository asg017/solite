//! Database schema introspection command.
//!
//! This module implements the `.schema` command which displays the CREATE
//! statements for all objects in the database.
//!
//! # Usage
//!
//! ```sql
//! .schema            -- all objects
//! .schema users      -- only `users` and objects on it (indexes, triggers)
//! .schema 'idx_%'    -- LIKE pattern matching, as in sqlite3
//! ```
//!
//! # Output
//!
//! Returns all CREATE TABLE, CREATE VIEW, CREATE INDEX, and CREATE TRIGGER
//! statements from the database's sqlite_master table, in creation order
//! (sqlite_master rowid order, matching sqlite3's `.schema`). Creation
//! order guarantees the dump is replayable: tables always precede the
//! indexes, triggers, and views that reference them. Every statement is
//! terminated with a `;` (sqlite_master.sql lacks it) so the output is
//! directly executable in every consumer (CLI, REPL, run mode, Jupyter).

use crate::dot::DotError;
use crate::Runtime;
use serde::Serialize;

/// Command to display database schema definitions.
#[derive(Serialize, Debug, PartialEq)]
pub struct SchemaCommand {
    /// Optional LIKE pattern filter on object names. Matches both the
    /// object name and the table it belongs to (`tbl_name`), so a table
    /// pattern also returns the table's indexes and triggers, like
    /// sqlite3's `.schema ?PATTERN?`.
    pub pattern: Option<String>,
}

impl SchemaCommand {
    /// Execute the schema command, returning CREATE statements.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context containing the database connection
    ///
    /// # Returns
    ///
    /// A vector of SQL CREATE statements (each terminated with `;`),
    /// or an error if the query fails.
    pub fn execute(&self, runtime: &Runtime) -> Result<Vec<String>, DotError> {
        let (_, stmt) = runtime.connection.prepare(
            r#"
            SELECT sql
            FROM sqlite_master
            WHERE sql IS NOT NULL
              AND (?1 IS NULL OR name LIKE ?1 OR tbl_name LIKE ?1)
            ORDER BY rowid
            "#,
        )?;

        let mut stmt = stmt.ok_or_else(|| {
            DotError::InvalidData("internal: schema query produced no statement".into())
        })?;

        if let Some(pattern) = &self.pattern {
            stmt.bind_text(1, pattern.as_str())?;
        }

        let mut schemas = Vec::new();
        loop {
            match stmt.next() {
                Ok(Some(row)) => {
                    if let Some(value) = row.first() {
                        let sql = value.as_str();
                        if !sql.is_empty() {
                            // sqlite_master.sql never includes the trailing
                            // terminator; add it so output is copy-pasteable
                            schemas.push(format!("{};", sql));
                        }
                    }
                }
                Ok(None) => break,
                // propagate instead of silently returning a truncated dump
                Err(e) => return Err(e.into()),
            }
        }

        Ok(schemas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_runtime() -> Runtime {
        Runtime::new(None).unwrap()
    }

    #[test]
    fn test_schema_empty_database() {
        let runtime = create_test_runtime();
        let cmd = SchemaCommand { pattern: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());
        // Empty database should have no user-created schemas
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_schema_with_table() {
        let runtime = create_test_runtime();

        // Create a test table
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE test_schema_table (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let cmd = SchemaCommand { pattern: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let schemas = result.unwrap();
        assert_eq!(schemas.len(), 1);
        assert!(schemas[0].contains("CREATE TABLE test_schema_table"));
        assert!(schemas[0].ends_with(';'));
    }

    #[test]
    fn test_schema_with_view() {
        let runtime = create_test_runtime();

        // Create a table and view
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE base_table (id INTEGER, value TEXT)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE VIEW test_view AS SELECT * FROM base_table")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let cmd = SchemaCommand { pattern: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let schemas = result.unwrap();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().any(|s| s.contains("CREATE TABLE")));
        assert!(schemas.iter().any(|s| s.contains("CREATE VIEW")));
    }

    #[test]
    fn test_schema_creation_order() {
        let runtime = create_test_runtime();

        // Deliberately create objects so that alphabetical type ordering
        // (index < table < trigger < view) would NOT match creation order.
        for sql in [
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            "CREATE INDEX idx_users_name ON users(name)",
            "CREATE VIEW v_users AS SELECT * FROM users",
            "CREATE TRIGGER trg AFTER INSERT ON users BEGIN SELECT 1; END",
        ] {
            let (_, stmt) = runtime.connection.prepare(sql).unwrap();
            stmt.unwrap().execute().unwrap();
        }

        let cmd = SchemaCommand { pattern: None };
        let schemas = cmd.execute(&runtime).unwrap();
        assert_eq!(schemas.len(), 4);

        // every statement is terminated so the dump is executable as-is
        for schema in &schemas {
            assert!(schema.ends_with(';'), "missing terminator: {schema}");
        }

        let pos = |needle: &str| {
            schemas
                .iter()
                .position(|s| s.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle}"))
        };
        // CREATE TABLE must come before the index/view/trigger that
        // reference it, so the dump is replayable.
        assert!(pos("CREATE TABLE users") < pos("CREATE INDEX idx_users_name"));
        assert!(pos("CREATE INDEX idx_users_name") < pos("CREATE VIEW v_users"));
        assert!(pos("CREATE VIEW v_users") < pos("CREATE TRIGGER trg"));
    }

    fn create_pattern_test_runtime() -> Runtime {
        let runtime = create_test_runtime();
        for sql in [
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
            "CREATE INDEX idx_users_name ON users(name)",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY)",
            "CREATE VIEW v_orders AS SELECT * FROM orders",
        ] {
            let (_, stmt) = runtime.connection.prepare(sql).unwrap();
            stmt.unwrap().execute().unwrap();
        }
        runtime
    }

    #[test]
    fn test_schema_pattern_matches_table_and_its_objects() {
        let runtime = create_pattern_test_runtime();

        // matching a table name also returns its indexes (tbl_name match),
        // like sqlite3's `.schema users`
        let cmd = SchemaCommand {
            pattern: Some("users".to_string()),
        };
        let schemas = cmd.execute(&runtime).unwrap();
        assert_eq!(schemas.len(), 2);
        assert!(schemas[0].contains("CREATE TABLE users"));
        assert!(schemas[1].contains("CREATE INDEX idx_users_name"));
    }

    #[test]
    fn test_schema_pattern_wildcards() {
        let runtime = create_pattern_test_runtime();

        let cmd = SchemaCommand {
            pattern: Some("idx_%".to_string()),
        };
        let schemas = cmd.execute(&runtime).unwrap();
        assert_eq!(schemas.len(), 1);
        assert!(schemas[0].contains("CREATE INDEX idx_users_name"));
    }

    #[test]
    fn test_schema_pattern_no_match() {
        let runtime = create_pattern_test_runtime();

        let cmd = SchemaCommand {
            pattern: Some("nonexistent".to_string()),
        };
        let schemas = cmd.execute(&runtime).unwrap();
        assert!(schemas.is_empty());
    }
}
