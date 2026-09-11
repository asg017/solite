//! Value formatting for cells.

use crate::format::html_escape;
use crate::format::json::format_json;
use crate::types::{CellValue, ValueType};
use solite_theme::Theme;

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
            // Show nothing for nulls (display is already empty). `theme.null`
            // is intentionally unused here — it only drives the JSON/HTML
            // paths, where a `null` literal inside a JSON value needs a color.
            String::new()
        }
        ValueType::Integer => theme.integer.paint(display),
        ValueType::Double => theme.double.paint(display),
        ValueType::Text => theme.text.paint(display),
        ValueType::Blob => theme.blob.paint(display),
        ValueType::Json => format_json(display, theme),
    }
}

/// Format a cell for HTML output.
pub fn format_cell_html(
    cell: &CellValue,
    theme: Option<&Theme>,
    max_width: usize,
    json_interactive: bool,
) -> String {
    // For interactive JSON, skip truncation and use the full value
    if json_interactive && cell.value_type == ValueType::Json {
        if let Some(theme) = theme {
            return crate::format::json::format_json_interactive_html(&cell.display, theme);
        }
    }

    let display = truncate_to_width(&cell.display, max_width);
    let escaped = html_escape(&display);

    match theme {
        Some(theme) => format_cell_html_with_theme(&escaped, &display, cell.value_type, theme),
        None => escaped,
    }
}

/// Cell foreground colors reference a `--solite-<role>` CSS custom property
/// (declared once on the table wrapper by `theme_css_vars`) rather than a
/// resolved color baked into every cell — see `format/theme_vars.rs`. The
/// `currentColor` fallback keeps cells legible even when a wrapper omits the
/// variable (e.g. an older cached render, or `theme: None`).
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
                "<span style=\"color: var(--solite-integer, currentColor); font-family: monospace;\">{}</span>",
                escaped
            )
        }
        ValueType::Double => {
            format!(
                "<span style=\"color: var(--solite-double, currentColor); font-family: monospace;\">{}</span>",
                escaped
            )
        }
        // Colored to match the ANSI path (`format_cell_with_theme` above),
        // which has always colored text values.
        ValueType::Text => {
            format!(
                "<span style=\"color: var(--solite-text, currentColor);\">{}</span>",
                escaped
            )
        }
        ValueType::Blob => {
            format!(
                "<span style=\"color: var(--solite-blob, currentColor);\">{}</span>",
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

    #[test]
    fn test_format_cell_catppuccin_truecolor_escape() {
        // Byte-identical to the old private theme's `Color::to_ansi_fg()` output.
        let theme = Theme::catppuccin_mocha();
        let cell = CellValue::new("42".to_string(), ValueType::Integer, Alignment::Right);
        let formatted = format_cell(&cell, Some(&theme), 100);

        assert_eq!(formatted, "\x1b[38;2;250;179;135m42\x1b[0m");
    }

    #[test]
    fn test_format_cell_terminal_theme_named_ansi() {
        let theme = Theme::terminal();
        let cell = CellValue::new("42".to_string(), ValueType::Integer, Alignment::Right);
        let formatted = format_cell(&cell, Some(&theme), 100);

        // Integer is ANSI yellow (SGR 33) in the terminal theme, not truecolor.
        assert_eq!(formatted, "\x1b[33m42\x1b[0m");
    }

    #[test]
    fn test_format_cell_terminal_theme_default_color_is_inert() {
        // Text is `ColorValue::Default` in the terminal theme: no escapes at all.
        let theme = Theme::terminal();
        let cell = CellValue::new("hello".to_string(), ValueType::Text, Alignment::Left);
        let formatted = format_cell(&cell, Some(&theme), 100);

        assert_eq!(formatted, "hello");
    }
}
