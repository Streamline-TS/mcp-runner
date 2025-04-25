use crate::error::Result;
use crate::Error;
use crate::proxy::types::{SSEEvent, SSEMessage};
use crate::server::ServerId;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;
use tracing;

/// Handles SSE event processing and broadcasting
pub struct EventManager {
    /// Broadcast channel for sending events
    sender: broadcast::Sender<SSEMessage>,
}

impl EventManager {
    /// Create a new event manager
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }
    
    /// Get a new receiver for the broadcast channel
    pub fn subscribe(&self) -> broadcast::Receiver<SSEMessage> {
        self.sender.subscribe()
    }
    
    /// Send a tool response event
    pub fn send_tool_response(
        &self,
        request_id: &str,
        server_id: &str,
        tool_name: &str,
        response: Value,
    ) {
        // Create the SSEEvent payload
        let event_payload = SSEEvent::ToolResponse {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            response,
        };

        // Send the event with the correct type name
        self.send_event("tool-response", &event_payload, Some(request_id));
    }

    /// Send a tool error event
    pub fn send_tool_error(
        &self,
        request_id: &str,
        server_id: &str,
        tool_name: &str,
        error: &str,
    ) {
        // Create the SSEEvent payload
        let event_payload = SSEEvent::ToolError {
            request_id: request_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: tool_name.to_string(),
            error: error.to_string(),
        };

        // Send the event with the correct type name
        self.send_event("tool-error", &event_payload, Some(request_id));
    }

    /// Send a server status update
    pub fn send_status_update(&self, server_id: ServerId, server_name: &str, status: &str) {
        // Create the SSEEvent payload
        let event_payload = SSEEvent::ServerStatus {
            server_id: format!("{:?}", server_id),
            server_name: server_name.to_string(),
            status: status.to_string(),
        };

        // Send the event with the correct type name
        self.send_event("server-status", &event_payload, None);
    }
    
    /// Handle SSE request stream
    pub async fn handle_sse_stream(
        mut writer: tokio::io::WriteHalf<tokio::net::TcpStream>,
        mut rx: broadcast::Receiver<SSEMessage>,
    ) -> Result<()> {
        // Send SSE headers
        let response = "HTTP/1.1 200 OK\r\n\
                       Content-Type: text/event-stream\r\n\
                       Cache-Control: no-cache\r\n\
                       Connection: keep-alive\r\n\
                       Access-Control-Allow-Origin: *\r\n\
                       \r\n";
        
        writer.write_all(response.as_bytes()).await.map_err(|e| {
            Error::Communication(format!("Failed to send SSE headers: {}", e))
        })?;
        
        // Send initial keep-alive comment
        writer.write_all(b": welcome to MCP Runner SSE stream\n\n").await.map_err(|e| {
            Error::Communication(format!("Failed to send welcome message: {}", e))
        })?;
        
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
    
    // Private helper to send an event
    // Takes a serializable payload (like SSEEvent)
    fn send_event<T: serde::Serialize>(&self, event_type: &str, payload: &T, id: Option<&str>) {
        match serde_json::to_string(payload) { // Serialize the payload (SSEEvent)
            Ok(json_data) => {
                // Create the SSEMessage envelope with the serialized data
                let message = SSEMessage::new(event_type, &json_data, id);
                if let Err(e) = self.sender.send(message) {
                    tracing::error!(error = %e, "Failed to broadcast SSE event");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize SSE event payload");
            }
        }
    }
}