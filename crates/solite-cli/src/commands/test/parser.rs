//! Parsing utilities for test file comments and references.

/// Parse an epilogue comment, stripping comment markers.
///
/// Handles both line comments (`-- ...`) and block comments (`/* ... */`).
///
/// # Examples
///
/// ```ignore
/// parse_epilogue_comment("-- expected value") // "expected value"
/// parse_epilogue_comment("/* expected */")    // "expected"
/// ```
pub fn parse_epilogue_comment(ep: &str) -> String {
    let s = ep.trim();
    let s = if let Some(rest) = s.strip_prefix("--") {
        rest.trim()
    } else if s.starts_with("/*") && s.ends_with("*/") {
        s.strip_prefix("/*").unwrap().strip_suffix("*/").unwrap().trim()
    } else if let Some(rest) = s.strip_prefix("/*") {
        // unterminated block style: strip leading /*
        rest.trim()
    } else {
        s
    };
    s.to_string()
}

/// Parse line and column from a reference display string.
///
/// Reference format: "file:line:column"
/// Returns (line, column) tuple.
pub fn parse_line_col_from_ref(ref_display: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = ref_display.rsplitn(3, ':').collect();
    if parts.len() < 2 {
        return None;
    }
    let col = parts[0].parse::<usize>().ok()?;
    let line = parts[1].parse::<usize>().ok()?;
    Some((line, col))
}

/// Parse file, line, and column from a reference display string.
///
/// Reference format: "file:line:column"
pub fn parse_ref_file_line_col(ref_display: &str) -> Option<(String, usize, usize)> {
    let parts: Vec<&str> = ref_display.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let file = parts[0].to_string();
    let line = parts[1].parse::<usize>().ok()?;
    let col = parts[2].parse::<usize>().ok()?;
    Some((file, line, col))
}

/// Recover an `error: <msg>` assertion for a statement that failed to
/// *prepare* (and so never produced a `Step` with an epilogue).
///
/// Scans from `offset` (the start of the failing statement) to the first
/// `;` and extracts the trailing same-line comment after it. Returns the
/// expected error message and the byte offset just past the comment, where
/// stepping can resume. The scan-to-`;` heuristic can be fooled by `;`
/// inside string literals, but the statement already failed to prepare, so
/// a missed assertion only means the file fails (as it would have anyway).
pub fn prepare_error_assertion(src: &str, offset: usize) -> Option<(String, usize)> {
    let semi = src.get(offset..)?.find(';')? + offset;
    let rest_index = semi + 1;
    let raw_epilogue = solite_core::extract_epilogue(src, rest_index)?;
    let resume = rest_index + raw_epilogue.len();
    let cleaned = parse_epilogue_comment(&raw_epilogue);
    let expected = cleaned.strip_prefix("error:")?.trim().to_string();
    Some((expected, resume))
}

/// A parsed `@snap` directive from an epilogue comment.
#[derive(Debug)]
pub struct SnapDirective {
    /// The snapshot name (required). Must match `[a-zA-Z0-9_-]+`.
    pub name: String,
}

/// Try to parse a `@snap <name>` directive from a stripped epilogue string.
///
/// Returns `Some(SnapDirective)` if the epilogue starts with `@snap`,
/// or `None` if it doesn't look like a snap directive.
/// Returns an error message string if `@snap` is present but the name is missing or invalid.
pub fn parse_snap_directive(epilogue: &str) -> Result<Option<SnapDirective>, String> {
    let trimmed = epilogue.trim();
    let Some(rest) = trimmed.strip_prefix("@snap") else {
        return Ok(None);
    };
    // require a word boundary so "@snapshot" errors instead of silently
    // creating a snapshot named "shot"
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return Err(format!(
            "unrecognized directive '@snap{}': did you mean '@snap <name>'?",
            rest
        ));
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("@snap requires a name (e.g. @snap my-snapshot)".to_string());
    }
    // Validate name: [a-zA-Z0-9_-]+
    if !rest
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "@snap name '{}' is invalid: must match [a-zA-Z0-9_-]+",
            rest
        ));
    }
    Ok(Some(SnapDirective {
        name: rest.to_string(),
    }))
}

/// Compute byte offset from a reference string.
///
/// Converts line:column to a byte offset in the source content.
pub fn compute_offset_from_reference(content: &str, ref_display: &str) -> Option<usize> {
    let (line, col) = parse_line_col_from_ref(ref_display)?;
    let lines: Vec<&str> = content.lines().collect();

    if line == 0 || line > lines.len() {
        return None;
    }

    let mut offset = 0usize;
    for l in &lines[..line - 1] {
        offset += l.len();
        offset += 1; // newline
    }

    let col0 = if col == 0 { 0 } else { col - 1 };
    offset += col0;
    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_epilogue_line_comment() {
        assert_eq!(parse_epilogue_comment("-- expected"), "expected");
        assert_eq!(parse_epilogue_comment("--expected"), "expected");
        assert_eq!(parse_epilogue_comment("  -- spaced  "), "spaced");
    }

    #[test]
    fn test_parse_epilogue_block_comment() {
        assert_eq!(parse_epilogue_comment("/* expected */"), "expected");
        assert_eq!(parse_epilogue_comment("/*expected*/"), "expected");
    }

    #[test]
    fn test_parse_epilogue_unterminated_block() {
        assert_eq!(parse_epilogue_comment("/* unterminated"), "unterminated");
    }

    #[test]
    fn test_parse_epilogue_plain() {
        assert_eq!(parse_epilogue_comment("plain text"), "plain text");
    }

    #[test]
    fn test_parse_line_col_valid() {
        assert_eq!(parse_line_col_from_ref("file.sql:10:5"), Some((10, 5)));
        assert_eq!(parse_line_col_from_ref("path/to/file.sql:1:1"), Some((1, 1)));
    }

    #[test]
    fn test_parse_line_col_invalid() {
        assert_eq!(parse_line_col_from_ref("no_colons"), None);
        assert_eq!(parse_line_col_from_ref("one:colon"), None);
        assert_eq!(parse_line_col_from_ref("file:notnum:5"), None);
    }

    #[test]
    fn test_parse_ref_file_line_col_valid() {
        let result = parse_ref_file_line_col("test.sql:10:5");
        assert_eq!(result, Some(("test.sql".to_string(), 10, 5)));
    }

    #[test]
    fn test_parse_ref_file_line_col_invalid() {
        assert_eq!(parse_ref_file_line_col("no_colons"), None);
        assert_eq!(parse_ref_file_line_col("one:two"), None);
    }

    #[test]
    fn test_compute_offset() {
        let content = "line1\nline2\nline3";
        // line 1, col 1 = offset 0
        assert_eq!(compute_offset_from_reference(content, "f:1:1"), Some(0));
        // line 2, col 1 = offset 6 (after "line1\n")
        assert_eq!(compute_offset_from_reference(content, "f:2:1"), Some(6));
        // line 2, col 3 = offset 8
        assert_eq!(compute_offset_from_reference(content, "f:2:3"), Some(8));
    }

    #[test]
    fn test_compute_offset_invalid_line() {
        let content = "line1\nline2";
        assert_eq!(compute_offset_from_reference(content, "f:0:1"), None);
        assert_eq!(compute_offset_from_reference(content, "f:10:1"), None);
    }

    // --- prepare_error_assertion tests ---

    #[test]
    fn test_prepare_error_assertion_found() {
        let src = "SELECT * FROM nope; -- error: no such table: nope\nSELECT 1;\n";
        let (expected, resume) = prepare_error_assertion(src, 0).unwrap();
        assert_eq!(expected, "no such table: nope");
        // resume points at the newline after the comment
        assert_eq!(&src[resume..], "\nSELECT 1;\n");
    }

    #[test]
    fn test_prepare_error_assertion_with_offset() {
        let src = "SELECT 1;\nSELECT * FROM nope; -- error: no such table: nope\n";
        let (expected, _) = prepare_error_assertion(src, 10).unwrap();
        assert_eq!(expected, "no such table: nope");
    }

    #[test]
    fn test_prepare_error_assertion_no_comment() {
        assert_eq!(prepare_error_assertion("SELECT * FROM nope;\n", 0), None);
    }

    #[test]
    fn test_prepare_error_assertion_non_error_comment() {
        assert_eq!(
            prepare_error_assertion("SELECT * FROM nope; -- 42\n", 0),
            None
        );
    }

    #[test]
    fn test_prepare_error_assertion_comment_on_next_line() {
        assert_eq!(
            prepare_error_assertion("SELECT * FROM nope;\n-- error: nope\n", 0),
            None
        );
    }

    #[test]
    fn test_prepare_error_assertion_no_semicolon() {
        assert_eq!(prepare_error_assertion("SELECT * FROM nope", 0), None);
        assert_eq!(prepare_error_assertion("", 5), None);
    }

    // --- @snap directive tests ---

    #[test]
    fn test_parse_snap_directive_with_name() {
        let result = parse_snap_directive("@snap my-snapshot").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "my-snapshot");
    }

    #[test]
    fn test_parse_snap_directive_not_snap() {
        let result = parse_snap_directive("some value").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_snap_directive_missing_name() {
        let result = parse_snap_directive("@snap");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("requires a name"));
    }

    #[test]
    fn test_parse_snap_directive_missing_name_trailing_space() {
        let result = parse_snap_directive("@snap   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_snap_directive_invalid_name_spaces() {
        let result = parse_snap_directive("@snap has spaces");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid"));
    }

    #[test]
    fn test_parse_snap_directive_invalid_name_dots() {
        let result = parse_snap_directive("@snap foo.bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_snap_directive_invalid_name_slash() {
        let result = parse_snap_directive("@snap foo/bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_snap_directive_invalid_name_special_chars() {
        assert!(parse_snap_directive("@snap foo@bar").is_err());
        assert!(parse_snap_directive("@snap foo!").is_err());
        assert!(parse_snap_directive("@snap foo#bar").is_err());
        assert!(parse_snap_directive("@snap foo=bar").is_err());
    }

    #[test]
    fn test_parse_snap_directive_valid_names() {
        assert!(parse_snap_directive("@snap foo").unwrap().is_some());
        assert!(parse_snap_directive("@snap foo-bar").unwrap().is_some());
        assert!(parse_snap_directive("@snap foo_bar").unwrap().is_some());
        assert!(parse_snap_directive("@snap foo123").unwrap().is_some());
        assert!(parse_snap_directive("@snap FOO").unwrap().is_some());
    }

    #[test]
    fn test_parse_snap_directive_single_char_name() {
        let result = parse_snap_directive("@snap x").unwrap().unwrap();
        assert_eq!(result.name, "x");
    }

    #[test]
    fn test_parse_snap_directive_numbers_only_name() {
        let result = parse_snap_directive("@snap 123").unwrap().unwrap();
        assert_eq!(result.name, "123");
    }

    #[test]
    fn test_parse_snap_directive_preserves_exact_name() {
        let result = parse_snap_directive("@snap My-Snap_01").unwrap().unwrap();
        assert_eq!(result.name, "My-Snap_01");
    }

    #[test]
    fn test_parse_snap_directive_extra_whitespace_before_name() {
        let result = parse_snap_directive("@snap    my-snap").unwrap().unwrap();
        assert_eq!(result.name, "my-snap");
    }

    #[test]
    fn test_parse_snap_directive_leading_whitespace() {
        let result = parse_snap_directive("  @snap my-snap").unwrap().unwrap();
        assert_eq!(result.name, "my-snap");
    }

    #[test]
    fn test_parse_snap_not_prefix_match() {
        // "@snapshot" must not be parsed as "@snap" + name "shot" — the
        // directive requires a word boundary after "@snap"
        let result = parse_snap_directive("@snapshot");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("did you mean"));

        let result = parse_snap_directive("@snapshot foo");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_snap_directive_empty_string() {
        let result = parse_snap_directive("").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_snap_directive_just_at_sign() {
        let result = parse_snap_directive("@").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_snap_directive_other_directive() {
        let result = parse_snap_directive("@test something").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_snap_directive_long_name() {
        let name = "a".repeat(100);
        let input = format!("@snap {}", name);
        let result = parse_snap_directive(&input).unwrap().unwrap();
        assert_eq!(result.name, name);
    }

    #[test]
    fn test_parse_snap_directive_hyphen_underscore_combo() {
        let result = parse_snap_directive("@snap a-b_c-d_e").unwrap().unwrap();
        assert_eq!(result.name, "a-b_c-d_e");
    }

    // --- epilogue → snap interaction tests ---

    #[test]
    fn test_epilogue_then_snap_roundtrip() {
        // Simulates the full pipeline: raw epilogue → parse_epilogue_comment → parse_snap_directive
        let raw = "-- @snap my-test";
        let epilogue = parse_epilogue_comment(raw);
        assert_eq!(epilogue, "@snap my-test");
        let snap = parse_snap_directive(&epilogue).unwrap().unwrap();
        assert_eq!(snap.name, "my-test");
    }

    #[test]
    fn test_epilogue_then_snap_block_comment() {
        let raw = "/* @snap block-test */";
        let epilogue = parse_epilogue_comment(raw);
        assert_eq!(epilogue, "@snap block-test");
        let snap = parse_snap_directive(&epilogue).unwrap().unwrap();
        assert_eq!(snap.name, "block-test");
    }

    #[test]
    fn test_epilogue_not_snap() {
        let raw = "-- 42";
        let epilogue = parse_epilogue_comment(raw);
        assert_eq!(epilogue, "42");
        assert!(parse_snap_directive(&epilogue).unwrap().is_none());
    }

    #[test]
    fn test_epilogue_error_not_snap() {
        let raw = "-- error: no such table";
        let epilogue = parse_epilogue_comment(raw);
        assert!(parse_snap_directive(&epilogue).unwrap().is_none());
    }

    #[test]
    fn test_epilogue_todo_not_snap() {
        let raw = "-- TODO fix later";
        let epilogue = parse_epilogue_comment(raw);
        assert!(parse_snap_directive(&epilogue).unwrap().is_none());
    }
}
