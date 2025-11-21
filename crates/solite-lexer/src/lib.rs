use std::str::Chars;

use serde::{Deserialize, Serialize};

pub mod json;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Token<'a> {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub value: TokenValue,
    pub contents: &'a str,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TokenValue {
    None,
    Int(i64),
    Text(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Eof, // end of file
    Number,

    /// '+'
    Plus,
    /// '-'
    Minus,
    Asterisk,
    Pipe,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Lt,
    Gt,
    Dot,
    Div,
    SingleArrowOperator,
    DoubleArrowOperator,

    Comment,
    String,
    Parameter,
    Select,
    From,
    Where,
    Order,
    Group,
    By,
    Limit,
    With,
    Recursive,
    Values,
    Union,
    All,
    And,
    As,
    Between,
    Descending,
    Ascending,

    Drop,
    Index,
    Indexed,
    Inner,
    Left,
    Right,
    Full,
    Outer,
    Join,
    Match,
    Partition,
    Alter,
    Rename,
    Column,
    Add,
    Immediate,
    Exclusive,
    View,
    Window,
    Vacuum,
    Transaction,
    Distinct,
    Returning,

    Create,
    Temp,
    Table,
    Virtual,
    Using,
    Attach,
    Database,
    Begin,
    Commit,
    Like,
    Regexp,
    Or,
    Not,
    Is,
    Null,
    Insert,
    Into,
    Update,
    Delete,

    Primary,
    Key,
    Foreign,
    References,
    Rollback,

    Trigger,
    Explain,
    Query,
    Plan,
    Detach,
    Pragma,
    Reindex,
    Release,
    Savepoint,
    Analyze,

    Text,
    Int,
    Float,
    Blob,
    Bit,

    Identifier,
    Unknown,
}
struct Lexer<'a> {
    source: &'a str,
    chars: Chars<'a>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.chars(),
        }
    }
}

impl<'a> Lexer<'a> {
    fn read_next_kind(&mut self) -> Kind {
        while let Some(c) = self.chars.next() {
            match c {
                ';' => return Kind::Semicolon,
                ',' => return Kind::Comma,
                '+' => return Kind::Plus,
                '*' => return Kind::Asterisk,
                '|' => return Kind::Pipe,
                '(' => return Kind::LParen,
                ')' => return Kind::RParen,
                '[' => return Kind::LBracket,
                ']' => return Kind::RBracket,
                '<' => return Kind::Lt,
                '>' => return Kind::Gt,
                '.' => return Kind::Dot,
                '/' => match self.peek() {
                    Some('*') => {
                        self.next();
                        loop {
                            match self.next() {
                                Some('*') => {
                                    if let Some('/') = self.peek() {
                                        self.next();
                                        break;
                                    }
                                    continue;
                                }
                                Some(_) => continue,
                                None => break,
                            }
                        }
                        return Kind::Comment;
                    }
                    Some(_) | None => return Kind::Div,
                },
                '-' => match self.peek() {
                    Some('-') => {
                        self.next();
                        loop {
                            match self.peek() {
                                Some('\n') | None => {
                                    self.next();
                                    break;
                                }
                                Some(_) => {
                                    self.next();
                                }
                            }
                        }
                        return Kind::Comment;
                    }
                    Some('>') => {
                        self.next();
                        if let Some('>') = self.peek() {
                            self.next();
                            return Kind::DoubleArrowOperator;
                        }
                        return Kind::SingleArrowOperator;
                    }
                    _ => return Kind::Minus,
                },
                '\'' => {
                    loop {
                        match self.peek() {
                            // TODO: can escape with double ''
                            Some('\'') | None => {
                                self.next();
                                break;
                            }
                            Some(_) => {
                                self.next();
                            }
                        }
                    }
                    return Kind::String;
                }

                '?' => {
                    loop {
                        match self.peek() {
                            None => {
                                self.next();
                                break;
                            }
                            Some('0'..='9') => {
                                self.next();
                            }
                            Some(_) => break,
                        }
                    }
                    return Kind::Parameter;
                }
                // TODO: '$' params can have `::` and `(whatever)` suffix
                ':' | '@' | '$' => {
                    loop {
                        match self.peek() {
                            None => {
                                self.next();
                                break;
                            }
                            Some(c) => {
                                match c {
                                    'a'..='z' | 'A'..='Z' | '0'..='9' | '$' | '_' => {
                                        self.next();
                                    }
                                    _ => break,
                                };
                            }
                        }
                    }
                    return Kind::Parameter;
                }
                '0'..='9' => {
                    let start = self.offset();
                    while let Some(ch) = self.peek() {
                        match ch {
                            '0'..='9' => {
                                self.next();
                            }
                            ' ' | '\n' | _ => break,
                        }
                    }
                    let end = self.offset();
                    return Kind::Number;
                }
                'a'..='z' | 'A'..='Z' => {
                    let mut identifier = String::new();
                    identifier.push(c);
                    while let Some(ch) = self.peek() {
                        match ch {
                            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                                identifier.push(ch);
                            }
                            ' ' | _ => break,
                        }
                        self.next();
                    }
                    //dbg!(identifier);
                    return match identifier.trim().to_lowercase().as_str() {
                        "select" => Kind::Select,
                        "from" => Kind::From,
                        "order" => Kind::Order,
                        "group" => Kind::Group,
                        "by" => Kind::By,
                        "limit" => Kind::Limit,
                        "where" => Kind::Where,
                        "recursive" => Kind::Recursive,
                        "values" => Kind::Values,
                        "union" => Kind::Union,
                        "all" => Kind::All,
                        "and" => Kind::And,
                        "with" => Kind::With,
                        "as" => Kind::As,
                        "create" => Kind::Create,
                        "temp" | "temporary" => Kind::Temp,
                        "table" => Kind::Table,
                        "virtual" => Kind::Virtual,
                        "using" => Kind::Using,
                        "attach" => Kind::Attach,
                        "database" => Kind::Database,
                        "begin" => Kind::Begin,
                        "commit" => Kind::Commit,
                        "like" => Kind::Like,
                        "regexp" => Kind::Regexp,
                        "or" => Kind::Or,
                        "not" => Kind::Not,
                        "is" => Kind::Is,
                        "null" => Kind::Null,
                        "insert" => Kind::Insert,
                        "into" => Kind::Into,
                        "update" => Kind::Update,
                        "delete" => Kind::Delete,

                        "primary" => Kind::Primary,
                        "key" => Kind::Key,
                        "foreign" => Kind::Foreign,
                        "references" => Kind::References,
                        "rollback" => Kind::Rollback,
                        "text" => Kind::Text,
                        "int" | "integer" => Kind::Int,
                        "float" => Kind::Float,
                        "blob" => Kind::Blob,
                        "bit" => Kind::Bit,

                        "between" => Kind::Between,
                        "ascending" | "asc" => Kind::Ascending,
                        "descending" | "desc" => Kind::Descending,
                        "drop" => Kind::Drop,
                        "index" => Kind::Index,
                        "indexed" => Kind::Indexed,
                        "inner" => Kind::Inner,
                        "left" => Kind::Left,
                        "right" => Kind::Right,
                        "full" => Kind::Full,
                        "outer" => Kind::Outer,
                        "join" => Kind::Join,
                        "match" => Kind::Match,
                        "partition" => Kind::Partition,
                        "alter" => Kind::Alter,
                        "rename" => Kind::Rename,
                        "column" => Kind::Column,
                        "add" => Kind::Add,
                        "immediate" => Kind::Immediate,
                        "exclusive" => Kind::Exclusive,
                        "view" => Kind::View,
                        "window" => Kind::Window,
                        "vacuum" => Kind::Vacuum,
                        "transaction" => Kind::Transaction,
                        "distinct" => Kind::Distinct,
                        "returning" => Kind::Returning,
                        "trigger" => Kind::Trigger,
                        "explain" => Kind::Explain,
                        "query" => Kind::Query,
                        "plan" => Kind::Plan,
                        "detach" => Kind::Detach,
                        "pragma" => Kind::Pragma,
                        "reindex" => Kind::Reindex,
                        "release" => Kind::Release,
                        "savepoint" => Kind::Savepoint,
                        "analyze" => Kind::Analyze,

                        _ => Kind::Identifier,
                    };
                }
                ' ' | '\n' | '\t' => continue,
                _ => return Kind::Unknown,
            }
        }
        Kind::Eof
    }

    fn read_next_token(&mut self) -> Token<'a> {
        let start = self.offset();
        let kind = self.read_next_kind();
        let end = self.offset();
        let value = match kind {
            Kind::Comment | Kind::String => TokenValue::Text(self.source[start..end].to_string()),
            _ => TokenValue::None,
        };
        Token {
            kind,
            start,
            end,
            value,
            contents: &self.source[start..end],
        }
    }

    /// Get the length offset from the source text, in UTF-8 bytes
    fn offset(&self) -> usize {
        self.source.len() - self.chars.as_str().len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.clone().next()
    }
    fn next(&mut self) -> Option<char> {
        self.chars.next()
    }
}

pub fn tokenize<'a>(src: &'a str) -> Vec<Token<'a>> {
    let mut l = Lexer::new(src);
    let mut tokens = vec![];
    loop {
        let token = l.read_next_token();
        let should_break = token.kind == Kind::Eof;
        tokens.push(token);
        if should_break {
            break;
        }
    }
    tokens
}

fn main() {
    let src = "select 1 + 2";
    let mut l = Lexer::new(src);
    loop {
        let token = l.read_next_token();
        println!("{:?} '{}'", token, token.contents);
        if token.kind == Kind::Eof {
            break;
        }
    }
}

#[test]
fn test_lexer() {
    let tests = vec![
        "select 1 + 2",
        "select 'rooga'",
        r#"-- comment!
    select 1 + 2"#,
    ];
    for (i, test) in tests.iter().enumerate() {
        let tokens = tokenize(test);
        let v: Vec<String> = tokens
            .iter()
            .map(|t| (&test[t.start..t.end]).to_string())
            .collect();
        let result: Vec<(&String, Token)> = v.iter().zip(tokens).collect();
        insta::assert_debug_snapshot!(format!("test_{i}"), result);
    }
}
