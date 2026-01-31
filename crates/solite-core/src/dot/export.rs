//! Query result export command.
//!
//! This module implements the `.export` command which exports query results
//! to a file in various formats (CSV, JSON, etc.).
//!
//! # Usage
//!
//! ```sql
//! .export output.csv SELECT * FROM users
//! .export :date.json SELECT * FROM orders  -- Uses parameter substitution
//! ```
//!
//! # Parameter Substitution
//!
//! The target path supports parameter substitution using `:param_name` syntax.
//! Parameters are looked up from the runtime's parameter table.

use crate::dot::DotError;
use crate::exporter::{format_from_path, output_from_path, write_output};
use crate::sqlite::{OwnedValue, Statement};
use crate::{ParseDotError, Runtime};
use regex::{Captures, Regex};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Regex for parameter substitution in paths.
static PARAM_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":[\w]+").unwrap());

/// Command to export query results to a file.
#[derive(Serialize, Debug)]
pub struct ExportCommand {
    /// Target file path.
    pub target: PathBuf,
    /// Prepared statement to execute.
    pub statement: Statement,
    /// Length consumed from rest input.
    pub rest_length: usize,
}

impl ExportCommand {
    /// Create a new export command from arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - The target path (may contain parameter references)
    /// * `runtime` - The runtime context for parameter lookup
    /// * `rest` - The SQL query to execute
    ///
    /// # Errors
    ///
    /// Returns `ParseDotError` if the SQL cannot be prepared.
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
        let (rest_len, stmt) = runtime
            .prepare_with_parameters(rest)
            .map_err(|e| ParseDotError::Generic(format!("Failed to prepare query: {}", e)))?;

        let stmt = stmt.ok_or_else(|| ParseDotError::Generic("No SQL statement provided".into()))?;

        // Substitute parameters in the target path
        let target = PARAM_REGEX.replace_all(&args, |cap: &Captures| {
            let param_name = cap[0].strip_prefix(':').unwrap_or(&cap[0]);
            match runtime.lookup_parameter(param_name) {
                Some(OwnedValue::Text(text)) => {
                    std::str::from_utf8(&text).unwrap_or("").to_string()
                }
                _ => String::new(),
            }
        });

        Ok(Self {
            target: PathBuf::from(target.to_string()),
            statement: stmt,
            rest_length: rest_len.unwrap_or(rest.len()),
        })
    }

    /// Execute the export command, writing results to the target file.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if export fails.
    ///
    /// # Errors
    ///
    /// - `DotError::InvalidData` if the format cannot be determined
    /// - `DotError::Io` if the file cannot be written
    pub fn execute(&mut self) -> Result<(), DotError> {
        let output = output_from_path(&self.target)
            .map_err(|e| DotError::Io(std::io::Error::other(e.to_string())))?;

        let format = format_from_path(&self.target).ok_or_else(|| {
            DotError::InvalidData(format!(
                "Cannot determine format from path: {}",
                self.target.display()
            ))
        })?;

        write_output(&mut self.statement, output, format)
            .map_err(|e| DotError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_export_csv() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.csv");

        let mut runtime = Runtime::new(None);

        // Create test data
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE test (id INTEGER, name TEXT)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("INSERT INTO test VALUES (1, 'alice'), (2, 'bob')")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = ExportCommand::new(
            output_path.to_string_lossy().to_string(),
            &mut runtime,
            "SELECT * FROM test",
        )
        .unwrap();

        let result = cmd.execute();
        assert!(result.is_ok());

        // Verify file was created
        assert!(output_path.exists());
        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("id,name"));
        assert!(content.contains("1,alice"));
        assert!(content.contains("2,bob"));
    }

    #[test]
    fn test_export_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.xyz");

        let mut runtime = Runtime::new(None);

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE test (id INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = ExportCommand::new(
            output_path.to_string_lossy().to_string(),
            &mut runtime,
            "SELECT * FROM test",
        )
        .unwrap();

        let result = cmd.execute();
        assert!(matches!(result, Err(DotError::InvalidData(_))));
    }
}
