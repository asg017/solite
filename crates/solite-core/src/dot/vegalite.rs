//! Vega-Lite chart generation.
//!
//! This module implements the `.vegalite` (or `.vl`) command which generates
//! Vega-Lite JSON specifications from query results.
//!
//! # Usage
//!
//! ```sql
//! .vl bar SELECT category, count(*) as y FROM sales GROUP BY category
//! .vegalite line SELECT date as x, revenue as y FROM daily_stats
//! ```
//!
//! # Mark Types
//!
//! Supported mark types: bar, line, point, area, circle, square, tick, etc.
//!
//! # Encoding
//!
//! - Columns named `x` or `y` are treated as quantitative
//! - Other columns are treated as nominal
//!
//! # Output
//!
//! Generates a Vega-Lite v6 JSON specification that can be rendered
//! with any Vega-Lite compatible renderer.

use crate::sqlite::Statement;
use crate::{ParseDotError, Runtime};
use serde::Serialize;
use serde_json::Map;

/// Command to generate a Vega-Lite chart specification.
#[derive(Serialize, Debug)]
pub struct VegaLiteCommand {
    /// Prepared statement to execute.
    pub statement: Statement,
    /// The mark type (bar, line, point, etc.).
    pub mark: String,
    /// Length consumed from rest input.
    pub rest_length: usize,
}

impl VegaLiteCommand {
    /// Create a new Vega-Lite command from arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - The mark type
    /// * `runtime` - The runtime context
    /// * `rest` - The SQL query to execute
    ///
    /// # Errors
    ///
    /// Returns `ParseDotError` if the SQL cannot be prepared.
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
        let (rest_len, stmt) = runtime
            .prepare_with_parameters(rest)
            .map_err(|e| ParseDotError::Generic(format!("Failed to prepare query: {}", e)))?;

        let stmt = stmt.ok_or_else(|| ParseDotError::Generic("No SQL statement provided".into()))?;

        Ok(Self {
            statement: stmt,
            mark: args.trim().to_string(),
            rest_length: rest_len.unwrap_or(rest.len()),
        })
    }

    /// Execute the command and generate a Vega-Lite specification.
    ///
    /// # Returns
    ///
    /// A JSON object containing the Vega-Lite specification.
    pub fn execute(&mut self) -> anyhow::Result<Map<String, serde_json::Value>> {
        let columns = self.statement.column_meta();
        let mut data = Vec::new();

        loop {
            match self.statement.nextx() {
                Ok(Some(row)) => {
                    let mut obj = Map::new();
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
                Err(e) => return Err(anyhow::anyhow!("Query execution failed: {}", e)),
            }
        }

        // Build encoding based on column names
        let mut encoding = Map::new();
        for column in columns {
            let field_type = if column.name == "x" || column.name == "y" {
                "quantitative"
            } else {
                "nominal"
            };

            encoding.insert(
                column.name.to_string(),
                serde_json::json!({
                    "field": column.name,
                    "type": field_type,
                }),
            );
        }

        let spec = serde_json::json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
            "description": "Generated Vega-Lite chart",
            "data": {
                "values": data,
            },
            "mark": self.mark,
            "encoding": encoding
        });

        // Convert to Map - spec is always an object
        match spec {
            serde_json::Value::Object(map) => Ok(map),
            _ => unreachable!("spec is always an object"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vegalite_bar_chart() {
        let mut runtime = Runtime::new(None);

        // Create test data
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE sales (category TEXT, amount INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("INSERT INTO sales VALUES ('A', 10), ('B', 20), ('C', 15)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = VegaLiteCommand::new(
            "bar".to_string(),
            &mut runtime,
            "SELECT category, amount as y FROM sales",
        )
        .unwrap();

        let result = cmd.execute();
        assert!(result.is_ok());

        let spec = result.unwrap();
        assert_eq!(
            spec.get("$schema").and_then(|v| v.as_str()),
            Some("https://vega.github.io/schema/vega-lite/v6.json")
        );
        assert_eq!(spec.get("mark").and_then(|v| v.as_str()), Some("bar"));

        let data = spec
            .get("data")
            .and_then(|d| d.get("values"))
            .and_then(|v| v.as_array());
        assert!(data.is_some());
        assert_eq!(data.unwrap().len(), 3);
    }

    #[test]
    fn test_vegalite_encoding_types() {
        let mut runtime = Runtime::new(None);

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE data (x INTEGER, y INTEGER, label TEXT)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let (_, stmt) = runtime
            .connection
            .prepare("INSERT INTO data VALUES (1, 10, 'a')")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = VegaLiteCommand::new(
            "point".to_string(),
            &mut runtime,
            "SELECT x, y, label FROM data",
        )
        .unwrap();

        let result = cmd.execute();
        assert!(result.is_ok());

        let spec = result.unwrap();
        let encoding = spec.get("encoding").and_then(|e| e.as_object());
        assert!(encoding.is_some());

        let encoding = encoding.unwrap();

        // x and y should be quantitative
        assert_eq!(
            encoding
                .get("x")
                .and_then(|x| x.get("type"))
                .and_then(|t| t.as_str()),
            Some("quantitative")
        );
        assert_eq!(
            encoding
                .get("y")
                .and_then(|y| y.get("type"))
                .and_then(|t| t.as_str()),
            Some("quantitative")
        );

        // label should be nominal
        assert_eq!(
            encoding
                .get("label")
                .and_then(|l| l.get("type"))
                .and_then(|t| t.as_str()),
            Some("nominal")
        );
    }

    #[test]
    fn test_vegalite_empty_data() {
        let mut runtime = Runtime::new(None);

        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE empty (x INTEGER)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let mut cmd = VegaLiteCommand::new(
            "line".to_string(),
            &mut runtime,
            "SELECT x FROM empty",
        )
        .unwrap();

        let result = cmd.execute();
        assert!(result.is_ok());

        let spec = result.unwrap();
        let data = spec
            .get("data")
            .and_then(|d| d.get("values"))
            .and_then(|v| v.as_array());
        assert!(data.is_some());
        assert!(data.unwrap().is_empty());
    }
}
