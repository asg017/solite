// Borrowed and adapted from https://github.com/modelcontextprotocol/rust-sdk/blob/c0b777c7f784ba2d456b03c2ec3b98c9b28b5e10/crates/rmcp/src/model.rs#L122
use std::{borrow::Cow, sync::Arc};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// A JSON object type alias for convenient handling of JSON data.
///
/// You can use [`crate::object!`] or [`crate::model::object`] to create a json object quickly.
/// This is commonly used for storing arbitrary JSON data in MCP messages.
pub type JsonObject<F = Value> = serde_json::Map<String, F>;

/// unwrap the JsonObject under [`serde_json::Value`]
///
/// # Panic
/// This will panic when the value is not a object in debug mode.
pub fn object(value: serde_json::Value) -> JsonObject {
    debug_assert!(value.is_object());
    match value {
        serde_json::Value::Object(map) => map,
        _ => JsonObject::default(),
    }
}

/// This is commonly used for representing empty objects in MCP messages.
///
/// without returning any specific data.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Copy, Eq)]
pub struct EmptyObject {}

pub trait ConstString: Default {
    const VALUE: &str;
    fn as_str(&self) -> &'static str {
        Self::VALUE
    }
}
#[macro_export]
macro_rules! const_string {
    ($name:ident = $value:literal) => {
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl ConstString for $name {
            const VALUE: &str = $value;
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                $value.serialize(serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<$name, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let s: String = serde::Deserialize::deserialize(deserializer)?;
                if s == $value {
                    Ok($name)
                } else {
                    Err(serde::de::Error::custom(format!(concat!(
                        "expect const string value \"",
                        $value,
                        "\""
                    ))))
                }
            }
        }
    };
}




const_string!(JsonRpcVersion2_0 = "2.0");

// =============================================================================
// CORE PROTOCOL TYPES
// =============================================================================

/// Represents the MCP protocol version used for communication.
///
/// This ensures compatibility between clients and servers by specifying
/// which version of the Model Context Protocol is being used.
#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd)]

pub struct ProtocolVersion(Cow<'static, str>);

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::LATEST
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ProtocolVersion {
    pub const BETA: Self = Self(Cow::Borrowed("beta"));
    pub const LATEST: Self = Self::BETA;
}

impl Serialize for ProtocolVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        #[allow(clippy::single_match)]
        match s.as_str() {
            "beta" => return Ok(ProtocolVersion::BETA),
            _ => {}
        }
        Ok(ProtocolVersion(Cow::Owned(s)))
    }
}

/// A flexible identifier type that can be either a number or a string.
///
/// This is commonly used for request IDs and other identifiers in JSON-RPC
/// where the specification allows both numeric and string values.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum NumberOrString {
    /// A numeric identifier
    Number(i64),
    /// A string identifier
    String(Arc<str>),
}

impl NumberOrString {
    pub fn into_json_value(self) -> Value {
        match self {
            NumberOrString::Number(n) => Value::Number(serde_json::Number::from(n)),
            NumberOrString::String(s) => Value::String(s.to_string()),
        }
    }
}

impl std::fmt::Display for NumberOrString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NumberOrString::Number(n) => n.fmt(f),
            NumberOrString::String(s) => s.fmt(f),
        }
    }
}

impl Serialize for NumberOrString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            NumberOrString::Number(n) => n.serialize(serializer),
            NumberOrString::String(s) => s.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NumberOrString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: Value = Deserialize::deserialize(deserializer)?;
        match value {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(NumberOrString::Number(i))
                } else if let Some(u) = n.as_u64() {
                    // Handle large unsigned numbers that fit in i64
                    if u <= i64::MAX as u64 {
                        Ok(NumberOrString::Number(u as i64))
                    } else {
                        Err(serde::de::Error::custom("Number too large for i64"))
                    }
                } else {
                    Err(serde::de::Error::custom("Expected an integer"))
                }
            }
            Value::String(s) => Ok(NumberOrString::String(s.into())),
            _ => Err(serde::de::Error::custom("Expect number or string")),
        }
    }
}


/// Type alias for request identifiers used in JSON-RPC communication.
pub type RequestId = NumberOrString;


// =============================================================================
// JSON-RPC MESSAGE STRUCTURES
// =============================================================================

/// Represents a JSON-RPC request with method, parameters, and extensions.
///
/// This is the core structure for all MCP requests, containing:
/// - `method`: The name of the method being called
/// - `params`: The parameters for the method
/// - `extensions`: Additional context data (similar to HTTP headers)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]

pub struct Request<M = String, P = JsonObject> {
    pub method: M,
    pub params: P,
}

impl<M: Default, P> Request<M, P> {
    pub fn new(params: P) -> Self {
        Self {
            method: Default::default(),
            params,
        }
    }
}


#[derive(Debug, Clone, Default)]

pub struct RequestOptionalParam<M = String, P = JsonObject> {
    pub method: M,
    // #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

impl<M: Default, P> RequestOptionalParam<M, P> {
    pub fn with_param(params: P) -> Self {
        Self {
            method: Default::default(),
            params: Some(params),
        }
    }
}

#[derive(Debug, Clone, Default)]

pub struct RequestNoParam<M = String> {
    pub method: M,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]

pub struct Notification<M = String, P = JsonObject> {
    pub method: M,
    pub params: P,
}

impl<M: Default, P> Notification<M, P> {
    pub fn new(params: P) -> Self {
        Self {
            method: Default::default(),
            params,
        }
    }
}

#[derive(Debug, Clone, Default)]

pub struct NotificationNoParam<M = String> {
    pub method: M,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]

pub struct JsonRpcRequest<R = Request> {
    pub jsonrpc: JsonRpcVersion2_0,
    pub id: RequestId,
    #[serde(flatten)]
    pub request: R,
}

type DefaultResponse = JsonObject;
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]

pub struct JsonRpcResponse<R = JsonObject> {
    pub jsonrpc: JsonRpcVersion2_0,
    pub id: RequestId,
    pub result: R,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]

pub struct JsonRpcError {
    pub jsonrpc: JsonRpcVersion2_0,
    pub id: RequestId,
    pub error: ErrorData,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]

pub struct JsonRpcNotification<N = Notification> {
    pub jsonrpc: JsonRpcVersion2_0,
    #[serde(flatten)]
    pub notification: N,
}

/// Standard JSON-RPC error codes used throughout the MCP protocol.
///
/// These codes follow the JSON-RPC 2.0 specification and provide
/// standardized error reporting across all MCP implementations.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]

pub struct ErrorCode(pub i32);

impl ErrorCode {
    pub const GENERIC: Self = Self(-32002);
}

/// Error information for JSON-RPC error responses.
///
/// This structure follows the JSON-RPC 2.0 specification for error reporting,
/// providing a standardized way to communicate errors between clients and servers.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]

pub struct ErrorData {
    /// The error type that occurred (using standard JSON-RPC error codes)
    pub code: ErrorCode,

    /// A short description of the error. The message SHOULD be limited to a concise single sentence.
    pub message: Cow<'static, str>,

    /// Additional information about the error. The value of this member is defined by the
    /// sender (e.g. detailed error information, nested errors etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorData {
    pub fn new(
        code: ErrorCode,
        message: impl Into<Cow<'static, str>>,
        data: Option<Value>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }
    pub fn generic(message: impl Into<Cow<'static, str>>, data: Option<Value>) -> Self {
        Self::new(ErrorCode::GENERIC, message, data)
    }
}

/// Represents any JSON-RPC message that can be sent or received.
///
/// This enum covers all possible message types in the JSON-RPC protocol:
/// individual requests/responses, notifications, and errors.
/// It serves as the top-level message container for MCP communication.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]

pub enum JsonRpcMessage<Req = Request, Resp = DefaultResponse, Noti = Notification> {
    /// A single request expecting a response
    Request(JsonRpcRequest<Req>),
    /// A response to a previous request
    Response(JsonRpcResponse<Resp>),
    /// A one-way notification (no response expected)
    Notification(JsonRpcNotification<Noti>),
    /// An error response
    Error(JsonRpcError),
}

impl<Req, Resp, Not> JsonRpcMessage<Req, Resp, Not> {
    #[inline]
    pub const fn request(request: Req, id: RequestId) -> Self {
        JsonRpcMessage::Request(JsonRpcRequest {
            jsonrpc: JsonRpcVersion2_0,
            id,
            request,
        })
    }
    #[inline]
    pub const fn response(response: Resp, id: RequestId) -> Self {
        JsonRpcMessage::Response(JsonRpcResponse {
            jsonrpc: JsonRpcVersion2_0,
            id,
            result: response,
        })
    }
    #[inline]
    pub const fn error(error: ErrorData, id: RequestId) -> Self {
        JsonRpcMessage::Error(JsonRpcError {
            jsonrpc: JsonRpcVersion2_0,
            id,
            error,
        })
    }
    #[inline]
    pub const fn notification(notification: Not) -> Self {
        JsonRpcMessage::Notification(JsonRpcNotification {
            jsonrpc: JsonRpcVersion2_0,
            notification,
        })
    }
    pub fn into_request(self) -> Option<(Req, RequestId)> {
        match self {
            JsonRpcMessage::Request(r) => Some((r.request, r.id)),
            _ => None,
        }
    }
    pub fn into_response(self) -> Option<(Resp, RequestId)> {
        match self {
            JsonRpcMessage::Response(r) => Some((r.result, r.id)),
            _ => None,
        }
    }
    pub fn into_notification(self) -> Option<Not> {
        match self {
            JsonRpcMessage::Notification(n) => Some(n.notification),
            _ => None,
        }
    }
    pub fn into_error(self) -> Option<(ErrorData, RequestId)> {
        match self {
            JsonRpcMessage::Error(e) => Some((e.error, e.id)),
            _ => None,
        }
    }
    pub fn into_result(self) -> Option<(Result<Resp, ErrorData>, RequestId)> {
        match self {
            JsonRpcMessage::Response(r) => Some((Ok(r.result), r.id)),
            JsonRpcMessage::Error(e) => Some((Err(e.error), e.id)),

            _ => None,
        }
    }
}

// =============================================================================
// INITIALIZATION AND CONNECTION SETUP
// =============================================================================

/// # Empty result
/// A response that indicates success but carries no data.
pub type EmptyResult = EmptyObject;

impl From<()> for EmptyResult {
    fn from(_value: ()) -> Self {
        EmptyResult {}
    }
}

impl From<EmptyResult> for () {
    fn from(_value: EmptyResult) {}
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]

pub struct CancelledNotificationParam {
    pub request_id: RequestId,
    pub reason: Option<String>,
}

const_string!(CancelledNotificationMethod = "notifications/cancelled");

/// # Cancellation
/// This notification can be sent by either side to indicate that it is cancelling a previously-issued request.
///
/// The request SHOULD still be in-flight, but due to communication latency, it is always possible that this notification MAY arrive after the request has already finished.
///
/// This notification indicates that the result will be unused, so any associated processing SHOULD cease.
///
/// A client MUST NOT attempt to cancel its `initialize` request.
pub type CancelledNotification =
    Notification<CancelledNotificationMethod, CancelledNotificationParam>;



const_string!(InitializedNotificationMethod = "notifications/initialized");
/// This notification is sent from the client to the server after initialization has finished.
pub type InitializedNotification = NotificationNoParam<InitializedNotificationMethod>;


const_string!(InitializeResultMethod = "initialize");

/// # Initialization
/// This request is sent from the client to the server when it first connects, asking it to begin initialization.
pub type InitializeRequest = Request<InitializeResultMethod, InitializeRequestParam>;

/// Parameters sent by a client when initializing a connection to an MCP server.
///
/// This contains the client's protocol version, capabilities, and implementation
/// information, allowing the server to understand what the client supports.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InitializeRequestParam {
    /// The MCP protocol version this client supports
    pub protocol_version: ProtocolVersion,  
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: ProtocolVersion,
}

// =============================================================================
// REVERSE METHOD
// =============================================================================

const_string!(ReverseMethod = "reverse");

/// Parameters for the reverse request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReverseRequestParam {
    pub text: String,
}

/// Result of the reverse request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ReverseResult {
    pub reversed: String,
}

/// Reverse request type
pub type ReverseRequest = Request<ReverseMethod, ReverseRequestParam>;


pub type ConnectRequest= Request<ConnectMethod, ConnectRequestParam>;
const_string!(ConnectMethod = "connect");
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ConnectRequestParam {
    pub path: Option<String>,
}
    