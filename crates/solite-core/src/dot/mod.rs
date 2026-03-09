//! Dot command parsing and execution.
//!
//! This module provides the `.command` functionality for the REPL and scripts,
//! including commands like `.tables`, `.schema`, `.load`, `.param`, etc.
//!
//! # Overview
//!
//! Dot commands are special commands that start with a `.` (period) and provide
//! various utilities for database introspection, configuration, and execution.
//!
//! # Available Commands
//!
//! - `.tables [schema]` - List tables and views
//! - `.schema` - Show CREATE statements
//! - `.graphviz` / `.gv` - Generate ERD in DOT format
//! - `.open <path>` - Open a different database
//! - `.load <path>` - Load an extension
//! - `.param set/unset/list/clear` - Manage query parameters
//! - `.env set/unset` - Manage environment variables
//! - `.dotenv` - Load .env file
//! - `.export <path> <query>` - Export query results
//! - `.bench <query>` - Benchmark query execution
//! - `.vegalite <mark> <query>` - Generate Vega-Lite chart
//! - `.sh <command>` - Execute shell command
//! - `.ask <question>` - Ask AI assistant
//! - `.timer on/off` - Toggle query timing
//! - `.clear` / `.c` - Clear screen
//! - `.print <message>` - Print a message

mod ask;
pub mod bench;
mod call;
mod clear;
mod dotenv;
pub mod env;
mod export;
mod graphviz;
mod load;
mod open;
pub mod param;
mod print;
mod run;
mod schema;
pub mod sh;
mod tables;
mod timer;
mod tui;
mod vegalite;

#[cfg(feature = "ritestream")]
pub mod stream;

pub use crate::dot::{
    ask::AskCommand,
    bench::BenchCommand,
    call::CallCommand,
    clear::ClearCommand,
    dotenv::{DotenvCommand, DotenvResult},
    env::{EnvAction, EnvCommand},
    export::ExportCommand,
    graphviz::GraphvizCommand,
    load::LoadCommand,
    open::OpenCommand,
    param::ParameterCommand,
    print::PrintCommand,
    run::RunCommand,
    schema::SchemaCommand,
    sh::ShellCommand,
    tables::TablesCommand,
    tui::TuiCommand,
    vegalite::VegaLiteCommand,
};
#[cfg(feature = "ritestream")]
pub use stream::{StreamAction, StreamCommand, StreamSyncResult};
pub use load::LoadCommandSource;

use crate::Runtime;
use env::parse_env;
use param::parse_parameter;
use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

/// Errors that can occur during dot command parsing.
#[derive(Serialize, Deserialize, Error, Debug, PartialEq)]
pub enum ParseDotError {
    /// Unknown command name.
    #[error("Unknown command '{0}'")]
    UnknownCommand(String),
    /// Invalid argument provided to command.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    /// Generic error message.
    #[error("{0}")]
    Generic(String),
}

/// Errors that can occur during dot command execution.
#[derive(Error, Debug)]
pub enum DotError {
    /// SQLite error during command execution.
    #[error("SQLite error: {0}")]
    Sqlite(crate::sqlite::SQLiteError),

    /// I/O error (file operations, shell commands, etc.).
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Environment variable error.
    #[error("Environment error: {0}")]
    Env(#[from] std::env::VarError),

    /// Missing or invalid data.
    #[error("{0}")]
    InvalidData(String),

    /// File not found.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Extension loading error.
    #[error("Extension error: {0}")]
    Extension(String),

    /// Command execution error.
    #[error("Command failed: {0}")]
    Command(String),
}

impl From<crate::sqlite::SQLiteError> for DotError {
    fn from(err: crate::sqlite::SQLiteError) -> Self {
        DotError::Sqlite(err)
    }
}

/// All supported dot commands.
#[derive(Serialize, Debug)]
pub enum DotCommand {
    // Introspection
    /// List tables in database.
    Tables(TablesCommand),
    /// Show schema definitions.
    Schema(SchemaCommand),
    /// Generate Graphviz ERD.
    Graphviz(GraphvizCommand),

    // Runtime
    /// Open a different database.
    Open(OpenCommand),
    /// Load an extension.
    Load(LoadCommand),
    /// Open TUI mode.
    Tui(TuiCommand),
    /// Clear screen.
    Clear(ClearCommand),

    /// Print a message.
    Print(PrintCommand),
    /// Ask AI assistant.
    Ask(AskCommand),

    /// Execute shell command.
    Shell(ShellCommand),
    /// Manage query parameters.
    Parameter(ParameterCommand),
    /// Manage environment variables.
    Env(EnvCommand),

    /// Toggle timer display.
    Timer(bool),
    /// Export query results.
    Export(ExportCommand),
    /// Generate Vega-Lite chart.
    Vegalite(VegaLiteCommand),
    /// Run benchmark.
    Bench(BenchCommand),
    /// Load .env file.
    Dotenv(DotenvCommand),
    /// Call a registered procedure.
    Call(CallCommand),
    /// Run a SQL file inline.
    Run(RunCommand),

    /// Stream replication command.
    #[cfg(feature = "ritestream")]
    Stream(StreamCommand),
}

/// Parse a boolean value from string.
///
/// Accepts: "yes", "y", "on" for true; "no", "n", "off" for false.
fn parse_bool(s: &str) -> Result<bool, ParseDotError> {
    match s.to_lowercase().as_str() {
        "yes" | "y" | "on" | "1" | "true" => Ok(true),
        "no" | "n" | "off" | "0" | "false" => Ok(false),
        _ => Err(ParseDotError::InvalidArgument(format!(
            "not a boolean value: '{}' (use yes/no, on/off, or true/false)",
            s
        ))),
    }
}

/// Parse a dot command from its components.
///
/// # Arguments
///
/// * `command` - The command name (e.g., "tables", "load")
/// * `args` - Arguments passed to the command
/// * `rest` - Remaining input (for multi-line commands)
/// * `runtime` - The runtime context
pub fn parse_dot<S: Into<String>>(
    command: S,
    args: S,
    rest: &str,
    runtime: &mut Runtime,
) -> Result<DotCommand, ParseDotError> {
    let command = command.into();
    let args = args.into();

    match command.to_lowercase().as_str() {
        "ask" => Ok(DotCommand::Ask(AskCommand { message: args })),
        "print" => Ok(DotCommand::Print(PrintCommand { message: args })),
        "sh" => Ok(DotCommand::Shell(ShellCommand { command: args })),
        "tables" => Ok(DotCommand::Tables(TablesCommand {
            schema: if args.is_empty() {
                None
            } else {
                Some(args.trim().to_string())
            },
        })),
        "schema" => Ok(DotCommand::Schema(SchemaCommand {})),
        "open" => Ok(DotCommand::Open(OpenCommand { path: args })),
        "tui" => Ok(DotCommand::Tui(TuiCommand {})),
        "c" | "clear" => Ok(DotCommand::Clear(ClearCommand {})),
        "graphviz" | "gv" => Ok(DotCommand::Graphviz(GraphvizCommand {})),
        "dotenv" | "loadenv" => Ok(DotCommand::Dotenv(DotenvCommand {})),
        "export" => Ok(DotCommand::Export(ExportCommand::new(args, runtime, rest)?)),
        "bench" => Ok(DotCommand::Bench(BenchCommand::new(args, runtime, rest)?)),
        "vl" | "vegalite" => Ok(DotCommand::Vegalite(VegaLiteCommand::new(
            args, runtime, rest,
        )?)),
        "load" => Ok(DotCommand::Load(LoadCommand::new(args))),
        "timer" => Ok(DotCommand::Timer(parse_bool(&args)?)),
        "param" | "parameter" => Ok(DotCommand::Parameter(parse_parameter(args)?)),
        "env" => Ok(DotCommand::Env(parse_env(args)?)),
        "run" => {
            let tokens: Vec<&str> = args.split_whitespace().collect();
            if tokens.is_empty() {
                return Err(ParseDotError::InvalidArgument(
                    "usage: .run <file.sql> [procedureName] [--key=value ...]".to_string(),
                ));
            }
            let file = tokens[0].to_string();
            let mut procedure: Option<String> = None;
            let mut parameters = std::collections::HashMap::new();
            let mut i = 1;
            while i < tokens.len() {
                let tok = tokens[i];
                if tok.starts_with("--") {
                    let key_part = &tok[2..];
                    if let Some((k, v)) = key_part.split_once('=') {
                        if k.is_empty() {
                            return Err(ParseDotError::InvalidArgument(
                                format!("empty parameter key in '{}'", tok),
                            ));
                        }
                        parameters.insert(k.to_string(), v.to_string());
                    } else {
                        if key_part.is_empty() {
                            return Err(ParseDotError::InvalidArgument(
                                "empty parameter key".to_string(),
                            ));
                        }
                        i += 1;
                        if i >= tokens.len() {
                            return Err(ParseDotError::InvalidArgument(
                                format!("parameter '--{}' requires a value", key_part),
                            ));
                        }
                        parameters.insert(key_part.to_string(), tokens[i].to_string());
                    }
                } else {
                    if procedure.is_some() {
                        return Err(ParseDotError::InvalidArgument(
                            format!("unexpected argument '{}'", tok),
                        ));
                    }
                    procedure = Some(tok.to_string());
                }
                i += 1;
            }
            Ok(DotCommand::Run(RunCommand {
                file,
                procedure,
                parameters,
            }))
        }
        #[cfg(feature = "ritestream")]
        "stream" => {
            let tokens: Vec<&str> = args.split_whitespace().collect();
            if tokens.len() != 2 {
                return Err(ParseDotError::InvalidArgument(
                    "usage: .stream sync <url> | .stream restore <url>".to_string(),
                ));
            }
            let action = match tokens[0] {
                "sync" => stream::StreamAction::Sync {
                    url: tokens[1].to_string(),
                },
                "restore" => stream::StreamAction::Restore {
                    url: tokens[1].to_string(),
                },
                other => {
                    return Err(ParseDotError::InvalidArgument(format!(
                        "unknown stream action '{}' (expected 'sync' or 'restore')",
                        other
                    )));
                }
            };
            Ok(DotCommand::Stream(stream::StreamCommand { action }))
        }
        "call" => {
            // Strip trailing -- comment (used as epilogue in test assertions)
            let args_clean = match args.find(" --") {
                Some(idx) => args[..idx].trim(),
                None => args.trim(),
            };
            let parts: Vec<&str> = args_clean.split_whitespace().collect();
            match parts.len() {
                1 => Ok(DotCommand::Call(CallCommand {
                    file: None,
                    procedure_name: parts[0].to_string(),
                })),
                2 => Ok(DotCommand::Call(CallCommand {
                    file: Some(parts[0].to_string()),
                    procedure_name: parts[1].to_string(),
                })),
                _ => Err(ParseDotError::InvalidArgument(
                    "usage: .call [file.sql] procedureName".to_string(),
                )),
            }
        }
        _ => Err(ParseDotError::UnknownCommand(command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool_true_variants() {
        assert!(parse_bool("yes").unwrap());
        assert!(parse_bool("YES").unwrap());
        assert!(parse_bool("y").unwrap());
        assert!(parse_bool("Y").unwrap());
        assert!(parse_bool("on").unwrap());
        assert!(parse_bool("ON").unwrap());
        assert!(parse_bool("1").unwrap());
        assert!(parse_bool("true").unwrap());
        assert!(parse_bool("TRUE").unwrap());
    }

    #[test]
    fn test_parse_bool_false_variants() {
        assert!(!parse_bool("no").unwrap());
        assert!(!parse_bool("NO").unwrap());
        assert!(!parse_bool("n").unwrap());
        assert!(!parse_bool("N").unwrap());
        assert!(!parse_bool("off").unwrap());
        assert!(!parse_bool("OFF").unwrap());
        assert!(!parse_bool("0").unwrap());
        assert!(!parse_bool("false").unwrap());
        assert!(!parse_bool("FALSE").unwrap());
    }

    #[test]
    fn test_parse_bool_invalid() {
        assert!(parse_bool("maybe").is_err());
        assert!(parse_bool("").is_err());
        assert!(parse_bool("yesno").is_err());
    }

    #[test]
    fn test_parse_run_file_only() {
        let mut rt = crate::Runtime::new(None);
        let cmd = parse_dot("run", "file.sql", "", &mut rt).unwrap();
        match cmd {
            DotCommand::Run(run_cmd) => {
                assert_eq!(run_cmd.file, "file.sql");
                assert!(run_cmd.procedure.is_none());
                assert!(run_cmd.parameters.is_empty());
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_file_and_procedure() {
        let mut rt = crate::Runtime::new(None);
        let cmd = parse_dot("run", "file.sql procName", "", &mut rt).unwrap();
        match cmd {
            DotCommand::Run(run_cmd) => {
                assert_eq!(run_cmd.file, "file.sql");
                assert_eq!(run_cmd.procedure.as_deref(), Some("procName"));
                assert!(run_cmd.parameters.is_empty());
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_file_and_params() {
        let mut rt = crate::Runtime::new(None);
        let cmd = parse_dot("run", "file.sql --name=alex --age 20", "", &mut rt).unwrap();
        match cmd {
            DotCommand::Run(run_cmd) => {
                assert_eq!(run_cmd.file, "file.sql");
                assert!(run_cmd.procedure.is_none());
                assert_eq!(run_cmd.parameters.get("name").unwrap(), "alex");
                assert_eq!(run_cmd.parameters.get("age").unwrap(), "20");
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_file_procedure_and_params() {
        let mut rt = crate::Runtime::new(None);
        let cmd = parse_dot("run", "file.sql procName --name alex --age=20", "", &mut rt).unwrap();
        match cmd {
            DotCommand::Run(run_cmd) => {
                assert_eq!(run_cmd.file, "file.sql");
                assert_eq!(run_cmd.procedure.as_deref(), Some("procName"));
                assert_eq!(run_cmd.parameters.get("name").unwrap(), "alex");
                assert_eq!(run_cmd.parameters.get("age").unwrap(), "20");
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_parse_run_missing_file() {
        let mut rt = crate::Runtime::new(None);
        let result = parse_dot("run", "", "", &mut rt);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_run_param_missing_value() {
        let mut rt = crate::Runtime::new(None);
        let result = parse_dot("run", "file.sql --name", "", &mut rt);
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseDotError::InvalidArgument(msg) => {
                assert!(msg.contains("requires a value"));
            }
            _ => panic!("Expected InvalidArgument error"),
        }
    }

    #[test]
    fn test_parse_dot_error_display() {
        let err = ParseDotError::UnknownCommand("foo".to_string());
        assert_eq!(err.to_string(), "Unknown command 'foo'");

        let err = ParseDotError::InvalidArgument("bad arg".to_string());
        assert_eq!(err.to_string(), "Invalid argument: bad arg");

        let err = ParseDotError::Generic("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }
}
