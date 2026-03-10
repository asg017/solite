# Solite — Agent Reference

Solite is a SQLite CLI and toolchain written in Rust. This document maps the full architecture so agents can navigate and modify the codebase without broad exploratory searches.

## Crate Map

```
crates/
├── solite-cli/          # CLI binary. Clap commands, REPL, test runner, codegen, TUI, Jupyter kernel
├── solite-core/         # Runtime engine. SQL execution, dot commands, procedures, SQLite bindings
├── solite-table/        # Table rendering for terminal, plain text, HTML
├── solite-lsp/          # Language Server Protocol implementation (tower-lsp)
├── solite-completion/   # Completion engine (shared by LSP and REPL)
├── solite-analyzer/     # Diagnostics, lint rules, symbol resolution, schema analysis
├── solite-schema/       # Schema providers (DDL, file, JSON), document parsing, introspection
├── solite-parser/       # SQL parser → AST
├── solite-lexer/        # SQL tokenizer (logos-based)
├── solite-ast/          # AST node types and spans
├── solite-fmt/          # SQL formatter
├── solite-fmt-wasm/     # WASM build of formatter
├── solite-stdlib/       # SQLite extension initialization (C bindings)
└── solite-mdtest/       # Markdown-based test framework
```

## Core Execution Model

### Runtime (`solite-core/src/lib.rs`)

```rust
pub struct Runtime {
    pub connection: Connection,
    stack: Vec<Block>,               // execution stack of SQL blocks
    procedures: HashMap<String, Procedure>,
    loaded_files: HashSet<String>,
    virtual_files: HashMap<String, String>,
    running_files: Vec<String>,      // cycle detection for .run
    initialized_sqlite_parameters_table: bool,
}
```

**Key methods:**
- `next_stepx() -> Option<Result<Step, StepError>>` — main stepping loop
- `enqueue(name, code, source)` — push a block onto the execution stack
- `prepare_with_parameters(sql) -> Result<(Option<usize>, Option<Statement>), SQLiteError>`
- `execute_to_completion()` — run all remaining steps
- `load_file(path)` — load and execute a .sql file
- `register_procedure() / get_procedure()`
- `define_parameter() / lookup_parameter() / delete_parameter()` — stored in `temp.sqlite_parameters`
- `run_file_begin() / run_file_end()` — `.run` command context with parameter scoping

### Step & StepResult

```rust
pub struct Step {
    pub preamble: Option<String>,   // leading comments
    pub epilogue: Option<String>,   // trailing same-line comment
    pub result: StepResult,
    pub reference: StepReference,   // file:line:col + region context
}

pub enum StepResult {
    SqlStatement { stmt: Statement, raw_sql: String },
    DotCommand(DotCommand),
    ProcedureDefinition(Procedure),
}

pub enum StepError {
    Prepare { file_name, src, offset, error: SQLiteError },
    ParseDot(ParseDotError),
}
```

### Processing flow in `next_stepx()`

1. `advance_through_ignorable()` — skips whitespace, comments, `#region` directives (preamble)
2. Detect command type: `.` → dot command, `!` → shell, `?` → ask, otherwise SQL
3. For SQL: `prepare_with_parameters()` returns `(remaining_offset, Statement)`
4. Check preamble for `-- name: procName :annotation` → procedure definition
5. `extract_epilogue()` — trailing same-line comment
6. Return `Step` with appropriate `StepResult`

### 7 Match Sites for StepResult

Every consumer of `next_stepx()` pattern-matches on `StepResult`. When adding a new variant, update all:

| # | File | Context |
|---|------|---------|
| 1 | `solite-cli/src/commands/run/mod.rs` | Script execution with timing/tracing |
| 2 | `solite-cli/src/commands/repl/mod.rs` | Interactive REPL |
| 3 | `solite-cli/src/commands/codegen/report.rs` | Code generation from procedures |
| 4 | `solite-cli/src/commands/jupyter/kernel.rs` | Jupyter notebook execution |
| 5 | `solite-cli/src/commands/test/mod.rs` | Test assertions and snapshots |
| 6 | `solite-cli/src/commands/test/snap/file.rs` | Snapshot file generation |
| 7 | `solite-core/src/lib.rs` (`execute_to_completion`) | Fallback runner |

### DotCommand Match Sites

Dot commands are handled differently per context. When adding a new dot command variant, update:

| File | Context |
|------|---------|
| `solite-cli/src/commands/run/dot.rs` | Script mode (most commands supported) |
| `solite-cli/src/commands/repl/mod.rs` | REPL mode (all commands) |
| `solite-cli/src/commands/jupyter/handlers.rs` | Jupyter (HTML/markdown output) |
| `solite-cli/src/commands/test/mod.rs` | Test mode (only Load, Parameter, Call, Run) |
| `solite-cli/src/commands/test/snap/file.rs` | Snapshot mode |

## Dot Commands

Defined in `solite-core/src/dot/mod.rs`. Each variant has its own file in `solite-core/src/dot/`.

```rust
pub enum DotCommand {
    // Introspection
    Tables(TablesCommand),         // .tables [schema]
    Schema(SchemaCommand),         // .schema
    Graphviz(GraphvizCommand),     // .graphviz / .gv

    // Runtime
    Open(OpenCommand),             // .open <path>
    Load(LoadCommand),             // .load <path>
    Tui(TuiCommand),               // .tui
    Clear(ClearCommand),           // .clear / .c

    // Output
    Print(PrintCommand),           // .print <msg>
    Ask(AskCommand),               // .ask <question>  (also ? shorthand)
    Shell(ShellCommand),           // .sh <cmd>  (also ! shorthand)

    // Configuration
    Parameter(ParameterCommand),   // .param set/unset/list/clear
    Env(EnvCommand),               // .env set/unset
    Timer(bool),                   // .timer on/off
    Dotenv(DotenvCommand),         // .dotenv

    // Multi-line (use rest_length to consume trailing SQL)
    Export(ExportCommand),         // .export <path>\n<query>
    Vegalite(VegaLiteCommand),     // .vegalite <mark>\n<query>
    Bench(BenchCommand),           // .bench\n<query>

    // Procedures & execution
    Call(CallCommand),             // .call [file.sql] procName
    Run(RunCommand),               // .run file.sql [procName] [--key=val]

    // Feature-gated
    Stream(StreamCommand),         // .stream sync/restore <url>  (ritestream feature)
}
```

**Multi-line commands**: Export, Vegalite, Bench have a `rest_length: usize` field. After `parse_dot()`, `next_stepx()` advances the block offset by `rest_length` to skip the consumed SQL body.

**Parsing**: `parse_dot(command, args, rest, runtime) -> Result<DotCommand, ParseDotError>` in `dot/mod.rs`.

## Procedure System

Procedures are named SQL blocks defined by comment annotations:

```sql
-- name: getUserById :row
SELECT * FROM users WHERE id = $id;
```

**Types:**
```rust
pub struct Procedure { name, sql, result_type: ResultType, parameters: Vec<ProcedureParam>, columns: Vec<ColumnMeta> }
pub struct ProcedureParam { full_name, name, annotated_type: Option<String> }
pub enum ResultType { Void, Rows, Row, Value, List }
```

**Result type annotations:** `:rows`, `:row`, `:value`, `:list`, or auto-detected from column count.

**Parameter syntax:** `$name`, `:name`, `$name::type`, `:name::type`

**Detection:** In `next_stepx()`, after `prepare_with_parameters()` succeeds, the preamble is checked for `-- name:` lines. Procedures are registered in `Runtime.procedures`.

**Invocation:** `.call procedureName` or `.call file.sql procedureName`

## SQLite Bindings (`solite-core/src/sqlite.rs`)

```rust
pub struct Connection { /* wraps *mut sqlite3 */ }
pub struct Statement { /* wraps *mut sqlite3_stmt */ }
pub enum OwnedValue { Null, Integer(i64), Double(f64), Text(Vec<u8>), Blob(Vec<u8>) }
```

**Statement methods:** `sql()`, `column_names()`, `column_meta()`, `next()` (returns `Vec<ValueRefX>`), `nextx()` (returns `Row`), `execute()` (returns `usize`), `bind_*()`, `parameter_info()`.

## CLI Commands (`solite-cli/src/cli.rs`)

```rust
pub enum Commands {
    Run(RunArgs),        Repl(ReplArgs),       Query(QueryArgs),
    Execute(ExecuteArgs), Test(TestArgs),       Jupyter(JupyterNamespace),
    Docs(DocsNamespace), Bench(BenchArgs),     Codegen(CodegenArgs),
    Tui(TuiArgs),        Format(FmtArgs),      Lint(LintArgs),
    Lsp(LspArgs),        Sqlite3(Sqlite3Args), Diff(DiffArgs),
    Rsync(RsyncArgs),    Schema(SchemaArgs),   Backup(BackupArgs),
    Vacuum(VacuumArgs),  Stream(StreamNamespace),
}
```

Entry point: `solite-cli/src/main.rs`

## AST (`solite-ast/src/lib.rs`)

**Statement enum variants:**
- DML: `Select`, `Insert`, `Update`, `Delete`
- DDL: `CreateTable`, `CreateIndex`, `CreateView`, `CreateTrigger`, `CreateVirtualTable`, `AlterTable`, `Drop` (table/index/view/trigger)
- TCL: `Begin`, `Commit`, `Rollback`, `Savepoint`, `Release`
- Admin: `Explain`, `Vacuum`, `Analyze`, `Reindex`, `Attach`, `Detach`, `Pragma`

**Key structs:**
- `CreateTableStmt` — has `as_select: Option<Box<SelectStmt>>` for `CREATE TABLE ... AS SELECT`
- `SelectStmt` — with, columns, from, where, group_by, having, order_by, limit
- `Expr` — 30+ variants (literals, binary ops, functions, CASE, IN, subqueries, JSON ops)
- `TableOrSubquery` — Table, TableFunction, Subquery, Join, TableList
- `ResultColumn` — Expr (with alias), Star, TableStar

All nodes carry `span: Span` for source mapping.

## Diagnostic Pipeline

### Three diagnostic sources → LSP

1. **Parser errors** (`solite-parser`): `ParseError` enum → `UnexpectedToken`, `Eof`, `Expected`, `InvalidBlob`
2. **Semantic analysis** (`solite-analyzer/src/lib.rs`): `analyze_with_schema(program, external_schema) -> Vec<Diagnostic>`
3. **Lint rules** (`solite-analyzer/src/rules/`): `lint_with_config(program, config) -> Vec<LintDiagnostic>`

All converted to LSP `Diagnostic` in `solite-lsp/src/server.rs` → `compute_diagnostics_for_document()`.

### Analyzer (`solite-analyzer/src/lib.rs`)

`analyze_with_schema()` handles these Statement variants:
- **CreateTable**: validates non-empty columns (unless `as_select`), duplicate columns, registers table
- **CreateVirtualTable**: registers with empty columns
- **Select**: `analyze_select()` validates column/table references against known schema

Other statement types are currently not analyzed.

### Lint Rules (`solite-analyzer/src/rules/`)

| File | Rule ID | Checks | Fixable |
|------|---------|--------|---------|
| `double_quoted.rs` | `double-quoted-string` | `"string"` used as identifier | Yes |
| `empty_blob.rs` | `empty-blob-literal` | `X''` empty blob | No |
| `missing_as.rs` | `missing-as` | Aliases without AS keyword | Yes |

Rules implement `LintRule` trait with `check_expr()`, `check_stmt()`, and optional `fix()`.

### Symbol Resolution (`solite-analyzer/src/symbols.rs`)

For hover and goto-definition:
- `find_symbol_at_offset(program, offset) -> Option<ResolvedSymbol>`
- `ResolvedSymbol` enum: `TableAlias`, `Column`, `Table`, `ColumnAlias`, `Cte`
- `format_hover_content()` generates markdown hover text
- `get_definition_span()` returns source span for goto-definition

## LSP (`solite-lsp/src/server.rs`)

**Protocol methods:** initialize, didOpen, didChange, didClose, completion, hover, gotoDefinition, semanticTokensFull, codeAction, formatting, rangeFormatting, inlayHint

**Completions** (`solite-lsp/src/completions.rs`): delegates to `solite-completion` engine

**Completion context** (`solite-completion/src/context.rs`): Token-based state machine producing `CompletionContext` enum (30+ variants: `StatementStart`, `AfterFrom`, `AfterJoin`, `SelectColumns`, `WhereClause`, `InsertColumns`, `UpdateSet`, `QualifiedColumn`, etc.)

**Schema management:** Tracks schemas per-document, discovers virtual table columns from `solite-stdlib`, supports `-- schema: <path>` annotations.

**Semantic tokens:** 7 types — KEYWORD, VARIABLE, NUMBER, STRING, COMMENT, OPERATOR, TYPE

## Schema System (`solite-schema/`)

**Providers** (implement `SchemaProvider` trait):
- `DdlSchemaProvider` — from parsed DDL SQL
- `FileSchemaProvider` — introspects SQLite database files
- `JsonSchemaProvider` — from JSON

**Schema contents:** tables (columns, WITHOUT ROWID, docs), indexes, views, triggers, functions

**Virtual table discovery:** `discover_virtual_table_columns()` in `introspect.rs`

## Formatter (`solite-fmt/`)

**API:** `format_sql()`, `format_document()`, `check_formatted()`

**Config:** keyword_case (Upper/Lower/Preserve), indent_style (Spaces/Tab), comma_position (Trailing/Leading), logical_operator_position, statement_separator_lines

**Architecture:** Parse → AST → Printer with FormatConfig → output. Comments preserved. `-- solite-fmt: ignore` directives supported.

## Test Framework (`solite-cli/src/commands/test/`)

Test files use inline assertions in SQL comments:

```sql
SELECT COUNT(*) FROM users; -- 5
SELECT * FROM bad;          -- error: no such table
SELECT name FROM users;     -- @snap user-names
SELECT * FROM empty;        -- [no results]
SELECT 1;                   -- TODO: implement later
```

**Snapshots:** Stored in `__snapshots__/<test>-<name>.snap`. Three modes: Default (CI, fail on mismatch), Update (`--update`, auto-accept), Review (`--review`, interactive).

Uses `insta` crate for snapshot management.

## Table Rendering (`solite-table/src/lib.rs`)

```rust
pub struct TableConfig {
    head_rows: usize,          // default 20
    tail_rows: usize,          // default 20
    max_width: Option<usize>,  // terminal width
    max_cell_width: usize,
    output_mode: OutputMode,   // Terminal, StringAnsi, StringPlain, Html
    theme: Option<Theme>,
    show_footer: bool,
}
```

Features: column collapsing when too wide, row truncation with ellipsis for large results, streaming (doesn't load all rows into memory), box-drawing borders.

## TUI (`solite-cli/src/commands/tui/`)

Pages: ListingPage (tables list) → TablePage (browse data with pagination) → RowPage (single row detail). Uses ratatui. Catppuccin Mocha theme.

## Common Pitfalls

- **Match arm types**: All StepResult arms must return compatible types (e.g. `stmt.execute()` returns `usize`, not `()`)
- **advance_through_ignorable()**: Skips ALL comments including `-- name:` lines — they become part of preamble, not separate steps
- **Multi-line dot commands**: Must set `rest_length` correctly or subsequent SQL will be re-processed
- **open_command.execute()**: Returns `Result` — check callers when signature changes
- **Stack insertion**: After processing SQL, remaining blocks insert at position 0 (not appended)
- **Cycle detection**: `.run` uses `running_files` Vec to detect recursive file inclusion
- **Feature flag**: `ritestream` gates StreamCommand and related CLI commands
- **CREATE TABLE ... AS SELECT**: The AST supports it (`as_select` field) — analyzer must check `as_select.is_none()` before requiring columns
