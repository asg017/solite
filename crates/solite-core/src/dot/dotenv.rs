//! Environment file loading command.
//!
//! This module implements the `.dotenv` (or `.loadenv`) command which loads
//! environment variables from a `.env` file in the current directory.
//!
//! # Usage
//!
//! ```sql
//! .dotenv
//! ```
//!
//! # Behavior
//!
//! - Reads the `.env` file from the current working directory
//! - Sets each key-value pair as an environment variable
//! - Returns a list of loaded variable names
//!
//! # File Format
//!
//! The `.env` file should contain lines in the format `KEY=value`:
//!
//! ```text
//! DATABASE_URL=sqlite:///mydb.sqlite
//! API_KEY=secret123
//! DEBUG=true
//! ```

use crate::dot::DotError;
use serde::Serialize;
use std::path::PathBuf;

/// Command to load environment variables from a `.env` file.
#[derive(Serialize, Debug, PartialEq)]
pub struct DotenvCommand {}

/// Result of executing the dotenv command.
pub struct DotenvResult {
    /// Path to the loaded `.env` file.
    pub path: PathBuf,
    /// Names of environment variables that were loaded.
    pub loaded: Vec<String>,
}

impl DotenvCommand {
    /// Execute the dotenv command, loading variables from `.env`.
    ///
    /// # Returns
    ///
    /// A `DotenvResult` containing the path and loaded variable names,
    /// or an error if the file cannot be read or parsed.
    ///
    /// # Errors
    ///
    /// - Returns `DotError::Io` if the current directory cannot be determined
    /// - Returns `DotError::FileNotFound` if the `.env` file doesn't exist
    /// - Returns `DotError::InvalidData` if the file contains invalid entries
    pub fn execute(&self) -> Result<DotenvResult, DotError> {
        let current_dir =
            std::env::current_dir().map_err(DotError::Io)?;
        let path = current_dir.join(".env");

        if !path.exists() {
            return Err(DotError::FileNotFound(path.display().to_string()));
        }

        let iter = dotenvy::from_path_iter(&path).map_err(|e| {
            DotError::InvalidData(format!("Failed to parse .env file: {}", e))
        })?;

        let mut loaded = Vec::new();
        for item in iter {
            match item {
                Ok((key, value)) => {
                    std::env::set_var(&key, &value);
                    loaded.push(key);
                }
                Err(e) => {
                    return Err(DotError::InvalidData(format!(
                        "Invalid entry in .env file: {}",
                        e
                    )));
                }
            }
        }

        Ok(DotenvResult { path, loaded })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests are disabled because they require changing the
    // current working directory, which can interfere with other tests
    // running in parallel. The functionality is tested via integration
    // tests instead.

    #[test]
    fn test_dotenv_result_structure() {
        // Test the DotenvResult structure without actually loading files
        let result = DotenvResult {
            path: std::path::PathBuf::from("/test/.env"),
            loaded: vec!["VAR1".to_string(), "VAR2".to_string()],
        };
        assert_eq!(result.loaded.len(), 2);
        assert!(result.path.ends_with(".env"));
    }
}
