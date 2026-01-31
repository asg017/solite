//! AI assistant integration.
//!
//! This module implements the `.ask` command which uses an AI model to
//! generate SQL queries from natural language questions.
//!
//! # Usage
//!
//! ```sql
//! .ask What are the top 10 customers by order count?
//! ```
//!
//! # Configuration
//!
//! Requires the `OPENROUTER_API_KEY` environment variable to be set.
//!
//! # TODO
//!
//! - Make prompt template configurable
//! - Make model configurable
//! - Make endpoint configurable

use crate::dot::DotError;
use crate::Runtime;
use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};

/// System prompt template for SQL generation.
static PROMPT: &str = r#"
Given the following SQLite database schema,
write a SQL query to answer the question below.
Use the most efficient query possible.
Provide only the SQL query as output.


{SCHEMA}

{QUESTION}

"#;

/// Command to ask the AI assistant for SQL help.
#[derive(Serialize, Debug, PartialEq)]
pub struct AskCommand {
    /// The natural language question to answer.
    pub message: String,
}

impl AskCommand {
    /// Build the full prompt including schema and question.
    pub fn prompt(&self, runtime: &mut Runtime) -> String {
        let schema = self.schema(runtime);
        PROMPT
            .replace("{SCHEMA}", &schema)
            .replace("{QUESTION}", &self.message)
    }

    /// Extract the database schema for context.
    fn schema(&self, runtime: &mut Runtime) -> String {
        let result = runtime
            .connection
            .prepare("SELECT sql FROM sqlite_master WHERE type = 'table'");

        let stmt = match result {
            Ok((_, Some(stmt))) => stmt,
            _ => return String::new(),
        };

        let mut schema = String::new();
        loop {
            match stmt.nextx() {
                Ok(None) => break,
                Ok(Some(row)) => {
                    schema.push_str(row.value_at(0).as_str());
                    schema.push('\n');
                }
                Err(_) => break,
            }
        }
        schema
    }

    /// Execute the ask command, streaming AI responses.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context containing the database connection
    ///
    /// # Returns
    ///
    /// A receiver for streaming response chunks, or an error if the API
    /// key is not set or the request fails.
    pub fn execute(
        &self,
        runtime: &mut Runtime,
    ) -> Result<std::sync::mpsc::Receiver<anyhow::Result<String>>, DotError> {
        let prompt = self.prompt(runtime);
        open_router_completions(&prompt)
    }
}

/// Make a streaming request to the OpenRouter API.
fn open_router_completions(
    prompt: &str,
) -> Result<std::sync::mpsc::Receiver<anyhow::Result<String>>, DotError> {
    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();

    let api_key = std::env::var("OPENROUTER_API_KEY").map_err(DotError::Env)?;

    let url = "https://openrouter.ai/api/v1/chat/completions";

    let payload = serde_json::json!({
        "model": "openai/gpt-4o",
        "messages": [{"role": "user", "content": prompt}],
        "stream": true
    });

    std::thread::spawn(move || -> anyhow::Result<()> {
        let resp = ureq::post(url)
            .header("Authorization", &format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .send(&payload.to_string())?;

        let reader = BufReader::new(resp.into_body().into_reader());
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => break,
            };
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }

            if let Some(data) = trimmed.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }

                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(content) = v["choices"]
                        .get(0)
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if tx.send(Ok(content.to_string())).is_err() {
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    });

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_template() {
        let cmd = AskCommand {
            message: "How many users are there?".to_string(),
        };

        let mut runtime = Runtime::new(None);

        // Create a test table
        let (_, stmt) = runtime
            .connection
            .prepare("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        stmt.unwrap().execute().unwrap();

        let prompt = cmd.prompt(&mut runtime);

        assert!(prompt.contains("How many users are there?"));
        assert!(prompt.contains("CREATE TABLE users"));
    }

    #[test]
    fn test_execute_missing_api_key() {
        // Ensure the API key is not set
        std::env::remove_var("OPENROUTER_API_KEY");

        let cmd = AskCommand {
            message: "test".to_string(),
        };

        let mut runtime = Runtime::new(None);
        let result = cmd.execute(&mut runtime);

        assert!(matches!(result, Err(DotError::Env(_))));
    }
}
