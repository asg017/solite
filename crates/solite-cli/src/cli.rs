use std::{env, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use solite_core::exporter::{BlobLimit, ExportFormat};

/// Extensions treated as SQLite database files (case-insensitive), for
/// positional-arg classification (`solite run`) and the bare `solite <file>`
/// REPL fallback.
pub(crate) fn is_database_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "db" | "sqlite" | "sqlite3"
            )
        })
}

/// Shared args for connecting to remote databases over SSH or custom transports.
#[derive(Args, Debug, Clone, Default)]
pub struct RemoteArgs {
    /// Path to the solite binary on the remote machine (for ssh:// connections)
    #[arg(long)]
    pub remote_bin: Option<String>,

    /// Custom transport command to reach the remote machine (e.g. "fly ssh console -a my-app -C")
    #[arg(long)]
    pub transport: Option<String>,

    /// Whether SSH/remote connections are allowed (set from top-level --allow-ssh)
    #[arg(skip)]
    pub allow_ssh: bool,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Positional args, in any order, classified by extension:
    /// .sql/.ipynb = script, .db/.sqlite/.sqlite3 = database
    /// (default: in-memory), anything else = procedure name to call
    pub args: Vec<String>,

    /// Execute SQL/dot commands from the given string (instead of a .sql file)
    #[arg(long, short = 'c')]
    pub command: Option<String>,

    /// Bind a SQL parameter. Use `-p name value` for TEXT, or `-p name @file`
    /// to bind the file's bytes as a BLOB.
    #[arg(long, short = 'p', num_args = 2, value_names = ["NAME", "VALUE"])]
    pub parameters: Vec<String>,

    /// Record an execution trace (statements + per-opcode bytecode stats)
    /// to a SQLite database at this path
    #[arg(long, value_name = "TRACE_DB")]
    pub trace: Option<PathBuf>,

    /// Open the database read-only; statements that write will fail
    #[arg(long, alias = "read-only")]
    pub readonly: bool,
}

const RUN_AFTER_HELP: &str = "\
Examples:
  solite run script.sql                      # against an in-memory database
  solite run app.db script.sql               # against app.db
  solite run app.db queries.sql getUser      # run one named procedure
  solite run notebook.ipynb                  # SQL cells of a notebook
  cat script.sql | solite run app.db         # SQL from stdin
  solite run app.db -c \"SELECT count(*) FROM users\"
  solite run -c \"SELECT * FROM 'data.csv'\"   # query a CSV/TSV file directly
  solite run --readonly app.db script.sql    # statements that write will fail
  solite run script.sql --trace trace.db     # record an execution trace

Trace output: --trace writes (replacing any existing file) a SQLite database
with two tables: statements(id, sql) has one row per executed statement, and
steps(id, statement_id, addr, opcode, p1, p2, p3, p4, p5, comment, subprog,
nexec, ncycle) has that statement's per-opcode stats from SQLite's bytecode()
virtual table. Explore it with, for example:
  solite query \"SELECT opcode, sum(ncycle) FROM steps \\
                GROUP BY 1 ORDER BY 2 DESC\" trace.db

Scripts may contain dot commands (.export, .param set, .run, .load, ...;
see `.help` in the REPL) and procedure definitions (`-- name: getUser :row`).
Not available in run mode: .ask, .tui, .clear.";

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
pub enum QueryFormat {
    Csv,
    Tsv,
    #[default]
    Json,
    /// Newline-delimited JSON, one object per row
    Ndjson,
    /// Bare value of the first column of the first row, for shell interpolation
    Value,
    /// Copy results to the system clipboard
    Clipboard,
}

impl From<QueryFormat> for ExportFormat {
    fn from(value: QueryFormat) -> ExportFormat {
        match value {
            QueryFormat::Csv => ExportFormat::Csv,
            QueryFormat::Tsv => ExportFormat::Tsv,
            QueryFormat::Json => ExportFormat::Json,
            QueryFormat::Ndjson => ExportFormat::Ndjson,
            QueryFormat::Value => ExportFormat::Value,
            QueryFormat::Clipboard => ExportFormat::Clipboard,
        }
    }
}

const QUERY_AFTER_HELP: &str = "\
Examples:
  solite query \"SELECT count(*) FROM users\" app.db
  solite query app.db report.sql -f json          # SQL from a file; order-agnostic
  solite query \"SELECT * FROM users\" app.db -o users.csv.gz
  solite query \"SELECT * FROM 'data.csv' LIMIT 5\" # query a CSV/TSV file directly
  solite query \"SELECT name FROM users WHERE id = $id\" app.db -p id 42
  solite q \"SELECT 1\"                             # 'q' alias, in-memory database
  solite q \"SELECT 1\" :memory:                    # explicit in-memory database
  echo \"SELECT 42\" | solite q -                   # SQL from stdin
  pbpaste | solite q app.db -f csv                # piped stdin + database
  solite query --allow-ssh \"SELECT 1\" ssh://user@host/app.db";

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// SQL to run (read-only; use `solite execute` for writes), a path
    /// to a .sql file containing it, or `-` to read SQL from stdin
    /// (also the default when stdin is piped)
    pub statement: Option<String>,

    /// Database file or ssh:// URL (with --allow-ssh). Omit for in-memory
    pub database: Option<PathBuf>,

    /// Write results to a file; format inferred from extension
    /// (.csv, .tsv, .json, .ndjson; .gz/.zst compression supported)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Output format (default: table on a TTY, json otherwise)
    #[arg(long, short = 'f', value_enum)]
    pub format: Option<QueryFormat>,

    /// Bind a SQL parameter, e.g. -p id 42 for `WHERE id = $id`.
    /// Integer/decimal values bind as numbers; single-quote to force
    /// text, e.g. -p id "'42'"
    #[arg(long, short = 'p', num_args = 2, value_names = ["NAME", "VALUE"], allow_negative_numbers = true)]
    pub parameters: Vec<String>,

    /// Load SQLite extension(s) before running the query
    #[arg(long, value_name = "PATH")]
    pub load_extension: Option<Vec<PathBuf>>,

    /// Max size of a single BLOB cell in exports: bytes (1048576), a
    /// k/kb/m/mb/g/gb suffix (16mb), or none/unlimited/0 for no limit.
    /// Defaults to 10mb (1mb for clipboard); `-f value` is never limited
    #[arg(long, value_name = "SIZE", value_parser = solite_core::exporter::parse_blob_limit)]
    pub blob_limit: Option<BlobLimit>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

const EXECUTE_AFTER_HELP: &str = "\
Examples:
  solite execute app.db \"INSERT INTO users(name) VALUES ('alex')\"
  solite execute \"CREATE TABLE t(a)\" app.db     # order doesn't matter
  solite execute new.db \"CREATE TABLE t(a)\"     # creates new.db if missing
  solite execute app.db migrate.sql             # SQL from a .sql file
  solite execute app.db \"DELETE FROM users WHERE id = $id\" -p id 42
  echo \"CREATE TABLE t(a)\" | solite exec - new.db  # SQL from stdin
  cat migrate.sql | solite exec app.db            # piped stdin + database

Arguments are classified by extension: .db/.sqlite/.sqlite3 (or :memory:)
is the database — created if it doesn't exist yet — and .sql is a file
whose contents are the SQL. Otherwise the argument that exists on disk is
the database and the other is the SQL. `-` (or piped stdin with no SQL
argument) reads the SQL from stdin. With a single SQL argument, the
statements run against an in-memory database.";

#[derive(Args, Debug)]
pub struct ExecuteArgs {
    /// SQL statement (.sql file, or `-` for stdin) and optional database
    /// path, in any order; classified by extension, then by existence
    #[arg(num_args = 0..=2)]
    pub args: Vec<String>,

    /// Bind a SQL parameter, e.g. -p id 42 for `WHERE id = $id`.
    /// Integer/decimal values bind as numbers; single-quote to force
    /// text, e.g. -p id "'42'"
    #[arg(long, short = 'p', num_args = 2, value_names = ["NAME", "VALUE"], allow_negative_numbers = true)]
    pub parameters: Vec<String>,
}

const REMOTE_HELP: &str = "\
Remote databases:
  solite repl --allow-ssh ssh://user@host/var/data/app.db
  solite repl --allow-ssh user@host:app.db      # scp-style also works
  solite tui --allow-ssh --transport \"fly ssh console -a my-app -C\" app.db

Requires solite installed on the remote machine (override the path with
--remote-bin; default: `solite` on the remote $PATH). --transport replaces
ssh with a custom command that connects stdin/stdout to the remote shell.";

const REPL_ENV_HELP: &str = "\
Inside the REPL, `.help` lists all dot commands. Environment:
  EDITOR              used by the \\e scratch-buffer command (default: vi)
  OPENROUTER_API_KEY  required by .ask / ?<question>
  SOLITE_HISTORY      readline history file (default: ~/.solite_history)";

#[derive(Args, Debug)]
pub struct ReplArgs {
    /// Database file or ssh:// URL (with --allow-ssh). Omit for in-memory
    pub database: Option<PathBuf>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

const BENCH_AFTER_HELP: &str = "\
Each SQL argument is benchmarked over -n/--iterations timed runs
(default 10), after --warmup untimed runs (default 0), reporting
mean ± stddev (sample, n-1; N/A for a single iteration), min … max,
and the statement's bytecode steps.

Examples:
  solite bench --database app.db \"SELECT count(*) FROM users\"
  solite bench \"SELECT 1\" \"SELECT 1 + 1\"        # compare two statements
  solite bench --database a.db --database b.db query.sql query.sql

Give --database once to run every SQL argument against that database
(one shared connection), or once per SQL argument to pair them by
position. Any other count is an error. --attach PATH NAME attaches an
extra database to every benchmark connection:

  solite bench --attach other.db aux \"SELECT count(*) FROM aux.t\"

Arguments ending in .sql are read as files; leading statements run
once as untimed setup and the last statement is benched. Statements
really execute every run — benchmarking an INSERT inserts rows — and
each run consumes the full result set, so result-set size dominates
timing for large SELECTs.

Also available inside scripts and the REPL as the multi-line `.bench`
dot command.";

#[derive(Args, Debug)]
pub struct BenchArgs {
    /// SQL statements (or .sql file paths) to benchmark
    #[arg(required = true)]
    pub sql: Vec<String>,

    /// Database(s) to bench against: give once to share across all SQL
    /// arguments, or once per SQL argument to pair by position
    /// (default: in-memory)
    #[arg(long)]
    pub database: Option<Vec<PathBuf>>,

    /// Attach an additional database to every benchmark connection;
    /// queries can reference it as NAME.table. Repeatable
    #[arg(long, num_args = 2, value_names = ["PATH", "NAME"])]
    pub attach: Option<Vec<PathBuf>>,

    /// Number of timed iterations per benchmark
    #[arg(
        long,
        short = 'n',
        value_name = "N",
        default_value_t = 10,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub iterations: u32,

    /// Untimed warmup executions before measurement begins
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub warmup: u32,

    /// Load SQLite extension(s) before benchmarking
    #[arg(long, value_name = "PATH")]
    pub load_extension: Option<Vec<PathBuf>>,
}
const CODEGEN_AFTER_HELP: &str = "\
Annotate queries with a name and result type:

  -- name: getUserById :row
  SELECT id, name FROM users WHERE id = $id::int;

Result types: :rows (many), :row (one), :value (single value),
:list (single column). Omit to auto-detect from the column count.
Parameters: $name or :name, optionally typed ($name::int); a trailing
`::` marks the parameter as optional ($name::int::).
Unannotated statements are emitted in the report as `setup`.

Example:
  solite codegen queries.sql --schema schema.sql -o report.json";

#[derive(Args, Debug)]
pub struct CodegenArgs {
    /// SQL file with `-- name: <proc> :<type>` annotated queries
    pub file: PathBuf,
    /// Schema to validate queries against: a SQLite database file or a
    /// .sql file of CREATE statements
    #[arg(long)]
    pub schema: Option<PathBuf>,
    /// Write the JSON report here instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}



const TEST_AFTER_HELP: &str = "\
Test files are plain SQL. The comment after a statement's semicolon is its
assertion, compared against the first column of the first row:

  SELECT 1 + 1;             -- 2
  SELECT 'hi';              -- 'hi'
  SELECT NULL;              -- NULL
  SELECT * FROM empty;      -- [no results]
  SELECT * FROM nope;       -- error: no such table: nope
  SELECT * FROM users;      -- @snap all-users
  SELECT slow();            -- TODO speed this up (fails until resolved)

Statements without an assertion comment are setup and run silently, against
an in-memory database. `error:` matches the error message exactly.
Snapshots (`@snap <name>`) are stored in __snapshots__/ next to the test
file; use --update to accept changes, --review to accept interactively.
Multiple files and directories may be given; each file runs against its
own fresh in-memory database and the summary is aggregated.
Dot commands available in tests: .load, .param, .print, .call, .run;
any other dot command (or a failing one) aborts the test file.";

#[derive(Args, Debug)]
pub struct TestArgs {
    /// SQL test files with inline `-- expected` assertions; a directory
    /// expands to the *.sql files directly inside it (non-recursive)
    #[arg(required = true, num_args = 1..)]
    pub files: Vec<PathBuf>,

    /// Seed each test file's in-memory database from this SQLite file
    /// (the file itself is never modified)
    #[arg(long, value_name = "PATH")]
    pub database: Option<PathBuf>,

    /// Also print expected/actual detail for failing assertions
    #[arg(long)]
    pub verbose: bool,

    /// Auto-accept all snapshot changes (new, updated, orphaned)
    #[arg(long, short = 'u')]
    pub update: bool,

    /// Interactively review each snapshot change
    #[arg(long)]
    pub review: bool,
}

#[derive(Args, Debug)]
pub struct JupyterNamespace {
    #[command(subcommand)]
    pub command: JupyterCommand,
}
#[derive(Subcommand, Debug)]
pub enum JupyterCommand {
    /// Install the Solite kernelspec so Jupyter can find the kernel
    Install(JupyterInstallArgs),
    /// Remove an installed Solite kernelspec
    Uninstall(JupyterUninstallArgs),
    /// List installed Jupyter kernelspecs
    List,
    /// Start the kernel from a Jupyter connection file (invoked by Jupyter,
    /// not directly by users)
    Up(JupyterUpArgs),
}

#[derive(Args, Debug)]
pub struct JupyterInstallArgs {
    /// Kernelspec directory name [default: solite]
    #[arg(long)]
    pub name: Option<String>,

    /// Kernel display name shown in the Jupyter UI [default: Solite]
    #[arg(long)]
    pub display: Option<String>,

    /// Overwrite an existing kernelspec
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct JupyterUninstallArgs {
    /// Kernelspec directory name [default: solite]
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args, Debug)]
pub struct JupyterUpArgs {
    /// Jupyter connection file (provided by Jupyter when launching the kernel)
    #[arg(long)]
    pub connection: PathBuf,
}

#[derive(Args, Debug)]
pub struct DocsNamespace {
    #[command(subcommand)]
    pub command: DocsCommand,
}
#[derive(Subcommand, Debug)]
pub enum DocsCommand {
    /// Execute SQL code blocks in a markdown file and inline their results
    Inline(DocsInlineArgs),
}

#[derive(Args, Debug)]
pub struct DocsInlineArgs {
    /// Markdown file with ```sql code blocks to execute
    pub input: PathBuf,

    /// SQLite extension to load before executing (also used to flag
    /// undocumented extension functions)
    #[arg(long)]
    pub extension: Option<String>,

    /// Write the resulting markdown here instead of stdout
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Database file or ssh:// URL (with --allow-ssh)
    pub database: PathBuf,
    /// Open directly on this table
    pub table: Option<String>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

const FMT_AFTER_HELP: &str = "\
Configuration (solite-fmt.toml), keys with defaults:

  keyword_case = \"lower\"               # lower | upper | preserve
  indent_style = \"spaces\"              # spaces | tabs
  indent_size = 2
  line_width = 80
  comma_position = \"trailing\"          # trailing | leading
  logical_operator_position = \"before\" # before | after
  statement_separator_lines = 2

Ignore directives in SQL comments:
  -- solite-fmt: off / -- solite-fmt: on   skip a region
  -- solite-fmt-ignore                     skip the next statement";

#[derive(Args, Debug)]
pub struct FmtArgs {
    /// SQL files to format (reads from stdin if none provided)
    pub files: Vec<PathBuf>,

    /// Write formatted output back to files
    #[arg(short, long)]
    pub write: bool,

    /// Check if files are formatted (exit 1 if not)
    #[arg(long)]
    pub check: bool,

    /// Show diff of formatting changes
    #[arg(long)]
    pub diff: bool,

    /// Config file (default: solite-fmt.toml in current/parent dirs,
    /// then ~/.config/solite/fmt.toml)
    #[arg(long)]
    pub config: Option<PathBuf>,
}

const LINT_AFTER_HELP: &str = "\
Configuration (solite-lint.toml) sets per-rule severities:

  [rules]
  double-quoted-string = \"off\"   # off | warning | error

Use --list-rules to see every rule with its description and fixability.";

#[derive(Args, Debug)]
pub struct LintArgs {
    /// SQL files to lint (reads from stdin if none provided)
    pub files: Vec<PathBuf>,

    /// Config file (default: solite-lint.toml in current/parent dirs,
    /// then ~/.config/solite/lint.toml)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Apply auto-fixes where available
    #[arg(long)]
    pub fix: bool,

    /// List all lint rules and exit
    #[arg(long)]
    pub list_rules: bool,
}

#[derive(Args, Debug)]
pub struct LspArgs {
    /// Communicate over stdin/stdout (the only supported transport)
    #[arg(long, default_value_t = true)]
    pub stdio: bool,
}

#[derive(Args, Debug)]
pub struct Sqlite3Args {
    /// Arguments passed directly to the sqlite3 shell
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Args, Debug)]
#[command(disable_help_flag = true)]
pub struct DiffArgs {
    /// Arguments passed directly to sqldiff
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Args, Debug)]
#[command(disable_help_flag = true)]
pub struct RsyncArgs {
    /// Arguments passed directly to sqlite3_rsync
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Args, Debug)]
pub struct SchemaArgs {
    /// Database file to print CREATE statements for
    pub database: PathBuf,

    /// Only show objects whose name (or owning table) matches this LIKE pattern, e.g. 'users' or 'idx_%'
    pub pattern: Option<String>,
}

#[derive(Args, Debug)]
pub struct BackupArgs {
    /// Source database path
    pub database: PathBuf,

    /// Destination backup file path
    pub destination: PathBuf,

    /// Which attached database to back up
    #[arg(long, default_value = "main")]
    pub db: String,

    /// Overwrite the destination file if it already exists
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct VacuumArgs {
    /// Database path to vacuum
    pub database: PathBuf,

    /// Write vacuumed database to a new file instead of in-place
    #[arg(long, alias = "output", short = 'o')]
    pub into: Option<PathBuf>,

    /// Positional alias for --into
    #[arg(hide = true)]
    pub destination: Option<PathBuf>,

    /// Overwrite the --into file if it already exists
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Path to the database file to serve
    pub database: String,
}

impl VacuumArgs {
    pub fn into_path(&self) -> Option<&PathBuf> {
        self.into.as_ref().or(self.destination.as_ref())
    }
}

#[cfg(feature = "ritestream")]
#[derive(Args, Debug)]
pub struct StreamNamespace {
    #[command(subcommand)]
    pub command: StreamCommand,
}

#[cfg(feature = "ritestream")]
#[derive(Subcommand, Debug)]
pub enum StreamCommand {
    /// Sync WAL changes to a replica
    Sync(StreamSyncArgs),
    /// Restore a database from a replica
    Restore(StreamRestoreArgs),
}

#[cfg(feature = "ritestream")]
#[derive(Args, Debug)]
pub struct StreamSyncArgs {
    /// Path to the database file
    pub database: PathBuf,
    /// Replica URL (s3://bucket/prefix, file:///path, or bare path)
    pub url: String,
}

#[cfg(feature = "ritestream")]
#[derive(Args, Debug)]
pub struct StreamRestoreArgs {
    /// Replica URL (s3://bucket/prefix, file:///path, or bare path)
    pub url: String,
    /// Destination database path
    pub database: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run SQL scripts
    #[command(after_long_help = RUN_AFTER_HELP)]
    Run(RunArgs),

    /// Start a REPL on a SQLite database
    #[command(after_long_help = format!("{REPL_ENV_HELP}\n\n{REMOTE_HELP}"))]
    Repl(ReplArgs),

    /// Run a read-only SQL query and output results to a file
    #[command(visible_alias = "q", after_long_help = QUERY_AFTER_HELP)]
    Query(QueryArgs),

    /// Execute a write SQL statement on a database
    ///
    /// The write counterpart of `solite query`; reports the number of
    /// affected rows on success and prints RETURNING rows.
    #[command(visible_alias = "exec", after_long_help = EXECUTE_AFTER_HELP)]
    Execute(ExecuteArgs),

    /// Run SQL-based inline tests in a single file
    #[command(after_long_help = TEST_AFTER_HELP)]
    Test(TestArgs),

    /// Manage the Solite Jupyter kernel
    Jupyter(JupyterNamespace),

    /// Tooling for documenting SQLite extensions
    Docs(DocsNamespace),

    /// Run benchmarks on SQL statements
    #[command(after_long_help = BENCH_AFTER_HELP)]
    Bench(BenchArgs),

    /// Generate a JSON IR from `-- name:` annotated SQL queries
    #[command(after_long_help = CODEGEN_AFTER_HELP)]
    Codegen(CodegenArgs),

    /// Tui for exploring a database
    #[command(after_long_help = REMOTE_HELP)]
    Tui(TuiArgs),

    /// Format SQL files
    #[command(visible_alias = "fmt", after_long_help = FMT_AFTER_HELP)]
    Format(FmtArgs),

    /// Lint SQL files for potential issues
    #[command(after_long_help = LINT_AFTER_HELP)]
    Lint(LintArgs),

    /// Start the Language Server Protocol (LSP) server
    ///
    /// Meant to be launched by an editor/LSP client. Speaks LSP over
    /// stdio and provides completions, hover, diagnostics, formatting,
    /// semantic tokens, and inlay hints for SQL files.
    Lsp(LspArgs),

    /// Run the sqlite3 shell directly
    Sqlite3(Sqlite3Args),

    /// Output SQL to transform one database into another
    Diff(DiffArgs),

    /// Efficiently replicate a SQLite database to a remote machine
    Rsync(RsyncArgs),

    /// Print the schema of a database
    Schema(SchemaArgs),

    /// Back up a SQLite database to a file
    Backup(BackupArgs),

    /// Rebuild a database file, repacking it into minimal disk space
    Vacuum(VacuumArgs),

    /// Serve a database over stdin/stdout (used by SSH remote connections)
    #[command(hide = true)]
    Serve(ServeArgs),

    /// Streaming replication, like litestream
    #[cfg(feature = "ritestream")]
    Stream(StreamNamespace),
}

// NOTE: subcommands are listed by hand to get the section grouping below.
// When adding a command (or alias), add it here too or it won't appear in
// `solite --help`.
const HELP_TEMPLATE_BASE: &str = "\
{name} {version}
{about}

{usage-heading} {usage}
       solite <file>.db        Open the REPL on a database (also .sqlite, .sqlite3)

Options:
{options}

Scripting and Query Execution:
  run              Run SQL scripts
  repl             Start a REPL on a SQLite database
  query, q         Run a read-only SQL query and output results to a file
  execute, exec    Execute a write SQL statement on a database
  schema           Print the schema of a database

Tooling:
  backup           Back up a SQLite database to a file
  vacuum           Rebuild a database, repacking into minimal disk space
  jupyter          Manage the Solite Jupyter kernel
  tui              Tui for exploring a database
  test             Run SQL-based inline tests in a single file
  bench            Run benchmarks on SQL statements
  codegen          Generate a JSON IR from annotated SQL queries
  docs             Tooling for documenting SQLite extensions

SQL:
  format, fmt      Format SQL files
  lint             Lint SQL files for potential issues
  lsp              Start the Language Server Protocol (LSP) server
{replication}
Compatibility:
  sqlite3          Run the sqlite3 shell directly
  diff             Output SQL to transform one database into another
  rsync            Efficiently replicate a SQLite database to a remote machine
";

/// Render the top-level help, including feature-gated sections.
fn help_template() -> String {
    let replication = if cfg!(feature = "ritestream") {
        "\nReplication:\n  stream           Streaming replication (sync/restore)\n"
    } else {
        ""
    };
    HELP_TEMPLATE_BASE.replace("{replication}", replication)
}

#[derive(Parser)]
#[command(
  name = "solite",
  author,
  long_version = env!("CARGO_PKG_VERSION"),
  about = "Solite CLI",
  version,
  subcommand_required = false,
  arg_required_else_help = false,
  help_template = help_template(),
)]
pub struct Cli {
    /// Allow connecting to ssh:// database URLs and custom --transport commands
    #[arg(long, global = true)]
    pub allow_ssh: bool,

    #[command(subcommand)]
    pub command: Box<Commands>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn database_path_extensions() {
        for ok in ["a.db", "a.sqlite", "a.sqlite3", "/x/y/a.db"] {
            assert!(is_database_path(Path::new(ok)), "{ok}");
        }
        for not in ["a.sql", "a.csv", "a", "a.db.bak", "db"] {
            assert!(!is_database_path(Path::new(not)), "{not}");
        }
    }

    #[test]
    fn database_path_extensions_case_insensitive() {
        for ok in ["a.DB", "a.Db", "a.SQLITE", "a.SQLite3"] {
            assert!(is_database_path(Path::new(ok)), "{ok}");
        }
    }
}
