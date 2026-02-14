//! Table rendering for documentation output.

use crate::commands::test::snap::ValueCopy;

use super::value::display_value;

/// Border characters for the table.
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

/// Render a result set as an ASCII table.
///
/// # Arguments
///
/// * `columns` - Column headers
/// * `results` - Rows of values
///
/// # Returns
///
/// A formatted table string.
pub fn render_table(columns: &[String], results: &[Vec<ValueCopy>]) -> String {
    if columns.is_empty() {
        return String::new();
    }

    // Convert values to strings
    let str_results: Vec<Vec<String>> = results
        .iter()
        .map(|row| row.iter().map(display_value).collect())
        .collect();

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in &str_results {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() && cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    let mut output = String::new();

    // Top border
    output.push(border::TOP_LEFT);
    for (i, &w) in widths.iter().enumerate() {
        output.push_str(&border::HORIZONTAL.to_string().repeat(w + 2));
        if i < widths.len() - 1 {
            output.push(border::TOP_TEE);
        }
    }
    output.push(border::TOP_RIGHT);
    output.push('\n');

    // Header row
    output.push(border::VERTICAL);
    for (i, col) in columns.iter().enumerate() {
        output.push(' ');
        output.push_str(col);
        output.push_str(&" ".repeat(widths[i] - col.len()));
        output.push(' ');
        if i < columns.len() - 1 {
            output.push(border::VERTICAL);
        }
    }
    output.push(border::VERTICAL);
    output.push('\n');

    // Header separator
    output.push(border::LEFT_TEE);
    for (i, &w) in widths.iter().enumerate() {
        output.push_str(&border::HORIZONTAL.to_string().repeat(w + 2));
        if i < widths.len() - 1 {
            output.push(border::CROSS);
        }
    }
    output.push(border::RIGHT_TEE);
    output.push('\n');

    // Data rows
    for row in &str_results {
        output.push(border::VERTICAL);
        for (i, cell) in row.iter().enumerate() {
            output.push(' ');
            output.push_str(cell);
            if i < widths.len() {
                output.push_str(&" ".repeat(widths[i] - cell.len()));
            }
            output.push(' ');
            if i < row.len() - 1 {
                output.push(border::VERTICAL);
            }
        }
        output.push(border::VERTICAL);
        output.push('\n');
    }

    // Bottom border
    output.push(border::BOTTOM_LEFT);
    for (i, &w) in widths.iter().enumerate() {
        output.push_str(&border::HORIZONTAL.to_string().repeat(w + 2));
        if i < widths.len() - 1 {
            output.push(border::BOTTOM_TEE);
        }
    }
    output.push(border::BOTTOM_RIGHT);
    output.push('\n');

    output
}
