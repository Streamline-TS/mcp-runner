//! Type definitions for the SSE proxy implementation.
//!
//! This module contains the core data structures used by the SSE proxy.

use crate::server::ServerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Server information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name
    pub name: String,
    /// Server ID (as string for serialization)
    pub id: String,
    /// Server status (as string for serialization)
    pub status: String,
}

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    /// Resource name/ID
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Resource metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Tool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Tool parameters schema
    pub parameters: Option<serde_json::Value>,
    /// Tool return type
    pub return_type: Option<String>,
}

/// Server-Sent Event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SSEEvent {
    /// Endpoint configuration event
    #[serde(rename = "endpoint")]
    Endpoint {
        /// URL for sending messages
        message_url: String,
        /// Available server endpoints
        servers: serde_json::Value,
    },

    /// Tool response event
    #[serde(rename = "tool-response")]
    ToolResponse {
        /// Request ID for correlation
        request_id: String,
        /// Server ID that processed the request
        server_id: String,
        /// Tool name that was called
        tool_name: String,
        /// Response data
        data: serde_json::Value,
    },

    /// Tool error event
    #[serde(rename = "tool-error")]
    ToolError {
        /// Request ID for correlation
        request_id: String,
        /// Server ID that processed the request
        server_id: String,
        /// Tool name that was called
        tool_name: String,
        /// Error message
        error: String,
    },

    /// Server status update event
    #[serde(rename = "server-status")]
    ServerStatus {
        /// Server name
        server_name: String,
        /// Server ID
        server_id: String,
        /// Status description
        status: String,
    },

    /// General notification event
    #[serde(rename = "notification")]
    Notification {
        /// Notification title
        title: String,
        /// Notification message
        message: String,
        /// Notification level (info, warn, error)
        level: String,
    },
}

/// Server-Sent Event message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSEMessage {
    /// Event type
    pub event: String,
    /// Event data as JSON string
    pub data: String,
    /// Optional event ID
    pub id: Option<String>,
}

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
}

/// Server information update message sent between
/// McpRunner and the SSEProxy
#[derive(Debug, Clone)]
pub enum ServerInfoUpdate {
    /// Update information about a specific server
    UpdateServer {
        /// Server name
        name: String,
        /// Server ID
        id: Option<ServerId>,
        /// Server status
        status: String,
    },

    /// Add a new server to the proxy cache
    AddServer {
        /// Server name
        name: String,
        /// Server information
        info: ServerInfo,
    },

    /// Shutdown the proxy
    Shutdown,
}
