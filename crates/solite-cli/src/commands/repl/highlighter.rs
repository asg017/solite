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

use solite_lexer::{tokenize, Kind, LegacyToken};

use crate::themes::{SoliteColor, ctp_mocha_colors};
pub fn highlight_sql(copy: &mut String) -> String {
    let tokens = tokenize(copy.as_str());
    let mut hl = String::new();
    let mut iter = tokens.iter().peekable();
    let mut prevs: Vec<&LegacyToken> = vec![];
    let theme = CTP_MOCHA_THEME.clone();
    while let Some(token) = iter.next() {
        let s = match token.kind {
            // Comments (line and block)
            Kind::Comment | Kind::BlockComment => theme.style_comment(
                &copy[token.start..token.end]
            ),
            // Bind parameters (all 4 variants)
            Kind::BindParam | Kind::BindParamColon | Kind::BindParamAt | Kind::BindParamDollar => {
                theme.style_parameter(&copy[token.start..token.end])
            }
            // Numbers (integer, float, hex)
            Kind::Integer | Kind::Float | Kind::HexInteger => {
                theme.style_number(&copy[token.start..token.end])
            }
            // Operators
            Kind::Plus | Kind::Minus | Kind::Pipe | Kind::Slash | Kind::Lt | Kind::Gt
            | Kind::Le | Kind::Ge | Kind::Eq | Kind::EqEq | Kind::Ne | Kind::BangEq
            | Kind::Ampersand | Kind::Tilde | Kind::LShift | Kind::RShift | Kind::Concat
            | Kind::Percent => {
                theme.style_operator(&copy[token.start..token.end])
            }
            // String literals
            Kind::String => theme.style_string(&copy[token.start..token.end]),
            // Blob literals
            Kind::Blob => theme.style_string(&copy[token.start..token.end]),
            // Punctuation (no styling)
            Kind::Star
            | Kind::LBracket
            | Kind::RBracket
            | Kind::Comma
            | Kind::Semicolon
            | Kind::Dot => (&copy[token.start..token.end]).to_string(),
            // Parentheses
            Kind::LParen | Kind::RParen => {
                theme.style_paren(&copy[token.start..token.end])
            }
            // JSON operators
            Kind::Arrow | Kind::ArrowArrow => {
                theme.style_operator(&copy[token.start..token.end])
            }
            // Identifiers (regular and quoted)
            Kind::Ident | Kind::QuotedIdent | Kind::BracketIdent | Kind::BacktickIdent => {
                // if the next token is a '('
                if matches!(iter.peek().map(|v| v.kind), Some(Kind::LParen))
                // and the previous token is NOT 'using' or 'table'
                    && !(matches!(prevs.last().map(|t| t.kind), Some(Kind::Using) | Some(Kind::Table)))
                {
                    if BUILTIN_FUNCTIONS
                        .iter()
                        .any(|r| *r == (&copy[token.start..token.end]).trim())
                    {
                        theme.style_operator(&copy[token.start..token.end])
                    } else {
                        theme.style_function(
                            &copy[token.start..token.end]
                        )
                    }
                } else {
                    (&copy[token.start..token.end]).to_string()
                }
            }
            // Everything else is a keyword
            _ => theme.style_keyword(
                    &copy[token.start..token.end]
                )
        };
        hl.push_str(s.as_str());
        prevs.push(token);
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
}
