use serde::Serialize;
use crate::Runtime;
use std::io::{BufRead, BufReader, Write};
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
    pub fn execute(&self, runtime: &mut Runtime) {
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

      let prompt = PROMPT.replace("{SCHEMA}", &schema).replace("{QUESTION}", &self.message);
        println!("{prompt}");
      xxx(&prompt);
    }
}

use serde_json::Value;

fn xxx(prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    let api_key = "TODO";
    let url = "https://openrouter.ai/api/v1/chat/completions";
    let url = "http://127.0.0.1:8080/v1/chat/completions";

    let payload = serde_json::json!({
        "model": "openai/gpt-4o",
        "messages": [{"role": "user", "content": prompt}],
        "stream": true
    });

    let resp = ureq::post(url)
        .header("Authorization", &format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .send(&payload.to_string())?;

    let reader = BufReader::new(resp.into_body().into_reader());
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

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
                    write!(handle, "{}", content)?;
                    handle.flush()?;
                }
            }
        }
    }
    println!("\n");

    Ok(())
}
