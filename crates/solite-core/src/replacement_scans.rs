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
pub fn replacement_scan(
    error: &SQLiteError,
    connection: &Connection,
) -> Option<Result<Statement, SQLiteError>> {
    let table_name = error.message.as_str().strip_prefix("no such table: ")?;

    /* TODO:
     * - [ ] .csv.gz, ztsd, zip, etc
     * - [ ] JSON, .gz, etc
     * - [ ] NDJSON/JSONL
     * - [ ] .txt files?
     * - [ ] XML??
     */
    let lower = table_name.to_lowercase();
    let using = if lower.ends_with(".csv") {
        "csv"
    } else if lower.ends_with(".tsv") {
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
