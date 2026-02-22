//! String rendering (with or without ANSI codes).

use crate::config::TableConfig;
use crate::render::terminal::render_terminal;
use crate::types::{CellValue, ColumnInfo, TableLayout};

/// Render table to string with ANSI codes.
pub fn render_string(
    columns: &[ColumnInfo],
    head_rows: &[Vec<CellValue>],
    tail_rows: &[Vec<CellValue>],
    layout: &TableLayout,
    config: &TableConfig,
    total_rows: usize,
) -> String {
    render_terminal(columns, head_rows, tail_rows, layout, config, total_rows)
}

/// Render table to plain string without ANSI codes.
pub fn render_string_plain(
    columns: &[ColumnInfo],
    head_rows: &[Vec<CellValue>],
    tail_rows: &[Vec<CellValue>],
    layout: &TableLayout,
    config: &TableConfig,
    total_rows: usize,
) -> String {
    // Use a config with no theme to avoid ANSI codes
    let plain_config = TableConfig {
        theme: None,
        ..config.clone()
    };
    render_terminal(columns, head_rows, tail_rows, layout, &plain_config, total_rows)
}
