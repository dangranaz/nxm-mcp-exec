//! Minimal JSON-RPC 2.0 types for the MCP protocol.
//!
//! Tipi JSON-RPC 2.0 minimi per il protocollo MCP.
//!
//! We implement the small slice of MCP we need by hand (no SDK) to keep the
//! dependency surface tiny and the behaviour transparent, exactly like the
//! sibling servers `nxm-mcp-webtomd` / `nxm-mcp-session`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An inbound JSON-RPC request (or notification, when `id` is absent).
///
/// Una richiesta JSON-RPC in ingresso (o notifica, quando `id` è assente).
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// Present for requests, absent for notifications.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// An outbound JSON-RPC response.
///
/// Una risposta JSON-RPC in uscita.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC error object.
///
/// Un oggetto errore JSON-RPC.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    /// Build a success response for `id` carrying `result`.
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response for `id` with `code` and `message`.
    pub fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Standard JSON-RPC error codes we use.
///
/// Codici di errore JSON-RPC standard che usiamo.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
}
