use rustyline::highlight::Highlighter;
use solite_stdlib::BUILTIN_FUNCTIONS;
use solite_theme::Theme;
use std::borrow::Cow::{self, Borrowed, Owned};

use solite_lexer::{lex, Token, TokenKind};

/// Tokenize `sql` and paint each token with the matching role from `theme`.
/// Whitespace and unstyled punctuation are emitted verbatim, so
/// ANSI-stripped output always equals the input.
///
/// Gated on [`crate::colors::use_color`]: this is the pure styling pass with
/// no gate of its own — see [`highlight_sql`] for the public, gated entry
/// point (kept separate so unit tests can exercise the styling logic
/// directly without depending on the process-global color resolution).
fn highlight_sql_with(sql: &str, theme: &Theme) -> String {
    let tokens = lex(sql);
    let mut hl = String::new();
    let mut iter = tokens.iter().peekable();
    let mut prevs: Vec<&Token> = vec![];
    let mut prev_end = 0usize;

    while let Some(token) = iter.next() {
        // Emit any whitespace/characters between tokens as plain text
        if token.span.start > prev_end {
            hl.push_str(&sql[prev_end..token.span.start]);
        }
        let s = match token.kind {
            // Comments (line and block)
            TokenKind::Comment | TokenKind::BlockComment => {
                theme.comment.paint(&sql[token.span.clone()])
            }
            // Bind parameters (all 4 variants)
            TokenKind::BindParam
            | TokenKind::BindParamColon
            | TokenKind::BindParamAt
            | TokenKind::BindParamDollar => theme.parameter.paint(&sql[token.span.clone()]),
            // Numbers (integer, float, hex)
            TokenKind::Integer | TokenKind::Float | TokenKind::HexInteger => {
                theme.number.paint(&sql[token.span.clone()])
            }
            // Operators
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Pipe
            | TokenKind::Slash
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::BangEq
            | TokenKind::Ampersand
            | TokenKind::Tilde
            | TokenKind::LShift
            | TokenKind::RShift
            | TokenKind::Concat
            | TokenKind::Percent => theme.operator.paint(&sql[token.span.clone()]),
            // String literals
            TokenKind::String | TokenKind::Blob => theme.string_literal.paint(&sql[token.span.clone()]),
            // Punctuation (no styling)
            TokenKind::Star
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::Comma
            | TokenKind::Semicolon
            | TokenKind::Dot => sql[token.span.clone()].to_string(),
            // Parentheses
            TokenKind::LParen | TokenKind::RParen => {
                theme.punctuation.paint(&sql[token.span.clone()])
            }
            // JSON operators
            TokenKind::Arrow | TokenKind::ArrowArrow => {
                theme.operator.paint(&sql[token.span.clone()])
            }
            // Identifiers (regular and quoted)
            TokenKind::Ident
            | TokenKind::QuotedIdent
            | TokenKind::BracketIdent
            | TokenKind::BacktickIdent => {
                // If the next token is a '(' and previous token is NOT 'using' or 'table'
                if matches!(iter.peek().map(|v| v.kind), Some(TokenKind::LParen))
                    && !matches!(
                        prevs.last().map(|t| t.kind),
                        Some(TokenKind::Using) | Some(TokenKind::Table)
                    )
                {
                    if BUILTIN_FUNCTIONS
                        .iter()
                        .any(|r| *r == sql[token.span.clone()].trim())
                    {
                        theme.function_builtin.paint(&sql[token.span.clone()])
                    } else {
                        theme.function.paint(&sql[token.span.clone()])
                    }
                } else {
                    sql[token.span.clone()].to_string()
                }
            }
            // Everything else is a keyword
            _ => theme.keyword.paint(&sql[token.span.clone()]),
        };
        hl.push_str(&s);
        prev_end = token.span.end;
        prevs.push(token);
    }
    // Emit any trailing content after the last token
    if prev_end < sql.len() {
        hl.push_str(&sql[prev_end..]);
    }
    hl
}

/// Syntax-highlight a SQL string with `theme`'s SQL-token roles.
///
/// Returns `sql` unstyled when [`crate::colors::use_color`] is `false` (e.g.
/// `--color never`, `NO_COLOR`, or non-tty stdout) — the REPL is interactive
/// so this mostly matters for scripted/piped use and `.schema`/`.describe`
/// output.
pub fn highlight_sql(sql: &str, theme: &Theme) -> String {
    if !crate::colors::use_color() {
        return sql.to_string();
    }
    highlight_sql_with(sql, theme)
}

/// Style the leading `.command` token of a dot-command line with `theme`'s
/// `dot_command` role, leaving arguments untouched. Only known command names
/// are styled, so an unknown command reads as unrecognized while typing.
///
/// Command names come from `DOT_COMMAND_NAMES` (shared with tab completion);
/// ticket 07 derives that list from the canonical help registry.
///
/// Gated on [`crate::colors::use_color`], same as [`highlight_sql`].
pub fn highlight_dot(copy: &mut String, theme: &Theme) {
    if !crate::colors::use_color() {
        return;
    }
    highlight_dot_with(copy, theme);
}

fn highlight_dot_with(copy: &mut String, theme: &Theme) {
    let Some(rest) = copy.strip_prefix('.') else {
        return;
    };
    // The command word spans from after the `.` to the first whitespace.
    let end = rest
        .find(char::is_whitespace)
        .map(|idx| idx + 1)
        .unwrap_or(copy.len());
    let word = &copy[1..end];
    if word.is_empty() {
        return;
    }
    if super::completer::DOT_COMMAND_NAMES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(word))
    {
        let styled = theme.dot_command.paint(word);
        copy.replace_range(1..end, styled.as_str());
    }
}

pub struct ReplHighlighter {
    theme: Theme,
}

impl Default for ReplHighlighter {
    fn default() -> Self {
        Self::new(Theme::default())
    }
}

impl ReplHighlighter {
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }
}
impl Highlighter for ReplHighlighter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.len() <= 1 {
            return Borrowed(line);
        }
        if line.starts_with('.') {
            let mut copy = line.to_owned();
            highlight_dot(&mut copy, &self.theme);
            Owned(copy)
        } else {
            Owned(highlight_sql(line, &self.theme))
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_binary_snapshot;

    use super::*;

    fn sql_html(s: &str, theme: &Theme) -> String {
        let result = ansi_to_html::convert(&highlight_sql_with(s, theme)).unwrap();
        format!("<html><body>{result}</body></html>")
    }

    fn assert_sql_snapshot(s: &str) {
        assert_binary_snapshot!(".html", sql_html(s, &Theme::catppuccin_mocha()).into());
    }

    #[test]
    fn it_works() {
        assert_sql_snapshot("select 1, 'asdf', sqlite_version() from t;");
        assert_sql_snapshot(r#"
        -- single line comment
        /* multi
        line comment */
        create table t (id int, name text);
        insert into t (id, name) values (1, 'Alice'), (2, 'Bob');
        select id, name from t where id = 1;
        "#);
    }

    /// Terminal (ANSI-16) theme: keywords use the named SGR magenta
    /// (`\x1b[35m`, bold ⇒ `\x1b[1;35m`), not truecolor escapes.
    #[test]
    fn terminal_theme_uses_named_ansi_codes() {
        let highlighted = highlight_sql_with("select 1;", &Theme::terminal());
        assert!(
            highlighted.contains("\x1b[1;35mselect\x1b[0m") || highlighted.contains("\x1b[35;1mselect\x1b[0m"),
            "expected named ANSI magenta+bold for keyword, got: {:?}",
            highlighted
        );
        assert!(
            highlighted.contains("\x1b[33m1\x1b[0m"),
            "expected named ANSI yellow for number, got: {:?}",
            highlighted
        );
    }

    /// A builtin function call (e.g. `sqlite_version()`) is styled with the
    /// `function_builtin` role, not `function` or `operator`.
    #[test]
    fn builtin_function_uses_function_builtin_role() {
        let theme = Theme::catppuccin_mocha();
        let highlighted = highlight_sql_with("select sqlite_version();", &theme);
        assert!(
            highlighted.contains(&theme.function_builtin.paint("sqlite_version")),
            "builtin function not styled with function_builtin role: {:?}",
            highlighted
        );
    }

    /// Strip ANSI escape codes from a string to get the plain text
    fn strip_ansi(s: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    fn highlight_dot_str(input: &str, theme: &Theme) -> String {
        let mut copy = input.to_owned();
        highlight_dot_with(&mut copy, theme);
        copy
    }

    #[test]
    fn test_highlight_dot_preserves_text() {
        let theme = Theme::catppuccin_mocha();
        // ANSI-stripped output must always equal the input
        let inputs = [
            ".open download.db",
            ".print loading...",
            ".sh cat tables.txt",
            ".tables",
            ".export out.csv",
            ".unknowncommand args",
            ".",
            ". leading space",
            ".timer on",
        ];
        for input in inputs {
            let highlighted = highlight_dot_str(input, &theme);
            assert_eq!(
                strip_ansi(&highlighted),
                input,
                "text not preserved for {:?}",
                input
            );
        }
    }

    #[test]
    fn test_highlight_dot_styles_only_command_token() {
        let theme = Theme::catppuccin_mocha();
        // `.open download.db`: only `open` is styled; the argument is
        // untouched (previously `load` inside `download.db` got styled).
        let highlighted = highlight_dot_str(".open download.db", &theme);
        assert!(
            highlighted.contains(" download.db"),
            "argument was mangled: {:?}",
            highlighted
        );
        assert!(highlighted.contains("\x1b["), "command token not styled");
        let styled_token = theme.dot_command.paint("open");
        assert!(highlighted.contains(styled_token.as_str()));
    }

    #[test]
    fn test_highlight_dot_styles_all_known_commands() {
        let theme = Theme::catppuccin_mocha();
        for name in crate::commands::repl::completer::DOT_COMMAND_NAMES.iter() {
            let line = format!(".{name} arg");
            let highlighted = highlight_dot_str(&line, &theme);
            assert!(
                highlighted.contains("\x1b["),
                "command {:?} not styled",
                name
            );
            assert_eq!(strip_ansi(&highlighted), line);
        }
    }

    #[test]
    fn test_highlight_dot_unknown_command_unstyled() {
        let theme = Theme::catppuccin_mocha();
        let highlighted = highlight_dot_str(".nope args", &theme);
        assert_eq!(highlighted, ".nope args");
    }

    #[test]
    fn test_whitespace_preservation() {
        let theme = Theme::catppuccin_mocha();
        // The highlighted output, when stripped of ANSI codes, should match the input exactly
        let inputs = [
            "select 1 + 2;",
            "select   1   +   2;",
            "SELECT * FROM users WHERE id = 1;",
            "select\n  a,\n  b\nfrom t;",
            "select 1, 2, 3 from t where x > 10",
        ];
        for input in inputs {
            let highlighted = highlight_sql_with(input, &theme);
            let plain = strip_ansi(&highlighted);
            assert_eq!(plain, input, "Whitespace not preserved for: {:?}", input);
        }
    }

    /// Gating off (`colors::use_color() == false`, the default in this test
    /// binary since stdout is never a tty under `cargo test`): `highlight_sql`
    /// and `highlight_dot` return their input completely unstyled.
    #[test]
    fn gating_off_returns_input_unstyled() {
        assert!(
            !crate::colors::use_color(),
            "test environment is expected to be non-tty/non-forced"
        );
        let theme = Theme::catppuccin_mocha();
        let sql = "select 1, 'asdf', sqlite_version() from t;";
        assert_eq!(highlight_sql(sql, &theme), sql);

        let mut copy = ".open download.db".to_string();
        highlight_dot(&mut copy, &theme);
        assert_eq!(copy, ".open download.db");
    }
}
