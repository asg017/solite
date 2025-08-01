mod bench;
mod load;
mod export;
mod open;
mod param;
mod print;
mod sh;
mod tables;
mod timer;
mod vegalite;

pub use crate::dot::{
  bench::BenchCommand,
  load::LoadCommand,
  sh::ShellCommand,
  print::PrintCommand,
  tables::TablesCommand,
  open::OpenCommand,
  export::ExportCommand,
  vegalite::VegaLiteCommand,
  param::ParameterCommand,
};
pub use load::LoadCommandSource;

use param::parse_parameter;
use thiserror::Error;

use crate::Runtime;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Error, Debug, PartialEq)]
pub enum ParseDotError {
    #[error("Unknown command '{0}'")]
    UnknownCommand(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("{0}")]
    Generic(String),
}





#[derive(Serialize, Debug)]
pub enum DotCommand {
    /*  introspection  */
    //Databases,
    Tables(TablesCommand),
    //Indexes,
    //Schema,

    /*  docs  */
    //Help,
    //Docs,

    /*  runtime  */
    /// .run file.sql -p a=1 -p b=2
    //Run,
    /// or .debugger?
    //Breakpoint,

    //Quit,
    //Exit,

    /// switches to different DB connection
    /// usage: .open file.db
    Open(OpenCommand),
    Load(LoadCommand),

    /// TODO sqlpkg/spm support
    //Load,
    Print(PrintCommand),

    Shell(ShellCommand),
    /// usage: .param set name 'alex garcia'
    Parameter(ParameterCommand),
    /// usage: .bail on/off
    //Bail,

    /// usage: .timer on/off
    Timer(bool),
    //
    //Mode,
    Export(ExportCommand),
    Vegalite(VegaLiteCommand),
    Bench(BenchCommand),
}

fn parse_bool(s: String) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "yes" | "y" | "on" => Ok(true),
        "no" | "n" | "off" => Ok(false),
        _ => Err(format!("Not a boolean value: {}", s)),
    }
}
pub fn parse_dot<S: Into<String>>(
    command: S,
    args: S,
    rest: &str,
    runtime: &mut Runtime,
) -> Result<DotCommand, ParseDotError> {
    let command = command.into();
    let args = args.into();
    match command.to_lowercase().as_str() {
        "print" => Ok(DotCommand::Print(PrintCommand { message: args })),
        "sh" => Ok(DotCommand::Shell(ShellCommand { command: args })),
        "tables" => Ok(DotCommand::Tables(TablesCommand {})),
        "open" => Ok(DotCommand::Open(OpenCommand { path: args })),
        "export" => Ok(DotCommand::Export(ExportCommand::new(args, runtime, rest)?)),
        "bench" => Ok(DotCommand::Bench(BenchCommand::new(args, runtime, rest)?)),
        "vl" | "vegalite" => Ok(DotCommand::Vegalite(VegaLiteCommand::new(
            args, runtime, rest,
        )?)),
        "load" => Ok(DotCommand::Load(LoadCommand::new(args))),
        "timer" => Ok(DotCommand::Timer(
            parse_bool(args).map_err(ParseDotError::InvalidArgument)?,
        )),
        "param" | "parameter" => Ok(DotCommand::Parameter(parse_parameter(args))),
        _ => Err(ParseDotError::UnknownCommand(command)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_dot() {}
}
