use logos::Logos;
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub mod json;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[logos(skip r"[ \t\n\r]+")]
pub enum TokenKind {
    // Comments
    #[regex(r"--[^\n]*")]
    Comment,

    // ========================================
    // Keywords (case-insensitive)
    // ========================================

    // --- DML (Data Manipulation Language) ---
    #[token("SELECT", ignore(ascii_case))]
    Select,
    #[token("INSERT", ignore(ascii_case))]
    Insert,
    #[token("UPDATE", ignore(ascii_case))]
    Update,
    #[token("DELETE", ignore(ascii_case))]
    Delete,
    #[token("REPLACE", ignore(ascii_case))]
    Replace,
    #[token("INTO", ignore(ascii_case))]
    Into,
    #[token("VALUES", ignore(ascii_case))]
    Values,
    #[token("SET", ignore(ascii_case))]
    Set,
    #[token("FROM", ignore(ascii_case))]
    From,

    // --- DDL (Data Definition Language) ---
    #[token("CREATE", ignore(ascii_case))]
    Create,
    #[token("DROP", ignore(ascii_case))]
    Drop,
    #[token("ALTER", ignore(ascii_case))]
    Alter,
    #[token("TABLE", ignore(ascii_case))]
    Table,
    #[token("INDEX", ignore(ascii_case))]
    Index,
    #[token("VIEW", ignore(ascii_case))]
    View,
    #[token("TRIGGER", ignore(ascii_case))]
    Trigger,
    #[token("VIRTUAL", ignore(ascii_case))]
    Virtual,
    #[token("TEMP", ignore(ascii_case))]
    Temp,
    #[token("TEMPORARY", ignore(ascii_case))]
    Temporary,
    #[token("IF", ignore(ascii_case))]
    If,
    #[token("ADD", ignore(ascii_case))]
    Add,
    #[token("COLUMN", ignore(ascii_case))]
    Column,
    #[token("RENAME", ignore(ascii_case))]
    Rename,

    // --- TCL (Transaction Control Language) ---
    #[token("BEGIN", ignore(ascii_case))]
    Begin,
    #[token("COMMIT", ignore(ascii_case))]
    Commit,
    #[token("ROLLBACK", ignore(ascii_case))]
    Rollback,
    #[token("SAVEPOINT", ignore(ascii_case))]
    Savepoint,
    #[token("RELEASE", ignore(ascii_case))]
    Release,
    #[token("TRANSACTION", ignore(ascii_case))]
    Transaction,
    #[token("DEFERRED", ignore(ascii_case))]
    Deferred,
    #[token("IMMEDIATE", ignore(ascii_case))]
    Immediate,
    #[token("EXCLUSIVE", ignore(ascii_case))]
    Exclusive,
    #[token("END", ignore(ascii_case))]
    End,

    // --- Query Clauses ---
    #[token("WHERE", ignore(ascii_case))]
    Where,
    #[token("ORDER", ignore(ascii_case))]
    Order,
    #[token("BY", ignore(ascii_case))]
    By,
    #[token("GROUP", ignore(ascii_case))]
    Group,
    #[token("HAVING", ignore(ascii_case))]
    Having,
    #[token("LIMIT", ignore(ascii_case))]
    Limit,
    #[token("OFFSET", ignore(ascii_case))]
    Offset,
    #[token("DISTINCT", ignore(ascii_case))]
    Distinct,
    #[token("ALL", ignore(ascii_case))]
    All,
    #[token("AS", ignore(ascii_case))]
    As,
    #[token("ASC", ignore(ascii_case))]
    Asc,
    #[token("DESC", ignore(ascii_case))]
    Desc,
    #[token("NULLS", ignore(ascii_case))]
    Nulls,
    #[token("FIRST", ignore(ascii_case))]
    First,
    #[token("LAST", ignore(ascii_case))]
    Last,
    #[token("UNION", ignore(ascii_case))]
    Union,
    #[token("INTERSECT", ignore(ascii_case))]
    Intersect,
    #[token("EXCEPT", ignore(ascii_case))]
    Except,
    #[token("INDEXED", ignore(ascii_case))]
    Indexed,

    // --- Join Operations ---
    #[token("JOIN", ignore(ascii_case))]
    Join,
    #[token("INNER", ignore(ascii_case))]
    Inner,
    #[token("LEFT", ignore(ascii_case))]
    Left,
    #[token("RIGHT", ignore(ascii_case))]
    Right,
    #[token("FULL", ignore(ascii_case))]
    Full,
    #[token("OUTER", ignore(ascii_case))]
    Outer,
    #[token("CROSS", ignore(ascii_case))]
    Cross,
    #[token("NATURAL", ignore(ascii_case))]
    Natural,
    #[token("ON", ignore(ascii_case))]
    On,
    #[token("USING", ignore(ascii_case))]
    Using,

    // --- Logical and Comparison Operators ---
    #[token("AND", ignore(ascii_case))]
    And,
    #[token("OR", ignore(ascii_case))]
    Or,
    #[token("NOT", ignore(ascii_case))]
    Not,
    #[token("IN", ignore(ascii_case))]
    In,
    #[token("BETWEEN", ignore(ascii_case))]
    Between,
    #[token("LIKE", ignore(ascii_case))]
    Like,
    #[token("GLOB", ignore(ascii_case))]
    Glob,
    #[token("REGEXP", ignore(ascii_case))]
    Regexp,
    #[token("MATCH", ignore(ascii_case))]
    Match,
    #[token("ESCAPE", ignore(ascii_case))]
    Escape,
    #[token("IS", ignore(ascii_case))]
    Is,
    #[token("ISNULL", ignore(ascii_case))]
    IsNull,
    #[token("NOTNULL", ignore(ascii_case))]
    NotNull,
    #[token("EXISTS", ignore(ascii_case))]
    Exists,

    // --- Literal Keywords ---
    #[token("NULL", ignore(ascii_case))]
    Null,
    #[token("TRUE", ignore(ascii_case))]
    True,
    #[token("FALSE", ignore(ascii_case))]
    False,
    #[token("CURRENT_DATE", ignore(ascii_case))]
    CurrentDate,
    #[token("CURRENT_TIME", ignore(ascii_case))]
    CurrentTime,
    #[token("CURRENT_TIMESTAMP", ignore(ascii_case))]
    CurrentTimestamp,

    // --- Conditional Expressions ---
    #[token("CASE", ignore(ascii_case))]
    Case,
    #[token("WHEN", ignore(ascii_case))]
    When,
    #[token("THEN", ignore(ascii_case))]
    Then,
    #[token("ELSE", ignore(ascii_case))]
    Else,
    #[token("CAST", ignore(ascii_case))]
    Cast,

    // --- Constraint Keywords ---
    #[token("CONSTRAINT", ignore(ascii_case))]
    Constraint,
    #[token("PRIMARY", ignore(ascii_case))]
    Primary,
    #[token("KEY", ignore(ascii_case))]
    Key,
    #[token("UNIQUE", ignore(ascii_case))]
    Unique,
    #[token("CHECK", ignore(ascii_case))]
    Check,
    #[token("DEFAULT", ignore(ascii_case))]
    Default,
    #[token("COLLATE", ignore(ascii_case))]
    Collate,
    #[token("FOREIGN", ignore(ascii_case))]
    Foreign,
    #[token("REFERENCES", ignore(ascii_case))]
    References,
    #[token("AUTOINCREMENT", ignore(ascii_case))]
    Autoincrement,

    // --- Foreign Key Actions ---
    #[token("CASCADE", ignore(ascii_case))]
    Cascade,
    #[token("RESTRICT", ignore(ascii_case))]
    Restrict,
    #[token("NO", ignore(ascii_case))]
    No,
    #[token("ACTION", ignore(ascii_case))]
    Action,
    #[token("DEFERRABLE", ignore(ascii_case))]
    Deferrable,
    #[token("INITIALLY", ignore(ascii_case))]
    Initially,

    // --- Trigger Keywords ---
    #[token("BEFORE", ignore(ascii_case))]
    Before,
    #[token("AFTER", ignore(ascii_case))]
    After,
    #[token("INSTEAD", ignore(ascii_case))]
    Instead,
    #[token("OF", ignore(ascii_case))]
    Of,
    #[token("FOR", ignore(ascii_case))]
    For,
    #[token("EACH", ignore(ascii_case))]
    Each,
    #[token("ROW", ignore(ascii_case))]
    Row,
    #[token("RAISE", ignore(ascii_case))]
    Raise,

    // --- Window Functions ---
    #[token("OVER", ignore(ascii_case))]
    Over,
    #[token("PARTITION", ignore(ascii_case))]
    Partition,
    #[token("WINDOW", ignore(ascii_case))]
    Window,
    #[token("ROWS", ignore(ascii_case))]
    Rows,
    #[token("RANGE", ignore(ascii_case))]
    Range,
    #[token("GROUPS", ignore(ascii_case))]
    Groups,
    #[token("UNBOUNDED", ignore(ascii_case))]
    Unbounded,
    #[token("PRECEDING", ignore(ascii_case))]
    Preceding,
    #[token("FOLLOWING", ignore(ascii_case))]
    Following,
    #[token("CURRENT", ignore(ascii_case))]
    Current,
    #[token("FILTER", ignore(ascii_case))]
    Filter,
    #[token("EXCLUDE", ignore(ascii_case))]
    Exclude,
    #[token("TIES", ignore(ascii_case))]
    Ties,
    #[token("OTHERS", ignore(ascii_case))]
    Others,

    // --- Common Table Expressions (CTE) ---
    #[token("WITH", ignore(ascii_case))]
    With,
    #[token("RECURSIVE", ignore(ascii_case))]
    Recursive,
    #[token("MATERIALIZED", ignore(ascii_case))]
    Materialized,

    // --- Conflict Resolution ---
    #[token("ABORT", ignore(ascii_case))]
    Abort,
    #[token("FAIL", ignore(ascii_case))]
    Fail,
    #[token("IGNORE", ignore(ascii_case))]
    Ignore,
    #[token("CONFLICT", ignore(ascii_case))]
    Conflict,
    #[token("DO", ignore(ascii_case))]
    Do,
    #[token("NOTHING", ignore(ascii_case))]
    Nothing,

    // --- Generated Columns ---
    #[token("GENERATED", ignore(ascii_case))]
    Generated,
    #[token("ALWAYS", ignore(ascii_case))]
    Always,
    #[token("STORED", ignore(ascii_case))]
    Stored,

    // --- Database Management ---
    #[token("EXPLAIN", ignore(ascii_case))]
    Explain,
    #[token("QUERY", ignore(ascii_case))]
    Query,
    #[token("PLAN", ignore(ascii_case))]
    Plan,
    #[token("PRAGMA", ignore(ascii_case))]
    Pragma,
    #[token("ANALYZE", ignore(ascii_case))]
    Analyze,
    #[token("ATTACH", ignore(ascii_case))]
    Attach,
    #[token("DETACH", ignore(ascii_case))]
    Detach,
    #[token("DATABASE", ignore(ascii_case))]
    Database,
    #[token("VACUUM", ignore(ascii_case))]
    Vacuum,
    #[token("REINDEX", ignore(ascii_case))]
    Reindex,
    #[token("RETURNING", ignore(ascii_case))]
    Returning,

    // --- Table Options ---
    #[token("WITHOUT", ignore(ascii_case))]
    Without,

    // --- Miscellaneous ---
    #[token("TO", ignore(ascii_case))]
    To,
    #[token("WITHIN", ignore(ascii_case))]
    Within,

    // Identifiers
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,

    // Quoted identifiers: "identifier" (standard SQL)
    #[regex(r#""([^"]|"")*""#)]
    QuotedIdent,

    // Square bracket identifiers: [identifier] (MS Access/SQL Server compatibility)
    #[regex(r"\[[^\]]*\]")]
    BracketIdent,

    // Backtick identifiers: `identifier` (MySQL compatibility)
    #[regex(r"`[^`]*`")]
    BacktickIdent,

    // Literals
    #[regex(r"[0-9]+")]
    Integer,

    #[regex(r"[0-9]+\.[0-9]*|[0-9]*\.[0-9]+")]
    Float,

    // String literal: 'text' (SQLite uses single quotes)
    #[regex(r"'([^']|'')*'")]
    String,

    // Blob literal: X'AABBCCDD'
    #[regex(r"[xX]'[0-9a-fA-F]*'")]
    Blob,

    // Punctuation
    #[token(",")]
    Comma,

    #[token(";")]
    Semicolon,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token(".")]
    Dot,

    #[token("*")]
    Star,

    // Arithmetic operators
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    // Comparison operators
    #[token("<")]
    Lt,

    #[token(">")]
    Gt,

    #[token("<=")]
    Le,

    #[token(">=")]
    Ge,

    #[token("=")]
    Eq,

    #[token("==")]
    EqEq,

    #[token("<>")]
    Ne,

    #[token("!=")]
    BangEq,

    // Bitwise operators
    #[token("&")]
    Ampersand,

    #[token("|")]
    Pipe,

    #[token("~")]
    Tilde,

    #[token("<<")]
    LShift,

    #[token(">>")]
    RShift,

    // String concatenation
    #[token("||")]
    Concat,

    // JSON operators
    #[token("->")]
    Arrow,

    #[token("->>")]
    ArrowArrow,

    // Bind parameters
    #[regex(r"\?[0-9]*")]
    BindParam,

    #[regex(r":[a-zA-Z_][a-zA-Z0-9_]*")]
    BindParamColon,

    #[regex(r"@[a-zA-Z_][a-zA-Z0-9_]*")]
    BindParamAt,

    #[regex(r"\$[a-zA-Z_][a-zA-Z0-9_]*")]
    BindParamDollar,

    // Hexadecimal integer literal
    #[regex(r"0[xX][0-9a-fA-F]+")]
    HexInteger,

    // Block comments
    #[regex(r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/")]
    BlockComment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

pub fn lex(source: &str) -> Vec<Token> {
    let mut lexer = TokenKind::lexer(source);
    let mut tokens = Vec::new();

    while let Some(result) = lexer.next() {
        if let Ok(kind) = result {
            tokens.push(Token {
                kind,
                span: lexer.span(),
            });
        }
        // Skip errors (invalid tokens) for now
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_select_integer() {
        let tokens = lex("SELECT 1;");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Select);
        assert_eq!(tokens[1].kind, TokenKind::Integer);
        assert_eq!(tokens[2].kind, TokenKind::Semicolon);
    }

    #[test]
    fn test_lex_select_string() {
        let tokens = lex("SELECT 'hello';");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Select);
        assert_eq!(tokens[1].kind, TokenKind::String);
        assert_eq!(tokens[2].kind, TokenKind::Semicolon);
    }

    #[test]
    fn test_case_insensitive() {
        let tokens = lex("select null");
        assert_eq!(tokens[0].kind, TokenKind::Select);
        assert_eq!(tokens[1].kind, TokenKind::Null);
    }

    #[test]
    fn test_lex_operators() {
        let tokens = lex("1 + 2 - 3 * 4 / 5");
        assert_eq!(tokens[1].kind, TokenKind::Plus);
        assert_eq!(tokens[3].kind, TokenKind::Minus);
        assert_eq!(tokens[5].kind, TokenKind::Star);
        assert_eq!(tokens[7].kind, TokenKind::Slash);
    }

    #[test]
    fn test_lex_json_operators() {
        let tokens = lex("data -> 'key' ->> 'value'");
        assert_eq!(tokens[1].kind, TokenKind::Arrow);
        assert_eq!(tokens[3].kind, TokenKind::ArrowArrow);
    }

    #[test]
    fn test_lex_bind_params() {
        let tokens = lex("SELECT ?, :name, @var, $param");
        assert_eq!(tokens[1].kind, TokenKind::BindParam);
        assert_eq!(tokens[3].kind, TokenKind::BindParamColon);
        assert_eq!(tokens[5].kind, TokenKind::BindParamAt);
        assert_eq!(tokens[7].kind, TokenKind::BindParamDollar);
    }

    #[test]
    fn test_lex_comments() {
        let tokens = lex("SELECT -- comment\n1");
        assert_eq!(tokens[0].kind, TokenKind::Select);
        assert_eq!(tokens[1].kind, TokenKind::Comment);
        assert_eq!(tokens[2].kind, TokenKind::Integer);

        let tokens = lex("SELECT /* block */ 1");
        assert_eq!(tokens[0].kind, TokenKind::Select);
        assert_eq!(tokens[1].kind, TokenKind::BlockComment);
        assert_eq!(tokens[2].kind, TokenKind::Integer);
    }

    #[test]
    fn test_lex_asc_desc() {
        let tokens = lex("ORDER BY x ASC, y DESC");
        // ORDER(0) BY(1) x(2) ASC(3) ,(4) y(5) DESC(6)
        assert_eq!(tokens[3].kind, TokenKind::Asc);
        assert_eq!(tokens[6].kind, TokenKind::Desc);
    }

    #[test]
    fn test_lex_table_function() {
        let tokens = lex("generate_series(1, 10, 2)");
        assert_eq!(tokens[0].kind, TokenKind::Ident);
        assert_eq!(tokens[1].kind, TokenKind::LParen);
        assert_eq!(tokens[2].kind, TokenKind::Integer);
        assert_eq!(tokens[3].kind, TokenKind::Comma);
        assert_eq!(tokens[4].kind, TokenKind::Integer);
        assert_eq!(tokens[5].kind, TokenKind::Comma);
        assert_eq!(tokens[6].kind, TokenKind::Integer);
        assert_eq!(tokens[7].kind, TokenKind::RParen);
    }
}
