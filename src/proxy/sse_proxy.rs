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

use crate::proxy::connection_handler::ConnectionHandler;
use crate::proxy::events::EventManager;
use crate::proxy::server_manager::ServerManager;
use crate::proxy::types::ServerInfo;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::net::TcpListener;
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
    /// Event manager for broadcasting events
    event_manager: Arc<EventManager>,
    /// Server information manager
    server_manager: Arc<ServerManager>,
    /// Server address
    address: SocketAddr,
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

        // Initialize event manager
        let event_manager = Arc::new(EventManager::new(100)); // Buffer up to 100 messages

        // Initialize server manager with event manager
        let server_manager = Arc::new(ServerManager::new(event_manager.clone()));

        tracing::debug!("Initialized SSE proxy with direct runner access");

        Self {
            config,
            runner_access,
            event_manager,
            server_manager,
            address,
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
                            if let Err(e) = ConnectionHandler::handle_connection(
                                stream,
                                addr,
                                inner_proxy_clone,
                            )
                            .await
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
                        self.server_manager
                            .update_server_info(&name, id, &status)
                            .await;
                    }
                    ServerInfoUpdate::AddServer { name, info } => {
                        let runner_access_clone = self.runner_access.clone();
                        self.server_manager
                            .add_server_info(&name, info, move |server_name| {
                                (runner_access_clone.get_server_id)(server_name)
                            })
                            .await;
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

    /// Get the server information cache
    pub fn get_server_info(&self) -> &Arc<Mutex<HashMap<String, ServerInfo>>> {
        self.server_manager.server_info()
    }

    /// Get the runner access functions
    pub fn get_runner_access(&self) -> &SSEProxyRunnerAccess {
        &self.runner_access
    }

    /// Get the event manager
    pub fn event_manager(&self) -> &Arc<EventManager> {
        &self.event_manager
    }

    /// Get the configuration
    pub fn config(&self) -> &SSEProxyConfig {
        &self.config
    }

    /// Check if a token is valid for authentication
    pub fn is_valid_token(&self, token: &str) -> bool {
        if let Some(auth) = &self.config.authenticate {
            if let Some(bearer) = &auth.bearer {
                return bearer.token == token;
            }
        }
        true // If no authentication is configured, any token is valid
    }

    /// Process a tool call request from a client
    pub async fn process_tool_call(
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

                // Send error event
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

                // Send error event
                self.event_manager.send_tool_error(
                    request_id,
                    "unknown", // Server ID is unknown
                    tool_name,
                    &format!("Server not found: {}", server_name),
                );

                return Err(e);
            }
        };
        let server_id_str = format!("{:?}", server_id);

        // Get a client
        let client = match (self.runner_access.get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");

                // Send error event
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

                // Send response event
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

                // Send error event
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
}
