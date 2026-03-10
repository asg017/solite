# solite-parser

A hand-written recursive descent parser for SQLite SQL. It takes a source string, lexes it via `solite-lexer`, and produces `solite_ast::Program` (a list of `Statement` AST nodes).

## Public API

The main entry point is the free function:

```rust
pub fn parse_program(source: &str) -> Result<Program, Vec<ParseError>>
```

This constructs a `Parser`, calls `parser.parse()`, and returns the result. On failure it returns all accumulated `ParseError`s (the parser recovers at semicolons to report multiple errors).

The `Parser` struct is also public:

```rust
let mut parser = Parser::new(source);
let program = parser.parse()?;
```

## ParseError

```rust
pub enum ParseError {
    UnexpectedToken { location: Location },
    Eof,
    Expected { expected: &'static str, found: Option<TokenKind>, location: Location },
    InvalidBlob { location: Location },
}
```

`Location` has `line`, `column` (1-indexed), and `offset` (byte offset). `ParseError::position()` returns the byte offset for all variants.

## Parser Structure

Everything lives in `src/lib.rs` (~5000+ lines, single file). The parser is a **recursive descent parser** with a **Pratt parser** for expressions.

### Token handling

- `Parser::new(source)` lexes via `solite_lexer::lex()`, builds a `DocCommentMap`, then filters out comment tokens.
- Navigation: `current()`, `current_kind()`, `peek_nth(n)`, `advance()`, `consume_if(kind)`, `expect(kind, label)`.
- `is_keyword_as_ident()` defines which keywords can be used as identifiers (SQLite allows most keywords unquoted as names).
- `ident_name()` handles dequoting of quoted/bracket/backtick identifiers.

### Statement parsing

`parse_statement()` dispatches on the leading keyword:

| Category | Keywords | Statements |
|----------|----------|------------|
| DML | `SELECT`, `INSERT`/`REPLACE`, `UPDATE`, `DELETE`, `WITH` | Select, Insert, Update, Delete |
| DDL | `CREATE`, `DROP`, `ALTER` | CreateTable, CreateIndex, CreateView, CreateTrigger, CreateVirtualTable, DropTable, DropIndex, DropView, DropTrigger, AlterTable |
| TCL | `BEGIN`, `COMMIT`/`END`, `ROLLBACK`, `SAVEPOINT`, `RELEASE` | Begin, Commit, Rollback, Savepoint, Release |
| Utility | `VACUUM`, `ANALYZE`, `REINDEX`, `ATTACH`, `DETACH`, `PRAGMA`, `EXPLAIN` | Vacuum, Analyze, Reindex, Attach, Detach, Pragma, Explain |

### Expression parsing

Uses a Pratt (binding power) parser in `parse_expr_bp(min_bp)`:
- `parse_atom()` handles literals, identifiers, function calls, `CAST`, `CASE`, subqueries, `EXISTS`, `RAISE`, parenthesized exprs.
- `parse_prefix_expr()` handles unary `NOT`, `-`, `+`, `~`.
- Infix/postfix operators: arithmetic, comparison, `AND`/`OR`, `IS [NOT] NULL`, `IN`, `BETWEEN`, `LIKE`/`GLOB`/`REGEXP`/`MATCH`, `COLLATE`, `ISNULL`/`NOTNULL`.
- `parse_function_call()` handles function args, `DISTINCT`, `ORDER BY`, `FILTER`, `OVER` (window specs).

### Error recovery

On parse error, `recover_to_semicolon()` advances to the next `;` so subsequent statements can still be parsed. Errors are collected and returned together.

## Doc Comments Module

`src/doc_comments.rs` handles SQLite documentation comments:

- `--!` prefix: table-level docs
- `---` prefix: column-level docs
- Tags like `@example`, `@source`, `@details` are parsed from comment content.

`build_doc_comment_map(tokens, source)` runs before comment filtering and produces a `DocCommentMap` keyed by byte position of the next non-comment token. During `CREATE TABLE` parsing, the parser looks up doc comments from this map and attaches them to table/column AST nodes.

### Exported from this module

- `DocComment` (re-exported as `ParserDocComment`): holds `description: String` and `tags: HashMap<String, Vec<String>>`.
- `DocCommentKind`: `Table`, `Column`, `None`.
- `DocCommentMap`: maps byte positions to `DocComment` for table-level and column-level docs.
- `build_doc_comment_map()`: constructs the map from raw tokens.

## AST Output

All AST types come from `solite-ast`. The parser produces `Program { statements: Vec<Statement> }`. Each `Statement` variant wraps a typed struct (e.g., `SelectStmt`, `CreateTableStmt`). Every AST node carries `Span` fields for source location tracking.

## File Layout

```
src/
  lib.rs          - Parser struct, ParseError, all statement/expression parsing, tests
  doc_comments.rs - Doc comment parsing (--! and ---), DocCommentMap, tests
Cargo.toml        - depends on solite-lexer, solite-ast, thiserror, ropey
```

## Dependencies

- `solite-lexer`: tokenization (`lex()`, `Token`, `TokenKind`)
- `solite-ast`: all AST node types
- `thiserror`: derive `Error` for `ParseError`
- `ropey`: efficient byte-offset-to-line/column conversion
- `insta` (dev): snapshot testing
