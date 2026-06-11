//! Shared TUI utility functions.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use solite_core::sqlite::OwnedValue;

/// Maximum characters to display in a cell before truncating
pub(crate) const MAX_CELL_DISPLAY_LEN: usize = 200;

/// Create a centered popup with fixed dimensions
pub fn popup_area_fixed(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

/// Replace characters that would corrupt a one-line-high cell: newlines,
/// carriage returns, and tabs become visible escapes, other control
/// characters become U+FFFD.
fn sanitize_control_chars(text: &str) -> String {
    if !text.chars().any(|c| c.is_control()) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

/// Render an OwnedValue to a display string, truncating if necessary.
///
/// The single value-to-display function shared by the table and row pages.
/// Newlines and control characters are rendered as visible escapes so they
/// can't break row alignment. (Clipboard copies go through
/// `value_to_string` instead, which is intentionally different: no
/// truncation, no escaping, NULL as empty string.)
pub(crate) fn render_value_for_display(value: &OwnedValue) -> String {
    render_value_for_display_capped(value, None)
}

/// Like [`render_value_for_display`], but aware that text/blob values may
/// have been truncated at `fetch_cap` bytes by the SQL layer: a blob of
/// exactly the cap displays as `[BLOB N+ bytes]` since its true size is
/// unknown (truncated text already carries the `…` indicator).
pub(crate) fn render_value_for_display_capped(
    value: &OwnedValue,
    fetch_cap: Option<usize>,
) -> String {
    match value {
        OwnedValue::Null => "NULL".to_owned(),
        OwnedValue::Integer(i) => i.to_string(),
        OwnedValue::Double(f) => f.to_string(),
        OwnedValue::Text(s) => {
            let text = String::from_utf8_lossy(s);
            // SQL substr() truncation counts characters for text
            let truncated_at_cap = fetch_cap.is_some_and(|cap| text.chars().count() >= cap);
            if text.len() > MAX_CELL_DISPLAY_LEN {
                // Find a valid character boundary at or before MAX_CELL_DISPLAY_LEN
                let mut end = MAX_CELL_DISPLAY_LEN;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", sanitize_control_chars(&text[..end]))
            } else if truncated_at_cap {
                format!("{}…", sanitize_control_chars(&text))
            } else {
                sanitize_control_chars(&text)
            }
        }
        OwnedValue::Blob(b) => {
            if fetch_cap.is_some_and(|cap| b.len() >= cap) {
                format!("[BLOB {}+ bytes]", b.len())
            } else if b.len() > 20 {
                format!("[BLOB {} bytes]", b.len())
            } else {
                "[BLOB]".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_value_truncates_long_text() {
        let long_text = "a".repeat(300);
        let value = OwnedValue::Text(long_text.into_bytes());
        let result = render_value_for_display(&value);
        assert!(result.len() <= MAX_CELL_DISPLAY_LEN + 3); // +3 for "…" (3 bytes)
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_render_value_handles_emoji_at_boundary() {
        // Create text where truncation point falls inside a multi-byte emoji
        // 📍 is 4 bytes, so put it near the 200-byte boundary
        let mut text = "x".repeat(197); // 197 ASCII chars
        text.push('📍'); // 4-byte emoji at position 197-200
        text.push_str("more text after");

        let value = OwnedValue::Text(text.into_bytes());
        let result = render_value_for_display(&value);

        // Should not panic and should truncate before the emoji
        assert!(result.ends_with('…'));
        assert!(!result.contains('📍')); // Emoji should be cut off
    }

    #[test]
    fn test_render_value_handles_text_with_emojis() {
        // Text with emojis that fits within limit
        let text = "Hello 🌺🌴 World";
        let value = OwnedValue::Text(text.as_bytes().to_vec());
        let result = render_value_for_display(&value);
        assert_eq!(result, text);
    }

    #[test]
    fn test_render_value_short_text_unchanged() {
        let text = "short text";
        let value = OwnedValue::Text(text.as_bytes().to_vec());
        let result = render_value_for_display(&value);
        assert_eq!(result, text);
    }

    #[test]
    fn test_render_value_null() {
        let result = render_value_for_display(&OwnedValue::Null);
        assert_eq!(result, "NULL");
    }

    #[test]
    fn test_render_value_integer() {
        let result = render_value_for_display(&OwnedValue::Integer(12345));
        assert_eq!(result, "12345");
    }

    #[test]
    fn test_render_value_blob() {
        let small_blob = OwnedValue::Blob(vec![1, 2, 3]);
        assert_eq!(render_value_for_display(&small_blob), "[BLOB]");

        let large_blob = OwnedValue::Blob(vec![0; 100]);
        assert_eq!(render_value_for_display(&large_blob), "[BLOB 100 bytes]");
    }
}
