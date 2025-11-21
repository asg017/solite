use crate::sqlite::{Connection, SQLiteError, Statement};

pub fn replacement_scan(
    error: &SQLiteError,
    connection: &Connection,
) -> Option<Result<Statement, ()>> {
    let table_name = match error.message.as_str().strip_prefix("no such table: ") {
        Some(table_name) => table_name,
        None => return None,
    };

    /* TODO:
     * - [ ] .csv.gz, ztsd, zip, etc
     * - [ ] JSON, .gz, etc
     * - [ ] NDJSON/JSONL
     * - [ ] .txt files?
     * - [ ] XML??
     */
    if table_name.to_lowercase().ends_with(".csv") {
        match connection
            .prepare(format!("create virtual table temp.\"{}\" using csv ", table_name).as_str())
        {
            Ok((_, Some(stmt))) => return Some(Ok(stmt)),
            _ => {
                panic!("replacement didnt work")
            }
        }
    }
    if table_name.to_lowercase().ends_with(".tsv") {
        match connection
            .prepare(format!("create virtual table temp.\"{}\" using tsv(flexible=true)", table_name).as_str())
        {
            Ok((_, Some(stmt))) => return Some(Ok(stmt)),
            _ => {
                panic!("replacement didnt work")
            }
        }
    }
    None
}
