//! Error handling helpers for HTTP and JSON-RPC responses.
//!
//! This module provides helper functions for generating and sending standard HTTP
//! and JSON-RPC error responses.

use crate::error::Result;
use crate::proxy::http::HttpResponse;
use crate::transport::json_rpc::JsonRpcResponse;
use serde_json::Value;
use tokio::net::TcpStream;

/// Send a JSON-RPC error response
///
/// Helper for consistently formatting and sending JSON-RPC error responses.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `id` - JSON-RPC ID from the request
/// * `code` - Error code
/// * `message` - Error message
/// * `data` - Optional additional error data
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_jsonrpc_error(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    id: impl Into<Value>,
    code: i32,
    message: &str,
    data: Option<Value>,
) -> Result<()> {
    let resp = JsonRpcResponse::error(id, code, message, data);
    match serde_json::to_string(&resp) {
        Ok(json) => HttpResponse::send_json_response(writer, &json).await,
        Err(e) => {
            tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
            HttpResponse::send_error_response(
                writer,
                500,
                "Internal server error during error reporting",
            )
            .await
        }
    }
}

/// Send a JSON-RPC parse error response
///
/// Helper for consistently handling JSON parse errors.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `error` - Serialization error
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_parse_error(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    error: &serde_json::Error,
) -> Result<()> {
    send_jsonrpc_error(
        writer,
        serde_json::json!(null),
        -32700, // Parse error
        &format!("Parse error: {}", error),
        None,
    )
    .await
}

/// Send a JSON-RPC method not found error response
///
/// Helper for consistently handling method not found errors.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `id` - JSON-RPC ID from the request
/// * `expected` - Expected method name
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_method_not_found(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    id: impl Into<Value>,
    expected: &str,
) -> Result<()> {
    send_jsonrpc_error(
        writer,
        id,
        -32601, // Method not found
        &format!("Method not found (expected '{}')", expected),
        None,
    )
    .await
}

/// Send a JSON-RPC invalid parameter error response
///
/// Helper for consistently handling invalid parameter errors.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `id` - JSON-RPC ID from the request
/// * `param_name` - Name of the invalid parameter
/// * `expected` - Description of expected parameter type/format
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_invalid_param(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    id: impl Into<Value>,
    param_name: &str,
    expected: &str,
) -> Result<()> {
    send_jsonrpc_error(
        writer,
        id,
        -32602, // Invalid params
        &format!("Invalid '{}' parameter: {}", param_name, expected),
        None,
    )
    .await
}

/// Send a JSON-RPC missing parameters error response
///
/// Helper for consistently handling missing parameters errors.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `id` - JSON-RPC ID from the request
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_missing_params(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    id: impl Into<Value>,
) -> Result<()> {
    send_jsonrpc_error(
        writer,
        id,
        -32602, // Invalid params
        "Missing required parameters",
        None,
    )
    .await
}

/// Send a JSON-RPC internal error response
///
/// Helper for consistently handling internal server errors.
///
/// # Arguments
///
/// * `writer` - TCP stream writer for sending the response
/// * `id` - JSON-RPC ID from the request
/// * `error` - Error that occurred
///
/// # Returns
///
/// A `Result<()>` indicating success or an error
pub async fn send_internal_error<E: std::fmt::Display>(
    writer: &mut tokio::io::WriteHalf<TcpStream>,
    id: impl Into<Value>,
    error: E,
) -> Result<()> {
    send_jsonrpc_error(
        writer,
        id,
        -32000, // Server error
        &format!("Internal server error: {}", error),
        None,
    )
    .await
}

/// Helper for extracting a string parameter from JSON-RPC params
///
/// # Arguments
///
/// * `params` - JSON-RPC parameters object
/// * `name` - Name of the parameter to extract
///
/// # Returns
///
/// An Option with the string value, or None if not found/not a string
pub fn get_string_param(params: &Value, name: &str) -> Option<String> {
    params
        .get(name)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
