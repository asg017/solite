# solite-completion

Shared, format-agnostic SQL completion engine used by both the REPL (`solite-cli`) and the LSP server (`solite-lsp`).

## What It Does

Given a SQL string and a cursor offset, detects the syntactic context and returns a list of completion items (tables, columns, keywords, functions, operators). It works on incomplete/invalid SQL by operating on raw tokens from `solite-lexer` rather than a parsed AST.

## Public API (lib.rs)

Entry points re-exported from `lib.rs`:

- `detect_context(source, cursor_offset) -> CompletionContext` -- tokenizes and runs the state machine
- `detect_context_from_tokens(tokens, source, cursor_offset) -> CompletionContext` -- same but accepts pre-lexed tokens
- `get_completions(ctx, schema, prefix) -> Vec<CompletionItem>` -- generates completions for a context
- `extract_used_insert_columns(source, cursor_offset) -> HashSet<String>` -- columns already listed in an INSERT column list
- `extract_used_select_columns(source, cursor_offset) -> HashSet<String>` -- columns already listed in a SELECT list

Types: `CompletionContext`, `CompletionItem`, `CompletionKind`, `SchemaSource`, `TableRef`, `CteRef`.

## CompletionContext Enum (context.rs)

Every variant represents a distinct syntactic position. The engine matches on these to decide what to suggest.

### Table name contexts
- `AfterFrom { ctes }` -- after FROM; suggests tables and CTEs
- `AfterFromTable { ctes }` -- after a table name in FROM; suggests JOIN keywords, WHERE, more tables
- `AfterJoin { ctes }` -- after JOIN; suggests tables and CTEs
- `AfterJoinTable { ctes }` -- after a table in JOIN; suggests ON, AS
- `AfterInto` -- after INSERT INTO; suggests tables
- `AfterUpdate` -- after UPDATE; suggests tables
- `AfterTable` -- after DROP TABLE / ALTER TABLE; suggests tables
- `AfterOn` -- after ON in CREATE INDEX; suggests tables
- `AfterIndex` -- after DROP INDEX; suggests index names
- `AfterView` -- after DROP VIEW; suggests view names

### Column name contexts
- `SelectColumns { tables, ctes }` -- in SELECT list
- `InsertColumns { table_name }` -- inside INSERT INTO t(...)
- `UpdateSet { table_name }` -- after UPDATE t SET
- `WhereClause { tables, ctes }` -- in WHERE
- `JoinOn { left_tables, right_table, ctes }` -- in JOIN ... ON
- `GroupByClause { tables, ctes }` -- in GROUP BY
- `HavingClause { tables, ctes }` -- in HAVING
- `OrderByClause { tables, ctes }` -- in ORDER BY
- `DeleteWhere { table_name }` -- in DELETE FROM ... WHERE
- `CreateIndexColumns { table_name }` -- inside CREATE INDEX ... ON t(...)
- `QualifiedColumn { qualifier, tables, ctes }` -- after `alias.` or `table.`; suggests columns from that qualifier only

### ALTER TABLE contexts
- `AlterTableAction { table_name }` -- after ALTER TABLE name; suggests ADD, DROP COLUMN, RENAME
- `AlterColumn { table_name }` -- after ALTER TABLE name DROP COLUMN; suggests columns (excludes implicit rowid)

### Keyword-only contexts
- `StatementStart { prefix }` -- start of statement; suggests top-level keywords filtered by prefix
- `AfterCreate` -- suggests TABLE, INDEX, VIEW, TRIGGER, VIRTUAL TABLE
- `AfterCreateTable` -- suggests IF NOT EXISTS
- `CreateTableColumnConstraint` -- after column type in CREATE TABLE; suggests constraints
- `AfterInsert` -- suggests INTO, OR ABORT/FAIL/IGNORE/REPLACE/ROLLBACK
- `AfterReplace` -- suggests INTO
- `AfterDrop` -- suggests TABLE, INDEX, VIEW, TRIGGER
- `AfterAlter` -- suggests TABLE

### Expression / operator contexts
- `Expression { tables }` -- general expression; suggests columns and functions
- `AfterExpr { tables, ctes }` -- after an expression; suggests operators (AND, OR, =, ->, ->>, LIKE, IN, etc.) and clause keywords

### Null
- `None` -- no context detected; returns empty completions

## Context Detection (context.rs)

`detect_context` / `detect_context_from_tokens` runs a state machine (`ContextState` enum, ~50 states) over tokens up to `cursor_offset`. Key behaviors:

- Tracks CTE definitions from WITH clauses, extracting explicit column lists and inferring columns from SELECT expressions (including `SELECT *` via `star_sources` for lazy schema resolution).
- Tracks `TableRef` entries (name + optional alias) as tables come into scope via FROM/JOIN.
- Detects `qualifier.` patterns (e.g., `u.`) to trigger `QualifiedColumn` context.
- Handles nested parentheses (subqueries, function calls) with a depth counter.
- Works on incomplete SQL -- the state machine simply stops at the cursor position and maps its final state to a `CompletionContext`.

## Completion Engine (engine.rs)

`get_completions(ctx, schema, prefix)` matches on the `CompletionContext` and returns `Vec<CompletionItem>`.

Key logic:
- **Table contexts**: returns table names from schema, plus CTE names where applicable.
- **Column contexts**: calls `suggest_columns_from_tables()` which handles ambiguity -- if a column name exists in multiple in-scope tables, it suggests qualified names (`u.id`, `o.id`); otherwise suggests bare column names.
- **Functions**: only suggested when `prefix` is non-empty (at least one character typed). Uses `SchemaSource::function_names()` and `function_nargs()`. Zero-arg functions get `()` appended; others get `(`.
- **Identifier quoting**: `quote_identifier_if_needed()` wraps names containing spaces, hyphens, dots, or starting with a digit in double quotes.

## SchemaSource Trait (schema.rs)

Abstraction over database metadata. Required methods:

```rust
fn table_names(&self) -> Vec<String>;
fn columns_for_table(&self, table: &str) -> Option<Vec<String>>;
fn index_names(&self) -> Vec<String>;
fn view_names(&self) -> Vec<String>;
```

Methods with defaults:
- `columns_for_table_with_rowid()` -- defaults to `columns_for_table()`, overridable to include implicit rowid
- `has_table()` -- case-insensitive check against `table_names()`
- `function_names()` -- defaults to empty
- `function_nargs()` -- defaults to None

Feature-gated impl: with the `analyzer` feature, `solite_analyzer::Schema` implements `SchemaSource`.

## CompletionItem and CompletionKind (items.rs)

`CompletionItem` fields: `label`, `insert_text` (optional, overrides label on accept), `kind`, `detail`, `sort_order`.

`CompletionKind` variants: `Keyword`, `Table`, `Column`, `Index`, `View`, `Function`, `Operator`, `Cte`.

Builder methods: `.with_insert_text()`, `.with_detail()`, `.with_sort_order()`.

## Integration

### REPL (`solite-cli/src/commands/repl/completer.rs`)
Implements `rustyline::Completer`. Creates a `LiveSchemaSource` that queries the live SQLite connection via `PRAGMA table_info`, `sqlite_master`, etc. Calls `detect_context` + `get_completions`, then converts `CompletionItem` to rustyline `Pair`.

### LSP (`solite-lsp/src/context.rs`)
Re-exports all public types from `solite-completion`. Uses the `analyzer` feature so `solite_analyzer::Schema` implements `SchemaSource` directly -- no live database connection needed.

## Dependencies

- `solite-lexer` (required) -- tokenization
- `solite-analyzer` (optional, behind `analyzer` feature) -- static schema analysis
