//! HTML rendering for Jupyter notebooks.

use crate::config::TableConfig;
use crate::format::value::format_cell_html;
use crate::types::{CellValue, ColumnInfo, TableLayout};

/// CSS for table styling.
const TABLE_CSS: &str = r#"
.solite-table {
    border-collapse: collapse;
    font-family: monospace;
    font-size: 14px;
}
.solite-table th, .solite-table td {
    border: 1px solid #6c7086;
    padding: 4px 8px;
    text-align: left;
}
.solite-table th {
    background-color: #313244;
    color: #cdd6f4;
    font-weight: bold;
}
.solite-table tr:nth-child(even) {
    background-color: #1e1e2e;
}
.solite-table tr:nth-child(odd) {
    background-color: #181825;
}
.solite-table .ellipsis-row {
    background-color: #45475a;
    text-align: center;
    color: #a6adc8;
    font-style: italic;
}
.solite-table .ellipsis-col {
    background-color: #45475a;
    text-align: center;
    color: #a6adc8;
}
.solite-footer {
    color: #a6adc8;
    font-size: 12px;
    margin-top: 4px;
}
"#;

/// Render table to HTML.
pub fn render_html(
    columns: &[ColumnInfo],
    head_rows: &[Vec<CellValue>],
    tail_rows: &[Vec<CellValue>],
    layout: &TableLayout,
    config: &TableConfig,
    total_rows: usize,
) -> String {
    let mut html = String::new();

    // Style tag
    html.push_str("<style>");
    html.push_str(TABLE_CSS);
    html.push_str("</style>\n");

    // Table
    html.push_str("<table class=\"solite-table\">\n");

    // Header
    html.push_str("<thead><tr>");
    for (i, &col_idx) in layout.visible_columns.iter().enumerate() {
        if layout.ellipsis_position == Some(i) {
            html.push_str("<th class=\"ellipsis-col\">…</th>");
        }
        html.push_str("<th>");
        html.push_str(&html_escape(&columns[col_idx].name));
        html.push_str("</th>");
    }
    if layout.ellipsis_position == Some(layout.visible_columns.len()) {
        html.push_str("<th class=\"ellipsis-col\">…</th>");
    }
    html.push_str("</tr></thead>\n");

    // Body
    html.push_str("<tbody>\n");

    // Head rows
    for row in head_rows {
        html.push_str(&render_html_row(row, layout, columns, config));
    }

    // Ellipsis row if truncated
    let skipped = total_rows.saturating_sub(head_rows.len() + tail_rows.len());
    if skipped > 0 {
        let colspan = layout.visible_columns.len() + if layout.ellipsis_position.is_some() { 1 } else { 0 };
        html.push_str(&format!(
            "<tr class=\"ellipsis-row\"><td colspan=\"{}\">· {} rows ·</td></tr>\n",
            colspan, skipped
        ));
    }

    // Tail rows
    for row in tail_rows {
        html.push_str(&render_html_row(row, layout, columns, config));
    }

    html.push_str("</tbody>\n");
    html.push_str("</table>\n");

    // Footer
    if config.show_footer {
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

        html.push_str(&format!("<div class=\"solite-footer\">{} × {}</div>\n", col_text, row_text));
    }

    html
}

fn render_html_row(
    row: &[CellValue],
    layout: &TableLayout,
    _columns: &[ColumnInfo],
    config: &TableConfig,
) -> String {
    let mut html = String::from("<tr>");

    for (i, &col_idx) in layout.visible_columns.iter().enumerate() {
        if layout.ellipsis_position == Some(i) {
            html.push_str("<td class=\"ellipsis-col\">·</td>");
        }

        let cell = row.get(col_idx).cloned().unwrap_or_else(|| {
            CellValue::new(String::new(), crate::types::ValueType::Null, crate::types::Alignment::Left)
        });

        let formatted = format_cell_html(&cell, config.theme.as_ref(), config.max_cell_width);

        // Add alignment style
        let align_style = match cell.alignment {
            crate::types::Alignment::Left => "text-align: left;",
            crate::types::Alignment::Right => "text-align: right;",
            crate::types::Alignment::Center => "text-align: center;",
        };

        html.push_str(&format!("<td style=\"{}\">{}</td>", align_style, formatted));
    }

    if layout.ellipsis_position == Some(layout.visible_columns.len()) {
        html.push_str("<td class=\"ellipsis-col\">·</td>");
    }

    html.push_str("</tr>\n");
    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Alignment, ValueType};

    #[test]
    fn test_render_simple_html() {
        let columns = vec![
            ColumnInfo::new("name".to_string()),
        ];

        let rows = vec![
            vec![CellValue::new("Alice".to_string(), ValueType::Text, Alignment::Left)],
        ];

        let layout = TableLayout::all_visible(vec![5]);
        let config = TableConfig::html();

        let html = render_html(&columns, &rows, &[], &layout, &config, 1);

        assert!(html.contains("<table"));
        assert!(html.contains("name"));
        assert!(html.contains("Alice"));
        assert!(html.contains("</table>"));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
    }
}
