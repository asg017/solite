use rustyline::highlight::Highlighter;
use solite_stdlib::BUILTIN_FUNCTIONS;
use std::{borrow::Cow::{self, Borrowed, Owned}, sync::LazyLock};

#[derive(Clone)]
pub (crate) struct SqlTheme {
  keyword: SoliteColor,
  dot: SoliteColor,
  comment: SoliteColor,
  parameter: SoliteColor,
  types: SoliteColor,
  string: SoliteColor,
  function: SoliteColor,
  builtin: SoliteColor,
  paren: SoliteColor,
  operator: SoliteColor,
  number: SoliteColor,
}

pub(crate) static CTP_MOCHA_THEME: LazyLock<SqlTheme> = LazyLock::new(|| SqlTheme {
  keyword: ctp_mocha_colors::MAUVE.clone(),
  dot: ctp_mocha_colors::BLUE.clone(),
  comment: ctp_mocha_colors::OVERLAY2.clone(),
  parameter: ctp_mocha_colors::MAROON.clone(),
  types: ctp_mocha_colors::YELLOW.clone(),
  string: ctp_mocha_colors::GREEN.clone(),
  function: ctp_mocha_colors::BLUE.clone(),
  builtin: ctp_mocha_colors::BLUE.clone(),
  paren: ctp_mocha_colors::OVERLAY2.clone(),
  operator: ctp_mocha_colors::SKY.clone(),
  number: ctp_mocha_colors::PEACH.clone(),
});

impl SqlTheme {
  pub(crate) fn style_keyword(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.keyword.clone().into()));
      style_spec.bold();
      style_spec
    }).to_string()
  }
  pub(crate) fn style_dot(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.dot.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_comment(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.comment.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_parameter(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.parameter.clone().into()));
      style_spec
    }).to_string()
  }
  #[allow(dead_code)]
  fn style_types(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.types.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_string(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.string.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_function(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.function.clone().into()));
      style_spec
    }).to_string()
  }
  #[allow(dead_code)]
  fn style_builtin(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.builtin.clone().into())).set_bold(true);
      style_spec
    }).to_string()
  }
  fn style_paren(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.paren.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_operator(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.operator.clone().into()));
      style_spec
    }).to_string()
  }
  fn style_number(&self, s: &str) -> String {
    sql_highlighter::style(s, &{
      let mut style_spec = termcolor::ColorSpec::new();
      style_spec.set_fg(Some(self.number.clone().into()));
      style_spec
    }).to_string()
  }

}

pub mod sql_highlighter {
    use core::fmt;
    use std::io::Write;
    use termcolor::Ansi;
    use termcolor::WriteColor;
    use termcolor::ColorSpec;

    pub fn style<S: AsRef<str>>(s: S, colorspec: &ColorSpec) -> impl fmt::Display {
        let mut v = Vec::new();
        let mut ansi_writer = Ansi::new(&mut v);
        ansi_writer.set_color(colorspec).unwrap();
        ansi_writer.write_all(s.as_ref().as_bytes()).unwrap();
        ansi_writer.reset().unwrap();
        String::from_utf8_lossy(&v).into_owned()
    }
}

use solite_lexer::{lex, Token, TokenKind};

use crate::themes::{SoliteColor, ctp_mocha_colors};
pub fn highlight_sql(copy: &mut String) -> String {
    let tokens = lex(copy.as_str());
    let mut hl = String::new();
    let mut iter = tokens.iter().peekable();
    let mut prevs: Vec<&Token> = vec![];
    let mut prev_end = 0usize; // Track where the last token ended
    let theme = CTP_MOCHA_THEME.clone();
    while let Some(token) = iter.next() {
        // Emit any whitespace/characters between tokens as plain text
        if token.span.start > prev_end {
            hl.push_str(&copy[prev_end..token.span.start]);
        }
        let s = match token.kind {
            // Comments (line and block)
            TokenKind::Comment | TokenKind::BlockComment => theme.style_comment(
                &copy[token.span.clone()]
            ),
            // Bind parameters (all 4 variants)
            TokenKind::BindParam | TokenKind::BindParamColon | TokenKind::BindParamAt | TokenKind::BindParamDollar => {
                theme.style_parameter(&copy[token.span.clone()])
            }
            // Numbers (integer, float, hex)
            TokenKind::Integer | TokenKind::Float | TokenKind::HexInteger => {
                theme.style_number(&copy[token.span.clone()])
            }
            // Operators
            TokenKind::Plus | TokenKind::Minus | TokenKind::Pipe | TokenKind::Slash | TokenKind::Lt | TokenKind::Gt
            | TokenKind::Le | TokenKind::Ge | TokenKind::Eq | TokenKind::EqEq | TokenKind::Ne | TokenKind::BangEq
            | TokenKind::Ampersand | TokenKind::Tilde | TokenKind::LShift | TokenKind::RShift | TokenKind::Concat
            | TokenKind::Percent => {
                theme.style_operator(&copy[token.span.clone()])
            }
            // String literals
            TokenKind::String => theme.style_string(&copy[token.span.clone()]),
            // Blob literals
            TokenKind::Blob => theme.style_string(&copy[token.span.clone()]),
            // Punctuation (no styling)
            TokenKind::Star
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::Comma
            | TokenKind::Semicolon
            | TokenKind::Dot => copy[token.span.clone()].to_string(),
            // Parentheses
            TokenKind::LParen | TokenKind::RParen => {
                theme.style_paren(&copy[token.span.clone()])
            }
            // JSON operators
            TokenKind::Arrow | TokenKind::ArrowArrow => {
                theme.style_operator(&copy[token.span.clone()])
            }
            // Identifiers (regular and quoted)
            TokenKind::Ident | TokenKind::QuotedIdent | TokenKind::BracketIdent | TokenKind::BacktickIdent => {
                // if the next token is a '('
                if matches!(iter.peek().map(|v| v.kind), Some(TokenKind::LParen))
                // and the previous token is NOT 'using' or 'table'
                    && !(matches!(prevs.last().map(|t| t.kind), Some(TokenKind::Using) | Some(TokenKind::Table)))
                {
                    if BUILTIN_FUNCTIONS
                        .iter()
                        .any(|r| *r == copy[token.span.clone()].trim())
                    {
                        theme.style_operator(&copy[token.span.clone()])
                    } else {
                        theme.style_function(
                            &copy[token.span.clone()]
                        )
                    }
                } else {
                    copy[token.span.clone()].to_string()
                }
            }
            // Everything else is a keyword
            _ => theme.style_keyword(
                    &copy[token.span.clone()]
                )
        };
        hl.push_str(s.as_str());
        prev_end = token.span.end;
        prevs.push(token);
    }
    // Emit any trailing content after the last token
    if prev_end < copy.len() {
        hl.push_str(&copy[prev_end..]);
    }
    hl
}
pub fn highlight_dot(copy: &mut String) {
    let keywords = ["load", "tables", "open", "export"];
    for kw in keywords.iter() {
        if let Some(s) = copy.find(kw) {
            copy.replace_range(
                s..s + kw.len(),
                CTP_MOCHA_THEME.style_dot(kw).as_str(),
            );
        }
    }
}

#[derive(Default)]
pub struct ReplHighlighter {}

impl ReplHighlighter {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}
impl Highlighter for ReplHighlighter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.len() <= 1 {
            return Borrowed(line);
        }
        let mut copy = line.to_owned();
        if line.starts_with('.') {
            highlight_dot(&mut copy);
        } else {
            return Owned(highlight_sql(&mut copy));
        }
        return Owned(copy);
    }

    fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_binary_snapshot;

    use super::*;

    fn sql_html(s: &str) -> String {
        let result = ansi_to_html::convert(&highlight_sql(&mut s.to_string())).unwrap();
        format!("<html><body>{result}</body></html>")
    }

    fn assert_sql_snapshot(s: &str) {
        assert_binary_snapshot!(".html", sql_html(s).into());
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

    /// Strip ANSI escape codes from a string to get the plain text
    fn strip_ansi(s: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    #[test]
    fn test_whitespace_preservation() {
        // The highlighted output, when stripped of ANSI codes, should match the input exactly
        let inputs = [
            "select 1 + 2;",
            "select   1   +   2;",
            "SELECT * FROM users WHERE id = 1;",
            "select\n  a,\n  b\nfrom t;",
            "select 1, 2, 3 from t where x > 10",
        ];
        for input in inputs {
            let highlighted = highlight_sql(&mut input.to_string());
            let plain = strip_ansi(&highlighted);
            assert_eq!(plain, input, "Whitespace not preserved for: {:?}", input);
        }
    }
}
