use std::{collections::HashMap, env, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use solite_core::exporter::ExportFormat;

/// Extensions treated as SQLite database files, for positional-arg
/// classification (`solite run`) and the bare `solite <file>` REPL fallback.
pub(crate) fn is_database_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("db" | "sqlite" | "sqlite3")
    )
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

Scripts may contain dot commands (.export, .param set, .run, .load, ...;
see `.help` in the REPL) and procedure definitions (`-- name: getUser :row`).
Not available in run mode: .ask, .tui, .clear, .vegalite, .bench.";

impl RunArgs {
    #[allow(dead_code)]
    pub fn params(&self) -> HashMap<String, String> {
        self.parameters
            .chunks(2)
            .map(|chunk| {
                if chunk.len() == 2 {
                    (chunk[0].clone(), chunk[1].clone())
                } else {
                    (chunk[0].clone(), String::new())
                }
            })
            .collect()
    }
}
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
  solite q \"SELECT 1\"                             # 'q' alias, in-memory database";

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// SQL to run (read-only; use `solite execute` for writes), or a
    /// path to a .sql file containing it
    pub statement: String,

    /// Database file or ssh:// URL (with --allow-ssh). Omit for in-memory
    pub database: Option<PathBuf>,

    /// Write results to a file; format inferred from extension
    /// (.csv, .tsv, .json, .ndjson; .gz/.zst compression supported)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Output format (default: table on a TTY, json otherwise)
    #[arg(long, short = 'f', value_enum)]
    pub format: Option<QueryFormat>,

    /// Bind a SQL parameter, e.g. -p id 42 for `WHERE id = $id`
    #[arg(long, short = 'p', num_args = 2, value_names = ["NAME", "VALUE"])]
    pub parameters: Vec<String>,

    /// Load SQLite extension(s) before running the query
    #[arg(long, value_name = "PATH")]
    pub load_extension: Option<Vec<PathBuf>>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

const EXECUTE_AFTER_HELP: &str = "\
Examples:
  solite execute app.db \"INSERT INTO users(name) VALUES ('alex')\"
  solite execute \"CREATE TABLE t(a)\" app.db     # order doesn't matter
  solite execute app.db \"DELETE FROM users WHERE id = $id\" -p id 42

The database file must already exist: the argument that exists on disk is
the database, the other is the SQL. With a single argument, the SQL runs
against an in-memory database.";

#[derive(Args, Debug)]
pub struct ExecuteArgs {
    /// SQL statement and optional database path, in any order; the
    /// argument that exists as a file is the database
    #[arg(num_args = 1..=2)]
    pub args: Vec<String>,

    /// Reserved; currently ignored
    #[arg(long, short = 'o', hide = true)]
    pub output: Option<PathBuf>,

    /// Reserved; currently ignored
    #[arg(long, short = 'f', value_enum, hide = true)]
    pub format: Option<QueryFormat>,

    /// Bind a SQL parameter, e.g. -p id 42 for `WHERE id = $id`
    #[arg(long, short = 'p', num_args = 2, value_names = ["NAME", "VALUE"])]
    pub parameters: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReplArgs {
    pub database: Option<PathBuf>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

#[derive(Args, Debug)]
pub struct BenchArgs {
    pub sql: Vec<String>,
    #[arg(long)]
    pub database: Option<Vec<PathBuf>>,

    #[arg(long, num_args = 2, value_names = ["PATH", "NAME"])]
    pub attach: Option<Vec<PathBuf>>,

    #[arg(long)]
    pub load_extension: Option<Vec<PathBuf>>,
}
#[derive(Args, Debug)]
pub struct CodegenArgs {
    pub file: PathBuf,
    #[arg(long)]
    pub schema: Option<PathBuf>,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}



#[derive(Args, Debug)]
pub struct TestArgs {
    pub file: PathBuf,

    #[arg(long)]
    pub database: Option<PathBuf>,

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
    Install(JupyterInstallArgs),
    //Uninstall(JupyterUninstallArgs),
    Up(JupyterUpArgs),
}

#[derive(Args, Debug)]
pub struct JupyterInstallArgs {
    #[arg(long)]
    pub name: Option<String>,

    #[arg(long)]
    pub display: Option<String>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct JupyterUpArgs {
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
    Inline(DocsInlineArgs),
}

#[derive(Args, Debug)]
pub struct DocsInlineArgs {
    pub input: PathBuf,

    #[arg(long)]
    pub extension: Option<String>,

    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct TuiArgs {
    pub database: PathBuf,
    pub table: Option<String>,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

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

    /// Path to config file
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct LintArgs {
    /// SQL files to lint (reads from stdin if none provided)
    pub files: Vec<PathBuf>,

    /// Path to config file
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Apply auto-fixes where available
    #[arg(long)]
    pub fix: bool,
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
    pub database: PathBuf,
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
    Repl(ReplArgs),

    /// Run a read-only SQL query and output results to a file
    #[command(alias = "q", after_long_help = QUERY_AFTER_HELP)]
    Query(QueryArgs),

    /// Execute a write SQL statement on a database
    ///
    /// The write counterpart of `solite query`; prints a checkmark on success.
    #[command(alias = "exec", after_long_help = EXECUTE_AFTER_HELP)]
    Execute(ExecuteArgs),

    /// Run SQL-based inline tests in a single file
    Test(TestArgs),

    /// Manage the Solite Jupyter kernel
    Jupyter(JupyterNamespace),

    /// Tooling for documenting SQLite extensions
    Docs(DocsNamespace),

    /// Run benchmarks on SQL statements
    Bench(BenchArgs),

    /// Codegen SQL queries into an intermediate representation
    Codegen(CodegenArgs),

    /// Tui for exploring a database
    Tui(TuiArgs),

    /// Format SQL files
    #[command(alias = "fmt")]
    Format(FmtArgs),

    /// Lint SQL files for potential issues
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

const HELP_TEMPLATE: &str = "\
{name} {version}
{about}

{usage-heading} {usage}
       solite <file>.db        Open the REPL on a database (also .sqlite, .sqlite3)

Options:
{options}

Scripting and Query Execution:
  run              Run SQL scripts
  repl             Start a REPL on a SQLite database
  query            Run a read-only SQL query and output results to a file
  execute          Execute a write SQL statement on a database
  schema           Print the schema of a database

Tooling:
  backup           Back up a SQLite database to a file
  vacuum           Rebuild a database, repacking into minimal disk space
  jupyter          Manage the Solite Jupyter kernel
  tui              Tui for exploring a database
  test             Run SQL-based inline tests in a single file
  bench            Run benchmarks on SQL statements
  codegen          Codegen SQL queries into an intermediate representation
  docs             Tooling for documenting SQLite extensions

SQL:
  format           Format SQL files
  lint             Lint SQL files for potential issues
  lsp              Start the Language Server Protocol (LSP) server

Replication:
  stream           Streaming replication (sync/restore)

Compatibility:
  sqlite3          Run the sqlite3 shell directly
  diff             Output SQL to transform one database into another
  rsync            Efficiently replicate a SQLite database to a remote machine
";

#[derive(Parser)]
#[command(
  name = "solite",
  author,
  long_version = env!("CARGO_PKG_VERSION"),
  about = "Solite CLI",
  version,
  subcommand_required = false,
  arg_required_else_help = false,
  help_template = HELP_TEMPLATE,
)]
pub struct Cli {
    /// Allow SSH and remote transport connections
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
}
