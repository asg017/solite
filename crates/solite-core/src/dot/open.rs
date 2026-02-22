//! Database opening command.
//!
//! This module implements the `.open` command which opens a different
//! SQLite database file, replacing the current connection.
//!
//! # Usage
//!
//! ```sql
//! .open mydb.sqlite
//! .open :memory:      -- Open in-memory database
//! ```
//!
//! # Behavior
//!
//! - Opens the specified database file
//! - Replaces the current database connection
//! - Initializes the standard library functions

use crate::dot::DotError;
use crate::{Connection, Runtime};
use serde::Serialize;
use solite_stdlib::solite_stdlib_init;

/// Command to open a database file.
#[derive(Serialize, Debug, PartialEq)]
pub struct OpenCommand {
    /// Path to the database file.
    pub path: String,
}

impl OpenCommand {
    /// Execute the open command, replacing the current connection.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context to update with the new connection
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if the database cannot be opened.
    ///
    /// # Errors
    ///
    /// Returns `DotError::Sqlite` if the database file cannot be opened.
    pub fn execute(&self, runtime: &mut Runtime) -> Result<(), DotError> {
        let connection = Connection::open(&self.path)?;

        // Initialize standard library functions
        unsafe {
            solite_stdlib_init(connection.db(), std::ptr::null_mut(), std::ptr::null_mut());
        }

        runtime.connection = connection;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let mut runtime = Runtime::new(None);
        let cmd = OpenCommand {
            path: db_path.to_string_lossy().to_string(),
        };

        let result = cmd.execute(&mut runtime);
        assert!(result.is_ok());

        // Verify the database was created
        assert!(db_path.exists());

        // Verify we can use it
        let (_, stmt) = runtime.connection.prepare("SELECT 1").unwrap();
        let stmt = stmt.unwrap();
        let result = stmt.next();
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_memory() {
        let mut runtime = Runtime::new(None);
        let cmd = OpenCommand {
            path: ":memory:".to_string(),
        };

        let result = cmd.execute(&mut runtime);
        assert!(result.is_ok());

        // Verify we can use it
        let (_, stmt) = runtime.connection.prepare("SELECT 1").unwrap();
        let stmt = stmt.unwrap();
        let result = stmt.next();
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("existing.db");

        // Create the database first
        {
            let conn = Connection::open(db_path.to_string_lossy().as_ref()).unwrap();
            let (_, stmt) = conn.prepare("CREATE TABLE foo (id INTEGER)").unwrap();
            stmt.unwrap().execute().unwrap();
        }

        let mut runtime = Runtime::new(None);
        let cmd = OpenCommand {
            path: db_path.to_string_lossy().to_string(),
        };

        let result = cmd.execute(&mut runtime);
        assert!(result.is_ok());

        // Verify the table exists
        let (_, stmt) = runtime
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='foo'")
            .unwrap();
        let stmt = stmt.unwrap();
        let row = stmt.next().unwrap();
        assert!(row.is_some());
    }
}
