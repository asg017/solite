use serde::Serialize;
use crate::Runtime;
use std::io::{BufRead, BufReader};

/**
 * Things that should be configurable:
 * 
 * 1. Prompt template
 * 2. Model
 * 3. Endpoint
 * 4. API key
 */


static PROMPT: &str = r#"

Given the following SQLite database schema,
write a SQL query to answer the question below.
Use the most efficient query possible.
Provide only the SQL query as output.


{SCHEMA}

{QUESTION}

"#;

#[derive(Serialize, Debug, PartialEq)]
pub struct AskCommand {
    pub message: String,
}

impl AskCommand {
    pub fn prompt(&self, runtime: &mut Runtime) -> String {
        let schema = self.schema(runtime);
        PROMPT.replace("{SCHEMA}", &schema).replace("{QUESTION}", &self.message)
    }
    pub fn schema(&self, runtime: &mut Runtime) -> String {
        let stmt = runtime.connection.prepare("select sql from sqlite_master where type = 'table'").unwrap().1.unwrap();
        let mut schema = String::new();
        loop {
          match stmt.nextx() {
            Ok(None) => break,
            Ok(Some(row)) => {
              schema += &format!("{}\n", row.value_at(0).as_str());
            }
            Err (e) => todo!("{}", e),
  
          }
        }
        schema
      }
    pub fn execute(&self, runtime: &mut Runtime) -> anyhow::Result<std::sync::mpsc::Receiver<anyhow::Result<String>>> {
      let prompt = self.prompt(runtime);
      open_router_completions(&prompt)
    }
}

use serde_json::Value;

fn open_router_completions(prompt: &str) -> anyhow::Result<std::sync::mpsc::Receiver<anyhow::Result<String>>> {
    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<String>>();
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("OPENROUTER_API_KEY environment variable not set");
    let url = "https://openrouter.ai/api/v1/chat/completions";
    //let url = "http://127.0.0.1:8080/v1/chat/completions";

    let payload = serde_json::json!({
        "model": "openai/gpt-4o",
        "messages": [{"role": "user", "content": prompt}],
        "stream": true
    });

    std::thread::spawn(move || ->anyhow::Result<()> {
      let resp = ureq::post(url)
        .header("Authorization", &format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .send(&payload.to_string())?;

    let reader = BufReader::new(resp.into_body().into_reader());
    for line in reader.lines() {
        let line = line?;
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
                  tx.send(Ok(content.to_string()))?;
                }
            }
        }
    }
    Ok(())
    });
    Ok(rx)
}
