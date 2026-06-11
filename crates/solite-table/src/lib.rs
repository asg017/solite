//! Table rendering for SQLite results.
//!
//! This crate provides table rendering with:
//! - Column collapsing when terminal is narrow (shows first N cols, "…", last M cols)
//! - Row truncation for large results (first 20 rows, "·" ellipsis, last 20 rows)
//! - Memory-efficient streaming (doesn't load all rows into memory)
//! - Multiple output modes (terminal, string, HTML for Jupyter)
//! - Preserved styling (colors, JSON highlighting, borders)
//!
//! # Example
//!
//! ```ignore
//! use solite_table::{render_statement, TableConfig};
//!
//! let config = TableConfig::terminal();
//! let result = render_statement(&mut stmt, &config)?;
//! println!("{}", result.output);
//! ```

pub mod buffer;
pub mod config;
pub mod format;
pub mod layout;
pub mod render;
pub mod theme;
pub mod types;

pub use config::{OutputMode, TableConfig};
pub use theme::Theme;
pub use types::{Alignment, CellValue, ColumnInfo, TableLayout, ValueType};

use buffer::RowBuffer;
use layout::compute_layout;
use render::{render_html, render_string, render_string_plain, render_terminal};
use solite_core::sqlite::{SQLiteError, Statement};

/// Result of rendering a table.
#[derive(Debug)]
pub struct RenderResult {
    /// The rendered output string (may contain ANSI codes or HTML).
    pub output: String,
    /// Total number of rows in the result.
    pub total_rows: usize,
    /// Number of rows shown (may be less than total if truncated).
    pub shown_rows: usize,
    /// Total number of columns.
    pub total_columns: usize,
    /// Number of columns shown (may be less than total if collapsed).
    pub shown_columns: usize,
}

/// Statement results buffered for rendering: column metadata plus the
/// retained head/tail rows. Produced by [`buffer_statement`]; render with
/// [`render_buffered`] — possibly several times in different output modes,
/// without re-executing the statement.
pub struct BufferedStatement {
    columns: Vec<ColumnInfo>,
    head: Vec<Vec<CellValue>>,
    tail: Vec<Vec<CellValue>>,
    total_rows: usize,
}

/// Stream through a statement's results once, retaining the first
/// `head_rows` and last `tail_rows` rows.
///
/// Returns `None` for column-less statements (e.g. CREATE TABLE), which are
/// still stepped to completion so they execute.
pub fn buffer_statement(
    stmt: &mut Statement,
    head_rows: usize,
    tail_rows: usize,
) -> Result<Option<BufferedStatement>, SQLiteError> {
    let column_names = stmt.column_names().map_err(|_| SQLiteError {
        result_code: 1,
        code_description: "SQLITE_ERROR".to_string(),
        message: "Failed to get column names".to_string(),
        offset: None,
    })?;

    if column_names.is_empty() {
        // Still need to step through the statement to execute it (e.g., CREATE TABLE)
        while stmt.next()?.is_some() {}
        return Ok(None);
    }

    // Initialize column info
    let mut columns: Vec<ColumnInfo> = column_names
        .into_iter()
        .map(ColumnInfo::new)
        .collect();

    // Create row buffer
    let mut buffer = RowBuffer::new(head_rows, tail_rows);

    // Stream through results
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                // Convert to CellValues and track column widths
                let cells: Vec<CellValue> = row
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let cell = CellValue::from_sqlite_value(v);
                        if let Some(col) = columns.get_mut(i) {
                            col.observe_width(cell.width);
                        }
                        cell
                    })
                    .collect();

                buffer.push(cells);
            }
            Ok(None) => break,
            Err(e) => return Err(e),
        }
    }

    let total_rows = buffer.total_count();
    let (head, tail) = buffer.into_parts();

    Ok(Some(BufferedStatement {
        columns,
        head,
        tail,
        total_rows,
    }))
}

/// Render previously buffered statement results with the given config.
///
/// The config's `head_rows`/`tail_rows` only affect the reported
/// `shown_rows`; which rows were retained was decided by
/// [`buffer_statement`].
pub fn render_buffered(buffered: &BufferedStatement, config: &TableConfig) -> RenderResult {
    let BufferedStatement {
        columns,
        head,
        tail,
        total_rows,
    } = buffered;
    let total_rows = *total_rows;
    let total_columns = columns.len();

    // Compute layout
    let max_width = config.effective_width();
    let layout = compute_layout(columns, max_width, config.max_cell_width);

    // Render based on output mode
    let output = match config.output_mode {
        OutputMode::Terminal => render_terminal(columns, head, tail, &layout, config, total_rows),
        OutputMode::StringAnsi => render_string(columns, head, tail, &layout, config, total_rows),
        OutputMode::StringPlain => {
            render_string_plain(columns, head, tail, &layout, config, total_rows)
        }
        OutputMode::Html => render_html(columns, head, tail, &layout, config, total_rows),
    };

    let shown_rows = (config.head_rows + config.tail_rows).min(total_rows);

    RenderResult {
        output,
        total_rows,
        shown_rows,
        total_columns,
        shown_columns: layout.shown_columns(),
    }
}

/// Render a SQLite statement result to a string.
///
/// This is the main entry point for table rendering. It streams through
/// the statement results, collecting rows into a buffer that retains
/// head and tail rows, then renders the table.
pub fn render_statement(stmt: &mut Statement, config: &TableConfig) -> Result<RenderResult, SQLiteError> {
    match buffer_statement(stmt, config.head_rows, config.tail_rows)? {
        Some(buffered) => Ok(render_buffered(&buffered, config)),
        None => Ok(RenderResult {
            output: String::new(),
            total_rows: 0,
            shown_rows: 0,
            total_columns: 0,
            shown_columns: 0,
        }),
    }
}

/// Print a SQLite statement result to stdout.
///
/// Convenience function that renders and prints in one call.
pub fn print_statement(stmt: &mut Statement, config: &TableConfig) -> Result<RenderResult, SQLiteError> {
    let result = render_statement(stmt, config)?;
    print!("{}", result.output);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_info_width() {
        let mut col = ColumnInfo::new("name".to_string());
        assert_eq!(col.header_width, 4);
        assert_eq!(col.display_width(), 4);

        col.observe_width(10);
        assert_eq!(col.display_width(), 10);

        col.observe_width(5);
        assert_eq!(col.display_width(), 10); // max is preserved
    }

    #[test]
    fn test_table_config_defaults() {
        let config = TableConfig::default();
        assert_eq!(config.head_rows, 20);
        assert_eq!(config.tail_rows, 20);
        assert!(config.theme.is_some());
        assert!(config.show_footer);
    }

    #[test]
    fn test_display_width() {
        use crate::types::display_width;
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
    }
}
