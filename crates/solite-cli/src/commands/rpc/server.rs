use super::jsonrpc::*;
use std::io::{self, BufRead, BufReader, Write};

/// Server that listens for JSON-RPC requests on stdin and responds on stdout
pub fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        // Try to parse as a JSON-RPC message
        let message = match serde_json::from_str::<JsonRpcMessage>(&line) {
            Ok(message) => message,
            Err(e) => {
                eprintln!("Failed to parse JSON-RPC message: {}", e);
                continue;
            }
        };

        // Handle the message and create response
        let response: Option<JsonRpcMessage> = match message {
            JsonRpcMessage::Request(req) => {
                let id = req.id.clone();
                match handle(&req) {
                    Ok(result) => Some(JsonRpcMessage::Response(JsonRpcResponse {
                        jsonrpc: JsonRpcVersion2_0,
                        id,
                        result,
                    })),
                    Err(error) => Some(JsonRpcMessage::Error(JsonRpcError {
                        jsonrpc: JsonRpcVersion2_0,
                        id,
                        error,
                    })),
                }
            }
            JsonRpcMessage::Notification(_) => {
                // We don't need to respond to notifications
                None
            }
            JsonRpcMessage::Error(_) | JsonRpcMessage::Response(_) => None,
        };

        // Send response if we have one
        if let Some(resp) = response {
            let json = serde_json::to_string(&resp).unwrap();
            writeln!(stdout, "{}", json)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

/// Handle a JSON-RPC request and return the result or error
fn handle(req: &JsonRpcRequest) -> Result<JsonObject, ErrorData> {
    match req.request.method.as_str() {
        "initialize" => {
            let _params: serde_json::Value = parse_params(&req.request.params)?;
            let result = handle_initialize();
            to_json_object(result)
        }
        "reverse" => {
            let params: ReverseRequestParam = parse_params(&req.request.params)?;
            let result = handle_reverse(params);
            to_json_object(result)
        }
        _ => Err(ErrorData::generic(
            format!("Unknown method: {}", req.request.method),
            None,
        )),
    }
}

/// Parse request parameters with better error handling
fn parse_params<T: serde::de::DeserializeOwned>(params: &JsonObject) -> Result<T, ErrorData> {
    let value = serde_json::Value::Object(params.clone());
    serde_json::from_value(value).map_err(|e| {
        ErrorData::generic(format!("Invalid parameters: {}", e), None)
    })
}

/// Convert a result to a JSON object with better error handling
fn to_json_object<T: serde::Serialize>(value: T) -> Result<JsonObject, ErrorData> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Object(obj)) => Ok(obj),
        Ok(_) => Ok(JsonObject::new()),
        Err(e) => Err(ErrorData::generic(
            format!("Failed to serialize result: {}", e),
            None,
        )),
    }
}

fn handle_initialize() -> InitializeResult {
    InitializeResult {
        protocol_version: ProtocolVersion::LATEST,
    }
}

fn handle_reverse(params: ReverseRequestParam) -> ReverseResult {
    let reversed = params.text.chars().rev().collect::<String>();
    ReverseResult { reversed }
}
