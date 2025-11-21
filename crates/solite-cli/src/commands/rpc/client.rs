use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use super::jsonrpc::*;

/// Client that spawns a subprocess and communicates via JSON-RPC over stdio
pub struct Client {
    process: Child,
    request_id: AtomicI64,
}

impl Client {
    /// Create a new client by starting a subprocess at the given executable path
    pub fn new(executable_path: PathBuf) -> io::Result<Self> {
        let process = Command::new(executable_path)
        .args(["rpc", "server"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        
        Ok(Self {
            process,
            request_id: AtomicI64::new(1),
        })
    }
    
    /// Get the next request ID
    fn next_id(&self) -> RequestId {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        RequestId::Number(id)
    }
    
    /// Send a request and wait for response
    fn send_request(&mut self, method: &str, params: serde_json::Value) -> io::Result<JsonRpcMessage> {
        let id = self.next_id();
        
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion2_0,
            id: id.clone(),
            request: Request {
                method: method.to_string(),
                params: match params {
                    serde_json::Value::Object(obj) => obj,
                    _ => JsonObject::new(),
                },
            },
        };
        
        let message: JsonRpcMessage = JsonRpcMessage::Request(request);
        let json = serde_json::to_string(&message)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Send request
        if let Some(stdin) = self.process.stdin.as_mut() {
            writeln!(stdin, "{}", json)?;
            stdin.flush()?;
        } else {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "stdin not available"));
        }
        
        // Read response
        if let Some(stdout) = self.process.stdout.as_mut() {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            
            serde_json::from_str(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        } else {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "stdout not available"))
        }
    }
    
    /// Initialize the connection with the server
    pub fn initialize(&mut self) -> io::Result<InitializeResult> {
        let params = InitializeRequestParam {
            protocol_version: ProtocolVersion::LATEST,
        };
        
        let params_value = serde_json::to_value(params)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        let response = self.send_request("initialize", params_value)?;
        
        match response {
            JsonRpcMessage::Response(resp) => {
                serde_json::from_value(serde_json::Value::Object(resp.result))
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            JsonRpcMessage::Error(err) => {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Server error: {}", err.error.message)
                ))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected response type")),
        }
    }
    
    /// Send a reverse request to the server
    pub fn reverse(&mut self, text: &str) -> io::Result<String> {
        let params = ReverseRequestParam {
            text: text.to_string(),
        };
        
        let params_value = serde_json::to_value(params)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        let response = self.send_request("reverse", params_value)?;
        
        match response {
            JsonRpcMessage::Response(resp) => {
                let result: ReverseResult = serde_json::from_value(serde_json::Value::Object(resp.result))
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(result.reversed)
            }
            JsonRpcMessage::Error(err) => {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Server error: {}", err.error.message)
                ))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected response type")),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Run the client - connects to a server subprocess and performs operations
pub fn run(executable_path: PathBuf) -> io::Result<()> {
    let mut client = Client::new(executable_path)?;
    
    let init_result = client.initialize()?;
    
    let test_text = "abc";
    let reversed = client.reverse(test_text)?;
    println!("{}: '{}'", test_text, reversed);
    
    let test_text2 = "Hello, World!";
    let reversed2 = client.reverse(test_text2)?;
    println!("{}: '{}'", test_text2, reversed2);
    
    Ok(())
}
