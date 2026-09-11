//! HTML rendering for Jupyter notebooks.

use crate::config::TableConfig;
use crate::format::html_escape;
use crate::format::value::format_cell_html;
use crate::types::{CellValue, ColumnInfo, TableLayout};

/// CSS for table styling.
///
/// No literal colors: every rule is `currentColor` or a `color-mix(in srgb,
/// currentColor N%, transparent)` tint of it, the same adaptive pattern
/// `jupyter/render/describe.rs` (`DESCRIBE_CSS`) uses. Since `currentColor`
/// resolves to the host notebook's own text color, the chrome (borders,
/// header background, row stripes, footer) is legible in light and dark
/// notebooks alike with no theme configuration at all. Cell *value* colors
/// are separate — see `theme_css_vars` — and come from the `Theme` on
/// `TableConfig`.
const TABLE_CSS: &str = r#"
.solite-table {
    border-collapse: collapse;
    font-family: monospace;
    font-size: 14px;
}
.solite-table th, .solite-table td {
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    padding: 4px 8px;
    text-align: left;
}
.solite-table th {
    background-color: color-mix(in srgb, currentColor 12%, transparent);
    font-weight: bold;
}
.solite-table tr:nth-child(even) {
    background-color: color-mix(in srgb, currentColor 6%, transparent);
}
.solite-table tr:nth-child(odd) {
    background-color: color-mix(in srgb, currentColor 3%, transparent);
}
.solite-table .ellipsis-row {
    background-color: color-mix(in srgb, currentColor 12%, transparent);
    text-align: center;
    opacity: 0.7;
    font-style: italic;
}
.solite-table .ellipsis-col {
    background-color: color-mix(in srgb, currentColor 12%, transparent);
    text-align: center;
    opacity: 0.7;
}
.solite-footer {
    opacity: 0.7;
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

    // Wrap in container div for scoped JS queries. Cell colors reference
    // `--solite-<role>` vars (see `theme_css_vars`) declared here, so they
    // stay adaptive: change the theme without touching a single cell.
    match &config.theme {
        Some(theme) => {
            let mut style = crate::format::theme_css_vars(theme);
            if config.json_interactive {
                style.push_str(&crate::format::json::json_viewer_theme_vars(theme));
            }
            html.push_str(&format!(
                "<div class=\"solite-output\" style=\"{}\">\n",
                style
            ));
        }
        None => html.push_str("<div class=\"solite-output\">\n"),
    }

    // Style tag
    html.push_str("<style>");
    html.push_str(TABLE_CSS);
    if config.json_interactive {
        html.push_str(crate::format::json::json_viewer_css());
    }
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

    // Add interactive JSON viewer script
    if config.json_interactive {
        html.push_str("<script>");
        html.push_str(crate::format::json::json_viewer_js());
        html.push_str("</script>\n");
    }

    // Close container div
    html.push_str("</div>\n");

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

        let formatted = format_cell_html(&cell, config.theme.as_ref(), config.max_cell_width, config.json_interactive);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Alignment, ValueType};

    #[test]
    fn test_render_simple_html() {
        let columns = vec![
            ColumnInfo::new("id".to_string()),
            ColumnInfo::new("name".to_string()),
        ];

        let rows = vec![
            vec![
                CellValue::new("1".to_string(), ValueType::Integer, Alignment::Right),
                CellValue::new("Alice".to_string(), ValueType::Text, Alignment::Left),
            ],
            vec![
                CellValue::new("2".to_string(), ValueType::Integer, Alignment::Right),
                CellValue::new("Bob".to_string(), ValueType::Text, Alignment::Left),
            ],
        ];

        let layout = TableLayout::all_visible(vec![2, 5]);
        let config = TableConfig::html();

        let html = render_html(&columns, &rows, &[], &layout, &config, 2);

        assert!(html.contains("<table"));
        assert!(html.contains("name"));
        assert!(html.contains("Alice"));
        assert!(html.contains("</table>"));

        // Chrome CSS is currentColor/color-mix based, not a baked palette.
        assert!(html.contains("color-mix(in srgb, currentColor"));
        // Cell values reference CSS custom properties, not resolved colors.
        assert!(html.contains("var(--solite-integer, currentColor)"));
        assert!(html.contains("var(--solite-text, currentColor)"));
        assert!(html.contains("--solite-integer:"));
    }

    #[test]
    fn html_default_theme_is_terminal_not_catppuccin() {
        // TableConfig::html() defaults to Theme::terminal() so the default
        // notebook table is host-theme-adaptive (see config.rs doc comment
        // for the readability tradeoff analysis).
        let config = TableConfig::html();
        assert_eq!(config.theme, Some(crate::Theme::terminal()));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
    }
}
