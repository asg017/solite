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
    let s = if s.starts_with("--") {
        s[2..].trim()
    } else if s.starts_with("/*") && s.ends_with("*/") {
        s[2..s.len() - 2].trim()
    } else if s.starts_with("/*") {
        // unterminated block style: strip leading /*
        s[2..].trim()
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
    for i in 0..(line - 1) {
        offset += lines[i].len();
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
}
