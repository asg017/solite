use serde::Serialize;
use crate::{
    exporter::write_output,
    sqlite::{OwnedValue,Statement},
    Runtime,
    ParseDotError
};
use std::path::PathBuf;
use regex::{Captures, Regex};


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
                let re = Regex::new(r":[\w]+").unwrap();
                let target = re.replace_all(&args, |cap:&Captures| {
                    let param_name = &cap[0].strip_prefix(":").unwrap(); 
                    match runtime.lookup_parameter(param_name) {
                        Some(value) => match value {
                          OwnedValue::Text(text) => {
                            std::str::from_utf8(&text).unwrap().to_string()
                          },
                          _ => "".to_owned(),
                        },
                        None => {
                            "".to_owned()
                        }
                    }
                });
                Ok(Self {
                    target: PathBuf::from(target.to_string()),
                    statement: stmt,
                    // TODO: suspicious
                    rest_length: rest2.unwrap_or(rest.len()),
                })
            }
            _ => todo!(),
        }
    }
    pub fn execute(&mut self) -> anyhow::Result<()> {
        let output = crate::exporter::output_from_path(&self.target)
            .map_err(|e| ParseDotError::Generic(e.to_string()))?;
        let format = crate::exporter::format_from_path(&self.target).unwrap();
        write_output(&mut self.statement, output, format).unwrap();
        Ok(())
    }
}
