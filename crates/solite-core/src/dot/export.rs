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
//! .export out.geojson --geometry geom --id apn
//! SELECT apn, geom FROM parcels;
//! ```
//!
//! # Parameter Substitution
//!
//! The target path supports parameter substitution using `:param_name` syntax.
//! Parameters are looked up from the runtime's parameter table.
//!
//! # `--geometry`/`--id` flags
//!
//! GeoJSON targets (`.geojson`, `.geojsonl`, `.geojsons`) accept `--geometry
//! <col>` and `--id <col>` (space or `=` form), same as `solite query`. The
//! line is only tokenized shell-style when it contains ` --` or starts with
//! `--`, so a bare path with spaces (`.export My Report.csv`) still works
//! untouched; a path that itself contains ` --` must be quoted
//! (`.export "a --b.csv"`).

use crate::dot::DotError;
#[cfg(feature = "object_store")]
use crate::exporter::write_output_to_bytes;
use crate::exporter::{format_from_path, output_from_path, write_output, BlobLimit, ExportFormat, GeoJsonOptions};
#[cfg(feature = "object_store")]
use crate::object_store;
use crate::sqlite::{OwnedValue, Statement};
use crate::{ParseDotError, Runtime};
use regex::{Captures, Regex};
use serde::Serialize;
use std::ffi::OsString;
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
    /// `--geometry`/`--id` options, applied only when the target resolves
    /// to a GeoJSON format. Defaulted (never lifted) for every other
    /// format, and an error in [`ExportCommand::execute`] if non-default
    /// options are given for a non-GeoJSON target.
    pub geojson: GeoJsonOptions,
}

/// Parse `--geometry`/`--id` off the `.export` argument line.
///
/// Only tokenizes when flags are present (`args` contains ` --` or starts
/// with `--`), so a bare target path with spaces and no flags
/// (`.export My Report.csv`) is returned untouched.
fn parse_target_and_options(args: String) -> Result<(String, GeoJsonOptions), ParseDotError> {
    if !args.contains(" --") && !args.starts_with("--") {
        return Ok((args, GeoJsonOptions::default()));
    }

    let tokens = shlex::split(&args).ok_or_else(|| {
        ParseDotError::InvalidArgument("malformed quoting in .export arguments".into())
    })?;
    let mut pargs =
        pico_args::Arguments::from_vec(tokens.into_iter().map(OsString::from).collect());

    let geometry_column: Option<String> = pargs
        .opt_value_from_str("--geometry")
        .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;
    let id_column: Option<String> = pargs
        .opt_value_from_str("--id")
        .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;

    let mut rest = pargs.finish();
    let path = match rest.len() {
        1 => rest.remove(0),
        0 => {
            return Err(ParseDotError::InvalidArgument(
                "missing target path (usage: .export <path> [--geometry <col>] [--id <col>])"
                    .to_string(),
            ))
        }
        _ => {
            return Err(ParseDotError::InvalidArgument(format!(
                "unexpected .export argument '{}'",
                rest[1].to_string_lossy()
            )))
        }
    };

    Ok((
        path.to_string_lossy().into_owned(),
        GeoJsonOptions {
            geometry_column,
            id_column,
        },
    ))
}

/// Layer `--geometry`/`--id` options onto a resolved [`ExportFormat`].
///
/// A default `opts` is a no-op for every format. Non-default `opts` on a
/// non-GeoJSON format is a [`DotError::InvalidData`]: the flags only make
/// sense for `.geojson`/`.geojsonl`/`.geojsons` targets.
fn apply_geojson_options(
    format: ExportFormat,
    opts: &GeoJsonOptions,
) -> Result<ExportFormat, DotError> {
    if opts == &GeoJsonOptions::default() {
        return Ok(format);
    }
    match format {
        ExportFormat::GeoJson(_) => Ok(ExportFormat::GeoJson(opts.clone())),
        ExportFormat::GeoJsonl(_) => Ok(ExportFormat::GeoJsonl(opts.clone())),
        ExportFormat::GeoJsonSeq(_) => Ok(ExportFormat::GeoJsonSeq(opts.clone())),
        _ => Err(DotError::InvalidData(
            "--geometry/--id only apply to .geojson, .geojsonl and .geojsons targets".to_string(),
        )),
    }
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

        let (path_arg, geojson) = parse_target_and_options(args)?;

        // Substitute parameters in the target path only.
        let target = PARAM_REGEX.replace_all(&path_arg, |cap: &Captures| {
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
            geojson,
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
        let format = apply_geojson_options(format, &self.geojson)?;

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

    /// Set up a runtime with a `parcels` table for the `--geometry`/`--id`
    /// tests below.
    fn runtime_with_parcels() -> Runtime {
        let runtime = Runtime::new(None).unwrap();
        let (_, stmt) = runtime
            .connection
            .prepare(
                "CREATE TABLE parcels AS
                 SELECT 'A-1' AS code, 'x' AS name,
                        '{\"type\":\"Point\",\"coordinates\":[1,2]}' AS geom
                 UNION ALL
                 SELECT 'A-2', 'y', '{\"type\":\"Point\",\"coordinates\":[3,4]}'",
            )
            .unwrap();
        stmt.unwrap().execute().unwrap();
        runtime
    }

    #[test]
    fn test_export_geometry_and_id_flags_space_form() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("out.geojson");
        let mut runtime = runtime_with_parcels();

        let mut cmd = ExportCommand::new(
            format!(
                "{} --geometry geom --id code",
                output_path.to_string_lossy()
            ),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap();
        assert_eq!(cmd.geojson.geometry_column.as_deref(), Some("geom"));
        assert_eq!(cmd.geojson.id_column.as_deref(), Some("code"));
        cmd.execute().unwrap();

        let doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
        let features = doc["features"].as_array().unwrap();
        assert_eq!(features[0]["id"], "A-1");
        assert_eq!(features[0]["geometry"]["type"], "Point");
        assert_eq!(
            features[0]["properties"],
            serde_json::json!({"name": "x"})
        );
    }

    #[test]
    fn test_export_geometry_flag_eq_form() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("out.geojsonl");
        let mut runtime = runtime_with_parcels();

        let mut cmd = ExportCommand::new(
            format!("{} --geometry=geom", output_path.to_string_lossy()),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap();
        assert_eq!(cmd.geojson.geometry_column.as_deref(), Some("geom"));
        cmd.execute().unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"type\":\"Point\""), "{content}");
    }

    #[test]
    fn test_export_bare_path_with_spaces_and_no_flags_still_works() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("My Report.csv");
        let mut runtime = Runtime::new(None).unwrap();

        let (_, stmt) = runtime.connection.prepare("SELECT 1 AS a").unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = ExportCommand::new(
            output_path.to_string_lossy().to_string(),
            &mut runtime,
            "SELECT 1 AS a",
        )
        .unwrap();
        assert_eq!(cmd.geojson, GeoJsonOptions::default());
        cmd.execute().unwrap();
        assert!(output_path.exists());
    }

    #[test]
    fn test_export_geojson_flags_on_non_geojson_target_is_invalid_data() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("out.csv");
        let mut runtime = runtime_with_parcels();

        let mut cmd = ExportCommand::new(
            format!("{} --geometry x", output_path.to_string_lossy()),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap();
        let err = cmd.execute().unwrap_err();
        assert!(matches!(err, DotError::InvalidData(_)), "{err:?}");
        assert!(
            err.to_string().contains("--geometry/--id"),
            "{err}"
        );
    }

    #[test]
    fn test_export_missing_path_with_flags_is_an_error() {
        let mut runtime = runtime_with_parcels();
        let err = ExportCommand::new(
            "--geometry g".to_string(),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing target path"), "{err}");
    }

    #[test]
    fn test_export_unknown_flag_is_an_error() {
        let mut runtime = runtime_with_parcels();
        let err = ExportCommand::new(
            "out.geojson --foo bar".to_string(),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap_err();
        assert!(err.to_string().contains("--foo"), "{err}");
    }

    #[test]
    fn test_export_param_substitution_applies_to_path_only() {
        let temp_dir = TempDir::new().unwrap();
        let mut runtime = runtime_with_parcels();
        runtime
            .define_parameter("region".to_string(), "west".to_string())
            .unwrap();

        let target = temp_dir.path().join(":region.geojson");
        let mut cmd = ExportCommand::new(
            format!("{} --geometry geom", target.to_string_lossy()),
            &mut runtime,
            "SELECT * FROM parcels",
        )
        .unwrap();
        assert_eq!(
            cmd.target,
            temp_dir.path().join("west.geojson"),
            "{:?}",
            cmd.target
        );
        cmd.execute().unwrap();
        assert!(temp_dir.path().join("west.geojson").exists());
    }
}
