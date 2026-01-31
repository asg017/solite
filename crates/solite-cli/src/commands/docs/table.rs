//! Table rendering for documentation output.

use cli_table::{Cell, CellStruct, Table};
use termcolor::ColorChoice;

use crate::commands::snapshot::ValueCopy;
use crate::ui::{BORDER, SEPARATOR};

use super::value::display_value;

/// Render a result set as an ASCII table.
///
/// # Arguments
///
/// * `columns` - Column headers
/// * `results` - Rows of values
///
/// # Returns
///
/// A formatted table string, or an error message if rendering fails.
pub fn render_table(columns: &[String], results: &[Vec<ValueCopy>]) -> String {
    let rows: Vec<Vec<CellStruct>> = results
        .iter()
        .map(|row| row.iter().map(|v| display_value(v).cell()).collect())
        .collect();

    match rows
        .table()
        .title(columns)
        .border(*BORDER)
        .separator(*SEPARATOR)
        .color_choice(ColorChoice::Never)
        .display()
    {
        Ok(display) => display.to_string(),
        Err(e) => format!("[Error rendering table: {}]", e),
    }
}
