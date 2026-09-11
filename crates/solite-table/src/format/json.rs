//! JSON syntax highlighting and interactive viewer.

use crate::format::html_escape;
use solite_lexer::json::{tokenize, Kind, StringContext};
use solite_theme::Theme;

/// Format JSON with ANSI color codes.
pub fn format_json(contents: &str, theme: &Theme) -> String {
    let tokens = tokenize(contents);
    let mut output = String::new();

    for token in tokens {
        match token.kind {
            Kind::String => {
                let style = if token.string_context == Some(StringContext::Key) {
                    &theme.json_key
                } else {
                    &theme.json_string
                };
                output.push_str(&style.paint(token.text));
            }
            Kind::Number => {
                output.push_str(&theme.json_number.paint(token.text));
            }
            Kind::Null => {
                output.push_str(&theme.null.paint(token.text));
            }
            Kind::True | Kind::False => {
                output.push_str(&theme.json_boolean.paint(token.text));
            }
            Kind::LBrace => output.push('{'),
            Kind::RBrace => output.push('}'),
            Kind::LBracket => output.push('['),
            Kind::RBracket => output.push(']'),
            Kind::Colon => output.push(':'),
            Kind::Comma => output.push(','),
            Kind::Whitespace => output.push(' '),
            Kind::Unknown => output.push_str(token.text),
            Kind::Eof => {}
        }
    }

    output
}

/// Format JSON for HTML output with inline styles.
pub fn format_json_html(contents: &str, theme: &Theme) -> String {
    let tokens = tokenize(contents);
    let mut output = String::new();

    for token in tokens {
        match token.kind {
            Kind::String => {
                let style = if token.string_context == Some(StringContext::Key) {
                    &theme.json_key
                } else {
                    &theme.json_string
                };
                output.push_str(&format!(
                    "<span style=\"{}\">{}</span>",
                    style.to_css(),
                    html_escape(token.text)
                ));
            }
            Kind::Number => {
                output.push_str(&format!(
                    "<span style=\"{}\">{}</span>",
                    theme.json_number.to_css(),
                    html_escape(token.text)
                ));
            }
            Kind::Null => {
                output.push_str(&format!(
                    "<span style=\"{}\">{}</span>",
                    theme.null.to_css(),
                    html_escape(token.text)
                ));
            }
            Kind::True | Kind::False => {
                output.push_str(&format!(
                    "<span style=\"{}\">{}</span>",
                    theme.json_boolean.to_css(),
                    html_escape(token.text)
                ));
            }
            Kind::LBrace => output.push('{'),
            Kind::RBrace => output.push('}'),
            Kind::LBracket => output.push('['),
            Kind::RBracket => output.push(']'),
            Kind::Colon => output.push(':'),
            Kind::Comma => output.push(','),
            Kind::Whitespace => {}
            Kind::Unknown => output.push_str(&html_escape(token.text)),
            Kind::Eof => {}
        }
    }

    output
}

/// Format a JSON cell for interactive HTML display.
/// Returns a `<span>` with `data-json` attribute containing the raw JSON,
/// and the flat syntax-highlighted HTML as fallback content.
pub fn format_json_interactive_html(contents: &str, theme: &Theme) -> String {
    let fallback = format_json_html(contents, theme);
    let escaped_json = html_escape(contents);
    format!(
        "<span class=\"solite-json-cell\" data-json=\"{}\">{}</span>",
        escaped_json, fallback
    )
}

/// CSS for the interactive JSON inspector (static, uses CSS custom properties).
pub fn json_viewer_css() -> &'static str {
    include_str!("json_viewer.css")
}

/// Self-contained JS for the interactive JSON viewer.
pub fn json_viewer_js() -> &'static str {
    include_str!("json_viewer.js")
}

/// Generate inline CSS custom property declarations for the JSON viewer theme.
/// Intended to be used as a `style` attribute on the container element, so
/// each value must be a bare CSS color (e.g. `#fab387`), not a full `color:
/// ...` declaration.
pub fn json_viewer_theme_vars(theme: &Theme) -> String {
    format!(
        "--jt-key: {}; --jt-str: {}; --jt-num: {}; --jt-bool: {}; \
         --jt-null: {}; --jt-footer: {}; --jt-text: {}; \
         --jt-border: {}; --jt-bg: {};",
        theme.json_key.fg.to_css(),
        theme.json_string.fg.to_css(),
        theme.json_number.fg.to_css(),
        theme.json_boolean.fg.to_css(),
        theme.null.fg.to_css(),
        theme.footer.fg.to_css(),
        theme.text.fg.to_css(),
        theme.border.fg.to_css(),
        theme.header.fg.to_css(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json_object() {
        let theme = Theme::catppuccin_mocha();
        let json = r#"{"key": "value"}"#;
        let formatted = format_json(json, &theme);

        assert!(formatted.contains("key"));
        assert!(formatted.contains("value"));
        assert!(formatted.contains("{"));
        assert!(formatted.contains("}"));
    }

    #[test]
    fn test_format_json_html() {
        let theme = Theme::catppuccin_mocha();
        let json = r#"{"num": 42}"#;
        let formatted = format_json_html(json, &theme);

        assert!(formatted.contains("<span"));
        assert!(formatted.contains("42"));
    }

    #[test]
    fn test_format_json_interactive_html() {
        let theme = Theme::catppuccin_mocha();
        let json = r#"{"name": "Alice"}"#;
        let result = format_json_interactive_html(json, &theme);

        assert!(result.starts_with("<span class=\"solite-json-cell\""));
        assert!(result.contains("data-json=\""));
        // Fallback content should be inside the span
        assert!(result.contains("<span style="));
    }

    #[test]
    fn test_format_json_interactive_html_escapes_json() {
        let theme = Theme::catppuccin_mocha();
        let json = r#"{"html": "<b>bold</b>"}"#;
        let result = format_json_interactive_html(json, &theme);

        // data-json attribute must have HTML-escaped content
        assert!(result.contains("&lt;b&gt;bold&lt;/b&gt;"));
    }

    #[test]
    fn test_json_viewer_theme_vars() {
        let theme = Theme::catppuccin_mocha();
        let vars = json_viewer_theme_vars(&theme);

        assert!(vars.contains("--jt-key:"));
        assert!(vars.contains("--jt-str:"));
        assert!(vars.contains("--jt-num:"));
        assert!(vars.contains("--jt-bool:"));
        assert!(vars.contains("--jt-null:"));
        assert!(vars.contains("#89b4fa")); // json_key blue
    }

    #[test]
    fn test_format_json_terminal_theme_named_ansi() {
        let theme = Theme::terminal();
        let json = r#"{"num": 42}"#;
        let formatted = format_json(json, &theme);

        // json_number is ANSI yellow (SGR 33) in the terminal theme.
        assert!(formatted.contains("\x1b[33m42\x1b[0m"));
    }

    #[test]
    fn test_json_viewer_css_is_valid() {
        let css = json_viewer_css();
        assert!(css.contains(".solite-json-tree"));
        assert!(css.contains("var(--jt-key)"));
    }

    #[test]
    fn test_json_viewer_js_is_valid() {
        let js = json_viewer_js();
        assert!(js.contains("solite-json-cell"));
        assert!(js.contains("sessionStorage"));
    }
}
