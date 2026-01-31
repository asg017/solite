//! Dot command parsing and execution.
//!
//! This module provides the `.command` functionality for the REPL and scripts,
//! including commands like `.tables`, `.schema`, `.load`, `.param`, etc.

mod ask;
pub mod bench;
mod clear;
mod dotenv;
pub mod env;
mod export;
mod graphviz;
mod load;
mod open;
pub mod param;
mod print;
mod schema;
pub mod sh;
mod tables;
mod timer;
mod tui;
mod vegalite;

pub use crate::dot::{
    ask::AskCommand,
    bench::BenchCommand,
    clear::ClearCommand,
    env::{EnvAction, EnvCommand},
    export::ExportCommand,
    graphviz::GraphvizCommand,
    load::LoadCommand,
    open::OpenCommand,
    param::ParameterCommand,
    print::PrintCommand,
    schema::SchemaCommand,
    sh::ShellCommand,
    tables::TablesCommand,
    tui::TuiCommand,
    vegalite::VegaLiteCommand,
};
pub use load::LoadCommandSource;

use crate::{dot::dotenv::DotenvCommand, Runtime};
use env::parse_env;
use param::parse_parameter;
use serde::{Deserialize, Serialize};
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
    fn test_parse_dot_error_display() {
        let err = ParseDotError::UnknownCommand("foo".to_string());
        assert_eq!(err.to_string(), "Unknown command 'foo'");

        let err = ParseDotError::InvalidArgument("bad arg".to_string());
        assert_eq!(err.to_string(), "Invalid argument: bad arg");

        let err = ParseDotError::Generic("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }
}
