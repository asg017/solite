use std::{collections::HashMap, env, path::{Path, PathBuf}};

use clap::{Args, Parser, Subcommand};
use solite_core::exporter::ExportFormat;

#[derive(Args, Debug)]
pub struct RunArgs {
    pub database: Option<PathBuf>,
    pub script: Option<PathBuf>,

    #[arg(long, short = 'p', num_args = 2)]
    pub parameters: Vec<String>,

    #[arg(long)]
    pub trace: Option<PathBuf>,
}

impl RunArgs {
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

impl Into<ExportFormat> for QueryFormat {
    fn into(self) -> ExportFormat {
        match self {
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
}

#[derive(Args, Debug)]
pub struct ExecuteArgs {
    pub database: Option<PathBuf>,
    pub statement: Vec<String>,

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
}

#[derive(Args, Debug)]
pub struct McpNamespace {
    #[command(subcommand)]
    pub command: McpCommand,
}
#[derive(Subcommand, Debug)]
pub enum McpCommand {
    Up(McpUpArgs),
    Install(McpInstallArgs),
}

#[derive(Args, Debug)]
pub struct McpUpArgs {
}
#[derive(Args, Debug)]
pub struct McpInstallArgs {
}

#[derive(Args, Debug)]
pub struct SnapNamespace {
    #[command(subcommand)]
    pub command: SnapCommand,
}

#[derive(Args, Debug)]
pub struct SnapTestArgs {
    pub file: PathBuf,

    #[arg(long)]
    pub trace: Option<PathBuf>,

    #[arg(long)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum SnapCommand {
    Test(SnapTestArgs),
    Review(SnapTestArgs),
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

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run SQL scripts
    Run(RunArgs),

    /// Start a REPL on a SQLite database
    Repl(ReplArgs),

    /// Run a read-only SQL query and output results to a file
    #[command(alias = "q")]
    Query(QueryArgs),

    #[command(alias = "exec")]
    /// Execute a write SQL statement on a database
    Execute(ExecuteArgs),

    /// Snapshot testing for extensions and SQL statements
    Snap(SnapNamespace),

    /// Manage the Solite Jupyter kernel
    Jupyter(JupyterNamespace),

    /// Tooling for documenting SQLite extensions
    Docs(DocsNamespace),

    /// Run benchmarks on SQL statements
    Bench(BenchArgs),

    /// MCP
    Mcp(McpNamespace),
    
    /// Codegen SQL queries into an intermediate representation
    Codegen(CodegenArgs)
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
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Box<Commands>,
}
