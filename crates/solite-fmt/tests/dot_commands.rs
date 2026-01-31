//! Tests for dot command handling in solite-fmt.
//!
//! These tests verify that the formatter correctly handles documents
//! containing dot commands (like `.open`, `.mode`) mixed with SQL.

use solite_fmt::{format_document, format_sql, FormatConfig};

/// Test formatting a document with dot commands (enabled by default).
#[test]
fn test_fmt_with_dot_commands_enabled() {
    let sql_content = ".open test.db\nSELECT    a,b    FROM    t;";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let formatted = result.unwrap();

    // Output should preserve .open command and format SQL
    assert!(
        formatted.contains(".open test.db"),
        "Expected .open to be preserved, got: {}",
        formatted
    );
    assert!(
        formatted.contains("select"),
        "Expected formatted SQL with lowercase keywords, got: {}",
        formatted
    );
}

/// Test that format_sql (without dot command handling) fails on dot commands.
#[test]
fn test_fmt_with_dot_commands_disabled() {
    let sql_content = ".open test.db\nSELECT 1;";
    let config = FormatConfig::default();

    // When using format_sql directly (no dot command handling),
    // ".open" is treated as SQL which should cause a parse error
    let result = format_sql(sql_content, &config);
    assert!(
        result.is_err(),
        "Expected parse error when dot commands not handled, got: {:?}",
        result.ok()
    );
}

/// Test formatting a pure SQL file (no dot commands).
#[test]
fn test_fmt_pure_sql_file() {
    let sql_content = "SELECT    a,b   FROM   t;";
    let config = FormatConfig::default();

    let result = format_sql(sql_content, &config);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let formatted = result.unwrap();

    // Output should be formatted with lowercase keywords
    assert!(
        formatted.contains("select"),
        "Expected formatted SQL with lowercase keywords, got: {}",
        formatted
    );
}

/// Test check mode: detect when formatting is needed.
#[test]
fn test_fmt_check_mode_with_dot_commands() {
    let config = FormatConfig::default();

    // Unformatted SQL with dot command
    let unformatted = ".open test.db\nSELECT    a,b    FROM    t;";
    let formatted_result = format_document(unformatted, &config);
    assert!(formatted_result.is_ok());

    let formatted = formatted_result.unwrap();

    // The input and output should be different (needs formatting)
    assert_ne!(
        unformatted, formatted,
        "Expected unformatted input to differ from formatted output"
    );

    // Now test with already-formatted content
    let already_formatted = format_document(&formatted, &config).unwrap();
    assert_eq!(
        formatted, already_formatted,
        "Already formatted content should not change"
    );
}

/// Test diff mode behavior by comparing input to formatted output.
#[test]
fn test_fmt_diff_mode_with_dot_commands() {
    let sql_content = ".open test.db\nSELECT    a,b    FROM    t;";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok());

    let formatted = result.unwrap();

    // Verify there are actual differences (which would show in a diff)
    assert_ne!(
        sql_content, formatted,
        "Input and output should differ for diff to show changes"
    );

    // The dot command should be preserved
    assert!(formatted.contains(".open test.db"));
    // The SQL should be reformatted
    assert!(formatted.contains("select"));
}

/// Test formatting multiple SQL regions separated by dot commands.
#[test]
fn test_fmt_multiple_sql_regions() {
    let sql_content =
        ".open db1.db\nSELECT   a   FROM   t1;\n.open db2.db\nSELECT   b   FROM   t2;";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let formatted = result.unwrap();

    // Output should preserve both .open commands
    assert!(
        formatted.contains(".open db1.db"),
        "Expected first .open, got: {}",
        formatted
    );
    assert!(
        formatted.contains(".open db2.db"),
        "Expected second .open, got: {}",
        formatted
    );

    // Both SQL regions should be formatted
    assert!(
        formatted.matches("select").count() >= 2,
        "Expected both SQL statements formatted, got: {}",
        formatted
    );
}

/// Test formatting a file with only dot commands (no SQL).
#[test]
fn test_fmt_only_dot_commands() {
    let sql_content = ".open db1.db\n.open db2.db";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let formatted = result.unwrap();

    // Output should preserve dot commands
    assert!(
        formatted.contains(".open db1.db"),
        "Expected first .open, got: {}",
        formatted
    );
    assert!(
        formatted.contains(".open db2.db"),
        "Expected second .open, got: {}",
        formatted
    );
}

/// Test formatting with unknown dot commands (like .mode, .headers).
#[test]
fn test_fmt_with_unknown_dot_commands() {
    let sql_content =
        ".mode csv\n.headers on\nSELECT    a,b    FROM    t    WHERE    x=1    AND    y=2;";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(
        result.is_ok(),
        "Expected success with unknown dot commands, got: {:?}",
        result.err()
    );

    let formatted = result.unwrap();

    // Unknown dot commands should be preserved
    assert!(
        formatted.contains(".mode csv"),
        "Expected .mode to be preserved, got: {}",
        formatted
    );
    assert!(
        formatted.contains(".headers on"),
        "Expected .headers to be preserved, got: {}",
        formatted
    );
    // SQL should be formatted
    assert!(
        formatted.contains("select"),
        "Expected SQL to be formatted, got: {}",
        formatted
    );
}

/// Test formatting with a mix of known (.open) and unknown (.mode) dot commands.
#[test]
fn test_fmt_with_open_and_unknown_dot_commands() {
    let sql_content = ".open test.db\n.mode csv\nSELECT    a,b    FROM    t;";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());

    let formatted = result.unwrap();

    // Both dot commands should be preserved
    assert!(
        formatted.contains(".open test.db"),
        "Expected .open, got: {}",
        formatted
    );
    assert!(
        formatted.contains(".mode csv"),
        "Expected .mode, got: {}",
        formatted
    );
    // SQL should be formatted
    assert!(
        formatted.contains("select"),
        "Expected formatted SQL, got: {}",
        formatted
    );
}

/// Test that format_document works the same as format_sql for pure SQL.
#[test]
fn test_format_document_pure_sql_equivalence() {
    let sql_content = "SELECT a, b FROM t WHERE x = 1;";
    let config = FormatConfig::default();

    let from_sql = format_sql(sql_content, &config).unwrap();
    let from_doc = format_document(sql_content, &config).unwrap();

    assert_eq!(
        from_sql, from_doc,
        "format_sql and format_document should produce same output for pure SQL"
    );
}

/// Test formatting preserves dot command line positions.
#[test]
fn test_dot_command_line_preservation() {
    let sql_content = ".open test.db\n\nSELECT 1;\n\n.mode csv";
    let config = FormatConfig::default();

    let result = format_document(sql_content, &config);
    assert!(result.is_ok());

    let formatted = result.unwrap();

    // Check that .open comes before .mode in the output
    let open_pos = formatted.find(".open").unwrap();
    let mode_pos = formatted.find(".mode").unwrap();
    assert!(
        open_pos < mode_pos,
        "Dot commands should maintain relative order"
    );
}
