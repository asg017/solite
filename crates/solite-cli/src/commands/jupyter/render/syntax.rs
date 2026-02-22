//! Syntax highlighting for SQL and JSON content in Jupyter cells.

use crate::themes::ctp_mocha_colors;
use std::sync::LazyLock;

use super::html::{Element, HtmlDoc};

/// CSS for statement cells, including JSON overflow handling.
pub static STATEMENT_CELL_CSS: LazyLock<String> = LazyLock::new(|| {
    let json_overflow_classname = "solite-json-overflow";
    format!(
        r#"
  td {{
    text-align: right;
  }}
  .{json_overflow_classname} {{
    font-size: 0;
    color: transparent;
  }}
  .{json_overflow_classname}::before {{
    content: "...";
    font-size: 1rem;
    color: #666;
  }}
  .{json_overflow_classname}::before::selection {{
    color: transparent;
    background: transparent;
  }}
"#
    )
});

/// Render JSON content with syntax highlighting into an HTML table cell.
///
/// Tokens are colorized based on their type:
/// - Keys: blue
/// - String values: green
/// - Numbers: peach
/// - Null: subtext
/// - Booleans: maroon
#[allow(dead_code)]
pub fn render_json_cell(element: &mut Element, contents: &str) {
    let td = element.child("td");
    td.style(
        "color",
        ctp_mocha_colors::TEXT.clone().to_hex_string(),
    );
    td.style("display", "inline-block");

    let tokens = solite_lexer::json::tokenize(contents);
    for token in tokens {
        match token.kind {
            solite_lexer::json::Kind::String => {
                let color = if token.string_context == Some(solite_lexer::json::StringContext::Key)
                {
                    ctp_mocha_colors::BLUE.clone().to_hex_string()
                } else {
                    ctp_mocha_colors::GREEN.clone().to_hex_string()
                };
                let span = td.child("span");
                span.style("color", color);
                span.set_text(token.text);
            }
            solite_lexer::json::Kind::Number => {
                let span = td.child("span");
                span.style("color", ctp_mocha_colors::PEACH.clone().to_hex_string());
                span.set_text(token.text);
            }
            solite_lexer::json::Kind::Null => {
                let span = td.child("span");
                span.style("color", ctp_mocha_colors::SUBTEXT1.clone().to_hex_string());
                span.set_text(token.text);
            }
            solite_lexer::json::Kind::True | solite_lexer::json::Kind::False => {
                let span = td.child("span");
                span.style("color", ctp_mocha_colors::MAROON.clone().to_hex_string());
                span.set_text(token.text);
            }
            solite_lexer::json::Kind::Whitespace => {
                // Skip whitespace - no visual output needed
            }
            solite_lexer::json::Kind::LBrace
            | solite_lexer::json::Kind::RBrace
            | solite_lexer::json::Kind::LBracket
            | solite_lexer::json::Kind::RBracket
            | solite_lexer::json::Kind::Colon
            | solite_lexer::json::Kind::Comma => {
                td.child("span").set_text(token.text);
            }
            solite_lexer::json::Kind::Unknown => {
                // Render unknown tokens as plain text rather than panicking
                td.child("span").set_text(token.text);
            }
            solite_lexer::json::Kind::Eof => {}
        }
    }
}

/// Render SQL with syntax highlighting as HTML.
///
/// Returns an HTML string with colorized SQL tokens.
pub fn render_sql_html(sql: &str) -> String {
    let doc = HtmlDoc::new();
    let mut root = doc.div();

    {
        let style = STATEMENT_CELL_CSS.clone();
        let style_el = root.child("style");
        style_el.set_text(style);
    }

    let code = root.child("pre");
    code.style("font-family", "monospace");

    let tokens = solite_lexer::lex(sql);
    let mut prev_end = 0usize;

    for token in tokens {
        // Emit any whitespace/characters between tokens as plain text
        if token.span.start > prev_end {
            code.child("span").set_text(&sql[prev_end..token.span.start]);
        }

        let color = match token.kind {
            // Numeric literals
            solite_lexer::TokenKind::Integer
            | solite_lexer::TokenKind::Float
            | solite_lexer::TokenKind::HexInteger
            | solite_lexer::TokenKind::Blob => ctp_mocha_colors::PEACH.clone(),

            // String literals
            solite_lexer::TokenKind::String => ctp_mocha_colors::GREEN.clone(),

            // Parameters (all variants)
            solite_lexer::TokenKind::BindParam
            | solite_lexer::TokenKind::BindParamColon
            | solite_lexer::TokenKind::BindParamAt
            | solite_lexer::TokenKind::BindParamDollar => ctp_mocha_colors::YELLOW.clone(),

            // Punctuation & operators
            solite_lexer::TokenKind::Plus
            | solite_lexer::TokenKind::Minus
            | solite_lexer::TokenKind::Star
            | solite_lexer::TokenKind::Slash
            | solite_lexer::TokenKind::Pipe
            | solite_lexer::TokenKind::Lt
            | solite_lexer::TokenKind::Gt
            | solite_lexer::TokenKind::Le
            | solite_lexer::TokenKind::Ge
            | solite_lexer::TokenKind::Eq
            | solite_lexer::TokenKind::EqEq
            | solite_lexer::TokenKind::Ne
            | solite_lexer::TokenKind::BangEq
            | solite_lexer::TokenKind::Arrow
            | solite_lexer::TokenKind::ArrowArrow
            | solite_lexer::TokenKind::Concat
            | solite_lexer::TokenKind::Ampersand
            | solite_lexer::TokenKind::Tilde
            | solite_lexer::TokenKind::LShift
            | solite_lexer::TokenKind::RShift
            | solite_lexer::TokenKind::Percent
            | solite_lexer::TokenKind::LParen
            | solite_lexer::TokenKind::RParen
            | solite_lexer::TokenKind::LBracket
            | solite_lexer::TokenKind::RBracket
            | solite_lexer::TokenKind::Comma
            | solite_lexer::TokenKind::Semicolon
            | solite_lexer::TokenKind::Dot => ctp_mocha_colors::SKY.clone(),

            // Comments (line and block)
            solite_lexer::TokenKind::Comment | solite_lexer::TokenKind::BlockComment => {
                ctp_mocha_colors::OVERLAY0.clone()
            }

            // Identifiers (regular and quoted)
            solite_lexer::TokenKind::Ident
            | solite_lexer::TokenKind::QuotedIdent
            | solite_lexer::TokenKind::BracketIdent
            | solite_lexer::TokenKind::BacktickIdent => ctp_mocha_colors::BLUE.clone(),

            // Everything else is a keyword
            _ => ctp_mocha_colors::MAUVE.clone(),
        };

        let span = code.child("span");
        span.style("color", color.to_hex_string());
        span.set_text(&sql[token.span.clone()]);
        prev_end = token.span.end;
    }

    // Emit any trailing content after the last token
    if prev_end < sql.len() {
        code.child("span").set_text(&sql[prev_end..]);
    }

    root.to_html()
}
