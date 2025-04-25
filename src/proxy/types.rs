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
pub enum SSEEvent { // Renamed from MessageType for clarity, represents payload structure
    /// Tool call response
    ToolResponse {
        request_id: String,
        server_id: String,
        tool_name: String,
        response: Value,
    },
    /// Tool call error
    ToolError {
        request_id: String,
        server_id: String,
        tool_name: String,
        error: String,
    },
    /// Server status update
    ServerStatus {
        server_id: String,
        server_name: String,
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
    // Removed redundant fields now covered by SSEEvent in data
    // pub message_type: MessageType, 
    // pub request_id: String,
    // pub server_id: String,
    // pub tool: Option<String>,
    // pub data: Value, 
    // pub timestamp: String, 
}

// Add helper methods for SSEMessage formatting
impl SSEMessage {
    pub fn new(event: &str, data: &str, id: Option<&str>) -> Self {
        Self {
            event: event.to_string(),
            data: data.to_string(),
            id: id.map(String::from),
        }
    }

    /// Format the message according to SSE spec
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
        formatted.push_str("\n"); // End with an extra newline
        formatted
    }
}