use crate::error::{Error, Result};
use crate::transport::Transport;
use super::json_rpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use async_process::{ChildStdin, ChildStdout};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use std::collections::HashMap;
use uuid::Uuid;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
use async_trait::async_trait;

/// StdioTransport provides communication with an MCP server via standard I/O.
///
/// This implementation uses JSON-RPC over standard input/output to communicate with
/// an MCP server. It handles concurrent requests using a background task for reading 
/// responses and dispatches them to the appropriate handler.
///
/// # Example
///
/// ```no_run
/// use mcp_runner::transport::StdioTransport;
/// use mcp_runner::error::Result;
/// use async_process::{Child, Command};
/// use serde_json::json;
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     // Start a child process
///     let mut child = Command::new("mcp-server")
///         .stdin(std::process::Stdio::piped())
///         .stdout(std::process::Stdio::piped())
///         .spawn()
///         .expect("Failed to start MCP server");
///     
///     // Take ownership of stdin/stdout
///     let stdin = child.stdin.take().expect("Failed to get stdin");
///     let stdout = child.stdout.take().expect("Failed to get stdout");
///     
///     // Create transport
///     let transport = StdioTransport::new("example-server".to_string(), stdin, stdout);
///     
///     // Initialize the transport
///     transport.initialize().await?;
///     
///     // List available tools
///     let tools = transport.list_tools().await?;
///     println!("Available tools: {:?}", tools);
///     
///     Ok(())
/// }
/// ```
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
    /// Creates a new StdioTransport instance.
    ///
    /// This constructor takes ownership of the child process's stdin and stdout,
    /// and sets up a background task to process incoming JSON-RPC messages.
    ///
    /// # Arguments
    ///
    /// * `name` - A name for this transport, typically the server name
    /// * `stdin` - The child process's stdin handle
    /// * `stdout` - The child process's stdout handle
    ///
    /// # Returns
    ///
    /// A new `StdioTransport` instance
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

    /// Gets the name of the server associated with this transport.
    ///
    /// # Returns
    ///
    /// A string slice containing the server name.
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Writes data to the child process's stdin.
    ///
    /// This is a helper function that handles the complexity of writing to
    /// stdin in a thread-safe and non-blocking way.
    ///
    /// # Arguments
    ///
    /// * `data` - The bytes to write to stdin
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
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
    
    /// Sends a JSON-RPC request and waits for a response.
    ///
    /// This method handles the details of sending a request, registering a response
    /// handler, and waiting for the response to arrive.
    ///
    /// # Arguments
    ///
    /// * `request` - The JSON-RPC request to send
    ///
    /// # Returns
    ///
    /// A `Result<JsonRpcResponse>` containing the response if successful
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
    
    /// Sends a JSON-RPC notification (no response expected).
    ///
    /// Unlike requests, notifications don't expect a response, so this method
    /// just sends the message without setting up a response handler.
    ///
    /// # Arguments
    ///
    /// * `notification` - The JSON-RPC notification to send
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
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
    
    /// Initializes the MCP server.
    ///
    /// Sends the `notifications/initialized` notification to the server,
    /// indicating that the client is ready to communicate.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    pub async fn initialize(&self) -> Result<()> {
        // Send initialized notification
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        
        self.send_notification(notification).await
    }
    
    /// Lists available tools provided by the MCP server.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` containing the list of tools if successful
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
    
    /// Calls a tool provided by the MCP server.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `args` - The arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// A `Result<Value>` containing the tool's response if successful
    pub async fn call_tool(&self, name: impl Into<String>, args: Value) -> Result<Value> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::call_tool(request_id, name, args);
        
        let response = self.send_request(request).await?;
        
        response.result.ok_or_else(|| Error::Communication("No result in response".to_string()))
    }
    
    /// Lists available resources provided by the MCP server.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` containing the list of resources if successful
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
    
    /// Retrieves a specific resource from the MCP server.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to retrieve
    ///
    /// # Returns
    ///
    /// A `Result<Value>` containing the resource data if successful
    pub async fn get_resource(&self, uri: impl Into<String>) -> Result<Value> {
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::get_resource(request_id, uri);
        
        let response = self.send_request(request).await?;
        
        response.result.ok_or_else(|| Error::Communication("No result in response".to_string()))
    }
    
    /// Closes the transport and cleans up resources.
    ///
    /// This method should be called when the transport is no longer needed
    /// to ensure proper cleanup of background tasks and resources.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
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

#[async_trait]
impl Transport for StdioTransport {
    async fn initialize(&self) -> Result<()> {
        self.initialize().await
    }
    
    async fn list_tools(&self) -> Result<Vec<Value>> {
        self.list_tools().await
    }
    
    async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.call_tool(name.to_string(), args).await
    }
    
    async fn list_resources(&self) -> Result<Vec<Value>> {
        self.list_resources().await
    }
    
    async fn get_resource(&self, uri: &str) -> Result<Value> {
        self.get_resource(uri.to_string()).await
    }
}