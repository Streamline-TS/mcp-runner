//! Type definitions for the proxy module.
//!
//! This module contains shared types used throughout the proxy module, including
//! structures for representing server information, tools, resources, and SSE events.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Information about an available server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Name of the server
    pub name: String,
    /// Server ID
    pub id: String,
    /// Current status
    pub status: String,
}

/// Information about an available tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Name of the tool
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for parameters
    pub parameters_schema: Value,
}

/// Information about an available resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    /// Name of the resource
    pub name: String,
    /// URI to access the resource
    pub uri: String,
    /// Human-readable description
    pub description: String,
}

/// Type of SSE message payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")] // Use tag/content for clearer serialization
pub enum SSEEvent {
    /// Tool call response event containing the result of a tool call
    ToolResponse {
        /// Unique ID of the request that triggered this response
        request_id: String,
        /// ID of the server that processed the request
        server_id: String,
        /// Name of the tool that was called
        tool_name: String,
        /// Response data from the tool
        response: Value,
    },
    /// Tool call error event when a tool call fails
    ToolError {
        /// Unique ID of the request that triggered this error
        request_id: String,
        /// ID of the server that processed the request
        server_id: String,
        /// Name of the tool that was called
        tool_name: String,
        /// Error message describing what went wrong
        error: String,
    },
    /// Server status update event when a server's status changes
    ServerStatus {
        /// ID of the server whose status changed
        server_id: String,
        /// Name of the server
        server_name: String,
        /// New status of the server (e.g., "Running", "Stopped", "Failed")
        status: String,
    },
}

/// Server-sent event message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSEMessage {
    /// Event type name (e.g., "tool-response", "server-status")
    pub event: String,
    /// Payload data (JSON string of SSEEvent)
    pub data: String,
    /// Optional event ID
    pub id: Option<String>,
}

// Add helper methods for SSEMessage formatting
impl SSEMessage {
    /// Creates a new SSE message with the given event type, data payload, and optional ID
    ///
    /// # Arguments
    ///
    /// * `event` - The event type (e.g., "tool-response", "tool-error", "server-status")
    /// * `data` - The data payload as a JSON string
    /// * `id` - Optional event ID for correlation
    ///
    /// # Returns
    ///
    /// A new `SSEMessage` instance
    pub fn new(event: &str, data: &str, id: Option<&str>) -> Self {
        Self {
            event: event.to_string(),
            data: data.to_string(),
            id: id.map(String::from),
        }
    }

    /// Format the message according to SSE spec
    ///
    /// Format an SSE message according to the Server-Sent Events specification,
    /// with proper line prefixes and formatting.
    ///
    /// # Returns
    ///
    /// A string containing the formatted SSE message
    pub fn format(&self) -> String {
        let mut formatted = String::new();
        if let Some(id) = &self.id {
            formatted.push_str(&format!("id: {}\n", id));
        }
        formatted.push_str(&format!("event: {}\n", self.event));
        // SSE data field can span multiple lines, prefix each with "data: "
        for line in self.data.lines() {
            formatted.push_str(&format!("data: {}\n", line));
        }
        formatted.push('\n'); // End with an extra newline
        formatted
    }
}
