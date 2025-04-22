use crate::error::{Error, Result};
use super::json_rpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use async_process::{ChildStdin, ChildStdout};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use std::collections::HashMap;
use uuid::Uuid;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

/// A transport for communicating with an MCP server via STDIO
pub struct StdioTransport {
    /// Server name
    name: String,
    /// Child process stdin
    stdin: Arc<Mutex<ChildStdin>>,
    /// Response handlers
    response_handlers: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    /// Task handle for reading from stdout
    reader_task: Option<JoinHandle<()>>,
}

impl StdioTransport {
    /// Create a new STDIO transport
    pub fn new(name: String, stdin: ChildStdin, mut stdout: ChildStdout) -> Self {
        let stdin = Arc::new(Mutex::new(stdin));
        let response_handlers = Arc::new(Mutex::new(HashMap::<String, oneshot::Sender<JsonRpcResponse>>::new()));
        
        // Clone for the reader task
        let response_handlers_clone = Arc::clone(&response_handlers);
        
        // Spawn a task to read from stdout
        let reader_task = tokio::spawn(async move {
            // Process stdout line by line
            let mut buffer = Vec::new();
            let mut buf = [0u8; 1];
            
            loop {
                // Try to read a single byte
                match stdout.read(&mut buf).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if buf[0] == b'\n' {
                            // Process the line
                            if let Ok(line) = String::from_utf8(buffer.clone()) {
                                if let Ok(message) = serde_json::from_str::<JsonRpcMessage>(&line) {
                                    if let JsonRpcMessage::Response(response) = message {
                                        // Get ID as string
                                        let id_str = match &response.id {
                                            Value::String(s) => s.clone(),
                                            Value::Number(n) => n.to_string(),
                                            _ => continue, // Skip responses with other ID types
                                        };
                                        
                                        // Send response to handler - handle lock errors gracefully
                                        if let Ok(mut handlers) = response_handlers_clone.lock() {
                                            if let Some(sender) = handlers.remove(&id_str) {
                                                let _ = sender.send(response);
                                            }
                                        }
                                    }
                                }
                            }
                            buffer.clear();
                        } else {
                            buffer.push(buf[0]);
                        }
                    },
                    Err(_) => break, // Error
                }
            }
        });
        
        Self {
            name,
            stdin,
            response_handlers,
            reader_task: Some(reader_task),
        }
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Helper function to write data to stdin
    async fn write_to_stdin(&self, data: Vec<u8>) -> Result<()> {
        let stdin_clone = self.stdin.clone();
        
        tokio::task::spawn_blocking(move || -> Result<()> {
            let stdin_lock = stdin_clone.lock()
                .map_err(|_| Error::Communication("Failed to acquire stdin lock".to_string()))?;
                
            // Use a blocking approach that doesn't require a nested runtime
            let mut stdin = stdin_lock;
            
            // Create a scope to ensure the mutex is released as soon as possible
            {
                // Use the existing tokio runtime instead of creating a new one
                futures_lite::future::block_on(async {
                    stdin.write_all(&data).await
                        .map_err(|e| Error::Communication(format!("Failed to write to stdin: {}", e)))?;
                    stdin.flush().await
                        .map_err(|e| Error::Communication(format!("Failed to flush stdin: {}", e)))?;
                    Ok::<(), Error>(())
                })?;
            }
            
            Ok(())
        }).await.map_err(|e| Error::Communication(format!("Task join error: {}", e)))??;
        
        Ok(())
    }
    
    /// Send a request and wait for a response
    pub async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        // Convert ID to string for map lookup
        let id_str = match &request.id {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return Err(Error::Communication("Invalid request ID type".to_string())),
        };
        
        // Create a channel for receiving the response
        let (sender, receiver) = oneshot::channel();
        
        // Register the response handler
        {
            let mut handlers = self.response_handlers.lock()
                .map_err(|_| Error::Communication("Failed to lock response handlers".to_string()))?;
            handlers.insert(id_str, sender);
        }
        
        // Serialize the request
        let request_json = serde_json::to_string(&request)
            .map_err(|e| Error::Serialization(format!("Failed to serialize request: {}", e)))?;
        let request_bytes = request_json.into_bytes();
        let mut request_bytes_with_newline = request_bytes;
        request_bytes_with_newline.push(b'\n');
        
        // Send the request
        self.write_to_stdin(request_bytes_with_newline).await?;
        
        // Wait for the response
        let response = receiver.await
            .map_err(|_| Error::Communication("Failed to receive response".to_string()))?;
        
        // Check for error
        if let Some(error) = response.error {
            return Err(Error::JsonRpc(error.message));
        }
        
        Ok(response)
    }
    
    /// Send a notification
    pub async fn send_notification(&self, notification: serde_json::Value) -> Result<()> {
        // Serialize the notification
        let notification_json = serde_json::to_string(&notification)
            .map_err(|e| Error::Serialization(format!("Failed to serialize notification: {}", e)))?;
        let notification_bytes = notification_json.into_bytes();
        let mut notification_bytes_with_newline = notification_bytes;
        notification_bytes_with_newline.push(b'\n');
        
        // Send the notification using our helper method
        self.write_to_stdin(notification_bytes_with_newline).await
    }
    
    /// Initialize the MCP server
    pub async fn initialize(&self) -> Result<()> {
        // Send initialized notification
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        
        self.send_notification(notification).await
    }
    
    /// List available tools
    pub async fn list_tools(&self) -> Result<Vec<Value>> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::list_tools(request_id);
        
        let response = self.send_request(request).await?;
        
        if let Some(Value::Object(result)) = response.result {
            if let Some(Value::Array(tools)) = result.get("tools") {
                return Ok(tools.clone());
            }
        }
        
        Ok(Vec::new())
    }
    
    /// Call a tool
    pub async fn call_tool(&self, name: impl Into<String>, args: Value) -> Result<Value> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::call_tool(request_id, name, args);
        
        let response = self.send_request(request).await?;
        
        response.result.ok_or_else(|| Error::Communication("No result in response".to_string()))
    }
    
    /// List available resources
    pub async fn list_resources(&self) -> Result<Vec<Value>> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::list_resources(request_id);
        
        let response = self.send_request(request).await?;
        
        if let Some(Value::Object(result)) = response.result {
            if let Some(Value::Array(resources)) = result.get("resources") {
                return Ok(resources.clone());
            }
        }
        
        Ok(Vec::new())
    }
    
    /// Get a resource
    pub async fn get_resource(&self, uri: impl Into<String>) -> Result<Value> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::get_resource(request_id, uri);
        
        let response = self.send_request(request).await?;
        
        response.result.ok_or_else(|| Error::Communication("No result in response".to_string()))
    }
    
    /// Close the transport
    pub async fn close(&mut self) -> Result<()> {
        // Drop the reader task
        if let Some(task) = self.reader_task.take() {
            task.abort();
            // Ignore errors from abort as it's expected
            let _ = task.await;
        }
        
        // Clear any pending response handlers to avoid resource leaks
        if let Ok(mut handlers) = self.response_handlers.lock() {
            // Send errors to all pending handlers
            for (_, sender) in handlers.drain() {
                let _ = sender.send(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(super::json_rpc::JsonRpcError {
                        code: -32099,
                        message: "Connection closed".to_string(),
                        data: None,
                    }),
                });
            }
        }
        
        Ok(())
    }
}