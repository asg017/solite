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