//! Tests for dot command handling in solite-fmt CLI.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the path to the solite-fmt binary
fn solite_fmt_bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up from solite_fmt
    path.pop(); // Go up from crates
    path.push("target");
    path.push("debug");
    path.push("solite-fmt");
    path
}

/// Get path to test fixtures directory
fn fixtures_dir() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path
}

#[test]
fn test_fmt_with_dot_commands_enabled() {
    let sql_file = fixtures_dir().join("fmt_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();
    let sql_content = ".open test.db\nSELECT    a,b    FROM    t;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt (dot commands enabled by default)
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should preserve .open command and format SQL
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".open test.db"),
        "Expected .open to be preserved, got: {}",
        stdout
    );
    assert!(
        stdout.contains("select"),
        "Expected formatted SQL, got: {}",
        stdout
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_with_dot_commands_disabled() {
    let sql_file = fixtures_dir().join("fmt_no_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();
    let sql_content = ".open test.db\nSELECT 1;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt with --no-dot-commands
    let output = Command::new(solite_fmt_bin())
        .arg("--no-dot-commands")
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    // When dot commands are disabled, ".open" is treated as SQL
    // which should cause a parse error
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "Expected parse error (exit 2), got stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_pure_sql_file() {
    let sql_file = fixtures_dir().join("fmt_pure_sql_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();
    let sql_content = "SELECT    a,b   FROM   t;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt on a file with no dot commands
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should be formatted
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("select"),
        "Expected formatted SQL with lowercase keywords, got: {}",
        stdout
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_check_mode_with_dot_commands() {
    let sql_file = fixtures_dir().join("fmt_check_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Unformatted SQL with dot command
    let sql_content = ".open test.db\nSELECT    a,b    FROM    t;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt --check
    let output = Command::new(solite_fmt_bin())
        .arg("--check")
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    // Should return exit code 1 because file needs formatting
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "Expected check failure (exit 1), got: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_diff_mode_with_dot_commands() {
    let sql_file = fixtures_dir().join("fmt_diff_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Unformatted SQL with dot command
    let sql_content = ".open test.db\nSELECT    a,b    FROM    t;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt --diff
    let output = Command::new(solite_fmt_bin())
        .arg("--diff")
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    // Diff mode should still show changes
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should have some diff output showing changes
    assert!(
        stdout.contains("-") || stdout.contains("+"),
        "Expected diff output, got: {}",
        stdout
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_multiple_sql_regions() {
    let sql_file = fixtures_dir().join("fmt_multi_region_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Multiple SQL regions separated by dot commands
    let sql_content = ".open db1.db\nSELECT   a   FROM   t1;\n.open db2.db\nSELECT   b   FROM   t2;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should preserve both .open commands and format both SQL regions
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".open db1.db"), "Expected first .open");
    assert!(stdout.contains(".open db2.db"), "Expected second .open");

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_only_dot_commands() {
    let sql_file = fixtures_dir().join("fmt_only_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // File with only dot commands, no SQL
    let sql_content = ".open db1.db\n.open db2.db";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should preserve dot commands
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".open db1.db"), "Expected first .open");
    assert!(stdout.contains(".open db2.db"), "Expected second .open");

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_with_unknown_dot_commands() {
    let sql_file = fixtures_dir().join("fmt_unknown_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // File with unknown dot commands (.mode, .headers) that should be preserved
    let sql_content = ".mode csv\n.headers on\nSELECT    a,b    FROM    t    WHERE    x=1    AND    y=2;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success with unknown dot commands, got stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should preserve unknown dot commands and format SQL
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".mode csv"),
        "Expected .mode to be preserved, got: {}",
        stdout
    );
    assert!(
        stdout.contains(".headers on"),
        "Expected .headers to be preserved, got: {}",
        stdout
    );
    // SQL should be formatted (AND on separate line)
    assert!(
        stdout.contains("and y = 2"),
        "Expected SQL to be formatted with 'and' on its own line, got: {}",
        stdout
    );

    let _ = fs::remove_file(&sql_file);
}

#[test]
fn test_fmt_with_open_and_unknown_dot_commands() {
    let sql_file = fixtures_dir().join("fmt_mixed_dot_cmd_test.sql");
    fs::create_dir_all(fixtures_dir()).unwrap();

    // Mix of known (.open) and unknown (.mode) dot commands
    let sql_content = ".open test.db\n.mode csv\nSELECT    a,b    FROM    t;";
    fs::write(&sql_file, sql_content).unwrap();

    // Run solite-fmt
    let output = Command::new(solite_fmt_bin())
        .arg(&sql_file)
        .output()
        .expect("Failed to execute solite-fmt");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "Expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output should preserve both dot commands
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".open test.db"), "Expected .open");
    assert!(stdout.contains(".mode csv"), "Expected .mode");
    assert!(stdout.contains("select"), "Expected formatted SQL");

    let _ = fs::remove_file(&sql_file);
}
