use super::json_rpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, error_codes};
use crate::error::{Error, Result};
use crate::transport::Transport;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{self, Instrument, span};
use uuid::Uuid;

/// StdioTransport provides communication with an MCP server via standard I/O.
///
/// This implementation uses JSON-RPC over standard input/output to communicate with
/// an MCP server. It handles concurrent requests using a background task for reading
/// responses and dispatches them to the appropriate handler.
/// Most public methods are instrumented with `tracing` spans.
///
/// # Example
///
/// ```no_run
/// use mcp_runner::transport::StdioTransport;
/// use mcp_runner::error::Result;
/// use tokio::process::{Child, Command};
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
    /// This method is instrumented with `tracing`.
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
    #[tracing::instrument(skip(stdin, stdout), fields(name = %name))]
    pub fn new(name: String, stdin: ChildStdin, mut stdout: ChildStdout) -> Self {
        tracing::debug!("Creating new StdioTransport");
        let stdin = Arc::new(Mutex::new(stdin));
        let response_handlers = Arc::new(Mutex::new(HashMap::<
            String,
            oneshot::Sender<JsonRpcResponse>,
        >::new()));

        // Clone for the reader task
        let response_handlers_clone = Arc::clone(&response_handlers);

        // Create the span for the reader task explicitly
        let reader_span = span!(tracing::Level::INFO, "stdout_reader", name = %name);

        // Spawn a task to read from stdout
        // Use .instrument() on the future
        let reader_task = tokio::spawn(async move {
            tracing::debug!("Stdout reader task started");
            // Process stdout line by line
            let mut buffer = Vec::new();
            let mut buf = [0u8; 1];

            loop {
                // Try to read a single byte
                match stdout.read(&mut buf).await {
                    Ok(0) => {
                        tracing::debug!("Stdout reached EOF");
                        break;
                    } // EOF
                    Ok(_) => {
                        if buf[0] == b'\n' {
                            // Process the line
                            if let Ok(line) = String::from_utf8(buffer.clone()) {
                                let trimmed_line = line.trim();
                                if trimmed_line.is_empty() {
                                    // Ignore empty lines
                                    buffer.clear();
                                    continue;
                                }

                                // Check if the line looks like a JSON object before attempting to parse
                                if !trimmed_line.starts_with('{') {
                                    tracing::trace!(output = "stdout", line = %trimmed_line, "Ignoring non-JSON line");
                                    buffer.clear();
                                    continue;
                                }

                                // Attempt to parse as JSON-RPC
                                tracing::trace!(output = "stdout", line = %trimmed_line, "Attempting to parse line as JSON-RPC");
                                match serde_json::from_str::<JsonRpcMessage>(trimmed_line) {
                                    Ok(JsonRpcMessage::Response(response)) => {
                                        // Get ID as string
                                        let id_str = match &response.id {
                                            Value::String(s) => s.clone(),
                                            Value::Number(n) => n.to_string(),
                                            _ => {
                                                tracing::warn!(response_id = ?response.id, "Received response with unexpected ID type");
                                                continue;
                                            }
                                        };
                                        tracing::debug!(response_id = %id_str, "Received JSON-RPC response");

                                        // Send response to handler - handle lock errors gracefully
                                        if let Ok(mut handlers) = response_handlers_clone.lock() {
                                            if let Some(sender) = handlers.remove(&id_str) {
                                                if sender.send(response).is_err() {
                                                    tracing::warn!(response_id = %id_str, "Response handler dropped before response could be sent");
                                                }
                                            } else {
                                                tracing::warn!(response_id = %id_str, "Received response for unknown or timed out request");
                                            }
                                        } else {
                                            tracing::error!("Response handler lock poisoned!");
                                        }
                                    }
                                    Ok(JsonRpcMessage::Request(req)) => {
                                        tracing::warn!(method = %req.method, "Received unexpected JSON-RPC request from server");
                                    }
                                    Ok(JsonRpcMessage::Notification(notif)) => {
                                        tracing::debug!(method = %notif.method, "Received JSON-RPC notification from server");
                                    }
                                    Err(e) => {
                                        // Keep WARN for lines that start like JSON but fail to parse
                                        tracing::warn!(line = %trimmed_line, error = %e, "Failed to parse potential JSON-RPC message");
                                    }
                                }
                            } else {
                                // Log if line is not valid UTF-8
                                tracing::warn!(bytes = ?buffer, "Received non-UTF8 data on stdout");
                            }
                            buffer.clear();
                        } else {
                            buffer.push(buf[0]);
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Error reading from stdout");
                        break;
                    } // Error
                }
            }
            tracing::debug!("Stdout reader task finished");
        }.instrument(reader_span)); // Apply the span to the future

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
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `data` - The bytes to write to stdin
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    #[tracing::instrument(skip(self, data), fields(name = %self.name))]
    async fn write_to_stdin(&self, data: Vec<u8>) -> Result<()> {
        tracing::trace!(bytes_len = data.len(), "Writing to stdin");
        let stdin_clone = self.stdin.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let stdin_lock = stdin_clone
                .lock()
                .map_err(|_| Error::Communication("Failed to acquire stdin lock".to_string()))?;

            let mut stdin = stdin_lock;

            futures_lite::future::block_on(async {
                stdin.write_all(&data).await.map_err(|e| {
                    Error::Communication(format!("Failed to write to stdin: {}", e))
                })?;
                stdin
                    .flush()
                    .await
                    .map_err(|e| Error::Communication(format!("Failed to flush stdin: {}", e)))?;
                Ok::<(), Error>(())
            })?;

            Ok(())
        })
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Stdin write task panicked");
            Error::Communication(format!("Task join error: {}", e))
        })??;

        tracing::trace!("Finished writing to stdin");
        Ok(())
    }

    /// Sends a JSON-RPC request and waits for a response.
    ///
    /// This method handles the details of sending a request, registering a response
    /// handler, and waiting for the response to arrive.
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `request` - The JSON-RPC request to send
    ///
    /// # Returns
    ///
    /// A `Result<JsonRpcResponse>` containing the response if successful
    #[tracing::instrument(skip(self, request), fields(name = %self.name, method = %request.method, request_id = ?request.id))]
    pub async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        tracing::debug!("Sending JSON-RPC request");
        let id_str = match &request.id {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return Err(Error::Communication("Invalid request ID type".to_string())),
        };

        let (sender, receiver) = oneshot::channel();

        {
            let mut handlers = self.response_handlers.lock().map_err(|_| {
                Error::Communication("Failed to lock response handlers".to_string())
            })?;
            handlers.insert(id_str, sender);
        }

        let request_json = serde_json::to_string(&request)
            .map_err(|e| Error::Serialization(format!("Failed to serialize request: {}", e)))?;
        tracing::trace!(request_json = %request_json, "Sending request JSON");
        let request_bytes = request_json.into_bytes();
        let mut request_bytes_with_newline = request_bytes;
        request_bytes_with_newline.push(b'\n');

        self.write_to_stdin(request_bytes_with_newline).await?;

        tracing::debug!("Waiting for response");
        let response = receiver.await.map_err(|_| {
            tracing::warn!("Sender dropped before response received (likely timeout or closed)");
            Error::Communication("Failed to receive response".to_string())
        })?;

        if let Some(error) = &response.error {
            tracing::error!(error_code = error.code, error_message = %error.message, "Received JSON-RPC error response");
            return Err(Error::JsonRpc(error.message.clone()));
        }

        tracing::debug!("Received successful response");
        Ok(response)
    }

    /// Sends a JSON-RPC notification (no response expected).
    ///
    /// Unlike requests, notifications don't expect a response, so this method
    /// just sends the message without setting up a response handler.
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `notification` - The JSON-RPC notification to send
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    #[tracing::instrument(skip(self, notification), fields(name = %self.name, method = notification.get("method").and_then(|v| v.as_str())))]
    pub async fn send_notification(&self, notification: serde_json::Value) -> Result<()> {
        tracing::debug!("Sending JSON-RPC notification");
        let notification_json = serde_json::to_string(&notification).map_err(|e| {
            Error::Serialization(format!("Failed to serialize notification: {}", e))
        })?;
        tracing::trace!(notification_json = %notification_json, "Sending notification JSON");
        let notification_bytes = notification_json.into_bytes();
        let mut notification_bytes_with_newline = notification_bytes;
        notification_bytes_with_newline.push(b'\n');

        self.write_to_stdin(notification_bytes_with_newline).await
    }

    /// Initializes the MCP server.
    ///
    /// Sends the `notifications/initialized` notification to the server,
    /// indicating that the client is ready to communicate.
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    #[tracing::instrument(skip(self), fields(name = %self.name))]
    pub async fn initialize(&self) -> Result<()> {
        tracing::info!("Initializing MCP connection");
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        self.send_notification(notification).await
    }

    /// Lists available tools provided by the MCP server.
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` containing the list of tools if successful
    #[tracing::instrument(skip(self), fields(name = %self.name))]
    pub async fn list_tools(&self) -> Result<Vec<Value>> {
        tracing::debug!("Listing tools");
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
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `args` - The arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// A `Result<Value>` containing the tool's response if successful
    #[tracing::instrument(skip(self, args), fields(name = %self.name, tool_name = %name.as_ref()))]
    pub async fn call_tool(
        &self,
        name: impl AsRef<str> + std::fmt::Debug,
        args: Value,
    ) -> Result<Value> {
        tracing::debug!(args = ?args, "Calling tool");
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::call_tool(request_id, name.as_ref().to_string(), args);

        let response = self.send_request(request).await?;

        response
            .result
            .ok_or_else(|| Error::Communication("No result in response".to_string()))
    }

    /// Lists available resources provided by the MCP server.
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` containing the list of resources if successful
    #[tracing::instrument(skip(self), fields(name = %self.name))]
    pub async fn list_resources(&self) -> Result<Vec<Value>> {
        tracing::debug!("Listing resources");
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
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to retrieve
    ///
    /// # Returns
    ///
    /// A `Result<Value>` containing the resource data if successful
    #[tracing::instrument(skip(self), fields(name = %self.name, uri = %uri.as_ref()))]
    pub async fn get_resource(&self, uri: impl AsRef<str> + std::fmt::Debug) -> Result<Value> {
        tracing::debug!("Getting resource");
        let request_id = Uuid::new_v4().to_string();
        let request = JsonRpcRequest::get_resource(request_id, uri.as_ref().to_string());

        let response = self.send_request(request).await?;

        response
            .result
            .ok_or_else(|| Error::Communication("No result in response".to_string()))
    }

    /// Closes the transport and cleans up resources.
    ///
    /// This method should be called when the transport is no longer needed
    /// to ensure proper cleanup of background tasks and resources.
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    #[tracing::instrument(skip(self), fields(name = %self.name))]
    pub async fn close(&mut self) -> Result<()> {
        tracing::info!("Closing transport");
        if let Some(task) = self.reader_task.take() {
            task.abort();
            let _ = task.await;
        }

        if let Ok(mut handlers) = self.response_handlers.lock() {
            for (_, sender) in handlers.drain() {
                let _ = sender.send(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(super::json_rpc::JsonRpcError {
                        code: error_codes::SERVER_ERROR,
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
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(name = %self.name()))]
    async fn initialize(&self) -> Result<()> {
        self.initialize().await
    }

    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(name = %self.name()))]
    async fn list_tools(&self) -> Result<Vec<Value>> {
        self.list_tools().await
    }

    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self, args), fields(name = %self.name(), tool_name = %name))]
    async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.call_tool(name.to_string(), args).await
    }

    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(name = %self.name()))]
    async fn list_resources(&self) -> Result<Vec<Value>> {
        self.list_resources().await
    }

    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(name = %self.name(), uri = %uri))]
    async fn get_resource(&self, uri: &str) -> Result<Value> {
        self.get_resource(uri.to_string()).await
    }
}
