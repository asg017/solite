use std::path::Path;

/// Whether the path ends in `.sql`, for positional-arg classification in
/// `query` and `execute` (the file's contents are then used as the SQL).
pub(crate) fn is_sql_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "sql")
}

/// Read the contents of a `.sql` file, trimmed. Errors with `NotFound` if
/// the path doesn't exist.
pub(crate) fn read_sql_file(path: &Path) -> Result<String, std::io::Error> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
    }
    std::fs::read_to_string(path).map(|contents| contents.trim().to_string())
}

pub mod repl;
pub mod run;
pub mod query;
pub mod exec;
pub mod docs;
pub mod bench;
pub mod jupyter;
pub mod test;
pub mod codegen;
pub mod tui;
pub mod fmt;
pub mod lint;
pub mod lsp;
pub mod sqlite3;
pub mod diff;
pub mod rsync;
pub mod schema;
pub mod backup;
pub mod vacuum;
pub mod serve;
#[cfg(feature = "ritestream")]
pub mod stream;

/// Write a Vega-Lite JSON spec to a unique temp file and return its path.
/// Terminal frontends (REPL, run mode) can't render charts, so `.vegalite`
/// writes the spec to disk and prints where it went.
pub fn write_vegalite_spec(
    spec: &serde_json::Map<String, serde_json::Value>,
) -> std::io::Result<std::path::PathBuf> {
    let file = tempfile::Builder::new()
        .prefix("solite-vegalite-")
        .suffix(".vl.json")
        .tempfile()?;
    std::fs::write(
        file.path(),
        serde_json::Value::Object(spec.clone()).to_string(),
    )?;
    let (_, path) = file.keep().map_err(|e| e.error)?;
    Ok(path)
}