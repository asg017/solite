//! Dot command parsing and execution.
//!
//! This module handles SQLite shell-style dot commands like `.open`, `.tables`, `.schema`, etc.
//!
//! Dot commands are lines that start with `.` at column 0.
//! All other non-empty lines are accumulated as SQL regions.

use solite_ast::Span;

/// A dot command parsed from the source
#[derive(Debug, Clone, PartialEq)]
pub enum DotCommand {
    /// .open <path> - opens a SQLite database file
    Open { path: String, span: Span },
    // Future commands can be added here
}

/// A region of SQL (non-dot-command) content
#[derive(Debug, Clone, PartialEq)]
pub struct SqlRegion {
    /// Starting byte offset in the source
    pub start: usize,
    /// Ending byte offset (exclusive) in the source
    pub end: usize,
}

/// Result of parsing a document with dot commands
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub dot_commands: Vec<DotCommand>,
    pub sql_regions: Vec<SqlRegion>,
    /// Whether any lines starting with '.' were encountered (recognized or not)
    pub has_dot_lines: bool,
}

/// Pre-process source, extracting dot commands and SQL regions.
///
/// Dot commands are lines that start with `.` at column 0.
/// All other non-empty lines are accumulated as SQL regions.
///
/// # Example
/// ```
/// use solite_schema::dotcmd::parse_dot_commands;
///
/// let source = ".open mydb.db\nSELECT * FROM users;";
/// let result = parse_dot_commands(source);
/// assert_eq!(result.dot_commands.len(), 1);
/// assert_eq!(result.sql_regions.len(), 1);
/// ```
/// This yields one DotCommand::Open and one SqlRegion containing the SELECT.
pub fn parse_dot_commands(source: &str) -> ParseResult {
    let mut dot_commands = Vec::new();
    let mut sql_regions = Vec::new();
    let mut current_sql_start: Option<usize> = None;
    let mut byte_offset = 0;
    let mut has_dot_lines = false;

    for line in source.lines() {
        let line_start = byte_offset;
        let line_end = byte_offset + line.len();

        if let Some(stripped) = line.strip_prefix('.') {
            // Track that we saw a dot-prefixed line
            has_dot_lines = true;

            // Finish any current SQL region before the dot command
            if let Some(start) = current_sql_start.take() {
                // The SQL region ends at the start of this line
                if start < line_start {
                    sql_regions.push(SqlRegion {
                        start,
                        end: line_start,
                    });
                }
            }

            // Parse the dot command
            if let Some(cmd) = parse_dot_command_line(stripped, line_start) {
                dot_commands.push(cmd);
            }
        } else if !line.trim().is_empty() {
            // Non-empty SQL line
            if current_sql_start.is_none() {
                current_sql_start = Some(line_start);
            }
            // We'll extend the region when we finish
        } else {
            // Empty line - finish current SQL region if any
            if let Some(start) = current_sql_start.take() {
                if start < line_start {
                    sql_regions.push(SqlRegion {
                        start,
                        end: line_start,
                    });
                }
            }
        }

        // Move to the next line (account for newline character)
        byte_offset = line_end;
        if byte_offset < source.len() {
            // Skip the newline character(s)
            if source[byte_offset..].starts_with("\r\n") {
                byte_offset += 2;
            } else if source[byte_offset..].starts_with('\n')
                || source[byte_offset..].starts_with('\r')
            {
                byte_offset += 1;
            }
        }
    }

    // Finish any remaining SQL region
    if let Some(start) = current_sql_start {
        if start < source.len() {
            sql_regions.push(SqlRegion {
                start,
                end: source.len(),
            });
        }
    }

    ParseResult {
        dot_commands,
        sql_regions,
        has_dot_lines,
    }
}

/// Parse a single dot command line (without the leading '.')
fn parse_dot_command_line(line: &str, line_start: usize) -> Option<DotCommand> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Split into command and arguments
    let (cmd, args) = match line.find(|c: char| c.is_whitespace()) {
        Some(pos) => (&line[..pos], line[pos..].trim()),
        None => (line, ""),
    };

    match cmd.to_lowercase().as_str() {
        "open" => {
            let path = parse_path_argument(args);
            if path.is_empty() {
                return None;
            }
            // The span covers the entire .open command line
            // line_start points to the '.'
            let span = Span::new(line_start, line_start + 1 + line.len());
            Some(DotCommand::Open { path, span })
        }
        _ => {
            // Unknown command - could add an Unknown variant in the future
            None
        }
    }
}

/// Parse a path argument, handling quoted paths
fn parse_path_argument(args: &str) -> String {
    let args = args.trim();
    if args.is_empty() {
        return String::new();
    }

    // Handle quoted paths (single or double quotes)
    if ((args.starts_with('"') && args.ends_with('"'))
        || (args.starts_with('\'') && args.ends_with('\'')))
        && args.len() >= 2
    {
        return args[1..args.len() - 1].to_string();
    }

    // Unquoted path - take everything up to the first whitespace or end
    match args.find(|c: char| c.is_whitespace()) {
        Some(pos) => args[..pos].to_string(),
        None => args.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let result = parse_dot_commands("");
        assert!(result.dot_commands.is_empty());
        assert!(result.sql_regions.is_empty());
    }

    #[test]
    fn test_sql_only() {
        let source = "SELECT * FROM users;\nSELECT 1;";
        let result = parse_dot_commands(source);
        assert!(result.dot_commands.is_empty());
        assert_eq!(result.sql_regions.len(), 1);
        assert_eq!(result.sql_regions[0].start, 0);
        assert_eq!(result.sql_regions[0].end, source.len());
    }

    #[test]
    fn test_dot_command_only() {
        let source = ".open mydb.db";
        let result = parse_dot_commands(source);
        assert_eq!(result.dot_commands.len(), 1);
        assert!(result.sql_regions.is_empty());

        match &result.dot_commands[0] {
            DotCommand::Open { path, span } => {
                assert_eq!(path, "mydb.db");
                assert_eq!(span.start, 0);
                assert_eq!(span.end, source.len());
            }
        }
    }

    #[test]
    fn test_open_relative_path() {
        let source = ".open ./data/test.db";
        let result = parse_dot_commands(source);
        assert_eq!(result.dot_commands.len(), 1);

        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "./data/test.db");
            }
        }
    }

    #[test]
    fn test_open_absolute_path() {
        let source = ".open /home/user/databases/test.db";
        let result = parse_dot_commands(source);
        assert_eq!(result.dot_commands.len(), 1);

        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "/home/user/databases/test.db");
            }
        }
    }

    #[test]
    fn test_open_quoted_path_double() {
        let source = ".open \"path with spaces/test.db\"";
        let result = parse_dot_commands(source);
        assert_eq!(result.dot_commands.len(), 1);

        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "path with spaces/test.db");
            }
        }
    }

    #[test]
    fn test_open_quoted_path_single() {
        let source = ".open 'path with spaces/test.db'";
        let result = parse_dot_commands(source);
        assert_eq!(result.dot_commands.len(), 1);

        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "path with spaces/test.db");
            }
        }
    }

    #[test]
    fn test_mixed_sql_and_dot_commands() {
        let source = ".open mydb.db\nSELECT * FROM users;";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        assert_eq!(result.sql_regions.len(), 1);

        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "mydb.db");
            }
        }

        let sql = &source[result.sql_regions[0].start..result.sql_regions[0].end];
        assert_eq!(sql, "SELECT * FROM users;");
    }

    #[test]
    fn test_sql_then_dot_command() {
        let source = "SELECT 1;\n.open mydb.db";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        assert_eq!(result.sql_regions.len(), 1);

        let sql = &source[result.sql_regions[0].start..result.sql_regions[0].end];
        assert_eq!(sql, "SELECT 1;\n");
    }

    #[test]
    fn test_multiple_dot_commands() {
        let source = ".open db1.db\n.open db2.db\n.open db3.db";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 3);
        assert!(result.sql_regions.is_empty());

        let paths: Vec<&str> = result
            .dot_commands
            .iter()
            .map(|cmd| match cmd {
                DotCommand::Open { path, .. } => path.as_str(),
            })
            .collect();
        assert_eq!(paths, vec!["db1.db", "db2.db", "db3.db"]);
    }

    #[test]
    fn test_interleaved_sql_and_dot_commands() {
        let source = "SELECT 1;\n.open db1.db\nSELECT 2;\n.open db2.db\nSELECT 3;";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 2);
        assert_eq!(result.sql_regions.len(), 3);

        // Verify SQL regions contain correct content
        let sql1 = &source[result.sql_regions[0].start..result.sql_regions[0].end];
        let sql2 = &source[result.sql_regions[1].start..result.sql_regions[1].end];
        let sql3 = &source[result.sql_regions[2].start..result.sql_regions[2].end];

        assert_eq!(sql1, "SELECT 1;\n");
        assert_eq!(sql2, "SELECT 2;\n");
        assert_eq!(sql3, "SELECT 3;");
    }

    #[test]
    fn test_span_correctness() {
        let source = "SELECT 1;\n.open test.db\nSELECT 2;";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        match &result.dot_commands[0] {
            DotCommand::Open { span, .. } => {
                // ".open test.db" starts at byte 10 (after "SELECT 1;\n")
                assert_eq!(span.start, 10);
                // The span should cover ".open test.db" which is 13 characters
                assert_eq!(span.end, 23);
            }
        }
    }

    #[test]
    fn test_empty_lines_between_sql() {
        let source = "SELECT 1;\n\nSELECT 2;";
        let result = parse_dot_commands(source);

        assert!(result.dot_commands.is_empty());
        // Empty lines split SQL regions
        assert_eq!(result.sql_regions.len(), 2);
    }

    #[test]
    fn test_whitespace_only_lines() {
        let source = "SELECT 1;\n   \nSELECT 2;";
        let result = parse_dot_commands(source);

        assert!(result.dot_commands.is_empty());
        // Whitespace-only lines split SQL regions
        assert_eq!(result.sql_regions.len(), 2);
    }

    #[test]
    fn test_open_without_path() {
        let source = ".open";
        let result = parse_dot_commands(source);

        // .open without a path should be ignored
        assert!(result.dot_commands.is_empty());
    }

    #[test]
    fn test_open_case_insensitive() {
        let source = ".OPEN mydb.db";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "mydb.db");
            }
        }
    }

    #[test]
    fn test_open_mixed_case() {
        let source = ".OpEn mydb.db";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "mydb.db");
            }
        }
    }

    #[test]
    fn test_unknown_dot_command() {
        let source = ".unknown_command arg1";
        let result = parse_dot_commands(source);

        // Unknown commands are ignored
        assert!(result.dot_commands.is_empty());
    }

    #[test]
    fn test_dot_alone() {
        let source = ".";
        let result = parse_dot_commands(source);

        assert!(result.dot_commands.is_empty());
    }

    #[test]
    fn test_multiline_sql_statement() {
        let source = "SELECT\n  a,\n  b\nFROM t;";
        let result = parse_dot_commands(source);

        assert!(result.dot_commands.is_empty());
        assert_eq!(result.sql_regions.len(), 1);
        assert_eq!(result.sql_regions[0].start, 0);
        assert_eq!(result.sql_regions[0].end, source.len());
    }

    #[test]
    fn test_windows_line_endings() {
        let source = ".open test.db\r\nSELECT 1;";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        assert_eq!(result.sql_regions.len(), 1);

        let sql = &source[result.sql_regions[0].start..result.sql_regions[0].end];
        assert_eq!(sql, "SELECT 1;");
    }

    #[test]
    fn test_sql_region_exact_offsets() {
        let source = "SELECT 1;";
        let result = parse_dot_commands(source);

        assert_eq!(result.sql_regions.len(), 1);
        assert_eq!(result.sql_regions[0].start, 0);
        assert_eq!(result.sql_regions[0].end, 9);
        assert_eq!(
            &source[result.sql_regions[0].start..result.sql_regions[0].end],
            "SELECT 1;"
        );
    }

    #[test]
    fn test_open_with_extra_whitespace() {
        let source = ".open    mydb.db";
        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 1);
        match &result.dot_commands[0] {
            DotCommand::Open { path, .. } => {
                assert_eq!(path, "mydb.db");
            }
        }
    }

    #[test]
    fn test_complex_scenario() {
        let source = r#"-- Initial setup
.open myapp.db

CREATE TABLE users (id INTEGER PRIMARY KEY);

.open backup.db

SELECT * FROM users;
INSERT INTO users VALUES (1);"#;

        let result = parse_dot_commands(source);

        assert_eq!(result.dot_commands.len(), 2);
        assert_eq!(result.sql_regions.len(), 3);

        // Verify paths
        let paths: Vec<&str> = result
            .dot_commands
            .iter()
            .map(|cmd| match cmd {
                DotCommand::Open { path, .. } => path.as_str(),
            })
            .collect();
        assert_eq!(paths, vec!["myapp.db", "backup.db"]);

        // Verify SQL regions contain expected content
        let sql1 = &source[result.sql_regions[0].start..result.sql_regions[0].end];
        assert!(sql1.contains("-- Initial setup"));

        let sql2 = &source[result.sql_regions[1].start..result.sql_regions[1].end];
        assert!(sql2.contains("CREATE TABLE users"));

        let sql3 = &source[result.sql_regions[2].start..result.sql_regions[2].end];
        assert!(sql3.contains("SELECT * FROM users"));
        assert!(sql3.contains("INSERT INTO users"));
    }
}
