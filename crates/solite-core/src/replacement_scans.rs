use crate::sqlite::{quote_identifier, Connection, SQLiteError, Statement};

/// If `error` is a "no such table" error referencing a file that looks like
/// a supported data file (csv/tsv) and exists on disk, return a prepared
/// `CREATE VIRTUAL TABLE temp."<name>"` statement that the caller should
/// execute before re-preparing the original SQL.
///
/// Returns:
/// - `None` — the error is not replacement-scannable (wrong error kind,
///   unsupported suffix, or the file doesn't exist). Callers surface the
///   original error.
/// - `Some(Ok(stmt))` — execute `stmt`, then retry the original SQL.
/// - `Some(Err(e))` — preparing the CREATE VIRTUAL TABLE itself failed.
/// Strip a recognized compression suffix (`.gz`, `.zst`) so the underlying
/// format suffix can be matched (`data.csv.gz` → `data.csv`). sqlite-xsv
/// decompresses gzip and zstd transparently based on the file extension.
fn strip_compression_suffix(name: &str) -> &str {
    name.strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".zst"))
        .unwrap_or(name)
}

pub fn replacement_scan(
    error: &SQLiteError,
    connection: &Connection,
) -> Option<Result<Statement, SQLiteError>> {
    let table_name = error.message.as_str().strip_prefix("no such table: ")?;

    /* TODO:
     * - [ ] JSON
     * - [ ] NDJSON/JSONL
     * - [ ] .txt files?
     * - [ ] XML??
     */
    let lower = table_name.to_lowercase();
    // The xsv vtab decompresses gzip/zstd based on the final file extension,
    // so `data.csv.gz` is handled by recognizing the suffix here and letting
    // the vtab open the full name as-is.
    let base = strip_compression_suffix(&lower);
    let using = if base.ends_with(".csv") {
        "csv"
    } else if base.ends_with(".tsv") {
        "tsv(flexible=true)"
    } else {
        return None;
    };

    // The table name doubles as the file path (resolved relative to cwd by
    // the vtab). If the file doesn't exist, fall through to the original
    // "no such table" error — it carries the proper source span and is the
    // clearest message for the user. Note this check still leaves a race
    // (file deleted between check and vtab create), so callers must handle
    // execute() errors on the returned statement as well.
    if !std::path::Path::new(table_name).exists() {
        return None;
    }

    let sql = format!(
        "create virtual table temp.{} using {}",
        quote_identifier(table_name),
        using
    );
    match connection.prepare(&sql) {
        Ok((_, Some(stmt))) => Some(Ok(stmt)),
        // A non-empty CREATE VIRTUAL TABLE always yields a statement; treat
        // the impossible empty-prepare as "not scannable".
        Ok((_, None)) => None,
        Err(e) => Some(Err(e)),
    }
}
