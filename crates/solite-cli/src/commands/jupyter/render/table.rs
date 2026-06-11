//! SQL result table rendering for Jupyter cells.

use anyhow::Result;
use solite_core::sqlite::Statement;
use solite_table::TableConfig;

/// Response containing both text and HTML representations of a result.
pub struct UiResponse {
    pub text: String,
    pub html: String,
}

/// Render a SQL statement result as both text and HTML, from a single pass
/// over the rows (a `Statement` can't be iterated twice).
pub fn render_statement(stmt: &Statement) -> Result<UiResponse> {
    let html_config = TableConfig::html();
    let buffered =
        solite_table::buffer_statement(stmt, html_config.head_rows, html_config.tail_rows)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Column-less statements (e.g. CREATE TABLE) have nothing to render.
    let Some(buffered) = buffered else {
        return Ok(UiResponse {
            text: String::new(),
            html: String::new(),
        });
    };

    let html = solite_table::render_buffered(&buffered, &html_config).output;

    // Plain-text fallback for nbconvert, terminal clients, copy-as-text.
    // Same retained rows as the HTML rendering.
    let plain_config =
        TableConfig::plain().with_row_limits(html_config.head_rows, html_config.tail_rows);
    let text = solite_table::render_buffered(&buffered, &plain_config).output;

    Ok(UiResponse { text, html })
}
