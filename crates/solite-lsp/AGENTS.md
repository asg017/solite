# solite-lsp

SQL Language Server Protocol implementation for Solite. Provides real-time diagnostics, completions, semantic highlighting, hover, go-to-definition, inlay hints, code actions, and formatting for `.sql` files and Jupyter notebook cells.

## File Layout

```
src/
  lib.rs              Public API re-exports (run_server, completions, context types)
  server.rs           Backend struct, LanguageServer trait impl, diagnostics, semantic tokens
  completions.rs      Completion item generation from CompletionContext + Schema
  context.rs          Re-exports from solite_completion (detect_context, CompletionContext, etc.)
  inlay_hints.rs      INSERT VALUES column name hints (AST-based and token-based)
  tests/
    mod.rs            Shared test helpers (build_test_schema, get_completions_at_end)
    completions.rs    DDL/DML/keyword/alias/rowid completion tests
    autocomplete.rs   Placeholder-based smart autocomplete test framework
    semantic_tokens.rs Snapshot tests for syntax highlighting (insta)
    hover.rs          Hover information and goto-definition tests
    cte.rs            CTE completion tests
    lsp_integration.rs LSP integration tests
    snapshots/        insta snapshot files
```

## LSP Protocol Methods

The `Backend` struct implements `LanguageServer` with these methods:

| Method | Description |
|--------|-------------|
| `initialize` | Declares capabilities (full sync, semantic tokens, completions, code actions, formatting, hover, definition, inlay hints) |
| `did_open` / `did_change` / `did_close` | Document lifecycle; triggers `on_change` which rebuilds schema and publishes diagnostics |
| `completion` | Context-aware SQL completions (tables, columns, keywords, functions, suppression directives) |
| `semantic_tokens_full` | Full-document semantic token highlighting |
| `code_action` | Quick fixes from lint results (overlapping range match) |
| `formatting` / `range_formatting` | Document formatting via `solite_fmt` (range formatting delegates to full) |
| `hover` | Symbol info at cursor (table/column/function details from schema) |
| `goto_definition` | Jump to definition within the same document |
| `inlay_hint` | Column name hints on INSERT VALUES expressions |

Completion triggers: space, comma, dot, newline.

## Diagnostics: `compute_diagnostics_for_document`

Three diagnostic sources, produced from the parsed `Document`:

1. **Parse errors** -- When `doc.program` is `Err(Vec<ParseError>)`, each error becomes a diagnostic at its position. No further analysis runs.

2. **Lint diagnostics** -- `lint_with_config(program, sql_source, &config, external_schema)` runs configurable lint rules (reads `solite-lint.toml`). Results include optional `Fix` with replacement text, stored in `self.lint_results` for code actions. Severity maps from `RuleSeverity` (Error/Warning/Off).

3. **Semantic analysis** -- `analyze_with_schema(program, external_schema)` catches issues like unknown tables/columns. These use `Severity` (Error/Warning), no fix data.

Additionally, `.open` and `-- schema:` directive failures produce WARNING diagnostics prepended to the list (in `on_change`, not in `compute_diagnostics_for_document`).

All spans are mapped from joined SQL text (with dot command lines stripped) back to original source coordinates via `map_span_to_source`.

## Completion System

Two-layer architecture:

1. **Context detection** (`context.rs` / `solite_completion`): `detect_context(text, offset)` returns a `CompletionContext` enum variant describing what the cursor is positioned after (e.g., `AfterFrom`, `SelectColumns`, `InsertColumns`, `QualifiedColumn`, `StatementStart`, etc.).

2. **Item generation** (`completions.rs`): `get_completions_extended(ctx, schema, options)` matches on the context to produce `CompletionItem` lists. Key behaviors:
   - **Table contexts** (`AfterFrom`, `AfterJoin`, `AfterInto`, etc.): suggests tables from schema + CTE names
   - **Column contexts** (`SelectColumns`, `WhereClause`, `JoinOn`, etc.): resolves columns from in-scope tables; ambiguous columns get qualified names (`t.id`); handles CTE column resolution
   - **SELECT without FROM**: suggests all columns with snippet that auto-inserts `FROM table`
   - **INSERT columns**: filters out already-used columns via `extract_used_insert_columns`
   - **Qualified columns** (`t.`): resolves qualifier to table or CTE, suggests its columns
   - **Statement start**: SQL keywords with documentation, filtered by typed prefix
   - **Functions**: only suggested when the user has typed at least one character (non-empty prefix)
   - **Suppression directives**: `-- solite-ignore:` triggers lint rule ID completions

`CompletionOptions` controls: `document_text` + `cursor_offset` (for column filtering), `include_documentation` (keyword docs), `prefix` (function gating).

## Semantic Tokens

Seven token types, order-dependent (index = protocol ID):

| Index | Type | Mapped from |
|-------|------|-------------|
| 0 | KEYWORD | All SQL keywords (100+ TokenKind variants) |
| 1 | VARIABLE | Identifiers, quoted identifiers, bind parameters |
| 2 | NUMBER | Integer, Float, HexInteger |
| 3 | STRING | String literals, Blob |
| 4 | COMMENT | Line comments, block comments |
| 5 | OPERATOR | Punctuation, arithmetic, comparison, JSON arrows |
| 6 | TYPE | Identifiers in type positions only |

Type position detection uses a `TypeContext` state machine that tracks:
- `AfterCreateTable` -> `ExpectColumnName` -> `ExpectColumnType` (identifiers become TYPE)
- `InCastExpr` -> `ExpectCastType` (after `CAST(... AS`)
- `ExpectAlterColumnName` -> `ExpectAlterColumnType` (after `ALTER TABLE t ADD`)
- `InsideTypeParen` for `VARCHAR(255)` etc.
- `InGeneratedExpr` / `InConstraintExpr` to avoid coloring expression identifiers as types

Multiline tokens (block comments) are split into per-line semantic tokens.

## Schema Management

Schema is layered per-document:

1. **Built-in schema** (`builtin_schema`): Discovered once at startup by opening an in-memory SQLite connection with `solite_stdlib` loaded. Introspects virtual tables (`generate_series`, `json_each`, etc.) and all available functions with argument counts.

2. **Document DDL schema** (`schemas` map): Built from `CREATE TABLE/VIEW/INDEX` statements parsed in the document via `build_schema(program)`.

3. **External schema** (`open_schemas` map): Loaded from `.open <path>` dot commands and `-- schema: <path>` hints. `.sql` files are parsed as DDL; other files are opened as SQLite databases and introspected.

4. **Notebook support**: Separate tracking for notebook cells:
   - `notebook_cells`: `notebook_path -> (cell_uri -> content)` -- all cell contents
   - `notebook_schemas`: DDL schema built from all cells combined (`build_combined_schema`)
   - `notebook_open_schemas`: External schemas from `.open` / `-- schema:` in any cell
   - Cell URIs use `vscode-notebook-cell:` scheme; the notebook path is extracted from the URI

Schemas are merged with `schema.merge()` where user tables override builtins. The `schema_with_builtins` method always layers the built-in schema as the base.

## Inlay Hints

Shows column names before each value expression in `INSERT INTO t(a, b, c) VALUES ([a] 1, [b] 2, [c] 3)`.

Two implementations:
- **AST-based** (`get_inlay_hints`): Works on a successfully parsed `Program`. Walks `InsertStmt` nodes with explicit columns and `Values` source.
- **Token-based** (`get_inlay_hints_from_tokens`): Fault-tolerant; works on incomplete SQL by scanning tokens directly. Handles `OR REPLACE`, `schema.table`, nested parens in function calls, multiple VALUE rows.

The LSP server uses the token-based approach with `get_inlay_hints_from_tokens_filtered(text, edit_offset)`, which only returns hints for the INSERT statement containing the last edit position (tracked via `last_edit_offset` map, computed by `find_first_difference` between old and new text).

## State Management

The `Backend` struct holds all state behind `RwLock<HashMap<...>>`:

| Field | Key | Value | Purpose |
|-------|-----|-------|---------|
| `documents` | URI | String | Full text of every open document |
| `schemas` | URI | Schema | DDL schema parsed from document |
| `open_schemas` | URI | Schema | External schema from `.open` / `-- schema:` |
| `lint_results` | URI | Vec\<LintResult\> | Stored for code action quick fixes |
| `last_edit_offset` | URI | usize | Last edit byte offset for contextual inlay hints |
| `notebook_cells` | notebook path | HashMap\<URI, String\> | Cell contents per notebook |
| `notebook_schemas` | notebook path | Schema | Combined DDL from all cells |
| `notebook_open_schemas` | notebook path | Schema | External schemas from all cells |
| `builtin_schema` | (single) | Schema | Built-in vtabs + functions (not behind lock) |

The `on_change` method is the central update path. For regular files it: parses `Document`, processes `.open` / `-- schema:` directives, builds DDL schema, computes diagnostics, stores everything, and publishes diagnostics. For notebook cells it additionally rebuilds the combined notebook schema and re-publishes diagnostics for all cells in that notebook.

## Test Structure

Tests use shared helpers from `tests/mod.rs`:
- `build_test_schema(sql)` -- parse SQL and build a `Schema`
- `get_completions_at_end(sql, schema)` -- detect context at end of string and get completions
- `get_completions_with_text(sql, schema)` -- same but with document context for column filtering
- `extract_prefix(sql, offset)` -- get the partial word at cursor

Semantic token tests use `insta` snapshots. The `autocomplete` module has a placeholder-based framework for testing completions at arbitrary cursor positions.
