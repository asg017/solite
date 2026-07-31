//! Table rendering for documentation output.

use crate::commands::test::snap::ValueCopy;
use solite_table::types::display_width;

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

    // Calculate column widths (display width, not byte length, so
    // multibyte text doesn't break the borders)
    let mut widths: Vec<usize> = columns.iter().map(|c| display_width(c)).collect();
    for row in &str_results {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() && display_width(cell) > widths[i] {
                widths[i] = display_width(cell);
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
        output.push_str(&" ".repeat(widths[i] - display_width(col)));
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
                output.push_str(&" ".repeat(widths[i] - display_width(cell)));
            }
            output.push(' ');
            if i < widths.len() - 1 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test::snap::ValueCopyValue;

    fn text(s: &str) -> ValueCopy {
        ValueCopy::new(ValueCopyValue::Text(s.as_bytes().to_vec()), None)
    }

    #[test]
    fn test_multibyte_alignment() {
        let columns = vec!["a".to_string()];
        let results = vec![vec![text("héllo wörld")], vec![text("plain ascii x")]];
        let out = render_table(&columns, &results);
        let widths: Vec<usize> = out.lines().map(display_width).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "all lines must have equal display width:\n{out}"
        );
    }

    #[test]
    fn test_empty_columns() {
        assert_eq!(render_table(&[], &[]), "");
    }

    #[test]
    fn test_basic_table() {
        let columns = vec!["name".to_string(), "n".to_string()];
        let results = vec![
            vec![text("alex"), ValueCopy::new(ValueCopyValue::Int(1), None)],
            vec![text("brian"), ValueCopy::new(ValueCopyValue::Int(2), None)],
        ];
        let out = render_table(&columns, &results);
        assert_eq!(
            out,
            "┌─────────┬───┐\n\
             │ name    │ n │\n\
             ├─────────┼───┤\n\
             │ 'alex'  │ 1 │\n\
             │ 'brian' │ 2 │\n\
             └─────────┴───┘\n"
        );
    }
}
