# solite-lexer

SQL and JSON lexer for the solite project. Built on the `logos` crate for SQL tokenization and a hand-written lexer for JSON tokenization.

## File Layout

- `src/lib.rs` -- SQL lexer: `TokenKind` enum, `Token` struct, `lex()` function, tests
- `src/json.rs` -- JSON lexer: `Kind` enum, `Token` struct, `tokenize()` function with key/value context tracking
- `src/snapshots/` -- insta snapshot tests for the JSON lexer

## SQL Lexer API

The primary entry point is `lex(source: &str) -> Vec<Token>`.

`Token` has two fields:
- `kind: TokenKind` -- what type of token this is
- `span: Range<usize>` -- byte offset range into the source string

Invalid tokens are silently skipped. Whitespace (`[ \t\n\r]+`) is skipped by logos and produces no tokens.

## TokenKind Enum

Derives `Logos`, `Debug`, `Clone`, `Copy`, `PartialEq`, `Serialize`, `Deserialize`.

### Keywords (case-insensitive)

All SQL keywords use `#[token("KEYWORD", ignore(ascii_case))]`. They are organized into groups:

- **DML**: `Select`, `Insert`, `Update`, `Delete`, `Replace`, `Into`, `Values`, `Set`, `From`
- **DDL**: `Create`, `Drop`, `Alter`, `Table`, `Index`, `View`, `Trigger`, `Virtual`, `Temp`, `Temporary`, `If`, `Add`, `Column`, `Rename`
- **TCL**: `Begin`, `Commit`, `Rollback`, `Savepoint`, `Release`, `Transaction`, `Deferred`, `Immediate`, `Exclusive`, `End`
- **Query Clauses**: `Where`, `Order`, `By`, `Group`, `Having`, `Limit`, `Offset`, `Distinct`, `All`, `As`, `Asc`, `Desc`, `Nulls`, `First`, `Last`, `Union`, `Intersect`, `Except`, `Indexed`
- **Joins**: `Join`, `Inner`, `Left`, `Right`, `Full`, `Outer`, `Cross`, `Natural`, `On`, `Using`
- **Logical/Comparison**: `And`, `Or`, `Not`, `In`, `Between`, `Like`, `Glob`, `Regexp`, `Match`, `Escape`, `Is`, `IsNull`, `NotNull`, `Exists`
- **Literals**: `Null`, `True`, `False`, `CurrentDate`, `CurrentTime`, `CurrentTimestamp`
- **Conditional**: `Case`, `When`, `Then`, `Else`, `Cast`
- **Constraints**: `Constraint`, `Primary`, `Key`, `Unique`, `Check`, `Default`, `Collate`, `Foreign`, `References`, `Autoincrement`
- **Foreign Key Actions**: `Cascade`, `Restrict`, `No`, `Action`, `Deferrable`, `Initially`
- **Triggers**: `Before`, `After`, `Instead`, `Of`, `For`, `Each`, `Row`, `Raise`
- **Window Functions**: `Over`, `Partition`, `Window`, `Rows`, `Range`, `Groups`, `Unbounded`, `Preceding`, `Following`, `Current`, `Filter`, `Exclude`, `Ties`, `Others`
- **CTE**: `With`, `Recursive`, `Materialized`
- **Conflict Resolution**: `Abort`, `Fail`, `Ignore`, `Conflict`, `Do`, `Nothing`
- **Generated Columns**: `Generated`, `Always`, `Stored`
- **Database Management**: `Explain`, `Query`, `Plan`, `Pragma`, `Analyze`, `Attach`, `Detach`, `Database`, `Vacuum`, `Reindex`, `Returning`
- **Misc**: `Without`, `To`, `Within`

### Identifiers

- `Ident` -- unquoted: `[a-zA-Z_][a-zA-Z0-9_]*`. Any word not matching a keyword falls through to this.
- `QuotedIdent` -- double-quoted: `"identifier"` (supports `""` escape)
- `BracketIdent` -- bracket-quoted: `[identifier]`
- `BacktickIdent` -- backtick-quoted: `` `identifier` ``

### Literals

- `Integer` -- `[0-9]+`
- `Float` -- `[0-9]+\.[0-9]*` or `[0-9]*\.[0-9]+`
- `HexInteger` -- `0x[0-9a-fA-F]+`
- `String` -- single-quoted: `'text'` (supports `''` escape, allows embedded newlines)
- `Blob` -- hex blob: `X'AABBCCDD'`

### Operators

- Arithmetic: `Plus`, `Minus`, `Slash`, `Percent`, `Star`
- Comparison: `Lt`, `Gt`, `Le`, `Ge`, `Eq`, `EqEq`, `Ne`, `BangEq`
- Bitwise: `Ampersand`, `Pipe`, `Tilde`, `LShift`, `RShift`
- String concatenation: `Concat` (`||`)
- JSON: `Arrow` (`->`), `ArrowArrow` (`->>`)

### Punctuation

`Comma`, `Semicolon`, `LParen`, `RParen`, `LBracket`, `RBracket`, `Dot`

### Bind Parameters

- `BindParam` -- `?` or `?NNN`
- `BindParamColon` -- `:name`
- `BindParamAt` -- `@name`
- `BindParamDollar` -- `$name`

### Comments

- `Comment` -- line comment: `--` to end of line
- `BlockComment` -- block comment: `/* ... */`

## Keyword vs Identifier Resolution

Logos matches keywords before the generic `Ident` regex. Because keywords use `#[token(...)]` (literal match) and identifiers use `#[regex(...)]`, logos gives priority to the longer or more specific match. A word like `SELECTED` will match as `Ident` since it does not exactly match any keyword token. All keyword matching is case-insensitive via `ignore(ascii_case)`.

## JSON Lexer (`json` module)

Separate hand-written lexer at `json::tokenize(src: &str) -> Vec<json::Token>`.

`json::Token` borrows from the source (`&'a str`) and includes:
- `kind: json::Kind` -- `LBrace`, `RBrace`, `LBracket`, `RBracket`, `Colon`, `Comma`, `String`, `Number`, `True`, `False`, `Null`, `Whitespace`, `Unknown`, `Eof`
- `start: usize`, `end: usize` -- byte offsets
- `string_context: Option<StringContext>` -- for `String` tokens, indicates `Key` vs `Value`
- `text: &'a str` -- the raw token text

The JSON lexer tracks a context stack to distinguish object keys from values. It handles escape sequences in strings and all JSON number formats (integer, decimal, exponential).
