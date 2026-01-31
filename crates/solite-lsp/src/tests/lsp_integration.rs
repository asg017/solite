//! LSP Integration Tests
//!
//! These tests create an actual LSP server and client to test the full protocol flow.
//! Run with: cargo test -p solite_lsp lsp_integration -- --nocapture

use tokio::io::{duplex, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tower_lsp::lsp_types::*;
use tower_lsp::{LspService, Server};

use crate::Backend;

/// A simple LSP client for testing that communicates over an async stream
struct TestClient {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    request_id: i64,
}

impl TestClient {
    fn new(stream: tokio::io::DuplexStream) -> Self {
        let (read, write) = tokio::io::split(stream);
        Self {
            writer: write,
            reader: BufReader::new(read),
            request_id: 0,
        }
    }

    /// Send a JSON-RPC request and wait for response
    async fn request<R: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: impl serde::Serialize,
    ) -> R {
        self.request_id += 1;
        let id = self.request_id;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.send_message(&request).await;
        let response = self.read_response(id).await;

        // Parse the result field
        serde_json::from_value(response["result"].clone())
            .expect("Failed to parse response result")
    }

    /// Send a JSON-RPC notification (no response expected)
    async fn notify(&mut self, method: &str, params: impl serde::Serialize) {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_message(&notification).await;

        // Small delay to let server process
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    /// Send a JSON-RPC message with LSP framing
    async fn send_message(&mut self, message: &serde_json::Value) {
        let content = serde_json::to_string(message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", content.len());

        self.writer.write_all(header.as_bytes()).await.unwrap();
        self.writer.write_all(content.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    /// Read a JSON-RPC message with LSP framing
    async fn read_message(&mut self) -> serde_json::Value {
        // Read headers until empty line
        let mut content_length: Option<usize> = None;

        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();

            if line == "\r\n" || line == "\n" || line.is_empty() {
                break;
            }

            if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                content_length = Some(len_str.trim().parse().unwrap());
            }
        }

        let len = content_length.expect("No Content-Length header");
        let mut buffer = vec![0u8; len];
        self.reader.read_exact(&mut buffer).await.unwrap();

        serde_json::from_slice(&buffer).unwrap()
    }

    /// Read a JSON-RPC response, skipping any notifications
    async fn read_response(&mut self, expected_id: i64) -> serde_json::Value {
        loop {
            let message = self.read_message().await;

            // Check if this is a response (has "id" field)
            if let Some(id) = message.get("id") {
                if id.as_i64() == Some(expected_id) {
                    return message;
                }
                // Wrong ID - keep reading
                println!("Got response with wrong id: {:?}", id);
            } else {
                // This is a notification - skip it
                println!("Skipping notification: {}", message.get("method").and_then(|m| m.as_str()).unwrap_or("unknown"));
            }
        }
    }

    /// Initialize the LSP connection
    async fn initialize(&mut self) -> InitializeResult {
        self.request(
            "initialize",
            InitializeParams {
                process_id: Some(std::process::id()),
                root_uri: Some(Url::parse("file:///test").unwrap()),
                capabilities: ClientCapabilities::default(),
                ..Default::default()
            },
        )
        .await
    }

    /// Send initialized notification
    async fn initialized(&mut self) {
        self.notify("initialized", InitializedParams {}).await;
    }

    /// Open a text document
    async fn did_open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: Url::parse(uri).unwrap(),
                    language_id: "sql".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        )
        .await;
    }

    /// Request hover at a position
    async fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        self.request_id += 1;
        let id = self.request_id;
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/hover",
            "params": params
        });

        self.send_message(&request).await;
        let response = self.read_response(id).await;

        println!("Hover raw response: {}", serde_json::to_string_pretty(&response).unwrap());

        // Check for error
        if let Some(error) = response.get("error") {
            println!("Hover error: {:?}", error);
            return None;
        }

        // Parse result - can be null
        if response["result"].is_null() {
            return None;
        }

        serde_json::from_value(response["result"].clone()).ok()
    }

    /// Shutdown the server
    async fn shutdown(&mut self) {
        let _: () = self.request("shutdown", serde_json::Value::Null).await;
    }
}

/// Spawn an LSP server and return a connected test client
async fn spawn_server() -> TestClient {
    // Create a bidirectional in-memory stream
    // Buffer size needs to be large enough for messages
    let (client_stream, server_stream) = duplex(64 * 1024);

    // Create the LSP service
    let (service, socket) = LspService::new(Backend::new);

    // Split the server stream
    let (server_read, server_write) = tokio::io::split(server_stream);

    // Spawn the server
    tokio::spawn(async move {
        Server::new(server_read, server_write, socket)
            .serve(service)
            .await;
    });

    TestClient::new(client_stream)
}

#[tokio::test]
async fn test_initialize() {
    let mut client = spawn_server().await;

    let result = client.initialize().await;
    println!("Initialize result: {:?}", result);

    assert!(result.capabilities.hover_provider.is_some());

    client.initialized().await;
    client.shutdown().await;
}

#[tokio::test]
async fn test_hover_on_table_with_doc_comments() {
    let mut client = spawn_server().await;

    // Initialize
    client.initialize().await;
    client.initialized().await;

    // Open a document with doc comments
    let sql = r#"CREATE TABLE students (
  --! All students at Foo University.
  --! @details https://foo.edu/students

  --- Student ID assigned at orientation
  --- @example 'S10483'
  student_id TEXT PRIMARY KEY,

  --- Full name of student
  name TEXT
);

select * from students where student_id = 3;
"#;

    client.did_open("file:///test.sql", sql).await;

    // Give server time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Debug: print the SQL with line numbers
    println!("=== SQL with line numbers ===");
    for (i, line) in sql.lines().enumerate() {
        println!("{:2}: {}", i, line);
    }
    println!("=============================");

    // Hover over "students" in the SELECT statement (line 12, around char 14)
    // Line 12 is: "select * from students where student_id = 3;"
    // Let's find the exact position
    let target_line = 12;
    let line_content = sql.lines().nth(target_line as usize).unwrap_or("");
    let char_pos = line_content.find("students").unwrap_or(0) as u32;
    println!("Hovering at line {}, char {} (line content: '{}')", target_line, char_pos, line_content);

    let hover = client.hover("file:///test.sql", target_line, char_pos).await;

    println!("Hover result: {:?}", hover);

    // Verify hover contains doc comments
    if let Some(hover) = hover {
        let content = match &hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            HoverContents::Scalar(MarkedString::String(s)) => s,
            HoverContents::Scalar(MarkedString::LanguageString(ls)) => &ls.value,
            HoverContents::Array(arr) => {
                panic!("Unexpected array hover content: {:?}", arr);
            }
        };

        println!("=== HOVER CONTENT ===\n{}\n=====================", content);

        assert!(
            content.contains("students"),
            "Hover should mention table name"
        );
        assert!(
            content.contains("All students at Foo University"),
            "Hover should contain table doc comment. Got:\n{}",
            content
        );
        assert!(
            content.contains("Student ID assigned at orientation"),
            "Hover should contain column doc comment. Got:\n{}",
            content
        );
    } else {
        panic!("Expected hover result, got None");
    }

    client.shutdown().await;
}

#[tokio::test]
async fn test_hover_on_column_with_doc_comments() {
    let mut client = spawn_server().await;

    client.initialize().await;
    client.initialized().await;

    // Use qualified column reference (users.id) so it resolves to the table
    let sql = r#"CREATE TABLE users (
  --- The unique user identifier
  --- @example 42
  id INTEGER PRIMARY KEY,

  --- User's email address
  --- @example 'user@example.com'
  email TEXT
);

SELECT users.id, users.email FROM users;
"#;

    client.did_open("file:///test.sql", sql).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Debug: print SQL with line numbers
    println!("=== SQL with line numbers ===");
    for (i, line) in sql.lines().enumerate() {
        println!("{:2}: {}", i, line);
    }
    println!("=============================");

    // Hover over "id" in "users.id" (line 10)
    // Line 10: "SELECT users.id, users.email FROM users;"
    //                        ^^ char 13-14
    let line_content = sql.lines().nth(10).unwrap();
    println!("Line 10: '{}'", line_content);

    // Find position of ".id" and hover on "id" part
    let id_pos = line_content.find(".id").map(|p| p + 1).unwrap_or(13) as u32;
    println!("Hovering at line 10, char {}", id_pos);

    let hover = client.hover("file:///test.sql", 10, id_pos).await;
    println!("Column hover result: {:?}", hover);

    if let Some(hover) = hover {
        let content = match &hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            _ => panic!("Expected markup content"),
        };

        println!("=== COLUMN HOVER ===\n{}\n====================", content);

        assert!(content.contains("id"), "Should mention column name");
        assert!(
            content.contains("unique user identifier"),
            "Should contain column doc. Got:\n{}",
            content
        );
    } else {
        panic!("Expected hover result for column");
    }

    client.shutdown().await;
}

#[tokio::test]
async fn test_hover_with_dot_open_command() {
    // Test that hover works when the document contains .open commands
    let mut client = spawn_server().await;

    client.initialize().await;
    client.initialized().await;

    // Document with .open command followed by SQL
    let sql = r#".open /some/database.db

CREATE TABLE students (
  --! All students at Foo University.
  student_id TEXT PRIMARY KEY,
  name TEXT
);

SELECT * FROM students;
"#;

    client.did_open("file:///test.sql", sql).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Debug: print SQL with line numbers
    println!("=== SQL with line numbers (has .open) ===");
    for (i, line) in sql.lines().enumerate() {
        println!("{:2}: {}", i, line);
    }
    println!("==========================================");

    // Hover over "students" in SELECT (line 8)
    let hover = client.hover("file:///test.sql", 8, 14).await;
    println!("Hover result with .open: {:?}", hover);

    if let Some(hover) = hover {
        let content = match &hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            _ => panic!("Expected markup content"),
        };

        println!("=== HOVER CONTENT (with .open) ===\n{}\n==================================", content);

        assert!(content.contains("students"), "Should mention table name");
        assert!(
            content.contains("All students at Foo University"),
            "Should contain table doc. Got:\n{}",
            content
        );
    } else {
        panic!("Expected hover result, got None");
    }

    client.shutdown().await;
}
