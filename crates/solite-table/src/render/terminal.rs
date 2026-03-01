//! Terminal rendering with ANSI colors.

use crate::config::TableConfig;
use crate::format::value::format_cell;
use crate::theme::{BOLD, RESET};
use crate::types::{Alignment, CellValue, ColumnInfo, TableLayout, display_width};

/// Border characters.
mod border {
    pub const TOP_LEFT: char = '┌';
    pub const TOP_RIGHT: char = '┐';
    pub const BOTTOM_LEFT: char = '└';
    pub const BOTTOM_RIGHT: char = '┘';
    pub const HORIZONTAL: char = '─';
    pub const VERTICAL: char = '│';
    pub const TOP_TEE: char = '┬';
    pub const BOTTOM_TEE: char = '┴';
    pub const LEFT_TEE: char = '├';
    pub const RIGHT_TEE: char = '┤';
    pub const CROSS: char = '┼';
}

/// Render table to terminal string with ANSI codes.
pub fn render_terminal(
    columns: &[ColumnInfo],
    head_rows: &[Vec<CellValue>],
    tail_rows: &[Vec<CellValue>],
    layout: &TableLayout,
    config: &TableConfig,
    total_rows: usize,
) -> String {
    let mut output = String::new();

    if columns.is_empty() {
        return output;
    }

    // Render top border
    output.push_str(&render_border_top(layout, columns));
    output.push('\n');

    // Render header row
    output.push_str(&render_header_row(layout, columns, config));
    output.push('\n');

    // Render header separator
    output.push_str(&render_header_separator(layout, columns));
    output.push('\n');

    // Render head rows
    for row in head_rows {
        output.push_str(&render_data_row(row, layout, columns, config));
        output.push('\n');
    }

    // Render ellipsis row if there's truncation
    let skipped = total_rows.saturating_sub(head_rows.len() + tail_rows.len());
    if skipped > 0 {
        output.push_str(&render_ellipsis_row(layout, columns, skipped));
        output.push('\n');
    }

    // Render tail rows
    for row in tail_rows {
        output.push_str(&render_data_row(row, layout, columns, config));
        output.push('\n');
    }

    // Render bottom border
    output.push_str(&render_border_bottom(layout, columns));
    output.push('\n');

    // Render footer
    if config.show_footer {
        output.push_str(&render_footer(layout, total_rows, config));
        output.push('\n');
    }

    output
}

fn get_column_width(layout: &TableLayout, col_idx: usize, columns: &[ColumnInfo]) -> usize {
    let actual_col = layout.visible_columns.get(col_idx).copied().unwrap_or(0);
    let base_width = columns.get(actual_col).map(|c| c.display_width()).unwrap_or(3);
    layout.column_widths.get(col_idx).copied().unwrap_or(base_width)
}

fn render_border_top(layout: &TableLayout, columns: &[ColumnInfo]) -> String {
    let mut line = String::new();
    line.push(border::TOP_LEFT);

    let num_visible = layout.visible_columns.len();
    for (i, _) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        // Check if ellipsis comes before this column
        if layout.ellipsis_position == Some(i) {
            line.push_str(&border::HORIZONTAL.to_string().repeat(3));
            line.push(border::TOP_TEE);
        }

        line.push_str(&border::HORIZONTAL.to_string().repeat(width + 2));

        if i < num_visible - 1 {
            line.push(border::TOP_TEE);
        }
    }

    // Check if ellipsis comes at the end
    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::TOP_TEE);
        line.push_str(&border::HORIZONTAL.to_string().repeat(3));
    }

    line.push(border::TOP_RIGHT);
    line
}

fn render_border_bottom(layout: &TableLayout, columns: &[ColumnInfo]) -> String {
    let mut line = String::new();
    line.push(border::BOTTOM_LEFT);

    let num_visible = layout.visible_columns.len();
    for (i, _) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        if layout.ellipsis_position == Some(i) {
            line.push_str(&border::HORIZONTAL.to_string().repeat(3));
            line.push(border::BOTTOM_TEE);
        }

        line.push_str(&border::HORIZONTAL.to_string().repeat(width + 2));

        if i < num_visible - 1 {
            line.push(border::BOTTOM_TEE);
        }
    }

    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::BOTTOM_TEE);
        line.push_str(&border::HORIZONTAL.to_string().repeat(3));
    }

    line.push(border::BOTTOM_RIGHT);
    line
}

fn render_header_separator(layout: &TableLayout, columns: &[ColumnInfo]) -> String {
    let mut line = String::new();
    line.push(border::LEFT_TEE);

    let num_visible = layout.visible_columns.len();
    for (i, _) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        if layout.ellipsis_position == Some(i) {
            line.push_str(&border::HORIZONTAL.to_string().repeat(3));
            line.push(border::CROSS);
        }

        line.push_str(&border::HORIZONTAL.to_string().repeat(width + 2));

        if i < num_visible - 1 {
            line.push(border::CROSS);
        }
    }

    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::CROSS);
        line.push_str(&border::HORIZONTAL.to_string().repeat(3));
    }

    line.push(border::RIGHT_TEE);
    line
}

fn render_header_row(layout: &TableLayout, columns: &[ColumnInfo], config: &TableConfig) -> String {
    let mut line = String::new();
    line.push(border::VERTICAL);

    let num_visible = layout.visible_columns.len();
    for (i, &col_idx) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        if layout.ellipsis_position == Some(i) {
            line.push_str(" … ");
            line.push(border::VERTICAL);
        }

        let name = &columns[col_idx].name;
        let formatted = if config.theme.is_some() {
            format!("{}{}{}", BOLD, name, RESET)
        } else {
            name.clone()
        };
        let padded = pad_cell(&formatted, name, width, Alignment::Center);
        line.push(' ');
        line.push_str(&padded);
        line.push(' ');

        if i < num_visible - 1 {
            line.push(border::VERTICAL);
        }
    }

    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::VERTICAL);
        line.push_str(" … ");
    }

    line.push(border::VERTICAL);
    line
}

fn render_data_row(
    row: &[CellValue],
    layout: &TableLayout,
    columns: &[ColumnInfo],
    config: &TableConfig,
) -> String {
    let mut line = String::new();
    line.push(border::VERTICAL);

    let num_visible = layout.visible_columns.len();
    for (i, &col_idx) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        if layout.ellipsis_position == Some(i) {
            line.push_str(" · ");
            line.push(border::VERTICAL);
        }

        let cell = row.get(col_idx).cloned().unwrap_or_else(|| {
            CellValue::new(String::new(), crate::types::ValueType::Null, Alignment::Left)
        });

        // Truncate cell content to fit column width
        let formatted = format_cell(&cell, config.theme.as_ref(), width);
        let display_for_pad = truncate_display(&cell.display, width);
        let padded = pad_cell(&formatted, &display_for_pad, width, cell.alignment);

        line.push(' ');
        line.push_str(&padded);
        line.push(' ');

        if i < num_visible - 1 {
            line.push(border::VERTICAL);
        }
    }

    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::VERTICAL);
        line.push_str(" · ");
    }

    line.push(border::VERTICAL);
    line
}

fn render_ellipsis_row(layout: &TableLayout, columns: &[ColumnInfo], _skipped: usize) -> String {
    let mut line = String::new();
    line.push(border::VERTICAL);

    let num_visible = layout.visible_columns.len();
    for (i, _) in layout.visible_columns.iter().enumerate() {
        let width = get_column_width(layout, i, columns);

        if layout.ellipsis_position == Some(i) {
            line.push_str(" · ");
            line.push(border::VERTICAL);
        }

        // Just show "·" centered - the footer has the count
        let content = "·";
        let padded = pad_cell(content, content, width, Alignment::Center);
        line.push(' ');
        line.push_str(&padded);
        line.push(' ');

        if i < num_visible - 1 {
            line.push(border::VERTICAL);
        }
    }

    if layout.ellipsis_position == Some(num_visible) {
        line.push(border::VERTICAL);
        line.push_str(" · ");
    }

    line.push(border::VERTICAL);
    line
}

fn render_footer(layout: &TableLayout, total_rows: usize, config: &TableConfig) -> String {
    let shown_rows = (config.head_rows + config.tail_rows).min(total_rows);
    let shown_cols = layout.shown_columns();

    let col_text = if shown_cols == layout.total_columns {
        format!("{} column{}", layout.total_columns, if layout.total_columns != 1 { "s" } else { "" })
    } else {
        format!("{} columns ({} shown)", layout.total_columns, shown_cols)
    };

    let row_text = if shown_rows == total_rows {
        format!("{} row{}", total_rows, if total_rows != 1 { "s" } else { "" })
    } else {
        format!("{} rows ({} shown)", total_rows, shown_rows)
    };

    format!("{} × {}", col_text, row_text)
}

/// Truncate a display string to fit within max_width, adding ellipsis if needed.
fn truncate_display(s: &str, max_width: usize) -> String {
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

/// Pad a cell to the given width with proper alignment.
/// `formatted` may contain ANSI codes, `raw` is the plain text for width calculation.
fn pad_cell(formatted: &str, raw: &str, width: usize, alignment: Alignment) -> String {
    let content_width = display_width(raw);
    if content_width >= width {
        return formatted.to_string();
    }

    let padding = width - content_width;

    match alignment {
        Alignment::Left => {
            format!("{}{}", formatted, " ".repeat(padding))
        }
        Alignment::Right => {
            format!("{}{}", " ".repeat(padding), formatted)
        }
        Alignment::Center => {
            let left_pad = padding / 2;
            let right_pad = padding - left_pad;
            format!("{}{}{}", " ".repeat(left_pad), formatted, " ".repeat(right_pad))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ValueType;

    #[test]
    fn test_pad_cell_left() {
        assert_eq!(pad_cell("hi", "hi", 5, Alignment::Left), "hi   ");
    }

    #[test]
    fn test_pad_cell_right() {
        assert_eq!(pad_cell("hi", "hi", 5, Alignment::Right), "   hi");
    }

    #[test]
    fn test_pad_cell_center() {
        assert_eq!(pad_cell("hi", "hi", 6, Alignment::Center), "  hi  ");
        assert_eq!(pad_cell("hi", "hi", 5, Alignment::Center), " hi  ");
    }

    #[test]
    fn test_render_simple_table() {
        let columns = vec![
            ColumnInfo::new("name".to_string()),
            ColumnInfo::new("age".to_string()),
        ];

        let rows = vec![vec![
            CellValue::new("Alice".to_string(), ValueType::Text, Alignment::Left),
            CellValue::new("30".to_string(), ValueType::Integer, Alignment::Right),
        ]];

        let layout = TableLayout::all_visible(vec![5, 3]);
        let config = TableConfig::plain();

        let output = render_terminal(&columns, &rows, &[], &layout, &config, 1);

        assert!(output.contains("name"));
        assert!(output.contains("age"));
        assert!(output.contains("Alice"));
        assert!(output.contains("30"));
        assert!(output.contains("┌"));
        assert!(output.contains("└"));
    }

    fn render_plain(
        columns: Vec<ColumnInfo>,
        rows: Vec<Vec<CellValue>>,
        col_widths: Vec<usize>,
    ) -> String {
        let total_rows = rows.len();
        let layout = TableLayout::all_visible(col_widths);
        let config = TableConfig::plain();
        render_terminal(&columns, &rows, &[], &layout, &config, total_rows)
    }

    #[test]
    fn test_render_newlines_in_values() {
        let mut col = ColumnInfo::new("value".to_string());
        let cell =
            CellValue::new("line 1\nline 2\nline 3".to_string(), ValueType::Text, Alignment::Left);
        col.observe_width(cell.width);

        let w = col.display_width();
        let output = render_plain(vec![col], vec![vec![cell]], vec![w]);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_render_newlines_in_header() {
        let mut col = ColumnInfo::new("col\nwith\nnewlines".to_string());
        let cell = CellValue::new("data".to_string(), ValueType::Text, Alignment::Left);
        col.observe_width(cell.width);

        let w = col.display_width();
        let output = render_plain(vec![col], vec![vec![cell]], vec![w]);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_render_tabs_in_values() {
        let mut col = ColumnInfo::new("value".to_string());
        let cell = CellValue::new("col1\tcol2".to_string(), ValueType::Text, Alignment::Left);
        col.observe_width(cell.width);

        let w = col.display_width();
        let output = render_plain(vec![col], vec![vec![cell]], vec![w]);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_render_crlf_in_values() {
        let mut col = ColumnInfo::new("value".to_string());
        let cell =
            CellValue::new("line 1\r\nline 2".to_string(), ValueType::Text, Alignment::Left);
        col.observe_width(cell.width);

        let w = col.display_width();
        let output = render_plain(vec![col], vec![vec![cell]], vec![w]);
        insta::assert_snapshot!(output);
    }
}
