//! JSON-RPC implementation for MCP communication.
//!
//! This module provides types and utilities for JSON-RPC 2.0 communication
//! with MCP servers. It includes structures for requests, responses, notifications,
//! and error handling, along with helper methods for creating common MCP request types.
//!
//! The implementation follows the JSON-RPC 2.0 specification and adapts it specifically
//! for the Model Context Protocol's requirements.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// JSON-RPC protocol version
pub const JSON_RPC_VERSION: &str = "2.0";

/// Standard JSON-RPC error codes as defined in the JSON-RPC 2.0 specification
/// and the Model Context Protocol (MCP) documentation
pub mod error_codes {
    /// Parse error: Invalid JSON was received
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid Request: The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found: The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params: Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error: Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;

    // Server-defined errors should be in the range -32000 to -32099
    /// Generic server error for MCP-specific issues
    pub const SERVER_ERROR: i32 = -32000;
}

/// A JSON-RPC message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// A JSON-RPC request
    Request(JsonRpcRequest),
    /// A JSON-RPC response
    Response(JsonRpcResponse),
    /// A JSON-RPC notification (request without ID)
    Notification(JsonRpcNotification),
}

/// A JSON-RPC request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC protocol version
    pub jsonrpc: String,
    /// Request ID
    pub id: Value,
    /// Method name
    pub method: String,
    /// Method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }

    /// Create a request to list MCP tools
    pub fn list_tools(id: impl Into<Value>) -> Self {
        Self::new(id, "tools/list", None)
    }

    /// Create a request to call an MCP tool
    pub fn call_tool(id: impl Into<Value>, name: impl Into<String>, args: Value) -> Self {
        let params = serde_json::json!({
            "name": name.into(),
            "arguments": args
        });
        Self::new(id, "tools/call", Some(params))
    }

    /// Create a request to list MCP resources
    pub fn list_resources(id: impl Into<Value>) -> Self {
        Self::new(id, "resources/list", None)
    }

    /// Create a request to get an MCP resource
    pub fn get_resource(id: impl Into<Value>, uri: impl Into<String>) -> Self {
        let params = serde_json::json!({
            "uri": uri.into()
        });
        Self::new(id, "resources/get", Some(params))
    }
}

/// A JSON-RPC notification (request without ID)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// JSON-RPC protocol version
    pub jsonrpc: String,
    /// Method name
    pub method: String,
    /// Method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC notification
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }

    /// Create an 'initialized' notification
    pub fn initialized() -> Self {
        Self::new("notifications/initialized", None)
    }
}

/// A JSON-RPC error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// A JSON-RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC protocol version
    pub jsonrpc: String,
    /// Request ID
    pub id: Value,
    /// Result (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Create a new successful JSON-RPC response
    pub fn success(id: impl Into<Value>, result: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    /// Create a new error JSON-RPC response
    pub fn error(
        id: impl Into<Value>,
        code: i32,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.into(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }

    /// Check if the response is successful
    pub fn is_success(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }
}
