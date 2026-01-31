//! Table listing command.
//!
//! This module implements the `.tables` command which lists all tables
//! and views in the database.
//!
//! # Usage
//!
//! ```sql
//! .tables           -- List tables in 'main' schema
//! .tables temp      -- List tables in 'temp' schema
//! ```
//!
//! # Output
//!
//! Returns a list of table and view names, excluding internal sqlite_*
//! system tables.

use crate::dot::DotError;
use crate::Runtime;
use serde::Serialize;

/// Command to list database tables and views.
#[derive(Serialize, Debug, PartialEq)]
pub struct TablesCommand {
    /// Optional schema name filter (defaults to 'main').
    pub schema: Option<String>,
}

impl TablesCommand {
    /// Execute the tables command, returning table/view names.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context containing the database connection
    ///
    /// # Returns
    ///
    /// A vector of table and view names, or an error if the query fails.
    pub fn execute(&self, runtime: &Runtime) -> Result<Vec<String>, DotError> {
        let (_, stmt) = runtime.connection.prepare(
            r#"
            SELECT name
            FROM pragma_table_list
            WHERE "schema" = COALESCE(?1, 'main')
              AND type IN ('table', 'view')
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            "#,
        )?;

        let stmt = stmt.ok_or_else(|| DotError::InvalidData("Failed to prepare query".into()))?;

        if let Some(schema) = &self.schema {
            stmt.bind_text(1, schema.as_str());
        }

        let mut tables = Vec::new();
        while let Ok(Some(row)) = stmt.next() {
            if let Some(value) = row.get(0) {
                tables.push(value.as_str().to_owned());
            }
        }

        Ok(tables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_runtime() -> Runtime {
        Runtime::new(None)
    }

    #[test]
    fn test_tables_empty_database() {
        let runtime = create_test_runtime();
        let cmd = TablesCommand { schema: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_tables_with_tables() {
        let runtime = create_test_runtime();

        // Create test tables
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE alpha (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE beta (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let cmd = TablesCommand { schema: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let tables = result.unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0], "alpha");
        assert_eq!(tables[1], "beta");
    }

    #[test]
    fn test_tables_includes_views() {
        let runtime = create_test_runtime();

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE source (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE VIEW myview AS SELECT * FROM source")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let cmd = TablesCommand { schema: None };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let tables = result.unwrap();
        assert_eq!(tables.len(), 2);
        assert!(tables.contains(&"source".to_string()));
        assert!(tables.contains(&"myview".to_string()));
    }

    #[test]
    fn test_tables_with_schema_filter() {
        let runtime = create_test_runtime();

        // Create a table in temp schema
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TEMP TABLE temp_table (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        // Create a table in main schema
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE main_table (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        // List temp tables
        let cmd = TablesCommand {
            schema: Some("temp".to_string()),
        };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let tables = result.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0], "temp_table");

        // List main tables
        let cmd = TablesCommand {
            schema: Some("main".to_string()),
        };
        let result = cmd.execute(&runtime);
        assert!(result.is_ok());

        let tables = result.unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0], "main_table");
    }
}
