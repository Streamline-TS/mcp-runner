//! SSE proxy implementation for MCP servers using Actix Web.
//!
//! This module provides the core implementation of the Actix Web-based SSE proxy,
//! including the main proxy server, runner access functions, and proxy handle.

use crate::client::McpClient;
use crate::config::{DEFAULT_WORKERS, SSEProxyConfig};
use crate::error::{Error, Result};
use crate::server::ServerId;
use crate::sse_proxy::auth::Authentication;
use crate::sse_proxy::events::EventManager;
use crate::sse_proxy::handlers;
use crate::sse_proxy::types::{ServerInfo, ServerInfoUpdate};

use actix_cors::Cors;
use actix_web::{
    App, HttpServer, middleware,
    web::{self, Data},
};

use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// Provides an HTTP and SSE proxy that allows web clients to interact with MCP servers
/// using Actix Web. The proxy supports authentication, server listing, tool calls,
/// and resource retrieval.
pub struct SSEProxy {
    /// Configuration for the proxy
    config: SSEProxyConfig,
    /// Direct access to McpRunner for server operations
    runner_access: SSEProxyRunnerAccess,
    /// Event manager for broadcasting events
    event_manager: Arc<EventManager>,
    /// Server information cache
    server_info: Arc<Mutex<HashMap<String, ServerInfo>>>,
    /// Channel for receiving server updates from McpRunner
    server_rx: mpsc::Receiver<ServerInfoUpdate>,
    /// Shutdown flag
    shutdown_flag: Arc<AtomicBool>,
}

// Custom Clone implementation for SSEProxy that creates a dummy receiver when cloning
impl Clone for SSEProxy {
    fn clone(&self) -> Self {
        // Create a dummy channel just for the clone - the main instance will still use the real one
        let (_, dummy_rx) = mpsc::channel::<ServerInfoUpdate>(1);

        Self {
            config: self.config.clone(),
            runner_access: self.runner_access.clone(),
            event_manager: self.event_manager.clone(),
            server_info: self.server_info.clone(),
            server_rx: dummy_rx, // Use dummy receiver for clones
            shutdown_flag: self.shutdown_flag.clone(),
        }
    }
}

impl SSEProxy {
    /// Create a new SSE proxy instance
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
    fn new(
        runner_access: SSEProxyRunnerAccess,
        config: SSEProxyConfig,
        server_rx: mpsc::Receiver<ServerInfoUpdate>,
    ) -> Self {
        // Initialize event manager
        let event_manager = Arc::new(EventManager::new(100)); // Buffer up to 100 messages

        // Initialize server info cache
        let server_info = Arc::new(Mutex::new(HashMap::new()));

        Self {
            config,
            runner_access,
            event_manager,
            server_info,
            server_rx,
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
        let server_tx_clone = server_tx.clone();

        // Create the shutdown flag
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown_flag_clone = shutdown_flag.clone();

        // Create the proxy instance
        let mut proxy = Self::new(runner_access.clone(), config.clone(), server_rx);

        // Initialize server info with current server statuses
        // Get all server names from runner_access
        let server_names = (runner_access.get_server_config_keys)();

        // Populate server info in a scoped block so the lock is released before moving proxy
        {
            // Lock the server info for updating
            let mut server_info = proxy.server_info.lock().await;

            // Add each server to the info cache
            for name in &server_names {
                // Try to get the server ID
                if let Ok(server_id) = (runner_access.get_server_id)(name) {
                    // Convert the ID to a string for storing
                    let id_str = format!("{:?}", server_id);

                    // Create a server info entry with "Running" status
                    let info = ServerInfo {
                        name: name.clone(),
                        id: id_str.clone(),
                        status: "Running".to_string(),
                    };

                    // Add to the cache
                    server_info.insert(name.clone(), info);

                    tracing::debug!(server = %name, id = %id_str, "Added server to initial cache");
                }
            }

            tracing::info!(
                num_servers = server_info.len(),
                "Initialized server information cache with running servers"
            );
        } // Lock is released here when server_info goes out of scope

        // Parse the socket address from the config
        let addr_str = format!("{}:{}", proxy.config.address, proxy.config.port);
        let addr = match addr_str.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(addr) => addr,
                None => {
                    return Err(Error::Other(format!(
                        "Could not parse socket address: {}",
                        addr_str
                    )));
                }
            },
            Err(e) => {
                return Err(Error::Other(format!(
                    "Failed to parse socket address: {}",
                    e
                )));
            }
        };

        tracing::info!(address = %addr_str, "Starting SSE proxy server with Actix Web");

        // Share the event manager and server info via Actix Data
        let event_manager = Data::new(proxy.event_manager.clone());
        let config_arc = Arc::new(proxy.config.clone());

        // Create copies of the fields needed for the handler
        let runner_access_for_handlers = proxy.runner_access.clone();
        let server_info_for_handlers = proxy.server_info.clone();
        let event_mgr_for_handlers = proxy.event_manager.clone();
        let shutdown_flag_for_handlers = proxy.shutdown_flag.clone();

        // Create a proxy data reference for handlers to use by creating a new SSEProxy instance
        let proxy_for_handlers = SSEProxy {
            config: proxy.config.clone(),
            runner_access: runner_access_for_handlers,
            event_manager: event_mgr_for_handlers,
            server_info: server_info_for_handlers,
            // Create a dummy receiver - the real one stays with self
            server_rx: {
                let (_, rx) = mpsc::channel::<ServerInfoUpdate>(1);
                rx
            },
            shutdown_flag: shutdown_flag_for_handlers,
        };

        let proxy_data = Data::new(Arc::new(Mutex::new(proxy_for_handlers)));

        // Create the HTTP server builder
        let mut server_builder = HttpServer::new(move || {
            // Configure CORS
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);

            // Configure authentication middleware if required
            let auth_middleware = Authentication::new(config_arc.clone());

            App::new()
                .wrap(middleware::Logger::default())
                .wrap(cors)
                .app_data(event_manager.clone()) // For sse_events handler
                .app_data(proxy_data.clone()) // Pass the SSEProxy directly
                .app_data(Data::new(config_arc.clone())) // Pass config if needed by handlers
                // Apply Authentication middleware unconditionally; its internal logic handles conditions
                .wrap(auth_middleware)
                // Define routes
                .route("/sse", web::get().to(handlers::sse_main_endpoint))
                .route("/sse/messages", web::post().to(handlers::sse_messages))
        });

        // Configure workers - use the config value if specified, otherwise default to 4
        let workers = proxy.config.workers.unwrap_or(DEFAULT_WORKERS);
        tracing::info!(workers = workers, "Setting number of Actix Web workers");
        server_builder = server_builder.workers(workers);

        // Bind to the address
        let server = server_builder
            .bind(addr)
            .map_err(|e| Error::Other(format!("Failed to bind server: {}", e)))?
            .run();

        // Get the server handle for stopping later
        let server_handle = server.handle();

        // Start two tasks:
        // 1. Run the Actix server
        let server_task = tokio::spawn(server);

        // 2. Run the update processing loop
        let update_handle = tokio::spawn(async move {
            if let Err(e) = proxy.process_updates(server_handle).await {
                tracing::error!(error = %e, "SSE proxy update processor error");
            }
        });

        tracing::info!("SSE proxy server started successfully");

        // Create the handle that will be returned to the caller
        let handle = tokio::spawn(async move {
            // Wait for both tasks to complete
            let (server_result, update_result) = tokio::join!(server_task, update_handle);

            // Log any errors
            if let Err(e) = server_result {
                tracing::error!(error = %e, "Actix server task error");
            }
            if let Err(e) = update_result {
                tracing::error!(error = %e, "Update processor task error");
            }

            tracing::info!("SSE proxy server shut down completely");
        });

        // Return a handle to control the proxy
        Ok(SSEProxyHandle::new(
            server_tx_clone,
            handle,
            config,
            shutdown_flag_clone,
        ))
    }

    /// Process server information updates
    ///
    /// This is the main loop that processes server information updates from the channel.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    async fn process_updates(&mut self, server_handle: actix_web::dev::ServerHandle) -> Result<()> {
        tracing::info!("SSE proxy update processor started");

        // Main loop to process server information updates
        while !self.shutdown_flag.load(Ordering::SeqCst) {
            // Check for server updates with timeout
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                self.server_rx.recv(),
            )
            .await
            {
                Ok(Some(update)) => match update {
                    ServerInfoUpdate::UpdateServer { name, id, status } => {
                        // Update server info in cache
                        let mut servers = self.server_info.lock().await;

                        if let Some(server_info) = servers.get_mut(&name) {
                            if let Some(server_id) = id {
                                server_info.id = format!("{:?}", server_id);
                            }
                            server_info.status = status.clone();

                            // Send server status update event
                            self.event_manager
                                .send_server_status(&name, &server_info.id, &status);

                            tracing::debug!(server = %name, status = %status, "Updated server status");
                        } else {
                            // Server not in cache yet, add it with default info
                            let server_info = ServerInfo {
                                name: name.clone(),
                                id: id.map_or_else(
                                    || "unknown".to_string(),
                                    |id| format!("{:?}", id),
                                ),
                                status: status.clone(),
                            };

                            servers.insert(name.clone(), server_info.clone());

                            // Send server status update event
                            self.event_manager
                                .send_server_status(&name, &server_info.id, &status);

                            tracing::debug!(server = %name, status = %status, "Added server to cache");
                        }
                    }
                    ServerInfoUpdate::AddServer { name, info } => {
                        // Add server to cache
                        let mut servers = self.server_info.lock().await;
                        servers.insert(name.clone(), info.clone());

                        // Send server status update event
                        self.event_manager
                            .send_server_status(&name, &info.id, &info.status);

                        tracing::debug!(server = %name, "Added server to cache");
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

        // Stop the server
        tracing::info!("Stopping Actix Web server");
        server_handle.stop(true).await;

        tracing::info!("SSE proxy update processor shut down");
        Ok(())
    }

    /// Process a tool call request from a client
    ///
    /// # Arguments
    ///
    /// * `server_name` - Name of the server to call the tool on
    /// * `tool_name` - Name of the tool to call
    /// * `args` - Arguments to pass to the tool
    /// * `request_id` - Request ID for correlation
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
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

        // Initialize client
        if let Err(e) = client.initialize().await {
            tracing::error!(server_id = ?server_id, error = %e, "Failed to initialize client");

            // Send error event
            self.event_manager.send_tool_error(
                request_id,
                &server_id_str,
                tool_name,
                &format!("Failed to initialize client: {}", e),
            );

            return Err(e);
        }

        // Call the tool with explicit type annotation for serde_json::Value
        let result: Result<serde_json::Value> = client.call_tool(tool_name, &args).await;

        match result {
            Ok(response) => {
                tracing::debug!(req_id = %request_id, "Tool call successful");

                // Send the raw response to the event manager
                // The event manager will now format it properly as a JSON-RPC response
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

    /// Get the server information cache
    pub fn get_server_info(&self) -> &Arc<Mutex<HashMap<String, ServerInfo>>> {
        &self.server_info
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
}

/// Shared state for the SSE proxy
///
/// This struct provides shared state that can be used by Actix Web handlers.
/// It's wrapped in an Arc<Mutex<_>> and passed to handlers via Data.
pub struct SSEProxySharedState {
    /// Runner access functions
    runner_access: SSEProxyRunnerAccess,
    /// Event manager
    event_manager: Arc<EventManager>,
    /// Server information cache
    server_info: Arc<Mutex<HashMap<String, ServerInfo>>>,
}

impl SSEProxySharedState {
    pub fn runner_access(&self) -> &SSEProxyRunnerAccess {
        &self.runner_access
    }

    pub fn event_manager(&self) -> &Arc<EventManager> {
        &self.event_manager
    }

    pub fn server_info(&self) -> &Arc<Mutex<HashMap<String, ServerInfo>>> {
        &self.server_info
    }
}
