//! File-level snapshot operations.

use solite_core::sqlite::Statement;
use solite_core::advance_through_ignorable;
use std::fmt::Write as _;

use super::value::{copy, snapshot_value, ValueCopy};

/// Dedent a string by removing common leading whitespace.
pub fn dedent(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();

    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line
            }
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Generate snapshot contents from a SQL statement execution.
pub fn generate_snapshot_contents(source: String, stmt: &Statement) -> Option<String> {
    let mut snapshot_contents = String::new();
    let sql = stmt.sql();
    if write!(
        &mut snapshot_contents,
        "Source: {}\n{}\n---\n",
        source,
        dedent(advance_through_ignorable(&sql))
    )
    .is_err()
    {
        eprintln!("Warning: Failed to write snapshot header");
        return None;
    }

    let columns = match stmt.column_names() {
        Ok(cols) => cols,
        Err(e) => {
            eprintln!("Warning: Failed to get column names: {}", e);
            return None;
        }
    };

    let mut results: Vec<Vec<ValueCopy>> = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let row = row.iter().map(copy).collect();
                results.push(row);
            }
            Ok(None) => break,
            Err(err) => {
                let _ = writeln!(
                    &mut snapshot_contents,
                    "ERROR[{}] {}\n{}",
                    err.result_code, err.code_description, err.message
                );
                return Some(snapshot_contents);
            }
        }
    }

    // single value result (ex `select 1`)
    if columns.len() == 1 && results.len() == 1 {
        let _ = write!(&mut snapshot_contents, "{}", snapshot_value(&results[0][0]));
    }
    // no columns and no results (ex `create table foo`)
    else if columns.is_empty() && results.is_empty() {
        return None;
    }
    // no row results (but still had columns)
    else if results.is_empty() {
        let _ = write!(&mut snapshot_contents, "[no results]");
    }
    // multiple rows
    else {
        for row in results {
            let _ = writeln!(&mut snapshot_contents, "{{");
            for (value, column_name) in row.iter().zip(&columns) {
                let _ = writeln!(
                    &mut snapshot_contents,
                    "\t {}: {}",
                    column_name,
                    snapshot_value(value)
                );
            }
            let _ = writeln!(&mut snapshot_contents, "}}");
        }
    }
    let _ = writeln!(&mut snapshot_contents);
    Some(snapshot_contents)
}
