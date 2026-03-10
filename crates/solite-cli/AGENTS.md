# solite-cli

The CLI frontend for Solite. Parses commands via `clap`, dispatches to command modules, and provides all user-facing functionality: script execution, REPL, testing, code generation, Jupyter kernel, TUI, formatting, linting, LSP, benchmarks, and SQLite utility wrappers.

## Entry Point

`main.rs` parses args with `clap`. If no subcommand is given, it launches the REPL. If the first arg is a `.db` file that isn't a valid subcommand, it launches the REPL connected to that database. Otherwise it dispatches to the matching command module.

## CLI Command Structure (`cli.rs`)

The `Commands` enum defines all subcommands:

| Command | Alias | Description | Module |
|---------|-------|-------------|--------|
| `run` | | Run SQL scripts (`.sql`, `.ipynb`) | `commands/run/` |
| `repl` | | Interactive REPL | `commands/repl/` |
| `query` | `q` | Read-only query with structured output | `commands/query.rs` |
| `execute` | `exec` | Write SQL statement | `commands/exec.rs` |
| `test` | | Inline assertion and snapshot testing | `commands/test/` |
| `jupyter` | | Jupyter kernel management | `commands/jupyter/` |
| `docs` | | Extension documentation tooling | `commands/docs/` |
| `bench` | | SQL benchmarking | `commands/bench/` |
| `codegen` | | SQL-to-IR code generation | `commands/codegen/` |
| `tui` | | Terminal database browser | `commands/tui/` |
| `format` | `fmt` | SQL formatter | `commands/fmt.rs` |
| `lint` | | SQL linter | `commands/lint.rs` |
| `lsp` | | Language Server Protocol server | `commands/lsp.rs` |
| `sqlite3` | | Pass-through to sqlite3 shell | `commands/sqlite3.rs` |
| `diff` | | Pass-through to sqldiff | `commands/diff.rs` |
| `rsync` | | Pass-through to sqlite3_rsync | `commands/rsync.rs` |
| `schema` | | Print database schema | `commands/schema.rs` |
| `backup` | | SQLite backup API wrapper | `commands/backup.rs` |
| `vacuum` | | VACUUM / VACUUM INTO | `commands/vacuum.rs` |
| `stream` | | Streaming replication (feature-gated: `ritestream`) | `commands/stream.rs` |

## File Layout

```
src/
  main.rs              -- Entry point, clap dispatch
  cli.rs               -- All Args/Commands structs
  colors.rs            -- Terminal color helpers
  errors.rs            -- codespan-reporting error display
  themes/mod.rs        -- Catppuccin Mocha color constants
  commands/
    mod.rs             -- Module declarations
    run/
      mod.rs           -- Script execution (SQL files, ipynb notebooks, stdin, -c flag)
      dot.rs           -- Dot command handler for run mode
      sql.rs           -- SQL statement handler (table output, progress, tracing)
      format.rs        -- Duration formatting
      status.rs        -- Progress bar for long-running statements
    repl/
      mod.rs           -- REPL loop (rustyline), dot command handling, \e editor
      completer.rs     -- Tab completion (tables, columns, SQL keywords)
      highlighter.rs   -- SQL syntax highlighting
    test/
      mod.rs           -- Test runner (inline assertions, snapshot assertions)
      parser.rs        -- Parse epilogue comments, @snap directives
      report.rs        -- Test result reporting (TestStats)
      value.rs         -- Value-to-string for assertion comparison
      snap/
        mod.rs         -- Snapshot orchestration (SnapState, SnapMode)
        file.rs        -- Snapshot file I/O
        diff.rs        -- Snapshot diff display
        value.rs       -- Snapshot value rendering
    codegen/
      mod.rs           -- Entry point, schema type detection
      report.rs        -- Builds Report from annotated SQL via Runtime stepping
      types.rs         -- Report, Export, Parameter structs (re-exports from core)
      snapshots/       -- insta test snapshots
    jupyter/
      mod.rs           -- install / up subcommands
      kernel.rs        -- SoliteKernel (Jupyter protocol, tokio async)
      handlers.rs      -- Dot command handling in kernel context
      protocol.rs      -- JupyterSender trait for message dispatch
      render/
        mod.rs         -- render_statement -> HTML table
        html.rs        -- HTML generation
        table.rs       -- Table rendering
        syntax.rs      -- SQL syntax highlighting for HTML
    tui/
      mod.rs           -- App struct, page navigation, ratatui event loop
      listing_page.rs  -- Table listing view
      table_page.rs    -- Table data view with scrolling
      row_page.rs      -- Single row detail view
      copy_popup.rs    -- Clipboard copy UI
      help_bar.rs      -- Help overlay
      tui_theme.rs     -- TUI theme (Catppuccin Mocha)
      utils.rs         -- Shared TUI utilities
    bench/
      mod.rs           -- Benchmark runner (10 iterations, progress bar)
      stats.rs         -- Statistical functions (mean, stddev, min, max)
      format.rs        -- Runtime formatting
    docs/
      mod.rs           -- Documentation generation
      sql.rs, table.rs, value.rs
    fmt.rs             -- Delegates to solite_fmt crate
    lint.rs            -- Delegates to solite_analyzer crate
    lsp.rs             -- Delegates to solite_lsp crate
    query.rs           -- Read-only query with format output (csv, json, ndjson, tsv, clipboard)
    exec.rs            -- Write statement execution
    schema.rs, backup.rs, vacuum.rs, sqlite3.rs, diff.rs, rsync.rs, stream.rs
```

## How Commands Work

### `run` (Script Execution)

Positional args are classified by extension: `.sql`/`.ipynb` = script, `.db`/`.sqlite`/`.sqlite3` = database, anything else = procedure name. Supports:

- `solite run script.sql` -- execute a SQL file
- `solite run db.db script.sql procName` -- load file, call a named procedure
- `solite run -c "SELECT 1"` -- inline SQL from flag
- Stdin piping when no script is given
- `--trace trace.db` -- records execution trace to a SQLite database
- `.ipynb` notebook support (extracts code cells)

The core loop calls `Runtime::next_stepx()` and dispatches on `StepResult`:
- `SqlStatement` -> print table output, optional timer, optional tracing
- `DotCommand` -> delegate to `dot.rs` handler
- `ProcedureDefinition` -> no-op (already registered in runtime)

### `query` vs `execute`

- `query`: Read-only. Outputs to stdout (pretty table if TTY) or file in csv/json/ndjson/tsv/clipboard format. Enforces `stmt.readonly()`. Supports replacement scans and extension loading.
- `execute`: Write statements. Prints a checkmark on success. Accepts 1-2 positional args in any order (database path detected by file existence).

### `format` and `lint`

Thin wrappers around `solite_fmt` and `solite_analyzer` crates respectively. Both support file arguments or stdin, config file discovery, and `--check`/`--fix` modes.

### `lsp`

Delegates to `solite_lsp::run_server()` on a tokio runtime. Communicates via stdio.

### `sqlite3`, `diff`, `rsync`

Pass-through commands that forward all arguments to the bundled sqlite3, sqldiff, and sqlite3_rsync binaries.

### `backup`, `vacuum`, `schema`

Direct SQLite operations. `backup` uses the SQLite backup API. `vacuum` supports `VACUUM INTO` via `--into`/`-o`. `schema` prints CREATE statements.

## REPL Implementation (`commands/repl/`)

Built on `rustyline` with Emacs edit mode. Key components:

- **ReplValidator**: Uses `solite_core::sqlite::complete()` to detect incomplete SQL (multi-line input). Special-cases `.export` (multi-line dot command) and dot command prefixes (`.`, `!`, `?`).
- **ReplHighlighter**: SQL syntax highlighting in the input line.
- **ReplCompleter**: Tab completion for SQL keywords, table/column names.
- **History**: Persisted to `~/.solite_history`.
- **Prompt**: `>` normally, `>*` inside a transaction.
- **`\e` command**: Opens `$EDITOR` with a temp file, executes the result.

The `execute()` function enqueues input via `runtime.enqueue("[repl]", ...)`, then steps through results. SQL statements are rendered as tables via `solite_table::print_statement`. Dot commands are handled inline with REPL-specific behavior (e.g., `.tui` launches the TUI, `.ask` streams LLM responses).

## Test Framework (`commands/test/`)

Test files are plain `.sql` files with assertion comments in the epilogue (after the semicolon):

```sql
SELECT 1 + 1; -- 2
SELECT 'hello'; -- 'hello'
SELECT NULL; -- NULL
SELECT * FROM empty; -- [no results]
INSERT INTO t VALUES (dup); -- error: UNIQUE constraint failed: t.x
SELECT 1; -- TODO not implemented yet
SELECT * FROM t; -- @snap my-snapshot
```

**Inline assertions**: Compare the first column of the first row against the comment text. Supports integer, float, string (quoted), NULL, `[no results]`, and `error: <message>`.

**TODO annotations**: Cause test failure (treated as unresolved items).

**Snapshot assertions** (`@snap <name>`):
- Stored in `__snapshots__/<filestem>-<name>.snap` alongside the test file.
- Three modes: `Default` (compare, fail on mismatch/new), `Update` (`-u`, auto-accept all changes and delete orphans), `Review` (`--review`, interactive).
- Snapshots capture full tabular output including column names and all rows.
- Orphan detection: snapshots no longer referenced in the test file are flagged or deleted depending on mode.

Statements without epilogue comments are treated as setup (executed silently). Dot commands `.load`, `.parameter set`, `.call`, and `.run` are supported during tests.

## Codegen System (`commands/codegen/`)

Parses SQL files with `-- name: <name> :<result_type>` annotations and produces a JSON report (`Report` struct) containing:

- `setup`: Non-annotated statements (CREATE TABLE, etc.)
- `exports`: Named queries with metadata:
  - `name`, `sql`, `result_type` (`:rows`, `:row`, `:value`, `:list`, or void)
  - `parameters`: Extracted from `$name::type` or `:name::type` syntax
  - `columns`: Column metadata from SQLite's prepare

Uses the same `Runtime::next_stepx()` loop. `ProcedureDefinition` steps become exports. Supports an optional `--schema` flag to load a `.db` or `.sql` schema for query validation.

Output is JSON to stdout or `--output` file. Types are defined in `types.rs` (re-exports `ProcedureParam` and `ResultType` from `solite-core`).

## Jupyter Kernel (`commands/jupyter/`)

Two subcommands:
- `jupyter install`: Writes a `kernel.json` spec to the Jupyter kernels directory.
- `jupyter up --connection <file>`: Starts the kernel from a Jupyter connection file.

The kernel (`SoliteKernel`) runs on tokio with:
- A runtime task that receives code strings via `mpsc` channel and steps through them with `Runtime::next_stepx()`
- Shell, control, heartbeat, and iopub connections via `runtimelib`
- SQL results rendered as HTML tables (via `render/`) and sent as `DisplayData` messages
- Dot commands handled in `handlers.rs` (exports produce file downloads, vegalite produces chart output)
- Errors reported via `ErrorOutput` messages

## TUI (`commands/tui/`)

Built on `ratatui`. Three-page navigation:

- **ListingPage**: Shows all tables/views in the database with row counts.
- **TablePage**: Scrollable data grid for a selected table. Supports horizontal scrolling, column shifting.
- **RowPage**: Detail view of a single row with all column values.

Navigation: Enter to drill in, Escape/Backspace to go back, `q` to quit, `?` for help. Clipboard copy via `arboard`. Theme uses Catppuccin Mocha. Can also be launched from the REPL via `.tui`.

## Key Integration Points

- **`solite_core::Runtime`**: All commands create a `Runtime` and call `next_stepx()` in a loop. The `StepResult` enum (`SqlStatement`, `DotCommand`, `ProcedureDefinition`) must be handled at every call site.
- **`solite_core::dot::DotCommand`**: Each command context (run, repl, test, jupyter) has its own dot command handler with different supported commands and behavior.
- **`solite_table`**: Used for terminal table rendering in run, repl, query, and TUI contexts.
- **`solite_core::exporter`**: Used by query command for structured output (csv, json, ndjson, tsv).
- **`crate::errors`**: Uses `codespan-reporting` to render SQLite errors with source location. Both terminal (`report_error`) and string (`report_error_string`, for Jupyter) variants.
- **Feature gate**: `ritestream` feature enables the `stream` command and `DotCommand::Stream` handling across all command contexts.

## Match Sites for StepResult

Seven places handle `StepResult` from `runtime.next_stepx()`:

1. `run/mod.rs` -- `execute_steps()`
2. `repl/mod.rs` -- `execute()` and `.run` dot command handler
3. `codegen/report.rs` -- builds the codegen report
4. `jupyter/kernel.rs` -- `handle_code()`
5. `test/mod.rs` -- `test_impl()` and `.run` dot command handler
6. `tui/` -- indirectly via runtime queries
7. `run/dot.rs` -- `.run` dot command handler

When adding a new `StepResult` variant or `DotCommand` variant, all of these sites must be updated.
