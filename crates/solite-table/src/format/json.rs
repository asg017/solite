//! JSON syntax highlighting.

use crate::theme::{Theme, RESET};
use solite_lexer::json::{tokenize, Kind, StringContext};

/// Format JSON with ANSI color codes.
pub fn format_json(contents: &str, theme: &Theme) -> String {
    let tokens = tokenize(contents);
    let mut output = String::new();

    for token in tokens {
        match token.kind {
            Kind::String => {
                let color = if token.string_context == Some(StringContext::Key) {
                    &theme.json_key
                } else {
                    &theme.json_string
                };
                output.push_str(&color.to_ansi_fg());
                output.push_str(token.text);
                output.push_str(RESET);
            }
            Kind::Number => {
                output.push_str(&theme.json_number.to_ansi_fg());
                output.push_str(token.text);
                output.push_str(RESET);
            }
            Kind::Null => {
                output.push_str(&theme.null.to_ansi_fg());
                output.push_str(token.text);
                output.push_str(RESET);
            }
            Kind::True | Kind::False => {
                output.push_str(&theme.json_boolean.to_ansi_fg());
                output.push_str(token.text);
                output.push_str(RESET);
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
                let color = if token.string_context == Some(StringContext::Key) {
                    &theme.json_key
                } else {
                    &theme.json_string
                };
                output.push_str(&format!(
                    "<span style=\"color: {};\">{}</span>",
                    color.to_hex_string(),
                    html_escape(token.text)
                ));
            }
            Kind::Number => {
                output.push_str(&format!(
                    "<span style=\"color: {};\">{}</span>",
                    theme.json_number.to_hex_string(),
                    html_escape(token.text)
                ));
            }
            Kind::Null => {
                output.push_str(&format!(
                    "<span style=\"color: {};\">{}</span>",
                    theme.null.to_hex_string(),
                    html_escape(token.text)
                ));
            }
            Kind::True | Kind::False => {
                output.push_str(&format!(
                    "<span style=\"color: {};\">{}</span>",
                    theme.json_boolean.to_hex_string(),
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

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json_object() {
        let theme = Theme::catppuccin_mocha();
        let json = r#"{"key": "value"}"#;
        let formatted = format_json(json, &theme);

        // Should contain the original text (without checking exact ANSI codes)
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
    fn test_html_escape() {
        assert_eq!(html_escape("<>&\""), "&lt;&gt;&amp;&quot;");
    }
}
