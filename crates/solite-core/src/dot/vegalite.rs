use serde::Serialize;
use serde_json::Map;
use crate::{
    sqlite::Statement,
    Runtime,
    ParseDotError,
};
#[derive(Serialize, Debug)]
pub struct VegaLiteCommand {
    pub statement: Statement,
    pub mark: String,
    pub rest_length: usize,
}

impl VegaLiteCommand {
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
        match runtime.prepare_with_parameters(rest) {
            Ok((rest2, Some(stmt))) => {
                Ok(Self {
                    statement: stmt,
                    mark: args.trim().to_string(),
                    // TODO: suspicious
                    rest_length: rest2.unwrap_or(rest.len()),
                })
            }
            _ => todo!(),
        }
    }
    pub fn execute(&mut self) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
        let columns = self.statement.column_meta();
        //let mut column_types = HashMap::new();
        let mut data = vec![];
        loop {
            match self.statement.nextx() {
                Ok(Some(row)) => {
                    let mut obj = serde_json::Map::new();
                    for (idx, column) in columns.iter().enumerate() {
                        let value = match row.value_at(idx).value {
                            crate::sqlite::ValueRefXValue::Blob(_) => serde_json::Value::Null,
                            crate::sqlite::ValueRefXValue::Int(value) => {
                                serde_json::Value::Number(value.into())
                            }
                            crate::sqlite::ValueRefXValue::Double(value) => {
                                serde_json::Number::from_f64(value)
                                    .map(serde_json::Value::Number)
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            crate::sqlite::ValueRefXValue::Text(value) => {
                                serde_json::Value::String(
                                    std::str::from_utf8(value).unwrap_or("").to_string(),
                                )
                            }
                            crate::sqlite::ValueRefXValue::Null => serde_json::Value::Null,
                        };
                        obj.insert(column.name.clone(), value);
                    }
                    data.push(obj);
                }
                Ok(None) => break,
                Err(_) => todo!(),
            }
        }

        let mut encoding = Map::new();
        for column in columns {
            encoding.insert(
                column.name.to_string(),
                serde_json::json!({
                  "field": column.name,
                  "type": if  column.name =="x" || column.name == "y" {"quantitative"} else {"nominal"},
                  //"type": column.column_type,
                }),
            );
        }

        let data = serde_json::json!({
          "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
          "description": "A simple bar chart with embedded data.",
          "data": {
            "values": data,
          },
          "mark": self.mark,
          "encoding": encoding
        });
        Ok(data.as_object().cloned().unwrap())
    }
}
