//! SSE event management for the Actix Web-based proxy.
//!
//! This module handles the broadcasting of Server-Sent Events (SSE) to connected clients
//! using Actix Web's streaming capabilities.

use crate::sse_proxy::types::{SSEEvent, SSEMessage};
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

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<SSEMessage> {
        self.sender.subscribe()
    }

    /// Send a tool response event
    pub fn send_tool_response(
        &self,
        request_id: &str,
        server_id: &str,
        tool_name: &str,
        data: serde_json::Value,
    ) {
        let event = SSEEvent::ToolResponse {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            data,
        };
        self.create_and_send_event(event, Some(request_id));
    }

    /// Send a tool error event
    pub fn send_tool_error(&self, request_id: &str, server_id: &str, tool_name: &str, error: &str) {
        let event = SSEEvent::ToolError {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            error: error.to_string(),
        };
        self.create_and_send_event(event, Some(request_id));
    }

    /// Send a server status update event
    pub fn send_server_status(&self, server_name: &str, server_id: &str, status: &str) {
        let event = SSEEvent::ServerStatus {
            server_name: server_name.to_string(),
            server_id: server_id.to_string(),
            status: status.to_string(),
        };
        self.create_and_send_event(event, None);
    }

    /// Send a notification event
    pub fn send_notification(&self, title: &str, message: &str, level: &str) {
        let event = SSEEvent::Notification {
            title: title.to_string(),
            message: message.to_string(),
            level: level.to_string(),
        };
        self.create_and_send_event(event, None);
    }

    /// Helper to serialize, create, and send an SSE message
    fn create_and_send_event(&self, event_payload: SSEEvent, request_id: Option<&str>) {
        // Get event type name from the variant
        let event_type_name = match &event_payload {
            SSEEvent::ToolResponse { .. } => "tool-response",
            SSEEvent::ToolError { .. } => "tool-error",
            SSEEvent::ServerStatus { .. } => "server-status",
            SSEEvent::Notification { .. } => "notification",
        };

        // Serialize event payload to JSON
        match serde_json::to_string(&event_payload) {
            Ok(json_data) => {
                // Create SSE message
                let message = SSEMessage::new(event_type_name, &json_data, request_id);

                // Send message through broadcast channel
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
            Err(e) => {
                tracing::error!(
                    error = %e,
                    event_type = event_type_name,
                    "Failed to serialize SSE event payload"
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
