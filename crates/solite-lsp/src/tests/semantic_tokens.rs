//! Semantic token snapshot tests

use crate::server::compute_semantic_tokens;
use insta::assert_snapshot;

/// Format semantic tokens as a human-readable string for snapshot testing.
/// Output format: each token on a line with its text and type.
fn format_semantic_tokens(sql: &str) -> String {
    let tokens = compute_semantic_tokens(sql);

    let token_type_names = ["keyword", "variable", "number", "string", "comment", "operator", "type"];

    let mut result = String::new();
    let mut line = 0u32;
    let mut col = 0u32;

    for token in tokens {
        // Update position from deltas
        if token.delta_line > 0 {
            line += token.delta_line;
            col = token.delta_start;
        } else {
            col += token.delta_start;
        }

        // Extract the text from the SQL
        let line_start = sql.lines().take(line as usize).map(|l| l.len() + 1).sum::<usize>();
        let start = line_start + col as usize;
        let end = start + token.length as usize;
        let text = &sql[start..end];

        let type_name = token_type_names
            .get(token.token_type as usize)
            .unwrap_or(&"unknown");

        result.push_str(&format!("{:12} {}\n", type_name, text));
    }

    result
}

#[test]
fn snapshot_semantic_tokens_create_table() {
    assert_snapshot!(format_semantic_tokens(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);"
    ));
}

#[test]
fn snapshot_semantic_tokens_quoted_identifiers() {
    assert_snapshot!(format_semantic_tokens(
        "CREATE TABLE \"Game Settings\" (\n  \"user ID\" INTEGER PRIMARY KEY,\n  \"Auto save\" BOOLEAN\n);"
    ));
}

#[test]
fn snapshot_semantic_tokens_select_with_cast() {
    assert_snapshot!(format_semantic_tokens(
        "SELECT CAST(x AS INTEGER), name FROM users WHERE id = 1;"
    ));
}

#[test]
fn snapshot_semantic_tokens_generated_column() {
    assert_snapshot!(format_semantic_tokens(
        "CREATE TABLE rect (w REAL, h REAL, area AS (w * h));"
    ));
}

#[test]
fn snapshot_semantic_tokens_check_constraint() {
    assert_snapshot!(format_semantic_tokens(
        "CREATE TABLE t (x INT CHECK(x > 0), y TEXT DEFAULT 'hello');"
    ));
}
