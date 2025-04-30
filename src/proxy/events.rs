//! Event management for Server-Sent Events (SSE).
//!
//! This module provides functionality for managing, broadcasting and streaming
//! Server-Sent Events (SSE). It handles event broadcasting to multiple clients
//! and the SSE connection lifecycle.
//!
//! SSE provides a mechanism for sending updates from the server to clients over
//! an HTTP connection. This module implements the SSE protocol specification.

use crate::Error;
use crate::error::Result;
use crate::proxy::types::{SSEEvent, SSEMessage};
use crate::server::ServerId;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tokio::time::{Duration, sleep};
use tracing;

/// Handles SSE event processing and broadcasting to connected clients
///
/// This struct manages the broadcasting of events to multiple SSE clients using a
/// Tokio broadcast channel. It provides methods for sending different types of events
/// and handling SSE connections.
pub struct EventManager {
    /// Broadcast channel for sending events to all connected clients
    sender: broadcast::Sender<SSEMessage>,
}

impl EventManager {
    /// Create a new event manager with the specified channel capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of messages that can be buffered before old messages are dropped
    ///
    /// # Returns
    ///
    /// A new `EventManager` instance
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Get a new receiver for the broadcast channel
    ///
    /// This method creates and returns a new subscriber to the broadcast channel,
    /// allowing a client to receive SSE messages.
    ///
    /// # Returns
    ///
    /// A new `broadcast::Receiver<SSEMessage>` instance
    pub fn subscribe(&self) -> broadcast::Receiver<SSEMessage> {
        self.sender.subscribe()
    }

    /// Helper to serialize, create, and send an SSE message
    fn create_and_send_event(&self, event_payload: SSEEvent, request_id: Option<&str>) {
        match serde_json::to_string(&event_payload) {
            Ok(json_data) => {
                let event_type = match event_payload {
                    SSEEvent::ToolResponse { .. } => "tool-response",
                    SSEEvent::ToolError { .. } => "tool-error",
                    SSEEvent::ServerStatus { .. } => "server-status",
                };
                let message = SSEMessage::new(event_type, &json_data, request_id);
                self.send_message(message);
            }
            Err(e) => {
                let event_type_name = match event_payload {
                    SSEEvent::ToolResponse { .. } => "tool response",
                    SSEEvent::ToolError { .. } => "tool error",
                    SSEEvent::ServerStatus { .. } => "server status",
                };
                tracing::error!(error = %e, event_type = event_type_name, "Failed to serialize SSE event payload");
            }
        }
    }

    /// Send a tool response event to all connected clients
    ///
    /// # Arguments
    ///
    /// * `request_id` - Unique identifier for the original request
    /// * `server_id` - Identifier of the server that executed the tool
    /// * `tool_name` - Name of the tool that was called
    /// * `response` - Response data from the tool call
    pub fn send_tool_response(
        &self,
        request_id: &str,
        server_id: &str,
        tool_name: &str,
        response: Value,
    ) {
        let event_payload = SSEEvent::ToolResponse {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            response,
        };
        self.create_and_send_event(event_payload, Some(request_id));
    }

    /// Send a tool error event to all connected clients
    ///
    /// # Arguments
    ///
    /// * `request_id` - Unique identifier for the original request
    /// * `server_id` - Identifier of the server that attempted to execute the tool
    /// * `tool_name` - Name of the tool that was called
    /// * `error` - Error message describing what went wrong
    pub fn send_tool_error(&self, request_id: &str, server_id: &str, tool_name: &str, error: &str) {
        let event_payload = SSEEvent::ToolError {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            error: error.to_string(),
        };
        self.create_and_send_event(event_payload, Some(request_id));
    }

    /// Send a server status update event to all connected clients
    ///
    /// # Arguments
    ///
    /// * `server_id` - Identifier of the server whose status changed
    /// * `server_name` - Name of the server
    /// * `status` - New status of the server
    pub fn send_status_update(&self, server_id: ServerId, server_name: &str, status: &str) {
        let event_payload = SSEEvent::ServerStatus {
            server_id: format!("{:?}", server_id),
            server_name: server_name.to_string(),
            status: status.to_string(),
        };
        self.create_and_send_event(event_payload, None);
    }

    /// Helper method to send an SSE message with retries
    fn send_message(&self, message: SSEMessage) {
        // Only try to send if there are receivers
        if self.sender.receiver_count() > 0 {
            let sender = self.sender.clone();
            let message_clone = message.clone();

            tokio::spawn(async move {
                let mut retry_count = 0;
                const MAX_RETRIES: usize = 3;
                const RETRY_DELAY: Duration = Duration::from_millis(10);

                while retry_count < MAX_RETRIES {
                    match sender.send(message_clone.clone()) {
                        Ok(_) => {
                            if retry_count > 0 {
                                tracing::debug!(
                                    retries = retry_count,
                                    "Successfully broadcast SSE event after retries"
                                );
                            }
                            return;
                        }
                        Err(e) => {
                            retry_count += 1;
                            if retry_count < MAX_RETRIES && sender.receiver_count() > 0 {
                                tracing::warn!(attempt = retry_count, error = %e, "Failed to broadcast SSE event, will retry");
                                sleep(RETRY_DELAY).await;
                            } else {
                                tracing::error!(error = %e, "Failed to broadcast SSE event after maximum retries or no receivers left");
                                return;
                            }
                        }
                    }
                }
            });
        } else {
            tracing::debug!("No SSE event receivers, event dropped: {:?}", message.event);
        }
    }

    /// Handle SSE request stream for a connected client
    ///
    /// This method manages an SSE connection with a client, sending events as they
    /// become available and handling connection lifecycle. It maintains the connection
    /// with keep-alive messages and proper error handling.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for the client connection
    /// * `rx` - Broadcast receiver for this specific client
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or a communication error
    pub async fn handle_sse_stream(
        writer: &mut tokio::io::WriteHalf<tokio::net::TcpStream>,
        mut rx: broadcast::Receiver<SSEMessage>,
    ) -> Result<()> {
        // Send SSE headers
        let response = "HTTP/1.1 200 OK\r\n\
                       Content-Type: text/event-stream\r\n\
                       Cache-Control: no-cache\r\n\
                       Connection: keep-alive\r\n\
                       Access-Control-Allow-Origin: *\r\n\
                       \r\n";

        writer
            .write_all(response.as_bytes())
            .await
            .map_err(|e| Error::Communication(format!("Failed to send SSE headers: {}", e)))?;

        // Send initial keep-alive comment
        writer
            .write_all(b": welcome to MCP Runner SSE stream\n\n")
            .await
            .map_err(|e| Error::Communication(format!("Failed to send welcome message: {}", e)))?;

        // Stream events
        loop {
            tokio::select! {
                message = rx.recv() => {
                    match message {
                        Ok(evt) => {
                            let formatted = evt.format();
                            if let Err(e) = writer.write_all(formatted.as_bytes()).await {
                                tracing::error!(error = %e, "Failed to write SSE message");
                                break;
                            }

                            // Ensure message is sent
                            if let Err(e) = writer.flush().await {
                                tracing::error!(error = %e, "Failed to flush SSE message");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Error receiving SSE message");
                            break;
                        }
                    }
                }
                // Send keep-alive comment every 30 seconds
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                    if let Err(e) = writer.write_all(b": keepalive\n\n").await {
                        tracing::error!(error = %e, "Failed to send keepalive");
                        break;
                    }

                    if let Err(e) = writer.flush().await {
                        tracing::error!(error = %e, "Failed to flush keepalive");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
