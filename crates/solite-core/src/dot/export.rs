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
#[cfg(feature = "object_store")]
use crate::exporter::write_output_to_bytes;
use crate::exporter::{format_from_path, output_from_path, write_output, BlobLimit};
#[cfg(feature = "object_store")]
use crate::object_store;
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
        let format = format_from_path(&self.target)
            .map_err(|e| DotError::InvalidData(e.to_string()))?;

        #[cfg(feature = "object_store")]
        {
            let target_str = self.target.to_string_lossy();
            if object_store::is_object_store_url(&target_str) {
                let data = write_output_to_bytes(&mut self.statement, format, BlobLimit::Default)
                    .map_err(|e| DotError::Io(std::io::Error::other(e.to_string())))?;
                object_store::upload(&target_str, data)
                    .map_err(|e| DotError::Io(std::io::Error::other(e.to_string())))?;
                return Ok(());
            }
        }

        let output = output_from_path(&self.target)
            .map_err(|e| DotError::Io(std::io::Error::other(e.to_string())))?;
        // .export has no override flag (yet); it uses the file default limit.
        // row-count is only returned for clipboard exports, which
        // format_from_path never produces
        let _ = write_output(&mut self.statement, output, format, BlobLimit::Default)
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

        let mut runtime = Runtime::new(None).unwrap();

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

    /// `.export out.geojson` and its gzipped newline-delimited sibling,
    /// end to end through `format_from_path` + `output_from_path`.
    #[test]
    fn test_export_geojson_round_trip() {
        use std::io::Read;

        let temp_dir = TempDir::new().unwrap();

        let mut runtime = Runtime::new(None).unwrap();
        let (_, stmt) = runtime
            .connection
            .prepare(
                "CREATE TABLE places AS
                 SELECT 1 AS id, 'a' AS name,
                        '{\"type\":\"Point\",\"coordinates\":[1,2]}' AS geometry
                 UNION ALL
                 SELECT 2, 'b', '{\"type\":\"Point\",\"coordinates\":[3,4]}'",
            )
            .unwrap();
        stmt.unwrap().execute().unwrap();

        // FeatureCollection
        let collection_path = temp_dir.path().join("out.geojson");
        let mut cmd = ExportCommand::new(
            collection_path.to_string_lossy().to_string(),
            &mut runtime,
            "SELECT * FROM places",
        )
        .unwrap();
        cmd.execute().unwrap();

        let doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&collection_path).unwrap()).unwrap();
        assert_eq!(doc["type"], "FeatureCollection");
        assert_eq!(doc["features"].as_array().unwrap().len(), 2);
        assert_eq!(doc["features"][0]["properties"]["name"], "a");

        // newline-delimited, gzipped
        let lines_path = temp_dir.path().join("out.geojsonl.gz");
        let mut cmd = ExportCommand::new(
            lines_path.to_string_lossy().to_string(),
            &mut runtime,
            "SELECT * FROM places",
        )
        .unwrap();
        cmd.execute().unwrap();

        let mut decoded = String::new();
        flate2::read::GzDecoder::new(fs::File::open(&lines_path).unwrap())
            .read_to_string(&mut decoded)
            .unwrap();
        let features: Vec<serde_json::Value> = decoded
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(features.len(), 2);
        assert_eq!(features[1]["geometry"]["coordinates"][0], 3);
    }

    #[test]
    fn test_export_invalid_format() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("output.xyz");

        let mut runtime = Runtime::new(None).unwrap();

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

    /// Regression test for the object_store feature-forwarding bug: without
    /// this feature compiled in, `.export s3://...` silently fell through to
    /// `output_from_path()` and failed with an ENOENT-style "No such file or
    /// directory" error instead of surfacing the real problem (missing
    /// credentials). This test removes `AWS_ACCESS_KEY_ID` from the process
    /// environment, so it must not run concurrently with anything else that
    /// depends on that variable being set.
    #[cfg(feature = "object_store")]
    #[test]
    fn test_export_s3_without_creds_is_not_a_file_error() {
        // SAFETY: env vars are process-global; this test's name and doc
        // comment call that out so future edits don't rely on
        // AWS_ACCESS_KEY_ID being set elsewhere in this process.
        let prev = std::env::var("AWS_ACCESS_KEY_ID").ok();
        unsafe {
            std::env::remove_var("AWS_ACCESS_KEY_ID");
        }

        let mut runtime = Runtime::new(None).unwrap();

        let mut cmd = ExportCommand::new(
            "s3://bucket/out.csv".to_string(),
            &mut runtime,
            "SELECT 1 AS a",
        )
        .unwrap();

        let result = cmd.execute();

        // Restore before asserting so a failed assertion doesn't leak the
        // mutated environment to later tests.
        if let Some(value) = prev {
            unsafe {
                std::env::set_var("AWS_ACCESS_KEY_ID", value);
            }
        }

        let err = result.expect_err("export without credentials should fail");
        let message = err.to_string();
        assert!(
            message.contains("AWS_ACCESS_KEY_ID"),
            "expected credentials error, got: {message}"
        );
        assert!(
            !message.contains("No such file or directory"),
            "expected credentials error, not an ENOENT fallback: {message}"
        );
    }
}
