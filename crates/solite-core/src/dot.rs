use std::{
    io::{BufRead, BufReader}, path::PathBuf, sync::mpsc::Receiver
};

use solite_stdlib::solite_stdlib_init;
use thiserror::Error;

use crate::{exporter::write_output, sqlite::{Connection, Statement}, Runtime};
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

#[derive(Serialize, Debug, PartialEq)]
pub struct PrintCommand {
    pub message: String,
}
impl PrintCommand {
    pub fn execute(&self) {
        println!("{}", self.message);
    }
}
#[derive(Serialize, Debug, PartialEq)]
pub struct ShellCommand {
    pub command: String,
}
impl ShellCommand {
    pub fn execute(&self) -> Receiver<String> {
        let (tx, mut rx) = std::sync::mpsc::channel::<String>();
        let command = self.command.clone();
        std::thread::spawn(move || {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let reader = BufReader::with_capacity(1, stdout); //new(stdout);
            let mut lines = reader.lines();
            while let Some(Ok(line)) = lines.next() {
                tx.send(line).unwrap();
            }
            let _ = child.wait();
        });
        return rx;
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct TablesCommand {}
impl TablesCommand {
    pub fn execute(&self, runtime: &Runtime) {
        let stmt = runtime
            .connection
            .prepare(
                r#"
                select name
                from pragma_table_list
                where "schema" = 'main'
                  and type in ('table', 'view')
                  and name not like 'sqlite_%'
                order by name;
                "#,
            )
            .unwrap()
            .1
            .unwrap();
        let mut tables = vec![];
        while let Ok(Some(row)) = stmt.next() {
            tables.push(row.get(0).unwrap().as_str().to_owned());
        }
        for table in tables {
            println!("{table}")
        }
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub struct OpenCommand {
    pub path: String,
}
impl OpenCommand {
    pub fn execute(&self, runtime: &mut Runtime) {
        runtime.connection = Connection::open(&self.path).unwrap();
        unsafe {
            solite_stdlib_init(
                runtime.connection.db(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
}

#[derive(Serialize, Debug)]
pub struct ExportCommand {
    pub target: PathBuf,
    pub statement: Statement,
    pub rest_length: usize,
}

impl ExportCommand {
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
      match runtime.prepare_with_parameters(rest) {
        Ok((rest2, Some(stmt))) => {
          Ok(Self {
            target: PathBuf::from(args),
            statement: stmt,
            // TODO: suspicious
            rest_length: rest2.unwrap_or(rest.len())
          })
        },
        _ => todo!(),
    }
  }
    pub fn execute(&mut self) -> anyhow::Result<()> {
        let output = crate::exporter::output_from_path(&self.target)
            .map_err(|e| ParseDotError::Generic(e.to_string()))?;
        let format = crate::exporter::format_from_path(&self.target)
            .unwrap();
          write_output(&mut self.statement, output, format).unwrap();
        Ok(())
  }

}

#[derive(Serialize, Debug)]
pub struct VegaLiteCommand {
    pub statement: Statement,
    pub rest_length: usize,
}

impl VegaLiteCommand {
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
      match runtime.prepare_with_parameters(rest) {
        Ok((rest2, Some(stmt))) => {
          Ok(Self {
            statement: stmt,
            // TODO: suspicious
            rest_length: rest2.unwrap_or(rest.len())
          })
        },
        _ => todo!(),
    }
  }
    pub fn execute(&mut self) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
        let columns = self.statement.column_names().unwrap();
        let mark = match columns[0].as_str() { 
          "bar" => "bar",
          _ => todo!(),
        };
        let mut data = vec![];
        loop {
          match self.statement.nextx() {
            Ok(Some(row)) => {
              let x = match row.value_at(1).value {
                crate::sqlite::ValueRefXValue::Blob(_) => serde_json::Value::Null,
                crate::sqlite::ValueRefXValue::Int(value) => serde_json::Value::Number(value.into()),
                crate::sqlite::ValueRefXValue::Double(value) => serde_json::Number::from_f64(value).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
                crate::sqlite::ValueRefXValue::Text(value) => serde_json::Value::Null,
                crate::sqlite::ValueRefXValue::Null => serde_json::Value::Null,
              };
              let y = match row.value_at(2).value {
                crate::sqlite::ValueRefXValue::Blob(_) => serde_json::Value::Null,
                crate::sqlite::ValueRefXValue::Int(value) => serde_json::Value::Number(value.into()),
                crate::sqlite::ValueRefXValue::Double(value) => serde_json::Number::from_f64(value).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
                crate::sqlite::ValueRefXValue::Text(value) => serde_json::Value::Null,
                crate::sqlite::ValueRefXValue::Null => serde_json::Value::Null,
              };
              data.push(serde_json::json!({
                "x": x,
                "y": y,
              }));
            },
            Ok(None) => break,
            Err(_) => todo!(),
            }
        }

        let data = serde_json::json!({
          "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
          "description": "A simple bar chart with embedded data.",
          "data": {
            "values": data,
          },
          "mark": mark,
          "encoding": {
            "x": {"field": "x", "type": "quantitative"},
            "y": {"field": "y", "type": "quantitative"}
          }
        });
        Ok(data.as_object().cloned().unwrap())
  }

}

#[derive(Serialize, Debug, PartialEq)]
pub struct LoadCommand {
    pub path: String,
    pub entrypoint: Option<String>,
    pub is_uv: bool,
}

pub enum LoadCommandSource {
    Path(String),
    Uv { directory: String, package: String },
}

impl LoadCommand {
    pub fn new(args: String) -> Self {
        let (args, is_uv) = match args.strip_prefix("uv:") {
            Some(args) => (args, true),
            None => (args.as_str(), false),
        };

        let (path, entrypoint) = match args.split_once(' ') {
            Some((path, entrypoint)) => (path.to_string(), Some(entrypoint.trim().to_string())),
            None => (args.to_owned(), None),
        };
        Self {
            path,
            entrypoint,
            is_uv,
        }
    }
    pub fn execute(&self, connection: &mut Connection) -> anyhow::Result<LoadCommandSource> {
        if self.is_uv {
            crate::load_uv::load(connection, &self.path, &self.entrypoint).map(|path| {
                LoadCommandSource::Uv {
                    directory: path,
                    package: self.path.clone(),
                }
            })
        } else {
            connection
                .load_extension(&self.path, &self.entrypoint)
                .map(|_| LoadCommandSource::Path(self.path.clone()))
        }
    }
}

#[derive(Serialize, Debug, PartialEq)]
pub enum ParameterCommand {
    Set { key: String, value: String },
    Unset(String),
    List,
    Clear,
}

fn parse_parameter(line: String) -> ParameterCommand {
    match line.trim_end().split_once(' ') {
        Some((word, rest)) => match word {
            "set" => {
                let (k, v) = rest.split_once(' ').unwrap();
                ParameterCommand::Set {
                    key: k.to_owned(),
                    value: v.to_owned(),
                }
            }
            "unset" => ParameterCommand::Unset(rest.to_owned()),
            _ => todo!(),
        },
        None => match line.trim_end() {
            "list" => ParameterCommand::List,
            "clear" => ParameterCommand::Clear,
            _ => todo!(),
        },
    }
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
}

fn parse_bool(s: String) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "yes" | "y" | "on" => Ok(true),
        "no" | "n" | "off" => Ok(false),
        _ => Err(format!("Not a boolean value: {}", s)),
    }
}
pub fn parse_dot<S: Into<String>>(command: S, args: S, rest: &str, runtime: &mut Runtime) -> Result<DotCommand, ParseDotError> {
    let command = command.into();
    let args = args.into();
    match command.to_lowercase().as_str() {
        "print" => Ok(DotCommand::Print(PrintCommand { message: args })),
        "sh" => Ok(DotCommand::Shell(ShellCommand { command: args })),
        "tables" => Ok(DotCommand::Tables(TablesCommand {})),
        "open" => Ok(DotCommand::Open(OpenCommand { path: args })),
        "export" => Ok(DotCommand::Export(ExportCommand::new(args, runtime, rest)?)),
        "vl" | "vegalite" => Ok(DotCommand::Vegalite(VegaLiteCommand::new(args, runtime, rest)?)),
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
    use super::*;

    #[test]
    fn test_parse_dot() {
    }
}