//! SQL result table rendering for Jupyter cells.

use anyhow::Result;
use html_builder::*;
use solite_core::sqlite::{self, ColumnMeta, Statement, ValueRefX, ValueRefXValue};
use std::fmt::Write;

use super::syntax::STATEMENT_CELL_CSS;

/// Response containing both text and HTML representations of a result.
pub struct UiResponse {
    pub text: String,
    pub html: Option<String>,
}

/// Render a table row from SQLite values into HTML.
fn render_row_html<'a>(tbody: &'a mut Node, row: &[ValueRefX]) -> Result<Node<'a>> {
    let mut tr = tbody.tr();
    for value in row {
        let raw: String = match value.value {
            ValueRefXValue::Null => String::new(),
            ValueRefXValue::Int(v) => v.to_string(),
            ValueRefXValue::Double(v) => v.to_string(),
            ValueRefXValue::Text(v) => String::from_utf8_lossy(v).into_owned(),
            ValueRefXValue::Blob(v) => format!("Blob<{}>", v.len()),
        };

        let style: String = match value.value {
            ValueRefXValue::Double(_) | ValueRefXValue::Int(_) | ValueRefXValue::Null => {
                "font-family: monospace".to_owned()
            }
            ValueRefXValue::Text(_) => match value.subtype() {
                Some(sqlite::JSON_SUBTYPE) => {
                    // Convert html_builder Node to our Element for JSON rendering
                    render_json_cell_compat(&mut tr, &raw);
                    continue;
                }
                Some(_) | None => "text-align: left".to_owned(),
            },
            ValueRefXValue::Blob(_) => match value.subtype() {
                Some(223) | Some(224) | Some(225) => "color: blue".to_owned(),
                Some(_) | None => String::new(),
            },
        };

        let mut td = tr.td().attr(format!("style=\"{}\"", style).as_str());
        writeln!(td, "{}", raw)?;
    }
    Ok(tr)
}

/// Compatibility wrapper to render JSON cells using our custom HTML builder
/// but within the html_builder Node tree.
fn render_json_cell_compat(tr: &mut Node, contents: &str) {
    use crate::themes::ctp_mocha_colors;

    let mut td = tr.td().attr(&format!(
        "style=\"color: {}; display: inline-block;\"",
        ctp_mocha_colors::TEXT.clone().to_hex_string()
    ));

    let tokens = solite_lexer::json::tokenize(contents);
    for token in tokens {
        match token.kind {
            solite_lexer::json::Kind::String => {
                let color = if token.string_context == Some(solite_lexer::json::StringContext::Key)
                {
                    format!("color: {};", ctp_mocha_colors::BLUE.clone().to_hex_string())
                } else {
                    format!(
                        "color: {};",
                        ctp_mocha_colors::GREEN.clone().to_hex_string()
                    )
                };
                let mut span = td.span().attr(format!("style=\"{}\"", color).as_str());
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::Number => {
                let mut span = td.span().attr(
                    format!(
                        "style=\"color: {};\"",
                        ctp_mocha_colors::PEACH.clone().to_hex_string()
                    )
                    .as_str(),
                );
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::Null => {
                let mut span = td.span().attr(
                    format!(
                        "style=\"color: {};\"",
                        ctp_mocha_colors::SUBTEXT1.clone().to_hex_string()
                    )
                    .as_str(),
                );
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::True | solite_lexer::json::Kind::False => {
                let mut span = td.span().attr(
                    format!(
                        "style=\"color: {};\"",
                        ctp_mocha_colors::MAROON.clone().to_hex_string()
                    )
                    .as_str(),
                );
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::Whitespace => {
                // Skip whitespace
            }
            solite_lexer::json::Kind::LBrace
            | solite_lexer::json::Kind::RBrace
            | solite_lexer::json::Kind::LBracket
            | solite_lexer::json::Kind::RBracket
            | solite_lexer::json::Kind::Colon
            | solite_lexer::json::Kind::Comma => {
                let mut span = td.span();
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::Unknown => {
                // Render unknown tokens as plain text
                let mut span = td.span();
                write!(span, "{}", token.text).unwrap();
            }
            solite_lexer::json::Kind::Eof => {}
        }
    }
}

/// Render table header row from statement column metadata.
fn render_thead_html(thead: &mut Node, columns: &[ColumnMeta]) -> Result<()> {
    let mut tr = thead.tr().attr("style=\"text-align: center;\"");
    for column in columns {
        let title = format!(
            "{} {}",
            // column type
            match column.decltype {
                Some(ref t) => format!("{t} . "),
                None => String::new(),
            },
            // "db.table.column"
            format!(
                "{}{}{}",
                match &column.origin_database {
                    None => String::new(),
                    Some(db) =>
                        if db == "main" {
                            String::new()
                        } else {
                            format!("{db}.")
                        },
                },
                match &column.origin_table {
                    None => String::new(),
                    Some(table) => format!("{table}."),
                },
                column.origin_column.as_ref().map_or("", |v| v)
            )
        )
        .replace('"', "&quot;");
        let mut th = tr.th().attr(format!("title=\"{}\"", title).as_str());
        writeln!(th, "{}", column.name)?;
    }

    Ok(())
}

/// Maximum number of rows to display for non-EXPLAIN queries.
const DEFAULT_MAX_ROWS: usize = 30;

/// Render a SQL statement result as both text and HTML.
pub fn render_statement(stmt: &Statement) -> Result<UiResponse> {
    let mut txt_rows = vec![];

    let mut buf = Buffer::new();
    let mut htmlx = buf.html();

    let mut root = htmlx.div();
    writeln!(root.style(), "{}", STATEMENT_CELL_CSS.clone())?;
    let mut table = root.table();

    let columns = stmt.column_meta();
    render_thead_html(&mut table.thead(), &columns)?;

    let mut row_count = 0;
    let max_rows = match stmt.is_explain() {
        None => Some(DEFAULT_MAX_ROWS),
        Some(_) => None,
    };
    let column_count = columns.len();

    let mut tbody = table.tbody();
    loop {
        match stmt.next() {
            Ok(result) => match result {
                Some(row) => {
                    row_count += 1;
                    if !max_rows.is_some_and(|max| row_count > max) {
                        txt_rows.push(crate::ui::ui_row(&row, None));
                        render_row_html(&mut tbody, &row)?;
                    }
                }
                None => break,
            },
            Err(error) => return Err(anyhow::anyhow!(error)),
        }
    }

    if max_rows.is_some_and(|max| row_count > max) {
        writeln!(
            tbody
                .tr()
                .attr("style=\"background: red\"")
                .td()
                .attr(format!("colspan=\"{column_count}\"").as_str()),
            "WARNING: Results truncated to {} rows ({} total)",
            DEFAULT_MAX_ROWS,
            row_count
        )?;
    }

    writeln!(
        root.div(),
        "{} column{} \u{00d7} {} row{}",
        column_count,
        if column_count < 2 { "" } else { "s" },
        row_count,
        if row_count < 2 { "" } else { "s" },
    )?;

    Ok(UiResponse {
        text: crate::ui::ui_table(columns.iter().map(|c| c.name.clone()).collect(), txt_rows)
            .display()?
            .to_string(),
        html: Some(buf.finish()),
    })
}
