use std::{collections::HashMap, env, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use solite_core::exporter::ExportFormat;

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Positional args: [database] [script.sql] [procedureName]
    pub args: Vec<String>,

    /// Execute SQL/dot commands from the given string (instead of a .sql file)
    #[arg(long, short = 'c')]
    pub command: Option<String>,

    #[arg(long, short = 'p', num_args = 2)]
    pub parameters: Vec<String>,

    #[arg(long)]
    pub trace: Option<PathBuf>,

    #[arg(long, alias = "read-only")]
    pub readonly: bool,
}

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
    Ndjson,
    Value,
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

#[derive(Args, Debug)]
pub struct QueryArgs {
    pub statement: String,
    pub database: Option<PathBuf>,

    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    #[arg(long, short = 'f', value_enum)]
    pub format: Option<QueryFormat>,

    #[arg(long, short = 'p', num_args = 2)]
    pub parameters: Vec<String>,

    #[arg(long)]
    pub load_extension: Option<Vec<PathBuf>>,
}

#[derive(Args, Debug)]
pub struct ExecuteArgs {
    #[arg(num_args = 1..=2)]
    pub args: Vec<String>,

    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    #[arg(long, short = 'f', value_enum)]
    pub format: Option<QueryFormat>,

    #[arg(long, short = 'p', num_args = 2)]
    pub parameters: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ReplArgs {
    pub database: Option<PathBuf>,
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
    /// Path to config file
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
    Run(RunArgs),

    /// Start a REPL on a SQLite database
    Repl(ReplArgs),

    /// Run a read-only SQL query and output results to a file
    #[command(alias = "q")]
    Query(QueryArgs),

    /// Execute a write SQL statement on a database
    #[command(alias = "exec")]
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

    /// Streaming replication, like litestream
    #[cfg(feature = "ritestream")]
    Stream(StreamNamespace),
}

const HELP_TEMPLATE: &str = "\
{name} {version}
{about}

{usage-heading} {usage}

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
    #[command(subcommand)]
    pub command: Box<Commands>,
}
