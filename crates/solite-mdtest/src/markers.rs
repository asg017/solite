//! Marker extraction from SQL code
//!
//! Markers are special placeholders in SQL that indicate positions for testing:
//! - `<acN>` - Autocomplete position (e.g., `<ac1>`, `<ac2>`)
//! - `<hvN>` - Hover position (e.g., `<hv1>`, `<hv2>`)

use once_cell::sync::Lazy;
use regex::Regex;

/// A marker found in SQL code
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The kind of marker (autocomplete or hover)
    pub kind: MarkerKind,
    /// The marker ID (the N in `<acN>`)
    pub id: u32,
    /// Byte offset in the ORIGINAL text (before marker removal)
    pub original_offset: usize,
    /// Byte offset in the CLEAN text (after marker removal)
    pub clean_offset: usize,
    /// Line number (0-indexed)
    pub line: u32,
    /// Column number (0-indexed, in clean text)
    pub column: u32,
}

/// The kind of marker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// `<acN>` - Autocomplete position
    Autocomplete,
    /// `<hvN>` - Hover position
    Hover,
}

/// Result of extracting markers from SQL
#[derive(Debug)]
pub struct ExtractResult {
    /// The SQL with markers removed
    pub clean_sql: String,
    /// The markers found, with positions in clean_sql
    pub markers: Vec<Marker>,
}

// Regex to match markers: <ac1>, <ac2>, <hv1>, <hv2>, etc.
static MARKER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<(ac|hv)(\d+)>").expect("Invalid marker regex"));

/// Extract markers from SQL and return clean SQL with marker positions
pub fn extract_markers(sql: &str) -> ExtractResult {
    let mut markers = Vec::new();
    let mut clean_sql = String::with_capacity(sql.len());
    let mut last_end = 0;
    let mut offset_adjustment = 0; // How many bytes we've removed so far

    for cap in MARKER_REGEX.captures_iter(sql) {
        let full_match = cap.get(0).unwrap();
        let kind_str = cap.get(1).unwrap().as_str();
        let id_str = cap.get(2).unwrap().as_str();

        let kind = match kind_str {
            "ac" => MarkerKind::Autocomplete,
            "hv" => MarkerKind::Hover,
            _ => unreachable!(),
        };
        let id: u32 = id_str.parse().expect("Invalid marker ID");

        // Add text before this marker
        clean_sql.push_str(&sql[last_end..full_match.start()]);

        // Calculate positions
        let original_offset = full_match.start();
        let clean_offset = original_offset - offset_adjustment;

        // Calculate line and column in clean_sql
        let (line, column) = offset_to_line_col(&clean_sql, clean_offset);

        markers.push(Marker {
            kind,
            id,
            original_offset,
            clean_offset,
            line,
            column,
        });

        // Track how much we've removed
        offset_adjustment += full_match.len();
        last_end = full_match.end();
    }

    // Add remaining text
    clean_sql.push_str(&sql[last_end..]);

    ExtractResult { clean_sql, markers }
}

/// Convert byte offset to (line, column), both 0-indexed
fn offset_to_line_col(text: &str, offset: usize) -> (u32, u32) {
    let text_before = &text[..offset.min(text.len())];
    let line = text_before.matches('\n').count() as u32;
    let last_newline = text_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = (offset - last_newline) as u32;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_autocomplete() {
        let sql = "select * from <ac1>;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, "select * from ;");
        assert_eq!(result.markers.len(), 1);

        let m = &result.markers[0];
        assert_eq!(m.kind, MarkerKind::Autocomplete);
        assert_eq!(m.id, 1);
        assert_eq!(m.clean_offset, 14); // "select * from " = 14 chars
        assert_eq!(m.line, 0);
        assert_eq!(m.column, 14);
    }

    #[test]
    fn test_extract_multiple_markers() {
        let sql = "select <ac1> from <ac2>;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, "select  from ;");
        assert_eq!(result.markers.len(), 2);

        assert_eq!(result.markers[0].id, 1);
        assert_eq!(result.markers[0].clean_offset, 7); // "select "
        assert_eq!(result.markers[1].id, 2);
        assert_eq!(result.markers[1].clean_offset, 13); // "select  from "
    }

    #[test]
    fn test_extract_hover_marker() {
        let sql = "select id<hv1> from users;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, "select id from users;");
        assert_eq!(result.markers.len(), 1);

        let m = &result.markers[0];
        assert_eq!(m.kind, MarkerKind::Hover);
        assert_eq!(m.id, 1);
        assert_eq!(m.clean_offset, 9); // "select id"
    }

    #[test]
    fn test_extract_multiline() {
        let sql = "select\n  <ac1>\nfrom users;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, "select\n  \nfrom users;");
        assert_eq!(result.markers.len(), 1);

        let m = &result.markers[0];
        assert_eq!(m.line, 1);
        assert_eq!(m.column, 2);
    }

    #[test]
    fn test_mixed_markers() {
        let sql = "select <ac1>, id<hv1> from <ac2>;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, "select , id from ;");
        assert_eq!(result.markers.len(), 3);

        assert_eq!(result.markers[0].kind, MarkerKind::Autocomplete);
        assert_eq!(result.markers[0].id, 1);

        assert_eq!(result.markers[1].kind, MarkerKind::Hover);
        assert_eq!(result.markers[1].id, 1);

        assert_eq!(result.markers[2].kind, MarkerKind::Autocomplete);
        assert_eq!(result.markers[2].id, 2);
    }

    #[test]
    fn test_no_markers() {
        let sql = "select * from users;";
        let result = extract_markers(sql);

        assert_eq!(result.clean_sql, sql);
        assert!(result.markers.is_empty());
    }
}
