//! Assertion parsing for mdtest
//!
//! Parses assertions from markdown text in two formats:
//!
//! 1. List format:
//!    - `<ac1>`: users, tables
//!    - `<hv1>`: "documentation text"
//!
//! 2. Code block format:
//!    ```assertions
//!    ac1: users, tables
//!    hv1: "documentation text"
//!    ```
//!
//! Also parses inline diagnostic assertions:
//!    select * from x; -- error: [rule-id] "message"

use once_cell::sync::Lazy;
use regex::Regex;

/// An assertion about a marker
#[derive(Debug, Clone, PartialEq)]
pub enum Assertion {
    /// Autocomplete assertion: marker should produce these completions
    Autocomplete {
        marker_id: u32,
        expected: Vec<String>,
        /// If true, must match EXACTLY (no extra items)
        strict: bool,
    },
    /// Hover assertion: hover content must contain these strings
    Hover {
        marker_id: u32,
        contains: Vec<String>,
    },
}

/// An inline diagnostic assertion from `-- error:` comments
#[derive(Debug, Clone, PartialEq)]
pub struct InlineDiagnostic {
    /// Line number (0-indexed)
    pub line: u32,
    /// Optional column number (1-indexed, as in the assertion)
    pub column: Option<u32>,
    /// Optional rule ID (e.g., "double-quoted-string")
    pub rule: Option<String>,
    /// Optional message substring to match
    pub message: Option<String>,
    /// Is this an "ok" assertion (no error expected)?
    pub is_ok: bool,
}

// Regex for list-style assertions: - `<ac1>`: users, tables
// Note: (.*) allows empty assertions which mean "expect no completions"
static LIST_ASSERTION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^-\s*`<(ac|hv)(\d+)>`:\s*(.*)$").expect("Invalid list assertion regex")
});

// Regex for code block assertions: ac1: users, tables
// Note: (.*) allows empty assertions which mean "expect no completions"
static CODE_ASSERTION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(ac|hv)(\d+):\s*(.*)$").expect("Invalid code assertion regex"));

// Regex for inline error assertions: -- error: [rule-id] "message"
// Captures: optional column, optional [rule], optional "message"
static INLINE_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"--\s*error:\s*(?:(\d+)\s+)?(?:\[([^\]]+)\])?\s*(?:"([^"]*)")?"#)
        .expect("Invalid inline error regex")
});

// Regex for inline ok assertions: -- ok
static INLINE_OK_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"--\s*ok\s*$").expect("Invalid inline ok regex"));

/// Parse assertions from markdown text (after the SQL code block)
pub fn parse_assertions(text: &str) -> Vec<Assertion> {
    let mut assertions = Vec::new();

    for line in text.lines() {
        let line = line.trim();

        // Try list format: - `<ac1>`: ...
        if let Some(caps) = LIST_ASSERTION_REGEX.captures(line) {
            if let Some(assertion) = parse_assertion_capture(&caps) {
                assertions.push(assertion);
            }
            continue;
        }

        // Try code block format: ac1: ...
        if let Some(caps) = CODE_ASSERTION_REGEX.captures(line) {
            if let Some(assertion) = parse_assertion_capture(&caps) {
                assertions.push(assertion);
            }
        }
    }

    assertions
}

fn parse_assertion_capture(caps: &regex::Captures) -> Option<Assertion> {
    let kind = caps.get(1)?.as_str();
    let id: u32 = caps.get(2)?.as_str().parse().ok()?;
    let value = caps.get(3)?.as_str().trim();

    match kind {
        "ac" => {
            let (expected, strict) = parse_completion_list(value);
            Some(Assertion::Autocomplete {
                marker_id: id,
                expected,
                strict,
            })
        }
        "hv" => {
            let contains = parse_hover_assertions(value);
            Some(Assertion::Hover {
                marker_id: id,
                contains,
            })
        }
        _ => None,
    }
}

/// Parse completion list: "!users, tables" -> (vec!["users", "tables"], strict=true)
fn parse_completion_list(value: &str) -> (Vec<String>, bool) {
    let value = value.trim();
    let (strict, value) = if let Some(stripped) = value.strip_prefix('!') {
        (true, stripped.trim())
    } else if let Some(stripped) = value.strip_prefix('~') {
        (false, stripped.trim())
    } else {
        (false, value)
    };

    let items: Vec<String> = value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    (items, strict)
}

/// Parse hover assertions: "doc text", "more text" -> vec!["doc text", "more text"]
fn parse_hover_assertions(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in value.chars() {
        match c {
            '"' if !in_quotes => {
                in_quotes = true;
            }
            '"' if in_quotes => {
                in_quotes = false;
                if !current.is_empty() {
                    items.push(current.clone());
                    current.clear();
                }
            }
            _ if in_quotes => {
                current.push(c);
            }
            ',' => {
                // Skip commas between quoted strings
            }
            _ => {
                // Skip whitespace outside quotes
            }
        }
    }

    // Handle unquoted value (for simple assertions)
    if items.is_empty() && !value.contains('"') {
        items.push(value.trim().to_string());
    }

    items
}

/// Parse inline diagnostic assertions from SQL
pub fn parse_inline_diagnostics(sql: &str) -> Vec<InlineDiagnostic> {
    let mut diagnostics = Vec::new();

    for (line_num, line) in sql.lines().enumerate() {
        // Check for -- ok assertion
        if INLINE_OK_REGEX.is_match(line) {
            diagnostics.push(InlineDiagnostic {
                line: line_num as u32,
                column: None,
                rule: None,
                message: None,
                is_ok: true,
            });
            continue;
        }

        // Check for -- error: assertion
        if let Some(caps) = INLINE_ERROR_REGEX.captures(line) {
            let column = caps.get(1).and_then(|m| m.as_str().parse().ok());
            let rule = caps.get(2).map(|m| m.as_str().to_string());
            let message = caps.get(3).map(|m| m.as_str().to_string());

            diagnostics.push(InlineDiagnostic {
                line: line_num as u32,
                column,
                rule,
                message,
                is_ok: false,
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_list_autocomplete() {
        let text = r#"
Some text here.

- `<ac1>`: users, tables
- `<ac2>`: id, name, email
"#;
        let assertions = parse_assertions(text);
        assert_eq!(assertions.len(), 2);

        match &assertions[0] {
            Assertion::Autocomplete {
                marker_id,
                expected,
                strict,
            } => {
                assert_eq!(*marker_id, 1);
                assert_eq!(expected, &["users", "tables"]);
                assert!(!strict);
            }
            _ => panic!("Expected Autocomplete"),
        }

        match &assertions[1] {
            Assertion::Autocomplete {
                marker_id,
                expected,
                strict,
            } => {
                assert_eq!(*marker_id, 2);
                assert_eq!(expected, &["id", "name", "email"]);
                assert!(!strict);
            }
            _ => panic!("Expected Autocomplete"),
        }
    }

    #[test]
    fn test_parse_strict_autocomplete() {
        let text = "- `<ac1>`: !users, tables";
        let assertions = parse_assertions(text);

        match &assertions[0] {
            Assertion::Autocomplete {
                strict, expected, ..
            } => {
                assert!(*strict);
                assert_eq!(expected, &["users", "tables"]);
            }
            _ => panic!("Expected Autocomplete"),
        }
    }

    #[test]
    fn test_parse_hover_assertion() {
        let text = r#"- `<hv1>`: "Student documentation", "@example""#;
        let assertions = parse_assertions(text);

        match &assertions[0] {
            Assertion::Hover {
                marker_id,
                contains,
            } => {
                assert_eq!(*marker_id, 1);
                assert_eq!(contains, &["Student documentation", "@example"]);
            }
            _ => panic!("Expected Hover"),
        }
    }

    #[test]
    fn test_parse_code_block_assertions() {
        let text = r#"
ac1: users, tables
hv1: "documentation"
ac2: !strict, list
"#;
        let assertions = parse_assertions(text);
        assert_eq!(assertions.len(), 3);

        match &assertions[0] {
            Assertion::Autocomplete { marker_id, .. } => assert_eq!(*marker_id, 1),
            _ => panic!("Expected Autocomplete"),
        }

        match &assertions[1] {
            Assertion::Hover { marker_id, .. } => assert_eq!(*marker_id, 1),
            _ => panic!("Expected Hover"),
        }

        match &assertions[2] {
            Assertion::Autocomplete { strict, .. } => assert!(*strict),
            _ => panic!("Expected Autocomplete"),
        }
    }

    #[test]
    fn test_parse_inline_error() {
        let sql = r#"
select * from missing; -- error: [unknown-table]
select "value";        -- error: [double-quoted-string] "string literal"
select X'';            -- error: 8 [empty-blob-literal]
"#;
        let diagnostics = parse_inline_diagnostics(sql);
        assert_eq!(diagnostics.len(), 3);

        assert_eq!(diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].rule, Some("unknown-table".to_string()));
        assert_eq!(diagnostics[0].message, None);
        assert!(!diagnostics[0].is_ok);

        assert_eq!(diagnostics[1].line, 2);
        assert_eq!(
            diagnostics[1].rule,
            Some("double-quoted-string".to_string())
        );
        assert_eq!(diagnostics[1].message, Some("string literal".to_string()));

        assert_eq!(diagnostics[2].line, 3);
        assert_eq!(diagnostics[2].column, Some(8));
        assert_eq!(diagnostics[2].rule, Some("empty-blob-literal".to_string()));
    }

    #[test]
    fn test_parse_inline_ok() {
        let sql = r#"
select 'value'; -- ok
select * from users; -- ok
"#;
        let diagnostics = parse_inline_diagnostics(sql);
        assert_eq!(diagnostics.len(), 2);

        assert!(diagnostics[0].is_ok);
        assert!(diagnostics[1].is_ok);
    }
}
