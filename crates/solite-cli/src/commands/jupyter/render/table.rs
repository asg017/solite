//! SQL result table rendering for Jupyter cells.

use anyhow::Result;
use solite_core::sqlite::Statement;
use solite_table::TableConfig;

/// Response containing both text and HTML representations of a result.
pub struct UiResponse {
    pub text: String,
    pub html: Option<String>,
}

/// Render a SQL statement result as both text and HTML.
pub fn render_statement(stmt: &Statement) -> Result<UiResponse> {
    // Render HTML version
    let html_config = TableConfig::html();
    let html_result = solite_table::render_statement(stmt, &html_config)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // For text, we need to reset the statement and render again
    // Since Statement doesn't support reset for iteration, we'll use the HTML output
    // with a plain text fallback from the same data

    // Create plain text version from the HTML render result metadata
    let text = format!(
        "{} column{} × {} row{}",
        html_result.total_columns,
        if html_result.total_columns != 1 { "s" } else { "" },
        html_result.total_rows,
        if html_result.total_rows != 1 { "s" } else { "" },
    );

    Ok(UiResponse {
        text,
        html: Some(html_result.output),
    })
}
