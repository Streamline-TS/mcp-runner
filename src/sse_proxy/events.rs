//! SSE event management for the Actix Web-based proxy.
//!
//! This module handles the broadcasting of Server-Sent Events (SSE) to connected clients
//! using Actix Web's streaming capabilities.

use crate::sse_proxy::types::{SSEEvent, SSEMessage};
use crate::transport::json_rpc::{error_codes, JSON_RPC_VERSION, JsonRpcError, JsonRpcResponse}; // Import JSON-RPC types
use actix_web::web::Bytes;
use tokio::sync::broadcast;
use tracing;

/// Handles SSE event processing and broadcasting to connected clients
///
/// This struct manages the broadcasting of events to multiple SSE clients using a
/// Tokio broadcast channel. It provides methods for sending different types of events
/// and handling SSE connections.
#[derive(Clone)]
pub struct EventManager {
    /// Broadcast channel for sending events to all connected clients
    sender: broadcast::Sender<SSEMessage>,
}

impl EventManager {
    /// Create a new EventManager with the specified channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Send initial configuration information to a newly connected client
    pub fn send_initial_config(&self, message_url: &str, _servers: &serde_json::Value) {
        // The Python client expects just the URL path as the data, not JSON
        // This matches the Python server implementation in SseServerTransport

        // Create SSE message with only the path as data
        let message = SSEMessage::new("endpoint", message_url, None);

        // Log the full SSE message for debugging
        let formatted_message = Self::format_sse_message(&message);
        let formatted_str = std::str::from_utf8(&formatted_message)
            .unwrap_or("Could not convert SSE message to string");

        tracing::debug!(
            raw_message = %formatted_str,
            event_type = "endpoint",
            "Raw SSE message being sent to client"
        );

        // Log the message data
        tracing::debug!(
            event = "endpoint",
            data = %message_url,
            "Sending message URL path as data"
        );

        // Send the message
        self.send_sse_message(message, "endpoint");
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<SSEMessage> {
        self.sender.subscribe()
    }

    /// Send a tool response event using JSON-RPC format
    pub fn send_tool_response(
        &self,
        request_id: &str,
        _server_id: &str, // No longer needed for the event payload itself
        _tool_name: &str, // No longer needed for the event payload itself
        data: serde_json::Value,
    ) {
        // Extract the result from the JSON-RPC response if it exists
        let result_value = if let Some(result) = data.get("result") {
            // We've found the result field in the JSON-RPC response
            // Now we need to return just the result instead of the full JSON-RPC response
            result.clone()
        } else {
            // If there's no result field, just pass the data as-is
            data
        };

        // Parse request_id to the appropriate type (number or string)
        let id_value = if let Ok(num_id) = request_id.parse::<i64>() {
            serde_json::Value::Number(serde_json::Number::from(num_id))
        } else {
            serde_json::Value::String(request_id.to_string())
        };

        // Create the JSON-RPC response directly with the parsed ID
        let json_rpc_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id_value,
            "result": result_value
        });

        // Serialize the JSON-RPC response
        match serde_json::to_string(&json_rpc_response) {
            Ok(json_data) => {
                // Create SSE message with event type "message" and request_id as SSE id
                let message = SSEMessage::new("message", &json_data, Some(request_id));

                // Log the message for debugging
                tracing::debug!(
                    request_id = %request_id,
                    message_data = %json_data,
                    "Sending tool response via SSE"
                );

                self.send_sse_message(message, "message"); // Use helper to send
            }
            Err(e) => {
                tracing::error!(error = %e, request_id = %request_id, "Failed to serialize JSON-RPC response");
            }
        }
    }

    /// Send a tool error event using JSON-RPC format
    pub fn send_tool_error(&self, request_id: &str, server_id: &str, tool_name: &str, error: &str) {
        // Parse request_id to the appropriate type (number or string)
        let id_value = if let Ok(num_id) = request_id.parse::<i64>() {
            serde_json::Value::Number(serde_json::Number::from(num_id))
        } else {
            serde_json::Value::String(request_id.to_string())
        };

        // Construct a JSON-RPC error object
        // Using the standard server error code from MCP spec
        // Include more details in the 'data' field
        let error_data = serde_json::json!({
            "serverId": server_id,
            "toolName": tool_name
        });
        let rpc_error = JsonRpcError {
            code: error_codes::SERVER_ERROR,
            message: error.to_string(),
            data: Some(error_data),
        };

        // Construct a JSON-RPC error response
        let response = JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id_value,
            result: None,
            error: Some(rpc_error),
        };

        // Serialize the JSON-RPC response
        match serde_json::to_string(&response) {
            Ok(json_data) => {
                // Create SSE message with event type "message" and request_id as SSE id
                let message = SSEMessage::new("message", &json_data, Some(request_id));

                // Log the message for debugging
                tracing::debug!(
                    request_id = %request_id,
                    message_data = %json_data,
                    "Sending tool error via SSE"
                );

                self.send_sse_message(message, "message"); // Use helper to send
            }
            Err(e) => {
                tracing::error!(error = %e, request_id = %request_id, "Failed to serialize JSON-RPC error response");
            }
        }
    }

    /// Send a server status update event
    pub fn send_server_status(&self, server_name: &str, server_id: &str, status: &str) {
        let event = SSEEvent::ServerStatus {
            server_name: server_name.to_string(),
            server_id: server_id.to_string(),
            status: status.to_string(),
        };
        // Serialize event payload to JSON
        match serde_json::to_string(&event) {
            Ok(json_data) => {
                // Create SSE message with specific event type
                let message = SSEMessage::new("server-status", &json_data, None);
                self.send_sse_message(message, "server-status"); // Use helper to send
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    event_type = "server-status",
                    "Failed to serialize SSE event payload"
                );
            }
        }
    }

    /// Send a notification event
    pub fn send_notification(&self, title: &str, message: &str, level: &str) {
        let event = SSEEvent::Notification {
            title: title.to_string(),
            message: message.to_string(),
            level: level.to_string(),
        };
        // Serialize event payload to JSON
        match serde_json::to_string(&event) {
            Ok(json_data) => {
                // Create SSE message with specific event type
                let message = SSEMessage::new("notification", &json_data, None);
                self.send_sse_message(message, "notification"); // Use helper to send
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    event_type = "notification",
                    "Failed to serialize SSE event payload"
                );
            }
        }
    }

    /// Helper to send an SSE message via the broadcast channel
    /// (Replaces the old create_and_send_event logic for broadcasting)
    fn send_sse_message(&self, message: SSEMessage, event_type_name: &str) {
        match self.sender.send(message) {
            Ok(receiver_count) => {
                if receiver_count > 0 {
                    tracing::debug!(
                        event_type = event_type_name,
                        receivers = receiver_count,
                        "SSE event sent to clients"
                    );
                } else {
                    tracing::trace!(
                        event_type = event_type_name,
                        "SSE event created but no clients connected"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    event_type = event_type_name,
                    "Failed to broadcast SSE event"
                );
            }
        }
    }

    /// Format an SSEMessage for the wire
    pub fn format_sse_message(message: &SSEMessage) -> Bytes {
        let mut result = String::new();

        if let Some(id) = &message.id {
            result.push_str(&format!("id: {}\n", id));
        }

        result.push_str(&format!("event: {}\n", message.event));
        result.push_str(&format!("data: {}\n\n", message.data));

        Bytes::from(result)
    }
}
