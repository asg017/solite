//! Core types for table rendering.

use solite_core::sqlite::{ValueRefX, ValueRefXValue, JSON_SUBTYPE};

/// Alignment of cell content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
}

/// Type of value in a cell, used for styling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Null,
    Integer,
    Double,
    Text,
    Blob,
    Json,
}

/// A cell value with its display string and metadata.
#[derive(Debug, Clone)]
pub struct CellValue {
    /// The display string (may contain ANSI codes if pre-formatted).
    pub display: String,
    /// The raw display width (excluding ANSI codes).
    pub width: usize,
    /// The type of value for styling.
    pub value_type: ValueType,
    /// Alignment for this cell.
    pub alignment: Alignment,
}

impl CellValue {
    pub fn new(display: String, value_type: ValueType, alignment: Alignment) -> Self {
        let width = display_width(&display);
        Self {
            display,
            width,
            value_type,
            alignment,
        }
    }

    pub fn from_sqlite_value(value: &ValueRefX) -> Self {
        match &value.value {
            ValueRefXValue::Null => Self::new(String::new(), ValueType::Null, Alignment::Center),
            ValueRefXValue::Int(v) => {
                Self::new(v.to_string(), ValueType::Integer, Alignment::Right)
            }
            ValueRefXValue::Double(v) => {
                Self::new(v.to_string(), ValueType::Double, Alignment::Right)
            }
            ValueRefXValue::Text(bytes) => {
                let text = String::from_utf8_lossy(bytes).into_owned();
                if value.subtype() == Some(JSON_SUBTYPE) {
                    Self::new(text, ValueType::Json, Alignment::Left)
                } else {
                    Self::new(text, ValueType::Text, Alignment::Left)
                }
            }
            ValueRefXValue::Blob(bytes) => Self::new(
                format!("Blob<{}>", bytes.len()),
                ValueType::Blob,
                Alignment::Center,
            ),
        }
    }
}

/// Information about a column for layout calculations.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name/header.
    pub name: String,
    /// Width of the header.
    pub header_width: usize,
    /// Maximum content width seen so far.
    pub max_content_width: usize,
}

impl ColumnInfo {
    pub fn new(name: String) -> Self {
        let header_width = display_width(&name);
        Self {
            name,
            header_width,
            max_content_width: 0,
        }
    }

    /// Update max content width with a new cell value.
    pub fn observe_width(&mut self, width: usize) {
        if width > self.max_content_width {
            self.max_content_width = width;
        }
    }

    /// Get the display width for this column (max of header and content).
    pub fn display_width(&self) -> usize {
        self.header_width.max(self.max_content_width)
    }
}

/// Layout information for rendering.
#[derive(Debug, Clone)]
pub struct TableLayout {
    /// Indices of columns to show (in order).
    pub visible_columns: Vec<usize>,
    /// Position to insert ellipsis column (None if all columns shown).
    pub ellipsis_position: Option<usize>,
    /// Calculated widths for each visible column.
    pub column_widths: Vec<usize>,
    /// Total number of columns.
    pub total_columns: usize,
}

impl TableLayout {
    pub fn all_visible(column_widths: Vec<usize>) -> Self {
        let n = column_widths.len();
        Self {
            visible_columns: (0..n).collect(),
            ellipsis_position: None,
            column_widths,
            total_columns: n,
        }
    }

    pub fn shown_columns(&self) -> usize {
        self.visible_columns.len()
    }
}

/// Calculate display width of a string, handling Unicode properly.
/// This strips ANSI escape codes and uses unicode-width for proper character widths.
pub fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;

    // Strip ANSI escape codes
    let stripped = strip_ansi_codes(s);
    stripped.width()
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (end of sequence)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("12345"), 5);
    }

    #[test]
    fn test_display_width_unicode() {
        // CJK characters are typically double-width
        assert_eq!(display_width("中文"), 4);
        // Emoji
        assert_eq!(display_width("👍"), 2);
    }

    #[test]
    fn test_display_width_ansi() {
        // ANSI codes should not count towards width
        assert_eq!(display_width("\x1b[31mred\x1b[0m"), 3);
        assert_eq!(display_width("\x1b[1;32mbold green\x1b[0m"), 10);
    }

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi_codes("no codes"), "no codes");
        assert_eq!(strip_ansi_codes("\x1b[1;32;44mcomplex\x1b[0m"), "complex");
    }
}
