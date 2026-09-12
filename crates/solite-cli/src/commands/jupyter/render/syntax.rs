//! Syntax highlighting for SQL and JSON content in Jupyter cells.

use solite_lexer::TokenKind;
use solite_theme::Theme;
use std::sync::LazyLock;

use crate::sql_tokens::classify;

use super::html::HtmlDoc;

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
    color: currentColor;
    opacity: 0.6;
  }}
  .{json_overflow_classname}::before::selection {{
    color: transparent;
    background: transparent;
  }}
"#
    )
});

/// Render SQL with syntax highlighting as HTML.
///
/// Tokens are classified by the shared [`classify`] classifier (also used
/// by the REPL highlighter, `commands/repl/highlighter.rs`, so the two
/// surfaces can't drift) and painted with `theme`'s matching role as an
/// inline `color` declaration. Returns an HTML string.
pub fn render_sql_html(sql: &str, theme: &Theme) -> String {
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
    let mut prev_kind: Option<TokenKind> = None;

    for (i, token) in tokens.iter().enumerate() {
        // Emit any whitespace/characters between tokens as plain text
        if token.span.start > prev_end {
            code.child("span").set_text(&sql[prev_end..token.span.start]);
        }

        let text = &sql[token.span.clone()];
        let next_is_lparen =
            matches!(tokens.get(i + 1).map(|t| t.kind), Some(TokenKind::LParen));
        let role = classify(token.kind, text, prev_kind, next_is_lparen);

        let span = code.child("span");
        if let Some(role) = role {
            span.style("color", role.style(theme).fg.to_css());
        }
        span.set_text(text);

        prev_end = token.span.end;
        prev_kind = Some(token.kind);
    }

    // Emit any trailing content after the last token
    if prev_end < sql.len() {
        code.child("span").set_text(&sql[prev_end..]);
    }

    root.to_html()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_and_string_get_distinct_colors() {
        let theme = Theme::catppuccin_mocha();
        let html = render_sql_html("select 'hi' from t;", &theme);
        assert!(html.contains(&theme.keyword.fg.to_css()));
        assert!(html.contains(&theme.string_literal.fg.to_css()));
    }

    #[test]
    fn parameter_uses_parameter_role_not_hardcoded_yellow() {
        // Regression: the old hardcoded map colored parameters YELLOW here
        // but MAROON in the REPL. Both now go through the shared classifier
        // and theme.parameter.
        let theme = Theme::catppuccin_mocha();
        let html = render_sql_html("select $x;", &theme);
        assert!(html.contains(&format!(
            "style=\"color:{}\"",
            theme.parameter.fg.to_css()
        )));
    }

    #[test]
    fn comment_uses_comment_role_not_hardcoded_overlay0() {
        let theme = Theme::catppuccin_mocha();
        let html = render_sql_html("select 1; -- a comment", &theme);
        assert!(html.contains(&format!(
            "style=\"color:{}\"",
            theme.comment.fg.to_css()
        )));
    }

    #[test]
    fn blob_literal_uses_string_literal_role_not_number() {
        // Regression: the old hardcoded map grouped blob literals with
        // numeric literals (PEACH); the shared classifier groups them with
        // string literals, matching the REPL.
        let theme = Theme::catppuccin_mocha();
        let html = render_sql_html("select X'CAFE';", &theme);
        assert!(html.contains(&format!(
            "style=\"color:{}\"",
            theme.string_literal.fg.to_css()
        )));
    }

    #[test]
    fn terminal_theme_default_text_is_unstyled() {
        let theme = Theme::terminal();
        let html = render_sql_html("select a from t;", &theme);
        // Bare identifiers are unstyled: no <span style=...> around "a".
        assert!(!html.contains("style=\"color:currentColor\">a<"));
    }
}
