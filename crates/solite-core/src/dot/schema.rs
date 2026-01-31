//! Database schema introspection command.
//!
//! This module implements the `.schema` command which displays the CREATE
//! statements for all objects in the database.
//!
//! # Usage
//!
//! ```sql
//! .schema
//! ```
//!
//! # Output
//!
//! Returns all CREATE TABLE, CREATE VIEW, CREATE INDEX, and CREATE TRIGGER
//! statements from the database's sqlite_master table.

use crate::dot::DotError;
use crate::Runtime;
use serde::Serialize;

/// Command to display database schema definitions.
#[derive(Serialize, Debug, PartialEq)]
pub struct SchemaCommand {}

impl SchemaCommand {
    /// Execute the schema command, returning CREATE statements.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context containing the database connection
    ///
    /// # Returns
    ///
    /// A vector of SQL CREATE statements, or an error if the query fails.
    pub fn execute(&self, runtime: &Runtime) -> Result<Vec<String>, DotError> {
        let (_, stmt) = runtime.connection.prepare(
            r#"
            SELECT sql
            FROM sqlite_master
            WHERE sql IS NOT NULL
            ORDER BY type, name
            "#,
        )?;

        let stmt = stmt.ok_or_else(|| DotError::InvalidData("Failed to prepare query".into()))?;

        let mut schemas = Vec::new();
        while let Ok(Some(row)) = stmt.next() {
            if let Some(value) = row.get(0) {
                let sql = value.as_str();
                if !sql.is_empty() {
                    schemas.push(sql.to_owned());
                }
            }
        }

        Ok(schemas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_runtime() -> Runtime {
        Runtime::new(None)
    }

    #[test]
    fn test_schema_empty_database() {
        let runtime = create_test_runtime();
        let cmd = SchemaCommand {};
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

        let cmd = SchemaCommand {};
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let schemas = result.unwrap();
        assert_eq!(schemas.len(), 1);
        assert!(schemas[0].contains("CREATE TABLE test_schema_table"));
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

        let cmd = SchemaCommand {};
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let schemas = result.unwrap();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().any(|s| s.contains("CREATE TABLE")));
        assert!(schemas.iter().any(|s| s.contains("CREATE VIEW")));
    }
}
