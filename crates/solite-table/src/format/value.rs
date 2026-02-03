//! Value formatting for cells.

use crate::format::json::format_json;
use crate::theme::{Theme, RESET};
use crate::types::{CellValue, ValueType};

/// Format a cell value with ANSI colors.
pub fn format_cell(cell: &CellValue, theme: Option<&Theme>, max_width: usize) -> String {
    let display = truncate_to_width(&cell.display, max_width);

    match theme {
        Some(theme) => format_cell_with_theme(&display, cell.value_type, theme),
        None => display,
    }
}

/// Format a cell with theme colors.
fn format_cell_with_theme(display: &str, value_type: ValueType, theme: &Theme) -> String {
    match value_type {
        ValueType::Null => {
            // Show nothing for nulls (display is already empty)
            String::new()
        }
        ValueType::Integer => {
            format!("{}{}{}", theme.integer.to_ansi_fg(), display, RESET)
        }
        ValueType::Double => {
            format!("{}{}{}", theme.double.to_ansi_fg(), display, RESET)
        }
        ValueType::Text => {
            format!("{}{}{}", theme.text.to_ansi_fg(), display, RESET)
        }
        ValueType::Blob => {
            format!("{}{}{}", theme.blob.to_ansi_fg(), display, RESET)
        }
        ValueType::Json => format_json(display, theme),
    }
}

/// Format a cell for HTML output.
pub fn format_cell_html(cell: &CellValue, theme: Option<&Theme>, max_width: usize) -> String {
    let display = truncate_to_width(&cell.display, max_width);
    let escaped = html_escape(&display);

    match theme {
        Some(theme) => format_cell_html_with_theme(&escaped, &display, cell.value_type, theme),
        None => escaped,
    }
}

fn format_cell_html_with_theme(
    escaped: &str,
    raw: &str,
    value_type: ValueType,
    theme: &Theme,
) -> String {
    match value_type {
        ValueType::Null => String::new(),
        ValueType::Integer => {
            format!(
                "<span style=\"color: {}; font-family: monospace;\">{}</span>",
                theme.integer.to_hex_string(),
                escaped
            )
        }
        ValueType::Double => {
            format!(
                "<span style=\"color: {}; font-family: monospace;\">{}</span>",
                theme.double.to_hex_string(),
                escaped
            )
        }
        ValueType::Text => escaped.to_string(),
        ValueType::Blob => {
            format!(
                "<span style=\"color: {};\">{}</span>",
                theme.blob.to_hex_string(),
                escaped
            )
        }
        ValueType::Json => crate::format::json::format_json_html(raw, theme),
    }
}

/// Truncate a string to fit within max_width display columns.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;

    if s.width() <= max_width {
        return s.to_string();
    }

    if max_width < 2 {
        return "…".to_string();
    }

    let target_width = max_width - 1; // Leave room for ellipsis
    let mut result = String::new();
    let mut current_width = 0;

    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + char_width > target_width {
            break;
        }
        result.push(c);
        current_width += char_width;
    }

    result.push('…');
    result
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Alignment;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate_to_width("hello world", 8), "hello w…");
    }

    #[test]
    fn test_truncate_unicode() {
        // Chinese characters are double-width
        let s = "你好世界"; // 8 display columns
        let truncated = truncate_to_width(s, 5);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn test_format_cell_no_theme() {
        let cell = CellValue::new("test".to_string(), ValueType::Text, Alignment::Left);
        assert_eq!(format_cell(&cell, None, 100), "test");
    }

    #[test]
    fn test_format_cell_with_theme() {
        let theme = Theme::catppuccin_mocha();
        let cell = CellValue::new("42".to_string(), ValueType::Integer, Alignment::Right);
        let formatted = format_cell(&cell, Some(&theme), 100);

        assert!(formatted.contains("42"));
        assert!(formatted.contains("\x1b[")); // Contains ANSI codes
    }
}
