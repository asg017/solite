use rustyline::highlight::Highlighter;
use solite_stdlib::BUILTIN_FUNCTIONS;
use std::borrow::Cow::{self, Borrowed, Owned};

pub mod sql_highlighter {
    use core::fmt;
    use std::io::Write;
    use std::sync::OnceLock;
    use termcolor::Ansi;
    use termcolor::WriteColor;
    use termcolor::{Color, ColorSpec};

    const KEYWORD_COLOR: Color = Color::Rgb(203, 166, 247);

    pub fn style<S: AsRef<str>>(s: S, colorspec: &ColorSpec) -> impl fmt::Display {
        let mut v = Vec::new();
        let mut ansi_writer = Ansi::new(&mut v);
        ansi_writer.set_color(colorspec).unwrap();
        ansi_writer.write_all(s.as_ref().as_bytes()).unwrap();
        ansi_writer.reset().unwrap();
        String::from_utf8_lossy(&v).into_owned()
    }

    pub(crate) fn keyword<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static KEYWORD: OnceLock<ColorSpec> = OnceLock::new();
        let k = KEYWORD.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(KEYWORD_COLOR)); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn dot<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static DOT: OnceLock<ColorSpec> = OnceLock::new();
        let k = DOT.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(139, 213, 202))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn comment<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static COMMENT: OnceLock<ColorSpec> = OnceLock::new();
        let k = COMMENT.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(148, 156, 187))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn parameter<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static PARAMETER: OnceLock<ColorSpec> = OnceLock::new();
        let k = PARAMETER.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(235, 160, 172))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn types<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static TYPES: OnceLock<ColorSpec> = OnceLock::new();
        let k = TYPES.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(249, 226, 175))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn string<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static STRING: OnceLock<ColorSpec> = OnceLock::new();
        let k = STRING.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(166, 209, 137))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn function<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static FUNCTION: OnceLock<ColorSpec> = OnceLock::new();
        let k = FUNCTION.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(138, 173, 244))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn builtin<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static BUILTIN: OnceLock<ColorSpec> = OnceLock::new();
        let k = BUILTIN.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec
                .set_fg(Some(Color::Rgb(138, 173, 244)))
                .set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn paren<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static PAREN: OnceLock<ColorSpec> = OnceLock::new();
        let k = PAREN.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(243, 139, 168))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn operator<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static OPERATOR: OnceLock<ColorSpec> = OnceLock::new();
        let k = OPERATOR.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(137, 220, 235))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn number<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static NUMBER: OnceLock<ColorSpec> = OnceLock::new();
        let k = NUMBER.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(245, 169, 127))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
}

use solite_lexer::{tokenize, Kind, Token};
pub fn highlight_sql(copy: &mut String) -> String {
    let tokens = tokenize(copy.as_str());
    let mut hl = String::new();
    let mut iter = tokens.iter().peekable();
    let mut prevs: Vec<&Token> = vec![];
    while let Some(token) = iter.next() {
        let s = match token.kind {
            Kind::Comment => sql_highlighter::comment(&copy[token.start..token.end]).to_string(),
            Kind::Parameter => {
                sql_highlighter::parameter(&copy[token.start..token.end]).to_string()
            }
            Kind::Text | Kind::Int | Kind::Float | Kind::Blob | Kind::Bit => {
                sql_highlighter::types(&copy[token.start..token.end]).to_string()
            }
            Kind::Number => sql_highlighter::number(&copy[token.start..token.end]).to_string(),
            Kind::Plus | Kind::Minus | Kind::Eof | Kind::Pipe | Kind::Div | Kind::Lt | Kind::Gt => {
                sql_highlighter::operator(&copy[token.start..token.end]).to_string()
            }
            Kind::String => sql_highlighter::string(&copy[token.start..token.end]).to_string(),
            Kind::Asterisk
            | Kind::LBracket
            | Kind::RBracket
            | Kind::Comma
            | Kind::Semicolon
            | Kind::Dot
            | Kind::Unknown => (&copy[token.start..token.end]).to_string(),
            Kind::LParen | Kind::RParen => {
                sql_highlighter::paren(&copy[token.start..token.end]).to_string()
            }
            Kind::SingleArrowOperator | Kind::DoubleArrowOperator => {
                sql_highlighter::builtin(&copy[token.start..token.end]).to_string()
            }
            Kind::Identifier => {
                // if the next token is a '('
                if matches!(iter.peek().map(|v| v.kind), Some(Kind::LParen))
                // and the previous token is NOT 'using' or 'table'
                    && !(matches!(prevs.last().map(|t| t.kind), Some(Kind::Using) | Some(Kind::Table)))
                {
                    if BUILTIN_FUNCTIONS
                        .iter()
                        .position(|r| *r == (&copy[token.start..token.end]).trim())
                        .is_some()
                    {
                        sql_highlighter::builtin(&copy[token.start..token.end]).to_string()
                    } else {
                        sql_highlighter::function(&copy[token.start..token.end]).to_string()
                    }
                } else {
                    (&copy[token.start..token.end]).to_string()
                }
            }
            _ => sql_highlighter::keyword(&copy[token.start..token.end]).to_string(),
        };
        hl.push_str(s.as_str());
        prevs.push(token);
    }
    hl

    /*
    let keywords = [
        "select",
        "from",
        "where",
        "group by",
        "order by",
        "limit",
        "offset",
        "with",
        "create",
        "table",
        "insert",
        "into",
        "returning",
    ];
    for kw in keywords.iter() {
        if let Some(s) = copy.find(kw) {
            //copy.replace_range(s..s + kw.len(), &format!("\x1b[1;34m{}\x1b[0m", kw));
            copy.replace_range(
                s..s + kw.len(),
                sql_highlighter::keyword(kw).to_string().as_str(),
            );
        }
    }
     */
}
pub fn highlight_dot(copy: &mut String) {
    let keywords = ["load", "tables", "open"];
    for kw in keywords.iter() {
        if let Some(s) = copy.find(kw) {
            //copy.replace_range(s..s + kw.len(), &format!("\x1b[1;34m{}\x1b[0m", kw));
            copy.replace_range(
                s..s + kw.len(),
                sql_highlighter::dot(kw).to_string().as_str(),
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
        //println!("highlight pos={}", pos);
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
        //Borrowed(line)
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
        //assert_binary_snapshot!(".html", sql_html("select 1, 'asdf', sqlite_version() from t;").into());
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
