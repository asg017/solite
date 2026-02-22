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
//! use solite_core::exporter::{ExportFormat, write_output};
//!
//! let output = Box::new(std::io::stdout());
//! write_output(&mut stmt, output, ExportFormat::Json)?;
//! ```

use arboard::Clipboard;
use std::path::Path;
use std::{
    fmt,
    fs::File,
    io::{BufWriter, Write},
};

use crate::sqlite::{Statement, ValueRefX, ValueRefXValue};

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
    /// Unsupported blob in format.
    UnsupportedBlob,
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
            ExportError::UnsupportedBlob => write!(f, "BLOB values not supported in this format"),
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
        ValueRefXValue::Blob(_) => Ok(String::new()),
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
        // BLOBs can't be serialized to JSON easily
        // TODO: maybe base64 option?
        ValueRefXValue::Blob(_) => Ok(serde_json::Value::Null),
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
fn write_csv<W: Write>(stmt: &mut Statement, output: W) -> Result<(), ExportError> {
    let mut writer = csv::Writer::from_writer(output);

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    writer.write_record(&columns)?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => write_csv_row(&mut writer, row)?,
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    writer.flush()?;
    Ok(())
}

/// Write statement results as TSV.
fn write_tsv<W: Write>(stmt: &mut Statement, output: W) -> Result<(), ExportError> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .from_writer(output);

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    writer.write_record(&columns)?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => write_csv_row(&mut writer, row)?,
            Ok(None) => break,
            Err(e) => return Err(ExportError::Sql(e.to_string())),
        }
    }

    writer.flush()?;
    Ok(())
}

/// Write statement results as JSON array.
fn write_json<W: Write>(stmt: &mut Statement, mut output: W) -> Result<(), ExportError> {
    output.write_all(b"[")?;

    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;
    let mut first = true;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
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
fn write_ndjson<W: Write>(stmt: &mut Statement, mut output: W) -> Result<(), ExportError> {
    let columns = stmt.column_names().map_err(|e| ExportError::Sql(format!("{:?}", e)))?;

    loop {
        match stmt.next() {
            Ok(Some(row)) => {
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
fn write_clipboard(stmt: &mut Statement) -> Result<(), ExportError> {
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

    let row_word = if num_rows == 1 { "row" } else { "rows" };
    println!("✓ Wrote {} {} to clipboard", num_rows, row_word);

    Ok(())
}

/// Write a single value from statement results.
fn write_value<W: Write>(stmt: &mut Statement, mut output: W) -> Result<(), ExportError> {
    // Get first row
    let row = match stmt.next() {
        Ok(Some(row)) => row,
        Ok(None) => return Err(ExportError::NoRows),
        Err(e) => return Err(ExportError::Sql(e.to_string())),
    };

    // Get first value
    let value = row.first().ok_or(ExportError::ColumnIndexOutOfBounds {
        index: 0,
        count: row.len(),
    })?;

    // Write value
    match &value.value {
        ValueRefXValue::Null => {}
        ValueRefXValue::Int(v) => write!(output, "{}", v)?,
        ValueRefXValue::Double(v) => write!(output, "{}", v)?,
        ValueRefXValue::Text(bytes) | ValueRefXValue::Blob(bytes) => {
            output.write_all(bytes)?;
        }
    }

    // Ensure no more rows
    match stmt.next() {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(ExportError::TooManyRows),
        Err(e) => Err(ExportError::Sql(format!("Error stepping through next row: {}", e))),
    }
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
pub fn write_output(
    stmt: &mut Statement,
    output: Box<dyn Write>,
    format: ExportFormat,
) -> Result<(), ExportError> {
    match format {
        ExportFormat::Csv => write_csv(stmt, output),
        ExportFormat::Tsv => write_tsv(stmt, output),
        ExportFormat::Json => write_json(stmt, output),
        ExportFormat::Ndjson => write_ndjson(stmt, output),
        ExportFormat::Clipboard => write_clipboard(stmt),
        ExportFormat::Value => write_value(stmt, output),
    }
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
