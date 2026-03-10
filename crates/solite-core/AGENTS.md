# solite-core

The core runtime for Solite. Provides the SQL stepping engine, SQLite bindings, dot command parsing, procedure system, and data export. This crate has no CLI or UI concerns; it is consumed by `solite-cli` and other frontends.

## File Layout

```
src/
  lib.rs            # Runtime, Block, Step, StepResult, StepError, next_stepx(), advance_through_ignorable()
  sqlite.rs         # Connection, Statement, OwnedValue, Row, ValueRefX, SQLiteError
  procedure.rs      # Procedure, ProcedureParam, ResultType, parse_name_line()
  exporter.rs       # ExportFormat, write_output(), format_from_path(), output_from_path()
  replacement_scans.rs  # Auto-creates virtual tables for .csv/.tsv files on "no such table" errors
  dot/
    mod.rs          # DotCommand enum, parse_dot(), ParseDotError, DotError
    ask.rs          # AskCommand (.ask / ? shorthand)
    bench.rs        # BenchCommand (.bench) - multi-line, has rest_length
    call.rs         # CallCommand (.call)
    clear.rs        # ClearCommand (.clear / .c)
    dotenv.rs       # DotenvCommand (.dotenv / .loadenv)
    env.rs          # EnvCommand (.env)
    export.rs       # ExportCommand (.export) - multi-line, has rest_length
    graphviz.rs     # GraphvizCommand (.graphviz / .gv)
    load.rs         # LoadCommand (.load)
    open.rs         # OpenCommand (.open)
    param.rs        # ParameterCommand (.param / .parameter)
    print.rs        # PrintCommand (.print)
    run.rs          # RunCommand (.run)
    schema.rs       # SchemaCommand (.schema)
    sh.rs           # ShellCommand (.sh / ! shorthand)
    stream.rs       # StreamCommand (.stream) - behind "ritestream" feature flag
    tables.rs       # TablesCommand (.tables)
    timer.rs        # Timer toggle (.timer on/off)
    tui.rs          # TuiCommand (.tui)
    vegalite.rs     # VegaLiteCommand (.vegalite / .vl) - multi-line, has rest_length
```

## Runtime

`Runtime` is the central struct. It holds a SQLite `Connection`, a stack of `Block`s to process, registered procedures, and parameter state.

```rust
pub struct Runtime {
    pub connection: Connection,
    stack: Vec<Block>,
    initialized_sqlite_parameters_table: bool,
    procedures: HashMap<String, Procedure>,
    loaded_files: HashSet<String>,
    virtual_files: HashMap<String, String>,
    running_files: Vec<String>,
}
```

### Key methods

- `new(path: Option<String>)` / `new_readonly(path: &str)` -- create a Runtime, opening the database and initializing `solite_stdlib`.
- `enqueue(name, code, source)` -- push a `Block` onto the stack for processing.
- `next_stepx() -> Option<Result<Step, StepError>>` -- the main stepping function (see below).
- `execute_to_completion()` -- drain all steps, executing SQL and registering procedures.
- `load_file(path)` -- read a SQL file (real or virtual), execute all statements, register procedures. Skips dot commands. Idempotent (tracks `loaded_files`).
- `run_file_begin(path, params) -> SavedRunState` / `run_file_end(saved)` -- bracket a `.run` invocation. Saves/restores the stack and parameters, with cycle detection.
- `prepare_with_parameters(sql)` -- prepare a statement via `Connection::prepare()`, then auto-bind parameters from `temp.sqlite_parameters`.
- `define_parameter(key, value)` / `delete_parameter(key)` / `lookup_parameter(key)` -- manage the `temp.sqlite_parameters` table.
- `register_procedure(proc)` / `get_procedure(name)` / `procedures()` -- manage the procedure registry.
- `add_virtual_file(path, content)` / `read_file(path)` -- virtual filesystem for testing; falls back to real `fs::read_to_string`.

## Block and BlockSource

A `Block` is a unit of input to process. It tracks a name (filename, `[stdin]`, etc.), the full contents as a `String` and `Rope`, and an `offset` cursor that advances as statements are consumed.

```rust
pub enum BlockSource {
    File(PathBuf),
    Repl,
    JupyerCell,
    CommandFlag,
    Stdin,
}
```

## Step, StepResult, StepError

`next_stepx()` returns `Option<Result<Step, StepError>>`:
- `None` -- no more input.
- `Some(Ok(Step))` -- a step was produced.
- `Some(Err(StepError))` -- a parse or prepare error.

```rust
pub struct Step {
    pub preamble: Option<String>,   // comments before the statement
    pub epilogue: Option<String>,   // inline comment after the statement on the same line
    pub result: StepResult,
    pub reference: StepReference,   // file:line:col location
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

There are 7 match sites for `StepResult` across the workspace: `run/mod.rs`, `repl/mod.rs`, `codegen/report.rs`, `jupyter/kernel.rs`, `test/mod.rs`, `snapshot/file.rs`, and `lib.rs` (`execute_to_completion`). When adding a new variant, all must be updated.

## next_stepx() Processing Flow

1. Pop a `Block` from the stack. Extract preamble (leading comments/whitespace via `advance_through_ignorable()`). Track `-- #region` / `-- #endregion` markers.
2. If the remaining code starts with `.`, `!`, or `?`:
   - `!` becomes `DotCommand::Shell`, `?` becomes `DotCommand::Ask`.
   - `.` is split into command name + args, then passed to `parse_dot()`.
   - For multi-line commands (Export, Vegalite, Bench), the block offset is advanced by `rest_length`.
   - `.call` is special-cased: it resolves to a `StepResult::SqlStatement` by looking up the procedure and preparing its SQL. If the call specifies a file, that file is loaded first via `load_file()`.
   - `.run` paths are resolved relative to the calling file's directory.
   - If there is remaining content after the dot line, the block is pushed back onto the stack.
   - Returns `StepResult::DotCommand(cmd)`.
3. Otherwise, call `prepare_with_parameters(code)`:
   - On success with a statement: check the preamble for a `-- name: foo :annotation` line. If found, build a `Procedure`, register it, and return `StepResult::ProcedureDefinition`. Otherwise return `StepResult::SqlStatement`.
   - On success with no statement: return `None`.
   - On error: try `replacement_scans::replacement_scan()` (auto-creates virtual tables for `.csv`/`.tsv` files). If the scan succeeds, push the block back and retry. Otherwise return `StepError::Prepare`.

## sqlite.rs -- Connection and Statement

### Connection

Wraps a raw `*mut sqlite3`. Key methods:

- `open(path)` / `open_readonly(path)` / `open_in_memory()` -- open a database.
- `prepare(sql) -> Result<(Option<usize>, Option<Statement>), SQLiteError>` -- prepare one statement. Returns the byte offset of remaining SQL (if any) and the prepared statement. Returns `(None, None)` for empty/comment-only input.
- `execute(sql)` / `execute_script(sql)` -- convenience wrappers.
- `load_extension(path, entrypoint)` -- load a SQLite extension.
- `serialize() -> Vec<u8>` -- serialize the database to bytes.
- `interrupt()` / `is_interrupted()` -- cancellation support.
- `in_transaction()` -- check autocommit state.
- `set_progress_handler()` -- register a progress callback.

`Connection` is `Send` but not `Sync`. It calls `sqlite3_close` on drop.

### Statement

Wraps a raw `*mut sqlite3_stmt`. Key methods:

- `next() -> Result<Option<Vec<ValueRefX>>, SQLiteError>` -- step and return column values as a vec.
- `nextx() -> Result<Option<Row>, SQLiteError>` -- step and return a `Row` for indexed access (faster, avoids allocation).
- `execute() -> Result<usize, SQLiteError>` -- step until `SQLITE_DONE`, return row count.
- `column_names()` / `column_meta()` -- column introspection.
- `bind_text()`, `bind_int64()`, `bind_double()`, `bind_blob()`, `bind_null()`, `bind_pointer()` -- parameter binding.
- `bind_parameters()` / `parameter_info()` -- list bound parameter names.
- `sql()` / `expanded_sql()` -- get the SQL text.
- `readonly()` -- check if statement is read-only.
- `reset()` -- reset for re-execution.
- `is_explain()` -- check if this is an EXPLAIN or EXPLAIN QUERY PLAN statement.

Calls `sqlite3_finalize` on drop.

### OwnedValue

Owned representation of a SQLite value, used for parameter storage and transfer:

```rust
pub enum OwnedValue {
    Null,
    Integer(i64),
    Double(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
```

### Other types

- `ValueRefX` / `ValueRefXValue` -- borrowed reference to a `sqlite3_value` with type-tagged enum. Created via `unsafe ValueRefX::from_value()`.
- `Row` -- lightweight wrapper over a statement for column access by index.
- `ColumnMeta` -- column metadata (name, origin database/table/column, declared type).
- `SQLiteError` -- result code, description, message, and optional byte offset.
- `BytecodeStep` -- a row from `bytecode()` virtual table, used by `.bench`.
- `complete(sql)` -- wraps `sqlite3_complete()` for checking if SQL is a complete statement.

## Procedure System

Procedures are named SQL blocks declared with a `-- name: foo :annotation` comment in the preamble. They are detected during `next_stepx()` after `prepare_with_parameters()` succeeds: the preamble is scanned for a `-- name:` line, and if found, a `Procedure` is built and registered in `Runtime.procedures`.

### Types (procedure.rs)

```rust
pub enum ResultType { Void, Rows, Row, Value, List }

pub struct ProcedureParam {
    pub full_name: String,          // "$name::text"
    pub name: String,               // "name"
    pub annotated_type: Option<String>,  // Some("text")
}

pub struct Procedure {
    pub name: String,
    pub sql: String,
    pub result_type: ResultType,
    pub parameters: Vec<ProcedureParam>,
    pub columns: Vec<ColumnMeta>,
}
```

### Key functions

- `parse_name_line(line) -> Option<(String, Vec<String>)>` -- parse `-- name: foo :row :returning` into name + annotations.
- `parse_parameter(param) -> ProcedureParam` -- parse `$name::text` into structured param.
- `determine_result_type(annotations, column_count) -> ResultType` -- map annotations to result type.

### Invocation

The `.call [file.sql] procedureName` dot command invokes a registered procedure. In `next_stepx()`, `.call` is resolved to a `StepResult::SqlStatement` -- the procedure's SQL is prepared and its statement is returned directly. If a file is specified, it is loaded first via `load_file()`.

## DotCommand Enum

All variants of `DotCommand` (from `dot/mod.rs`):

| Variant | Command | Description |
|---------|---------|-------------|
| `Tables(TablesCommand)` | `.tables [schema]` | List tables and views |
| `Schema(SchemaCommand)` | `.schema` | Show CREATE statements |
| `Graphviz(GraphvizCommand)` | `.graphviz` / `.gv` | Generate ERD in DOT format |
| `Open(OpenCommand)` | `.open <path>` | Open a different database |
| `Load(LoadCommand)` | `.load <path>` | Load a SQLite extension |
| `Tui(TuiCommand)` | `.tui` | Open TUI mode |
| `Clear(ClearCommand)` | `.clear` / `.c` | Clear screen |
| `Print(PrintCommand)` | `.print <msg>` | Print a message |
| `Ask(AskCommand)` | `.ask <question>` / `?` | Ask AI assistant |
| `Shell(ShellCommand)` | `.sh <cmd>` / `!` | Execute shell command |
| `Parameter(ParameterCommand)` | `.param set/unset/list/clear` | Manage query parameters |
| `Env(EnvCommand)` | `.env set/unset` | Manage environment variables |
| `Timer(bool)` | `.timer on/off` | Toggle query timing |
| `Export(ExportCommand)` | `.export <path> <SQL>` | Export query results (multi-line) |
| `Vegalite(VegaLiteCommand)` | `.vegalite <mark> <SQL>` / `.vl` | Generate Vega-Lite chart (multi-line) |
| `Bench(BenchCommand)` | `.bench [--name N] <SQL>` | Benchmark query execution (multi-line) |
| `Dotenv(DotenvCommand)` | `.dotenv` / `.loadenv` | Load .env file |
| `Call(CallCommand)` | `.call [file.sql] procName` | Call a registered procedure |
| `Run(RunCommand)` | `.run <file> [proc] [--k=v]` | Run a SQL file inline |
| `Stream(StreamCommand)` | `.stream sync/restore <url>` | Stream replication (feature-gated) |

### Shorthand syntax

- `!command` is equivalent to `.sh command`
- `?question` is equivalent to `.ask question`

## Multi-line Dot Commands and rest_length

Three dot commands consume SQL from the lines *after* the dot command line: `.export`, `.vegalite`, and `.bench`. Their structs have a `rest_length: usize` field that indicates how many bytes of the remaining block content were consumed by `prepare_with_parameters()` during parsing.

In `next_stepx()`, when one of these commands is parsed, the block offset is advanced by `rest_length` so that the consumed SQL is not re-processed:

```rust
DotCommand::Export(cmd) => { block.offset += cmd.rest_length; }
DotCommand::Vegalite(cmd) => { block.offset += cmd.rest_length; }
DotCommand::Bench(cmd) => { block.offset += cmd.rest_length; }
```

The SQL is prepared at parse time inside their `new()` constructors, which take `(args, runtime, rest)` where `rest` is the remaining block content after the dot command line.

## parse_dot()

```rust
pub fn parse_dot(command: S, args: S, rest: &str, runtime: &mut Runtime) -> Result<DotCommand, ParseDotError>
```

Dispatches on the lowercased command name. Most commands only use `args`. Multi-line commands (export, bench, vegalite) also use `rest` to prepare a SQL statement. The `runtime` is needed for parameter lookup during statement preparation and for path substitution.

Returns `ParseDotError::UnknownCommand` for unrecognized commands.

## Exporter System (exporter.rs)

Formats for exporting SQL results:

```rust
pub enum ExportFormat { Csv, Tsv, Json, Ndjson, Value, Clipboard }
```

Key functions:

- `write_output(stmt, output, format)` -- step through a statement and write results in the given format.
- `format_from_path(path) -> Option<ExportFormat>` -- infer format from file extension. Handles double extensions for compression (`.csv.gz`, `.json.zst`).
- `output_from_path(path) -> Box<dyn Write>` -- create a writer, automatically adding gzip or zstd compression based on extension.

JSON export respects SQLite's JSON subtype (subtype 74): text values with JSON subtype are emitted as raw JSON rather than quoted strings.

## Replacement Scans (replacement_scans.rs)

When `prepare_with_parameters()` fails with a "no such table: X" error, `next_stepx()` checks if `X` ends with `.csv` or `.tsv`. If so, it creates a virtual table (`CREATE VIRTUAL TABLE temp."X" USING csv/tsv`) and retries the prepare. This allows queries like `SELECT * FROM "data.csv"` to work without explicit table creation.

## advance_through_ignorable()

Skips leading whitespace, `--` line comments, `/* */` block comments, and `#` line comments. Returns the remaining string slice. This function is what turns preamble comments (including `-- name:` lines) into the preamble field on `Step`. Important: it skips ALL comments, which is why `-- name:` annotations end up in the preamble rather than being processed as SQL.
