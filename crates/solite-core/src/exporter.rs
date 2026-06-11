//! Data export functionality for SQL query results.
//!
//! This module provides utilities for exporting SQL query results to various
//! formats including CSV, TSV, JSON, NDJSON, clipboard, and raw values.
//!
//! # Supported Formats
//!
//! - **CSV**: Comma-separated values
//! - **TSV**: Tab-separated values
//! - **JSON**: JSON array of objects
//! - **NDJSON**: Newline-delimited JSON (one object per line)
//! - **Clipboard**: HTML table copied to system clipboard
//! - **Value**: Raw value output (single cell)
//!
//! # Example
//!
//! ```ignore
//! use solite_core::exporter::{BlobLimit, ExportFormat, write_output};
//!
//! let output = Box::new(std::io::stdout());
//! write_output(&mut stmt, output, ExportFormat::Json, BlobLimit::Default)?;
//! ```

use arboard::Clipboard;
use std::path::Path;
use std::{
    fmt,
    fs::File,
    io::{BufWriter, Write},
};

use crate::sqlite::{OwnedValue, Statement, ValueRefX, ValueRefXValue};

/// Errors that can occur during export operations.
#[derive(Debug)]
pub enum ExportError {
    /// I/O error during write operations.
    Io(std::io::Error),
    /// CSV writing error.
    Csv(csv::Error),
    /// JSON serialization error.
    Json(serde_json::Error),
    /// SQL execution error.
    Sql(String),
    /// Invalid UTF-8 in text data.
    InvalidUtf8,
    /// Invalid floating point value for JSON.
    InvalidFloat(f64),
    /// Column index out of bounds.
    ColumnIndexOutOfBounds { index: usize, count: usize },
    /// No rows returned when one was expected.
    NoRows,
    /// Too many rows returned.
    TooManyRows,
    /// Clipboard operation failed.
    Clipboard(String),
    /// Compression error.
    Compression(String),
    /// A BLOB cell exceeded the export size limit.
    BlobTooLarge {
        /// Name of the column containing the oversized BLOB.
        column: String,
        /// Size of the BLOB in bytes.
        size: u64,
        /// The active limit in bytes.
        limit: u64,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "I/O error: {}", e),
            ExportError::Csv(e) => write!(f, "CSV error: {}", e),
            ExportError::Json(e) => write!(f, "JSON error: {}", e),
            ExportError::Sql(msg) => write!(f, "SQL error: {}", msg),
            ExportError::InvalidUtf8 => write!(f, "Invalid UTF-8 in text data"),
            ExportError::InvalidFloat(v) => write!(f, "Invalid float value for JSON: {}", v),
            ExportError::ColumnIndexOutOfBounds { index, count } => {
                write!(f, "Column index {} out of bounds (have {} columns)", index, count)
            }
            ExportError::NoRows => write!(f, "No rows returned in query"),
            ExportError::TooManyRows => {
                write!(f, "More than 1 row returned, expected a single row. Try a `LIMIT 1`")
            }
            ExportError::Clipboard(msg) => write!(f, "Clipboard error: {}", msg),
            ExportError::Compression(msg) => write!(f, "Compression error: {}", msg),
            ExportError::BlobTooLarge {
                column,
                size,
                limit,
            } => write!(
                f,
                "BLOB in column '{}' is {} bytes, which exceeds the {}-byte export limit; \
                 pass --blob-limit <SIZE> (e.g. --blob-limit {}mb or --blob-limit none) to \
                 export it",
                column,
                size,
                limit,
                size.div_ceil(1024 * 1024),
            ),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Io(e) => Some(e),
            ExportError::Csv(e) => Some(e),
            ExportError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

impl From<csv::Error> for ExportError {
    fn from(e: csv::Error) -> Self {
        ExportError::Csv(e)
    }
}

impl From<serde_json::Error> for ExportError {
    fn from(e: serde_json::Error) -> Self {
        ExportError::Json(e)
    }
}

/// Output format for exported data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFormat {
    /// Comma-separated values.
    Csv,
    /// Tab-separated values.
    Tsv,
    /// JSON array of objects.
    Json,
    /// Newline-delimited JSON.
    Ndjson,
    /// Raw value output (single cell).
    Value,
    /// HTML table to clipboard.
    Clipboard,
}

/// Default BLOB size limit for clipboard exports (1 MiB).
pub const DEFAULT_CLIPBOARD_BLOB_LIMIT: u64 = 1024 * 1024;

/// Default BLOB size limit for file/stdout exports (10 MiB).
pub const DEFAULT_FILE_BLOB_LIMIT: u64 = 10 * 1024 * 1024;

/// Maximum size of a single BLOB cell allowed in exports.
///
/// Exceeding the limit is an error ([`ExportError::BlobTooLarge`]), never a
/// truncation, so exports can't silently dump or mangle a giant blob. The
/// [`ExportFormat::Value`] path is always unlimited: explicitly requesting a
/// single raw value is intentional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlobLimit {
    /// Format-dependent default: [`DEFAULT_CLIPBOARD_BLOB_LIMIT`] for
    /// clipboard exports, [`DEFAULT_FILE_BLOB_LIMIT`] otherwise.
    #[default]
    Default,
    /// Explicit limit in bytes.
    Bytes(u64),
    /// No limit.
    Unlimited,
}

impl BlobLimit {
    /// Resolve to a concrete byte limit for the given format
    /// (`None` = unlimited). The `Value` format is never limited.
    pub fn resolve(&self, format: &ExportFormat) -> Option<u64> {
        if matches!(format, ExportFormat::Value) {
            return None;
        }
        match self {
            BlobLimit::Unlimited => None,
            BlobLimit::Bytes(n) => Some(*n),
            BlobLimit::Default => Some(match format {
                ExportFormat::Clipboard => DEFAULT_CLIPBOARD_BLOB_LIMIT,
                _ => DEFAULT_FILE_BLOB_LIMIT,
            }),
        }
    }
}

/// Parse a user-supplied blob size limit.
///
/// Accepts plain byte counts (`1048576`), sizes with a case-insensitive
/// `k`/`kb`/`m`/`mb`/`g`/`gb` suffix (`10mb`, `512K`), or
/// `none`/`unlimited`/`0` for no limit.
pub fn parse_blob_limit(s: &str) -> Result<BlobLimit, String> {
    const HINT: &str =
        "expected a byte count (1048576), a size with a k/kb/m/mb/g/gb suffix (10mb), \
         or 'none'/'unlimited'/'0' for no limit";
    let t = s.trim().to_ascii_lowercase();
    match t.as_str() {
        "" => return Err(format!("empty blob limit: {HINT}")),
        "none" | "unlimited" => return Ok(BlobLimit::Unlimited),
        _ => {}
    }
    let (digits, multiplier): (&str, u64) = if let Some(d) =
        t.strip_suffix("kb").or_else(|| t.strip_suffix('k'))
    {
        (d, 1024)
    } else if let Some(d) = t.strip_suffix("mb").or_else(|| t.strip_suffix('m')) {
        (d, 1024 * 1024)
    } else if let Some(d) = t.strip_suffix("gb").or_else(|| t.strip_suffix('g')) {
        (d, 1024 * 1024 * 1024)
    } else {
        (t.as_str(), 1)
    };
    let n: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("invalid blob limit '{s}': {HINT}"))?;
    let bytes = n
        .checked_mul(multiplier)
        .ok_or_else(|| format!("blob limit '{s}' is too large"))?;
    if bytes == 0 {
        Ok(BlobLimit::Unlimited)
    } else {
        Ok(BlobLimit::Bytes(bytes))
    }
}

/// Error if any BLOB cell in the row exceeds `limit` (`None` = unlimited).
///
/// Checked on the raw blob size, before any hex/base64 encoding.
fn check_blob_limit(
    row: &[ValueRefX],
    columns: &[String],
    limit: Option<u64>,
) -> Result<(), ExportError> {
    let Some(limit) = limit else {
        return Ok(());
    };
    for (idx, value) in row.iter().enumerate() {
        if let ValueRefXValue::Blob(bytes) = &value.value {
            if bytes.len() as u64 > limit {
                return Err(ExportError::BlobTooLarge {
                    column: columns
                        .get(idx)
                        .cloned()
                        .unwrap_or_else(|| format!("#{}", idx + 1)),
                    size: bytes.len() as u64,
                    limit,
                });
            }
        }
    }
    Ok(())
}

/// Encode a BLOB as a SQL-style hex literal, e.g. `x'DEADBEEF'`.
/// Used by CSV/TSV/clipboard so blobs stay distinguishable from empty
/// strings and NULLs (and round-trip losslessly).
fn blob_to_hex_literal(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + 3);
    out.push_str("x'");
    for b in bytes {
        out.push_str(&format!("{:02X}", b));
    }
    out.push('\'');
    out
}

/// Convert a SQLite value to a string representation.
fn value_to_string(value: &ValueRefX) -> Result<String, ExportError> {
    match &value.value {
        ValueRefXValue::Null => Ok(String::new()),
        ValueRefXValue::Int(v) => Ok(v.to_string()),
        ValueRefXValue::Double(v) => Ok(v.to_string()),
        ValueRefXValue::Text(bytes) => {
            std::str::from_utf8(bytes)
                .map(|s| s.to_owned())
                .map_err(|_| ExportError::InvalidUtf8)
        }
        ValueRefXValue::Blob(bytes) => Ok(blob_to_hex_literal(bytes)),
    }
}

/// Convert a SQLite value to a JSON value.
fn value_to_json(value: &ValueRefX) -> Result<serde_json::Value, ExportError> {
    match &value.value {
        ValueRefXValue::Null => Ok(serde_json::Value::Null),
        ValueRefXValue::Int(v) => Ok(serde_json::Value::Number((*v).into())),
        ValueRefXValue::Double(v) => {
            serde_json::Number::from_f64(*v)
                .map(serde_json::Value::Number)
                .ok_or(ExportError::InvalidFloat(*v))
        }
        ValueRefXValue::Text(bytes) => {
            // Check for JSON subtype (74 = 'J')
            if value.subtype() == Some(74) {
                serde_json::from_slice(bytes).map_err(ExportError::Json)
            } else {
                let text = std::str::from_utf8(bytes).map_err(|_| ExportError::InvalidUtf8)?;
                Ok(serde_json::Value::String(text.to_owned()))
            }
        }
        // BLOBs are emitted as plain base64 strings (lossless and easy to
        // decode with jq/standard tooling)
        ValueRefXValue::Blob(bytes) => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(serde_json::Value::String(encoded))
        }
    }
}

/// Write a row as a JSON object.
fn write_json_row<W: Write>(
    output: &mut W,
    columns: &[String],
    row: Vec<ValueRefX>,
) -> Result<(), ExportError> {
    let mut obj = serde_json::Map::new();

    for (idx, value) in row.iter().enumerate() {
        let key = columns
            .get(idx)
            .ok_or(ExportError::ColumnIndexOutOfBounds {
                index: idx,
                count: columns.len(),
            })?
            .to_owned();

        let json_value = value_to_json(value)?;
        obj.insert(key, json_value);
    }

    serde_json::to_writer(output, &serde_json::Value::Object(obj))?;
    Ok(())
}

/// Write a row to a CSV writer.
fn write_csv_row<W: Write>(
    writer: &mut csv::Writer<W>,
    row: Vec<ValueRefX>,
) -> Result<(), ExportError> {
    let record: Result<Vec<String>, ExportError> = row.iter().map(value_to_string).collect();
    writer.write_record(record?)?;
    Ok(())
}

/// Write statement results as CSV.
fn write_csv<W: Write>(
    stmt: &mut Statement,
    output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError> {
    let mut writer = csv::Writer::from_writer(output);

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    writer.write_record(&columns)?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                write_csv_row(&mut writer, row)?;
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    writer.flush()?;
    Ok(())
}

/// Write statement results as TSV.
fn write_tsv<W: Write>(
    stmt: &mut Statement,
    output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .from_writer(output);

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    writer.write_record(&columns)?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                write_csv_row(&mut writer, row)?;
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    writer.flush()?;
    Ok(())
}

/// Write statement results as JSON array.
fn write_json<W: Write>(
    stmt: &mut Statement,
    mut output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError> {
    output.write_all(b"[")?;

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    let mut first = true;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                if first {
                    first = false;
                } else {
                    output.write_all(b",")?;
                }
                write_json_row(&mut output, &columns, row)?;
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    output.write_all(b"]\n")?;
    Ok(())
}

/// Write statement results as NDJSON (newline-delimited JSON).
fn write_ndjson<W: Write>(
    stmt: &mut Statement,
    mut output: W,
    blob_limit: Option<u64>,
) -> Result<(), ExportError> {
    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                write_json_row(&mut output, &columns, row)?;
                output.write_all(b"\n")?;
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    Ok(())
}

/// Write statement results to clipboard as HTML table.
/// Returns the number of rows written; the caller is responsible for any
/// user-facing confirmation message.
fn write_clipboard(stmt: &mut Statement, blob_limit: Option<u64>) -> Result<usize, ExportError> {
    let mut html = String::from("<table><thead><tr>");
    let mut num_rows = 0;

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    for column in &columns {
        html.push_str("<td>");
        html.push_str(&html_escape(column));
        html.push_str("</td>");
    }
    html.push_str("</tr></thead><tbody>");

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                check_blob_limit(&row, &columns, blob_limit)?;
                html.push_str("<tr>");
                for cell in row {
                    let value = value_to_string(&cell)?;
                    html.push_str("<td>");
                    html.push_str(&html_escape(&value));
                    html.push_str("</td>");
                }
                html.push_str("</tr>");
                num_rows += 1;
            }
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    html.push_str("</tbody></table>");

    let mut clipboard =
        Clipboard::new().map_err(|e| ExportError::Clipboard(e.to_string()))?;

    // TODO: write TSV equivalent to alt_text
    let alt_text = String::new();
    clipboard
        .set_html(&html, Some(&alt_text))
        .map_err(|e| ExportError::Clipboard(e.to_string()))?;

    Ok(num_rows)
}

/// Write a single value from statement results.
///
/// The single-row requirement is checked *before* anything is written, so
/// a failing query never leaves a partial value on stdout or in `-o` files.
fn write_value<W: Write>(stmt: &mut Statement, mut output: W) -> Result<(), ExportError> {
    // Get first row
    let row = match stmt.next() {
        Ok(Some(row)) => row,
        Ok(None) => return Err(ExportError::NoRows),
        Err(e) => return Err(ExportError::Sql(e.to_string())),
    };

    // Copy the first value out of the row: stepping to the next row
    // invalidates the borrowed ValueRefX values.
    let value = row.first().ok_or(ExportError::ColumnIndexOutOfBounds {
        index: 0,
        count: row.len(),
    })?;
    let value = OwnedValue::from_value_ref(value);

    // Ensure no more rows before writing anything
    match stmt.next() {
        Ok(None) => {}
        Ok(Some(_)) => return Err(ExportError::TooManyRows),
        Err(e) => {
            return Err(ExportError::Sql(format!(
                "Error stepping through next row: {}",
                e
            )))
        }
    }

    // Write value
    match &value {
        OwnedValue::Null => {}
        OwnedValue::Integer(v) => write!(output, "{}", v)?,
        OwnedValue::Double(v) => write!(output, "{}", v)?,
        OwnedValue::Text(bytes) | OwnedValue::Blob(bytes) => {
            output.write_all(bytes)?;
        }
    }

    Ok(())
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Create an output writer from a file path.
///
/// Automatically handles compression based on file extension:
/// - `.gz`: gzip compression
/// - `.zst`: zstd compression
pub fn output_from_path(path: &Path) -> Result<Box<dyn Write>, ExportError> {
    let file = File::create(path)?;

    let extension = path.extension().and_then(|e| e.to_str());

    match extension {
        Some("gz") => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            Ok(Box::new(BufWriter::new(encoder)))
        }
        Some("zst") => {
            let encoder = zstd::stream::write::Encoder::new(file, 3)
                .map_err(|e| ExportError::Compression(e.to_string()))?;
            Ok(Box::new(BufWriter::new(encoder)))
        }
        _ => Ok(Box::new(BufWriter::new(file))),
    }
}

/// Write statement results to output in the specified format.
///
/// `blob_limit` bounds the raw size of any BLOB cell (see [`BlobLimit`]);
/// the `Value` format is never limited.
///
/// Returns `Some(row_count)` for clipboard exports (which ignore `output`
/// and need a caller-printed confirmation), `None` for stream formats.
pub fn write_output(
    stmt: &mut Statement,
    output: Box<dyn Write>,
    format: ExportFormat,
    blob_limit: BlobLimit,
) -> Result<Option<usize>, ExportError> {
    let limit = blob_limit.resolve(&format);
    match format {
        ExportFormat::Csv => write_csv(stmt, output, limit).map(|()| None),
        ExportFormat::Tsv => write_tsv(stmt, output, limit).map(|()| None),
        ExportFormat::Json => write_json(stmt, output, limit).map(|()| None),
        ExportFormat::Ndjson => write_ndjson(stmt, output, limit).map(|()| None),
        ExportFormat::Clipboard => write_clipboard(stmt, limit).map(Some),
        ExportFormat::Value => write_value(stmt, output).map(|()| None),
    }
}

/// Write statement results to an in-memory buffer.
#[cfg(feature = "object_store")]
pub fn write_output_to_bytes(
    stmt: &mut Statement,
    format: ExportFormat,
    blob_limit: BlobLimit,
) -> Result<Vec<u8>, ExportError> {
    let mut buf = Vec::new();
    let limit = blob_limit.resolve(&format);
    match format {
        ExportFormat::Csv => write_csv(stmt, &mut buf, limit)?,
        ExportFormat::Tsv => write_tsv(stmt, &mut buf, limit)?,
        ExportFormat::Json => write_json(stmt, &mut buf, limit)?,
        ExportFormat::Ndjson => write_ndjson(stmt, &mut buf, limit)?,
        ExportFormat::Value => write_value(stmt, &mut buf)?,
        ExportFormat::Clipboard => {
            return Err(ExportError::Io(std::io::Error::other(
                "clipboard export is not supported for remote targets",
            )));
        }
    }
    Ok(buf)
}

/// Determine export format from file path extension.
///
/// Handles compressed files by looking at the extension before `.gz` or `.zst`.
pub fn format_from_path(path: &Path) -> Option<ExportFormat> {
    let extension = path.extension().and_then(|e| e.to_str())?;

    // Handle compression extensions
    let ext = if extension == "gz" || extension == "zst" {
        path.with_extension("")
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string())?
    } else {
        extension.to_string()
    };

    match ext.as_str() {
        "csv" => Some(ExportFormat::Csv),
        "tsv" => Some(ExportFormat::Tsv),
        "json" => Some(ExportFormat::Json),
        "ndjson" | "jsonl" => Some(ExportFormat::Ndjson),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_from_path_csv() {
        assert_eq!(
            format_from_path(&PathBuf::from("data.csv")),
            Some(ExportFormat::Csv)
        );
    }

    #[test]
    fn test_format_from_path_tsv() {
        assert_eq!(
            format_from_path(&PathBuf::from("data.tsv")),
            Some(ExportFormat::Tsv)
        );
    }

    #[test]
    fn test_format_from_path_json() {
        assert_eq!(
            format_from_path(&PathBuf::from("data.json")),
            Some(ExportFormat::Json)
        );
    }

    #[test]
    fn test_format_from_path_ndjson() {
        assert_eq!(
            format_from_path(&PathBuf::from("data.ndjson")),
            Some(ExportFormat::Ndjson)
        );
        assert_eq!(
            format_from_path(&PathBuf::from("data.jsonl")),
            Some(ExportFormat::Ndjson)
        );
    }

    #[test]
    fn test_format_from_path_compressed() {
        assert_eq!(
            format_from_path(&PathBuf::from("data.csv.gz")),
            Some(ExportFormat::Csv)
        );
        assert_eq!(
            format_from_path(&PathBuf::from("data.json.zst")),
            Some(ExportFormat::Json)
        );
    }

    #[test]
    fn test_format_from_path_unknown() {
        assert_eq!(format_from_path(&PathBuf::from("data.txt")), None);
        assert_eq!(format_from_path(&PathBuf::from("data.xml")), None);
    }

    #[test]
    fn test_format_from_path_no_extension() {
        assert_eq!(format_from_path(&PathBuf::from("data")), None);
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("hello"), "hello");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#39;s");
    }

    #[test]
    fn test_html_escape_combined() {
        assert_eq!(
            html_escape("<a href=\"test\">link & stuff</a>"),
            "&lt;a href=&quot;test&quot;&gt;link &amp; stuff&lt;/a&gt;"
        );
    }

    fn first_value_of(sql: &str) -> crate::sqlite::Statement {
        let conn = crate::sqlite::Connection::open_in_memory().unwrap();
        let (_, stmt) = conn.prepare(sql).unwrap();
        stmt.unwrap()
    }

    #[test]
    fn test_blob_to_hex_literal() {
        assert_eq!(blob_to_hex_literal(&[]), "x''");
        assert_eq!(blob_to_hex_literal(&[0xDE, 0xAD, 0xBE, 0xEF]), "x'DEADBEEF'");
        assert_eq!(blob_to_hex_literal(&[0x00, 0x01]), "x'0001'");
    }

    #[test]
    fn test_value_to_string_blob_is_hex_literal() {
        let mut stmt = first_value_of("select x'DEADBEEF', zeroblob(2), '', null");
        let row = stmt.next().unwrap().unwrap();
        assert_eq!(value_to_string(&row[0]).unwrap(), "x'DEADBEEF'");
        assert_eq!(value_to_string(&row[1]).unwrap(), "x'0000'");
        // blob, empty string, and NULL are all distinguishable
        assert_eq!(value_to_string(&row[2]).unwrap(), "");
        assert_eq!(value_to_string(&row[3]).unwrap(), "");
        assert_ne!(
            value_to_string(&row[1]).unwrap(),
            value_to_string(&row[2]).unwrap()
        );
    }

    #[test]
    fn test_value_to_json_blob_is_base64() {
        let mut stmt = first_value_of("select x'DEADBEEF', zeroblob(2), null");
        let row = stmt.next().unwrap().unwrap();
        assert_eq!(
            value_to_json(&row[0]).unwrap(),
            serde_json::Value::String("3q2+7w==".to_string())
        );
        assert_eq!(
            value_to_json(&row[1]).unwrap(),
            serde_json::Value::String("AAA=".to_string())
        );
        // blob is no longer conflated with NULL
        assert_eq!(value_to_json(&row[2]).unwrap(), serde_json::Value::Null);
    }

    #[test]
    fn test_parse_blob_limit_plain_bytes() {
        assert_eq!(parse_blob_limit("1048576"), Ok(BlobLimit::Bytes(1048576)));
        assert_eq!(parse_blob_limit("1"), Ok(BlobLimit::Bytes(1)));
        assert_eq!(parse_blob_limit(" 42 "), Ok(BlobLimit::Bytes(42)));
    }

    #[test]
    fn test_parse_blob_limit_suffixes() {
        assert_eq!(parse_blob_limit("1k"), Ok(BlobLimit::Bytes(1024)));
        assert_eq!(parse_blob_limit("1kb"), Ok(BlobLimit::Bytes(1024)));
        assert_eq!(parse_blob_limit("10m"), Ok(BlobLimit::Bytes(10 * 1024 * 1024)));
        assert_eq!(parse_blob_limit("10mb"), Ok(BlobLimit::Bytes(10 * 1024 * 1024)));
        assert_eq!(parse_blob_limit("2g"), Ok(BlobLimit::Bytes(2 * 1024 * 1024 * 1024)));
        assert_eq!(parse_blob_limit("2gb"), Ok(BlobLimit::Bytes(2 * 1024 * 1024 * 1024)));
        // suffixes are case-insensitive
        assert_eq!(parse_blob_limit("1MB"), Ok(BlobLimit::Bytes(1024 * 1024)));
        assert_eq!(parse_blob_limit("512K"), Ok(BlobLimit::Bytes(512 * 1024)));
        assert_eq!(parse_blob_limit("1Gb"), Ok(BlobLimit::Bytes(1024 * 1024 * 1024)));
    }

    #[test]
    fn test_parse_blob_limit_unlimited() {
        assert_eq!(parse_blob_limit("none"), Ok(BlobLimit::Unlimited));
        assert_eq!(parse_blob_limit("NONE"), Ok(BlobLimit::Unlimited));
        assert_eq!(parse_blob_limit("unlimited"), Ok(BlobLimit::Unlimited));
        assert_eq!(parse_blob_limit("0"), Ok(BlobLimit::Unlimited));
        // 0 with a suffix is still zero bytes = unlimited
        assert_eq!(parse_blob_limit("0kb"), Ok(BlobLimit::Unlimited));
    }

    #[test]
    fn test_parse_blob_limit_rejects_garbage() {
        for bad in ["", "  ", "abc", "10x", "10bk", "-5", "-5mb", "1.5mb", "mb", "k", "9999999999999999999gb"] {
            let err = parse_blob_limit(bad);
            assert!(err.is_err(), "expected error for {bad:?}, got {err:?}");
        }
        // errors mention what's accepted
        let msg = parse_blob_limit("10x").unwrap_err();
        assert!(msg.contains("10x"), "{msg}");
        assert!(msg.contains("none"), "{msg}");
    }

    #[test]
    fn test_blob_limit_resolve_defaults() {
        // clipboard defaults to 1 MiB, file/stdout formats to 10 MiB
        assert_eq!(
            BlobLimit::Default.resolve(&ExportFormat::Clipboard),
            Some(1024 * 1024)
        );
        for format in [
            ExportFormat::Csv,
            ExportFormat::Tsv,
            ExportFormat::Json,
            ExportFormat::Ndjson,
        ] {
            assert_eq!(
                BlobLimit::Default.resolve(&format),
                Some(10 * 1024 * 1024),
                "{format:?}"
            );
        }
    }

    #[test]
    fn test_blob_limit_resolve_override_and_unlimited() {
        assert_eq!(
            BlobLimit::Bytes(64).resolve(&ExportFormat::Csv),
            Some(64)
        );
        assert_eq!(
            BlobLimit::Bytes(64).resolve(&ExportFormat::Clipboard),
            Some(64)
        );
        assert_eq!(BlobLimit::Unlimited.resolve(&ExportFormat::Csv), None);
        // the raw value path is never limited, even with an explicit limit
        assert_eq!(BlobLimit::Default.resolve(&ExportFormat::Value), None);
        assert_eq!(BlobLimit::Bytes(1).resolve(&ExportFormat::Value), None);
    }

    #[test]
    fn test_csv_blob_over_limit_errors() {
        let mut stmt = first_value_of("select 1 as id, zeroblob(32) as payload");
        let mut buf = Vec::new();
        let err = write_csv(&mut stmt, &mut buf, Some(16)).unwrap_err();
        match &err {
            ExportError::BlobTooLarge {
                column,
                size,
                limit,
            } => {
                assert_eq!(column, "payload");
                assert_eq!(*size, 32);
                assert_eq!(*limit, 16);
            }
            other => panic!("expected BlobTooLarge, got {other:?}"),
        }
        // the error names the column, sizes, and the override flag
        let msg = err.to_string();
        assert!(msg.contains("payload"), "{msg}");
        assert!(msg.contains("32 bytes"), "{msg}");
        assert!(msg.contains("16-byte"), "{msg}");
        assert!(msg.contains("--blob-limit"), "{msg}");
    }

    #[test]
    fn test_csv_blob_under_limit_ok() {
        let mut stmt = first_value_of("select zeroblob(16) as payload");
        let mut buf = Vec::new();
        write_csv(&mut stmt, &mut buf, Some(16)).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("x'00000000000000000000000000000000'"), "{out}");
    }

    #[test]
    fn test_json_blob_over_limit_errors() {
        let mut stmt = first_value_of("select zeroblob(32) as payload");
        let mut buf = Vec::new();
        let err = write_json(&mut stmt, &mut buf, Some(16)).unwrap_err();
        assert!(matches!(err, ExportError::BlobTooLarge { .. }), "{err:?}");
    }

    #[test]
    fn test_write_output_enforces_default_limit() {
        // over the 10 MiB file default → error before serializing
        let mut stmt = first_value_of("select zeroblob(10*1024*1024 + 1) as payload");
        let err = write_output(
            &mut stmt,
            Box::new(std::io::sink()),
            ExportFormat::Csv,
            BlobLimit::Default,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::BlobTooLarge { .. }), "{err:?}");

        // an explicit higher limit allows it
        let mut stmt = first_value_of("select zeroblob(10*1024*1024 + 1) as payload");
        write_output(
            &mut stmt,
            Box::new(std::io::sink()),
            ExportFormat::Csv,
            BlobLimit::Bytes(11 * 1024 * 1024),
        )
        .unwrap();

        // and `none` disables enforcement entirely
        let mut stmt = first_value_of("select zeroblob(10*1024*1024 + 1) as payload");
        write_output(
            &mut stmt,
            Box::new(std::io::sink()),
            ExportFormat::Ndjson,
            BlobLimit::Unlimited,
        )
        .unwrap();
    }

    #[test]
    fn test_write_value_blob_is_never_limited() {
        // -f value is exempt: explicitly requesting one raw value is
        // intentional, so even a blob over any limit writes fine
        let mut stmt = first_value_of("select zeroblob(64) as payload");
        write_output(
            &mut stmt,
            Box::new(std::io::sink()),
            ExportFormat::Value,
            BlobLimit::Bytes(1),
        )
        .unwrap();

        // the raw bytes still come through untouched
        let mut stmt = first_value_of("select x'DEADBEEF'");
        let mut buf = Vec::new();
        write_value(&mut stmt, &mut buf).unwrap();
        assert_eq!(buf, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_write_value_single_row() {
        let mut stmt = first_value_of("select 42");
        let mut buf = Vec::new();
        write_value(&mut stmt, &mut buf).unwrap();
        assert_eq!(buf, b"42");
    }

    #[test]
    fn test_write_value_multi_row_writes_nothing() {
        // The error must surface *before* any partial value lands in the
        // output (stdout or a half-written -o file)
        let mut stmt = first_value_of("select column1 from (values (1), (2))");
        let mut buf = Vec::new();
        let result = write_value(&mut stmt, &mut buf);
        assert!(matches!(result, Err(ExportError::TooManyRows)));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_write_value_no_rows_writes_nothing() {
        let mut stmt = first_value_of("select 1 limit 0");
        let mut buf = Vec::new();
        let result = write_value(&mut stmt, &mut buf);
        assert!(matches!(result, Err(ExportError::NoRows)));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_export_error_display() {
        let err = ExportError::NoRows;
        assert!(err.to_string().contains("No rows"));

        let err = ExportError::TooManyRows;
        assert!(err.to_string().contains("More than 1 row"));

        let err = ExportError::InvalidFloat(f64::NAN);
        assert!(err.to_string().contains("Invalid float"));

        let err = ExportError::ColumnIndexOutOfBounds { index: 5, count: 3 };
        assert!(err.to_string().contains("5"));
        assert!(err.to_string().contains("3"));
    }

    #[test]
    fn test_export_format_equality() {
        assert_eq!(ExportFormat::Csv, ExportFormat::Csv);
        assert_ne!(ExportFormat::Csv, ExportFormat::Tsv);
    }
}
