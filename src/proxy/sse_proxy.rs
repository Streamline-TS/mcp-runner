//! SSE proxy implementation for MCP servers.
//!
//! This module provides an HTTP and Server-Sent Events (SSE) proxy for MCP servers,
//! allowing web clients to interact with MCP servers through a unified REST-like API.
//! The proxy handles HTTP routing, authentication, and SSE event streaming.
//!
//! It enables clients to:
//! - Subscribe to SSE events from MCP servers
//! - Make tool calls to MCP servers via JSON-RPC
//! - List available servers, tools, and resources
//! - Retrieve resources from MCP servers
//!
//! The proxy is designed to be robust against network errors and malformed requests,
//! with comprehensive logging and error handling.

use crate::Error;
use crate::McpClient;
use crate::config::SSEProxyConfig;
use crate::error::Result;
use crate::server::ServerId;

use crate::proxy::events::EventManager;
use crate::proxy::http::HttpResponse;
use crate::proxy::types::{ResourceInfo, ServerInfo, ToolInfo};
use crate::transport::json_rpc::{JsonRpcRequest, JsonRpcResponse};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing;

/// Type alias for server ID retrieval function
type ServerIdRetriever = dyn Fn(&str) -> Result<ServerId> + Send + Sync;
/// Type alias for client retrieval function
type ClientRetriever = dyn Fn(ServerId) -> Result<McpClient> + Send + Sync;
/// Type alias for allowed servers retrieval function
type AllowedServersRetriever = dyn Fn() -> Option<Vec<String>> + Send + Sync;
/// Type alias for server config keys retrieval function
type ServerConfigKeysRetriever = dyn Fn() -> Vec<String> + Send + Sync;

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

/// Handle for controlling the SSE proxy
///
/// This handle is stored by the McpRunner to communicate with the SSE proxy.
/// It allows the runner to send updates to the proxy about server status changes
/// and other events without needing to access the proxy directly.
#[derive(Clone)]
pub struct SSEProxyHandle {
    /// Channel for sending server information updates to the proxy
    server_tx: mpsc::Sender<ServerInfoUpdate>,
    /// Proxy task handle
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Configuration for the proxy
    config: SSEProxyConfig,
    /// Shutdown flag
    shutdown_flag: Arc<AtomicBool>,
}

impl SSEProxyHandle {
    /// Create a new SSE proxy handle
    fn new(
        server_tx: mpsc::Sender<ServerInfoUpdate>,
        handle: JoinHandle<()>,
        config: SSEProxyConfig,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            server_tx,
            handle: Arc::new(Mutex::new(Some(handle))),
            config,
            shutdown_flag,
        }
    }

    /// Update server information in the proxy
    pub async fn update_server_info(
        &self,
        server_name: &str,
        server_id: Option<ServerId>,
        status: &str,
    ) -> Result<()> {
        let update = ServerInfoUpdate::UpdateServer {
            name: server_name.to_string(),
            id: server_id,
            status: status.to_string(),
        };

        self.server_tx.send(update).await.map_err(|e| {
            Error::Communication(format!("Failed to send server info update to proxy: {}", e))
        })
    }

    /// Add a new server to the proxy cache
    pub async fn add_server_info(&self, server_name: &str, server_info: ServerInfo) -> Result<()> {
        let update = ServerInfoUpdate::AddServer {
            name: server_name.to_string(),
            info: server_info,
        };

        self.server_tx.send(update).await.map_err(|e| {
            Error::Communication(format!("Failed to send server info update to proxy: {}", e))
        })
    }

    /// Shutdown the proxy
    pub async fn shutdown(&self) -> Result<()> {
        // Set the shutdown flag to signal the proxy to stop
        self.shutdown_flag.store(true, Ordering::SeqCst);

        // Send a shutdown message through the channel
        let _ = self.server_tx.send(ServerInfoUpdate::Shutdown).await;

        // Wait for the proxy task to finish
        let mut handle = self.handle.lock().await;
        if let Some(h) = handle.take() {
            // Wait with a timeout
            match tokio::time::timeout(std::time::Duration::from_secs(5), h).await {
                Ok(result) => {
                    if let Err(e) = result {
                        tracing::warn!("Error while joining proxy task: {}", e);
                    }
                }
                Err(_) => {
                    tracing::warn!("Timeout waiting for proxy task to finish");
                }
            }
        }

        Ok(())
    }

    /// Get the proxy configuration
    pub fn config(&self) -> &SSEProxyConfig {
        &self.config
    }
}

/// Access to McpRunner operations needed by the SSE proxy
///
/// This struct provides a controlled interface to the operations
/// the SSE proxy needs from the McpRunner, rather than giving
/// it direct access to the entire runner.
#[derive(Clone)]
pub struct SSEProxyRunnerAccess {
    /// Function to get server ID by name
    pub get_server_id: Arc<ServerIdRetriever>,
    /// Function to get a client for a server
    pub get_client: Arc<ClientRetriever>,
    /// Function to get allowed servers if configured
    pub get_allowed_servers: Arc<AllowedServersRetriever>,
    /// Function to get server config keys
    pub get_server_config_keys: Arc<ServerConfigKeysRetriever>,
}

/// SSE Proxy server for MCP servers
///
/// Provides an HTTP and SSE proxy that allows web clients to interact with MCP servers.
/// The proxy supports authentication, server listing, tool calls, and resource retrieval.
#[derive(Clone)]
pub struct SSEProxy {
    /// Configuration for the proxy
    config: SSEProxyConfig,
    /// Direct access to McpRunner for server operations
    runner_access: SSEProxyRunnerAccess,
    /// Event manager for broadcasting events (shared via Arc)
    event_manager: Arc<EventManager>,
    /// Server address
    address: SocketAddr,
    /// Server information cache (shared via Arc)
    server_info: Arc<Mutex<HashMap<String, ServerInfo>>>,
    /// Channel for receiving server updates from McpRunner
    server_rx: Arc<Mutex<Option<mpsc::Receiver<ServerInfoUpdate>>>>,
    /// Shutdown flag
    shutdown_flag: Arc<AtomicBool>,
}

impl SSEProxy {
    /// Create a new SSE proxy with runner access functions
    ///
    /// # Arguments
    ///
    /// * `runner_access` - Functions to access McpRunner operations
    /// * `config` - Configuration for the SSE proxy
    /// * `server_rx` - Channel for receiving server information updates
    ///
    /// # Returns
    ///
    /// A new `SSEProxy` instance
    pub fn new(
        runner_access: SSEProxyRunnerAccess,
        config: SSEProxyConfig,
        server_rx: mpsc::Receiver<ServerInfoUpdate>,
    ) -> Self {
        // Create socket address from config
        let address = match SocketAddr::from_str(&format!("{}:{}", config.address, config.port)) {
            Ok(addr) => addr,
            Err(e) => {
                // Log the error but fallback to a default address to avoid panicking
                tracing::error!(error = %e, "Failed to create socket address from config, using fallback address");
                SocketAddr::from_str("127.0.0.1:3000")
                    .expect("Hardcoded fallback address should be valid")
            }
        };

        // Initialize empty server info cache
        let server_info = HashMap::new();

        tracing::debug!("Initialized SSE proxy with direct runner access");

        Self {
            config,
            runner_access,
            event_manager: Arc::new(EventManager::new(100)), // Buffer up to 100 messages
            address,
            server_info: Arc::new(Mutex::new(server_info)),
            server_rx: Arc::new(Mutex::new(Some(server_rx))),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the SSE proxy server
    ///
    /// Creates a proxy handle and starts the server in a background task.
    /// Returns a handle that can be used to control and communicate with the proxy.
    ///
    /// # Arguments
    ///
    /// * `runner_access` - Functions to access McpRunner operations
    /// * `config` - Configuration for the SSE proxy
    ///
    /// # Returns
    ///
    /// A `Result` containing a `SSEProxyHandle` or an error
    pub async fn start_proxy(
        runner_access: SSEProxyRunnerAccess,
        config: SSEProxyConfig,
    ) -> Result<SSEProxyHandle> {
        // Create channel for communication between McpRunner and proxy
        let (server_tx, server_rx) = mpsc::channel(32);

        // Create the shutdown flag
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown_flag_clone = shutdown_flag.clone();

        // Create the proxy instance
        let proxy = Self::new(runner_access, config.clone(), server_rx);

        // Start the proxy in a background task
        let handle = tokio::spawn(async move {
            let _ = proxy.run().await;
        });

        // Return a handle to control the proxy
        Ok(SSEProxyHandle::new(
            server_tx,
            handle,
            config,
            shutdown_flag_clone,
        ))
    }

    /// Main execution loop for the proxy server
    ///
    /// This method manages both the HTTP server and
    /// the channel that receives server information updates.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    async fn run(&self) -> Result<()> {
        // Take the receiver out of the Option
        let mut server_rx = match self.server_rx.lock().await.take() {
            Some(rx) => rx,
            None => return Err(Error::Other("Server receiver already taken".to_string())),
        };

        // Start listening for connections
        tracing::info!(address = %self.address, "Starting SSE proxy server");

        let listener = TcpListener::bind(self.address).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to bind SSE proxy server");
            Error::Other(format!("Failed to bind SSE proxy: {}", e))
        })?;

        tracing::info!("SSE proxy server started, listening for connections");

        // Clone the shutdown flag for the connection acceptor loop
        let shutdown_flag = self.shutdown_flag.clone();
        let proxy_clone = self.clone();

        // Spawn a task to handle accepting connections
        let connection_handle = tokio::spawn(async move {
            while !shutdown_flag.load(Ordering::SeqCst) {
                match tokio::time::timeout(
                    tokio::time::Duration::from_millis(100),
                    listener.accept(),
                )
                .await
                {
                    Ok(Ok((stream, addr))) => {
                        tracing::debug!(client_addr = %addr, "New client connection");

                        // Clone the proxy (which clones the Arcs)
                        let inner_proxy_clone = proxy_clone.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                Self::handle_connection(stream, addr, inner_proxy_clone).await
                            {
                                tracing::error!(client_addr = %addr, error = %e, "Error handling client connection");
                            }
                        });
                    }
                    Ok(Err(e)) => {
                        tracing::error!(error = %e, "Error accepting client connection");
                    }
                    Err(_) => {
                        // Timeout - check shutdown flag and continue
                    }
                }
            }
        });

        // Main loop to process server information updates
        while !self.shutdown_flag.load(Ordering::SeqCst) {
            match tokio::time::timeout(tokio::time::Duration::from_millis(100), server_rx.recv())
                .await
            {
                Ok(Some(update)) => match update {
                    ServerInfoUpdate::UpdateServer { name, id, status } => {
                        self.handle_update_server_info(&name, id, &status).await;
                    }
                    ServerInfoUpdate::AddServer { name, info } => {
                        self.handle_add_server_info(&name, info).await;
                    }
                    ServerInfoUpdate::Shutdown => {
                        tracing::info!("Received shutdown message");
                        self.shutdown_flag.store(true, Ordering::SeqCst);
                        break;
                    }
                },
                Ok(None) => {
                    // Channel closed
                    tracing::info!("Server information channel closed, shutting down proxy");
                    self.shutdown_flag.store(true, Ordering::SeqCst);
                    break;
                }
                Err(_) => {
                    // Timeout - check shutdown flag and continue
                }
            }
        }

        // Wait for the connection acceptor to finish
        if let Err(e) = connection_handle.await {
            tracing::warn!("Error joining connection acceptor task: {}", e);
        }

        tracing::info!("SSE proxy server shut down");
        Ok(())
    }

    /// Handle updates to server information
    async fn handle_update_server_info(
        &self,
        server_name: &str,
        server_id: Option<ServerId>,
        status: &str,
    ) {
        let mut server_info_cache = self.server_info.lock().await;

        if let Some(info) = server_info_cache.get_mut(server_name) {
            if let Some(id) = server_id {
                // Update with new information
                info.id = format!("{:?}", id);
                info.status = status.to_string();
                tracing::debug!(
                    server = %server_name,
                    server_id = ?id,
                    status = %status,
                    "Updated server info in SSE proxy cache"
                );
            } else {
                // Server was removed or stopped
                info.id = "not_running".to_string();
                info.status = "Stopped".to_string();
                tracing::debug!(
                    server = %server_name,
                    "Marked server as stopped in SSE proxy cache"
                );
            }

            // Send a status update event to clients
            self.send_status_update(server_id.unwrap_or_else(ServerId::new), server_name, status);
        } else {
            tracing::warn!(
                server = %server_name,
                "Attempted to update server info in SSE proxy cache, but server not found"
            );
        }
    }

    /// Handle adding new server information
    async fn handle_add_server_info(&self, server_name: &str, server_info: ServerInfo) {
        let mut server_info_cache = self.server_info.lock().await;

        if server_info_cache.contains_key(server_name) {
            tracing::warn!(
                server = %server_name,
                "Attempted to add server to SSE proxy cache, but server already exists"
            );
        } else {
            // Add the new server info
            server_info_cache.insert(server_name.to_string(), server_info.clone());
            tracing::info!(
                server = %server_name,
                "Added new server to SSE proxy cache"
            );

            // Send a status update event to clients
            if let Ok(id) = (self.runner_access.get_server_id)(server_name) {
                self.send_status_update(id, server_name, &server_info.status);
            }
        }
    }

    /// Handle an incoming HTTP connection
    ///
    /// Processes an incoming HTTP connection, parsing the request and routing it
    /// to the appropriate handler based on the path and method.
    ///
    /// # Arguments
    ///
    /// * `stream` - TCP stream for the client connection
    /// * `_addr` - Socket address of the client
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_connection(
        stream: TcpStream,
        _addr: SocketAddr,
        proxy: SSEProxy,
    ) -> Result<()> {
        // Create a buffered reader for the stream
        let (reader, mut writer) = tokio::io::split(stream);
        let mut buf_reader = tokio::io::BufReader::new(reader);

        // Read the request line
        let mut headers = HashMap::new();
        let mut body = Vec::new();

        // Read the request line and headers
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut buf_reader, &mut line)
            .await
            .map_err(|e| Error::Communication(format!("Failed to read request line: {}", e)))?;
        tracing::debug!(request = %line.trim(), "Received HTTP request");

        // Parse request line
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            tracing::warn!(line = %line.trim(), "Invalid HTTP request line");
            return HttpResponse::send_bad_request_response(
                &mut writer,
                "Invalid HTTP request format",
            )
            .await;
        }
        let method = parts[0];
        let path = parts[1];

        // Read headers
        loop {
            let mut header_line = String::new();
            tokio::io::AsyncBufReadExt::read_line(&mut buf_reader, &mut header_line)
                .await
                .map_err(|e| Error::Communication(format!("Failed to read header: {}", e)))?;

            let line = header_line.trim();
            if line.is_empty() {
                break;
            }

            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_lowercase(), value.trim().to_string());
            }
        }

        // Check for authentication if required
        if let Some(auth) = &proxy.config.authenticate {
            if let Some(bearer) = &auth.bearer {
                let token = if let Some(auth_header) = headers.get("authorization") {
                    if let Some(stripped) = auth_header.strip_prefix("Bearer ") {
                        stripped.to_string()
                    } else {
                        return HttpResponse::send_unauthorized_response(&mut writer).await;
                    }
                } else {
                    return HttpResponse::send_unauthorized_response(&mut writer).await;
                };

                if token != bearer.token {
                    return HttpResponse::send_unauthorized_response(&mut writer).await;
                }
            }
        }

        // Route based on the path and method
        match (method, path) {
            // SSE event stream endpoint
            ("GET", "/events") => {
                // Use the cloned Arc<EventManager>
                EventManager::handle_sse_stream(&mut writer, proxy.event_manager.subscribe()).await
            }
            // JSON-RPC initialize endpoint
            ("POST", "/initialize") => {
                // Add max size limit for security
                const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MB
                let content_length = headers
                    .get("content-length")
                    .and_then(|len| len.parse::<usize>().ok())
                    .unwrap_or(0);

                if content_length > MAX_BODY_SIZE {
                    tracing::warn!(length = content_length, "Request body too large");
                    return HttpResponse::send_bad_request_response(
                        &mut writer,
                        "Request body too large",
                    )
                    .await;
                }

                if content_length > 0 {
                    body = vec![0; content_length];
                    match tokio::io::AsyncReadExt::read_exact(&mut buf_reader, &mut body).await {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to read request body");
                            return HttpResponse::send_bad_request_response(
                                &mut writer,
                                "Failed to read request body",
                            )
                            .await;
                        }
                    }
                }
                // Pass writer directly
                Self::handle_initialize(&mut writer, &body).await
            }
            // Tool call endpoint (JSON-RPC enforced)
            ("POST", "/tool") => {
                // Add max size limit for security
                const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MB
                let content_length = headers
                    .get("content-length")
                    .and_then(|len| len.parse::<usize>().ok())
                    .unwrap_or(0);

                if content_length > MAX_BODY_SIZE {
                    tracing::warn!(length = content_length, "Request body too large");
                    return HttpResponse::send_bad_request_response(
                        &mut writer,
                        "Request body too large",
                    )
                    .await;
                }

                if content_length > 0 {
                    body = vec![0; content_length];
                    match tokio::io::AsyncReadExt::read_exact(&mut buf_reader, &mut body).await {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to read request body");
                            return HttpResponse::send_bad_request_response(
                                &mut writer,
                                "Failed to read request body",
                            )
                            .await;
                        }
                    }
                }
                // Pass writer and proxy directly
                Self::handle_tool_call_jsonrpc(&mut writer, &body, proxy).await
            }
            // List available servers endpoint
            ("GET", "/servers") => {
                // Pass writer and proxy directly
                Self::handle_list_servers(&mut writer, proxy).await
            }
            // List tools for a specific server
            ("GET", p) if p.starts_with("/servers/") && p.ends_with("/tools") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() == 4 {
                    let server_name = parts[2];
                    // Pass writer and proxy directly
                    Self::handle_list_tools(&mut writer, server_name, proxy).await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // List resources for a specific server
            ("GET", p) if p.starts_with("/servers/") && p.ends_with("/resources") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() == 4 {
                    let server_name = parts[2];
                    // Pass writer and proxy directly
                    Self::handle_list_resources(&mut writer, server_name, proxy).await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // Get a specific resource
            ("GET", p) if p.starts_with("/resource/") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() >= 4 {
                    let server_name = parts[2];
                    let resource_uri = parts[3..].join("/");
                    // Pass writer and proxy directly
                    Self::handle_get_resource(&mut writer, server_name, &resource_uri, proxy).await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // OPTIONS for CORS
            ("OPTIONS", _) => HttpResponse::handle_options_request(&mut writer).await,
            // Not found for other paths
            _ => HttpResponse::send_not_found_response(&mut writer).await,
        }
    }

    /// Handle JSON-RPC initialize request
    ///
    /// Processes a JSON-RPC initialize request, validating it and returning
    /// information about the proxy capabilities.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `body` - Request body bytes
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_initialize(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        body: &[u8],
    ) -> Result<()> {
        use crate::transport::json_rpc::{JSON_RPC_VERSION, JsonRpcRequest, JsonRpcResponse};
        let req: JsonRpcRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::json!(null),
                    -32700, // Parse error
                    format!("Parse error: {}", e),
                    None,
                );
                // Use HttpResponse helper for error
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        // Fallback to generic 500 if serialization fails
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        if req.method != "initialize" {
            let resp = JsonRpcResponse::error(
                req.id,
                -32601, // Method not found
                "Method not found (expected 'initialize')",
                None,
            );
            // Use HttpResponse helper for error
            match serde_json::to_string(&resp) {
                Ok(json) => return HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                Err(serialize_err) => {
                    tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                    return HttpResponse::send_error_response(
                        writer,
                        500,
                        "Internal server error during error reporting",
                    )
                    .await;
                }
            }
        }

        // Respond with protocol version and capabilities
        let result = serde_json::json!({
            "protocolVersion": JSON_RPC_VERSION,
            "capabilities": {
                "sse": true,
                "tools": true,
                "resources": true
            }
        });
        let resp = JsonRpcResponse::success(req.id, result);

        // Use HttpResponse helper for success
        match serde_json::to_string(&resp) {
            Ok(json) => HttpResponse::send_json_response(writer, &json).await,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize JSON-RPC success response");
                HttpResponse::send_error_response(
                    writer,
                    500,
                    "Internal server error during response serialization",
                )
                .await
            }
        }
    }

    /// Handle tool call request as JSON-RPC
    ///
    /// Processes a JSON-RPC tool call request, parsing and validating the parameters,
    /// and then forwarding the call to the appropriate MCP server through the runner.
    /// The actual tool response is sent asynchronously via SSE.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `body` - Request body bytes
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_tool_call_jsonrpc(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        body: &[u8],
        proxy: SSEProxy,
    ) -> Result<()> {
        let req: JsonRpcRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::json!(null),
                    -32700, // Parse error
                    format!("Parse error: {}", e),
                    None,
                );
                // Use HttpResponse helper for error
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        if req.method != "tools/call" {
            let resp = JsonRpcResponse::error(
                req.id.clone(), // Clone id for error response
                -32601,         // Method not found
                "Method not found (expected 'tools/call')",
                None,
            );
            // Use HttpResponse helper for error
            match serde_json::to_string(&resp) {
                Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                Err(serialize_err) => {
                    tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                    return HttpResponse::send_error_response(
                        writer,
                        500,
                        "Internal server error during error reporting",
                    )
                    .await;
                }
            }
        }

        // Extract params
        let (server, tool, args) = match &req.params {
            Some(params) => {
                // Improved parameter validation with explicit error handling
                let server = match params.get("server").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        let resp = JsonRpcResponse::error(
                            req.id.clone(),
                            -32602, // Invalid params
                            "Invalid 'server' parameter: must be a string",
                            None,
                        );
                        match serde_json::to_string(&resp) {
                            Ok(json) => {
                                return HttpResponse::send_json_response(writer, &json).await;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                                return HttpResponse::send_error_response(
                                    writer,
                                    500,
                                    "Internal server error during error reporting",
                                )
                                .await;
                            }
                        }
                    }
                };

                let tool = match params.get("tool").and_then(|v| v.as_str()) {
                    Some(t) => t.to_string(),
                    None => {
                        let resp = JsonRpcResponse::error(
                            req.id.clone(),
                            -32602, // Invalid params
                            "Invalid 'tool' parameter: must be a string",
                            None,
                        );
                        match serde_json::to_string(&resp) {
                            Ok(json) => {
                                return HttpResponse::send_json_response(writer, &json).await;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                                return HttpResponse::send_error_response(
                                    writer,
                                    500,
                                    "Internal server error during error reporting",
                                )
                                .await;
                            }
                        }
                    }
                };

                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                (server, tool, args)
            }
            None => {
                let resp = JsonRpcResponse::error(
                    req.id.clone(),
                    -32602, // Invalid params
                    "Missing required parameters",
                    None,
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        // Call the tool and respond
        // Use proxy directly, pass request_id as string
        let request_id_str = req.id.to_string(); // Convert JsonRpcId to string for process_tool_call
        let result = proxy
            .process_tool_call(&server, &tool, args, &request_id_str)
            .await;

        match result {
            Ok(_) => {
                // Send success response (tool result is sent via SSE)
                let resp = JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({ "status": "accepted", "request_id": request_id_str }),
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => HttpResponse::send_json_response(writer, &json).await,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize JSON-RPC success response");
                        HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during response serialization",
                        )
                        .await
                    }
                }
            }
            Err(e) => {
                // Send error response
                let resp = JsonRpcResponse::error(
                    req.id,
                    -32000, // Server error
                    format!("Tool call failed: {}", e),
                    None, // No additional data for this error
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await
                    }
                }
            }
        }
    }

    /// Handle a request to list available servers
    ///
    /// Returns a JSON array of server information, including name, ID, and status.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_list_servers(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::info!("Handling list servers request");

        // Debug the entire proxy config structure
        if let Ok(config_json) = serde_json::to_string(&proxy.config) {
            tracing::info!("Full proxy config: {}", config_json);
        } else {
            tracing::warn!("Unable to serialize proxy config for debugging");
        }

        let server_info_cache = proxy.server_info.lock().await;
        let mut servers = Vec::new();

        // Log allowed servers configuration
        if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
            tracing::info!("Allowed servers configured: {:?}", allowed_servers);
        } else {
            tracing::info!("No allowed servers list configured - all servers will be visible");
        }

        // First check if we have any cached server info
        if !server_info_cache.is_empty() {
            tracing::info!("Using cached server information for /servers endpoint");
            // Log all servers in cache
            tracing::info!(
                "Servers in cache: {:?}",
                server_info_cache.keys().collect::<Vec<_>>()
            );

            // Use the cached info directly
            for (name, info) in server_info_cache.iter() {
                // Check if this server is in the allowed list (if we have one)
                if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
                    if !allowed_servers.contains(name) {
                        tracing::info!(server = %name, "Server '{}' not in allowed list, excluding from response", name);
                        continue;
                    }
                    tracing::info!(server = %name, "Server '{}' is in allowed list, including in response", name);
                }
                servers.push(info.clone());
            }
        } else {
            tracing::info!("No cached server info available, using config-only information");
            // Get all server names from config
            let config_servers = (proxy.runner_access.get_server_config_keys)();
            tracing::info!("Servers in config: {:?}", config_servers);

            // Fall back to the original behavior if no cache is available
            // Collect information about all servers in the config
            for name in config_servers {
                // Check if this server is in the allowed list (if we have one)
                if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
                    if !allowed_servers.contains(&name) {
                        tracing::info!(server = %name, "Server '{}' not in allowed list, excluding from response", name);
                        continue;
                    }
                    tracing::info!(server = %name, "Server '{}' is in allowed list, including in response", name);
                }

                // Try to get server information
                let server_info = match (proxy.runner_access.get_server_id)(&name) {
                    Ok(id) => ServerInfo {
                        name: name.clone(),
                        id: format!("{:?}", id),
                        status: "Running".to_string(),
                    },
                    Err(_) => {
                        // Server not started yet
                        ServerInfo {
                            name: name.clone(),
                            id: "not_started".to_string(),
                            status: "Stopped".to_string(),
                        }
                    }
                };

                servers.push(server_info);
            }
        }

        tracing::info!(
            "Returning list of {} servers: {:?}",
            servers.len(),
            servers.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        // Convert to JSON
        let json = serde_json::to_string(&servers)
            .map_err(|e| Error::Serialization(format!("Failed to serialize server list: {}", e)))?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to list tools for a specific server
    ///
    /// Returns a JSON array of tool information for the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to list tools for
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_list_tools(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, "Handling list tools request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.runner_access.get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.runner_access.get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // List tools
        let tools = match client.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(server = %server_name, error = %e, "Failed to list tools");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to list tools: {}", e),
                )
                .await;
            }
        };

        // Convert to response format
        let tool_infos: Vec<ToolInfo> = tools
            .into_iter()
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                // Handle Option for parameters_schema, defaulting to json!(null)
                parameters_schema: t.input_schema.unwrap_or(serde_json::Value::Null),
            })
            .collect();

        // Convert to JSON
        let json = serde_json::to_string(&tool_infos)
            .map_err(|e| Error::Serialization(format!("Failed to serialize tool list: {}", e)))?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to list resources for a specific server
    ///
    /// Returns a JSON array of resource information for the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to list resources for
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_list_resources(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, "Handling list resources request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.runner_access.get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.runner_access.get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // List resources
        let resources = match client.list_resources().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(server = %server_name, error = %e, "Failed to list resources");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to list resources: {}", e),
                )
                .await;
            }
        };

        // Convert to response format
        let resource_infos: Vec<ResourceInfo> = resources
            .into_iter()
            .map(|r| ResourceInfo {
                name: r.name,
                uri: r.uri,
                // Handle Option for description, defaulting to empty string
                description: r.description.unwrap_or_default(),
            })
            .collect();

        // Convert to JSON
        let json = serde_json::to_string(&resource_infos).map_err(|e| {
            Error::Serialization(format!("Failed to serialize resource list: {}", e))
        })?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to get a specific resource
    ///
    /// Retrieves and returns a specific resource from the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to get resource from
    /// * `resource_uri` - URI of the resource to retrieve
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn handle_get_resource(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        resource_uri: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, uri = %resource_uri, "Handling get resource request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.runner_access.get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.runner_access.get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.runner_access.get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // Get resource (explicitly use Value as the return type)
        let resource_data: serde_json::Value = match client.get_resource(resource_uri).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(server = %server_name, uri = %resource_uri, error = %e, "Failed to get resource");
                // Use 404 for resource not found specifically
                return HttpResponse::send_error_response(
                    writer,
                    404,
                    &format!("Resource not found or error getting resource: {}", e),
                )
                .await;
            }
        };

        // Convert to JSON
        let json = serde_json::to_string(&resource_data).map_err(|e| {
            Error::Serialization(format!("Failed to serialize resource data: {}", e))
        })?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Process a tool call request from a client
    ///
    /// Asynchronously calls a tool on the specified server and sends the result
    /// via SSE when it completes. This method handles authentication, parameter validation,
    /// and error handling.
    ///
    /// # Arguments
    ///
    /// * `server_name` - Name of the server to call the tool on
    /// * `tool_name` - Name of the tool to call
    /// * `args` - Arguments to pass to the tool
    /// * `request_id` - Unique identifier for the request
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    async fn process_tool_call(
        &self,
        server_name: &str,
        tool_name: &str,
        args: serde_json::Value,
        request_id: &str,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, tool = %tool_name, req_id = %request_id, "Processing tool call");

        // Check if this server is allowed
        if let Some(allowed_servers) = (self.runner_access.get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");

                // Send error event via shared event_manager
                self.event_manager.send_tool_error(
                    request_id,
                    "unknown", // Server ID is unknown if name isn't allowed/found
                    tool_name,
                    &format!("Server not in allowed list: {}", server_name),
                );

                return Err(Error::Unauthorized(
                    "Server not in allowed list".to_string(),
                ));
            }
        }

        // Get server ID
        let server_id = match (self.runner_access.get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");

                // Send error event via shared event_manager
                self.event_manager.send_tool_error(
                    request_id,
                    "unknown", // Server ID is unknown
                    tool_name,
                    &format!("Server not found: {}", server_name),
                );

                return Err(e);
            }
        };
        let server_id_str = format!("{:?}", server_id); // Format server_id once

        // Get a client
        let client = match (self.runner_access.get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");

                // Send error event via shared event_manager
                self.event_manager.send_tool_error(
                    request_id,
                    &server_id_str,
                    tool_name,
                    &format!("Failed to get client: {}", e),
                );

                return Err(e);
            }
        };

        // Call the tool
        let result = client.call_tool(tool_name, &args).await;

        match result {
            Ok(response) => {
                tracing::debug!(req_id = %request_id, "Tool call successful");

                // Send response event via shared event_manager
                self.event_manager.send_tool_response(
                    request_id,
                    &server_id_str,
                    tool_name,
                    response,
                );

                Ok(())
            }
            Err(e) => {
                tracing::error!(req_id = %request_id, error = %e, "Tool call failed");

                // Send error event via shared event_manager
                self.event_manager.send_tool_error(
                    request_id,
                    &server_id_str,
                    tool_name,
                    &format!("Tool call failed: {}", e),
                );

                Err(e)
            }
        }
    }

    /// Send a server status update to all connected clients
    ///
    /// Broadcasts a status update event to all connected SSE clients.
    ///
    /// # Arguments
    ///
    /// * `server_id` - ID of the server whose status changed
    /// * `server_name` - Name of the server
    /// * `status` - New status of the server
    pub fn send_status_update(&self, server_id: ServerId, server_name: &str, status: &str) {
        // Use shared event_manager
        self.event_manager
            .send_status_update(server_id, server_name, status);
    }

    /// Check if a token is valid for authentication
    ///
    /// Validates a bearer token against the configured authentication settings.
    ///
    /// # Arguments
    ///
    /// * `token` - Bearer token to validate
    ///
    /// # Returns
    ///
    /// `true` if the token is valid or if no authentication is configured, `false` otherwise
    pub fn is_valid_token(&self, token: &str) -> bool {
        if let Some(auth) = &self.config.authenticate {
            if let Some(bearer) = &auth.bearer {
                return bearer.token == token;
            }
        }

        // If no authentication is configured, any token is valid
        true
    }

    /// Get the proxy configuration
    ///
    /// Returns a reference to the SSE proxy configuration.
    pub fn config(&self) -> &SSEProxyConfig {
        &self.config
    }
}
