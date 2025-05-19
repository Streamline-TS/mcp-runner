/*!
 # MCP Runner

 A Rust library for running and interacting with Model Context Protocol (MCP) servers.

 ## Overview

 MCP Runner provides functionality to:
 - Start and manage MCP server processes
 - Communicate with MCP servers using JSON-RPC
 - List and call tools provided by MCP servers
 - Access resources exposed by MCP servers
 - Optionally proxy SSE (Server-Sent Events) to the servers for external clients

 ## Basic Usage

 ```no_run
 use mcp_runner::{McpRunner, Result};
 use serde_json::{json, Value};

 #[tokio::main]
 async fn main() -> Result<()> {
     // Create a runner from config file
     let mut runner = McpRunner::from_config_file("config.json")?;

     // Start all configured servers
     let server_ids = runner.start_all_servers().await?;

     // Or start a specific server
     let server_id = runner.start_server("fetch").await?;

     // Get a client to interact with the server
     let client = runner.get_client(server_id)?;

     // Initialize the client
     client.initialize().await?;

     // List available tools
     let tools = client.list_tools().await?;
     println!("Available tools: {:?}", tools);

     // Call a tool
     let args = json!({
         "url": "https://modelcontextprotocol.io"
     });
     let result: Value = client.call_tool("fetch", &args).await?;

     println!("Result: {:?}", result);

     Ok(())
 }
 ```

 ## Features

 - **Server Management**: Start, stop, and monitor MCP servers
 - **JSON-RPC Communication**: Communicate with MCP servers using JSON-RPC
 - **Configuration**: Configure servers through JSON config files
 - **Error Handling**: Comprehensive error handling
 - **Async Support**: Full async/await support
 - **SSE Proxy**: Support for SSE proxying with authentication and CORS

 ## License

 This project is licensed under the terms in the LICENSE file.
*/

pub mod client;
pub mod config;
pub mod error;
pub mod server;
pub mod sse_proxy;
pub mod transport;

pub use client::McpClient;
pub use config::Config;
pub use error::{Error, Result};
pub use server::{ServerId, ServerProcess, ServerStatus};
pub use sse_proxy::SSEProxyHandle;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use transport::StdioTransport;

use sse_proxy::types::ServerInfo;
use sse_proxy::{SSEProxy, SSEProxyRunnerAccess};

/// Configure and run MCP servers
///
/// This struct is the main entry point for managing MCP server lifecycles
/// and obtaining clients to interact with them.
/// All public methods are instrumented with `tracing` spans.
pub struct McpRunner {
    /// Configuration
    config: Config,
    /// Running server processes
    servers: HashMap<ServerId, ServerProcess>,
    /// Map of server names to server IDs
    server_names: HashMap<String, ServerId>,
    /// SSE proxy handle (if running)
    sse_proxy_handle: Option<SSEProxyHandle>,
    /// Cached clients for servers
    clients: HashMap<ServerId, Option<McpClient>>,
}

impl McpRunner {
    /// Create a new MCP runner from a configuration file path
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(path), fields(config_path = ?path.as_ref()))]
    pub fn from_config_file(path: impl AsRef<Path>) -> Result<Self> {
        tracing::info!("Loading configuration from file");
        let config = Config::from_file(path)?;
        Ok(Self::new(config))
    }

    /// Create a new MCP runner from a configuration string
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(config))]
    pub fn from_config_str(config: &str) -> Result<Self> {
        tracing::info!("Loading configuration from string");
        let config = Config::parse_from_str(config)?;
        Ok(Self::new(config))
    }

    /// Create a new MCP runner from a configuration
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(config), fields(num_servers = config.mcp_servers.len()))]
    pub fn new(config: Config) -> Self {
        tracing::info!("Creating new McpRunner");
        Self {
            config,
            servers: HashMap::new(),
            server_names: HashMap::new(),
            sse_proxy_handle: None,
            clients: HashMap::new(),
        }
    }

    /// Start a specific MCP server
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %name))]
    pub async fn start_server(&mut self, name: &str) -> Result<ServerId> {
        // Check if server is already running
        if let Some(id) = self.server_names.get(name) {
            tracing::debug!(server_id = %id, "Server already running");
            return Ok(*id);
        }

        tracing::info!("Attempting to start server");
        // Get server configuration
        let config = self
            .config
            .mcp_servers
            .get(name)
            .ok_or_else(|| {
                tracing::error!("Configuration not found for server");
                Error::ServerNotFound(name.to_string())
            })?
            .clone();

        // Create and start server process
        let mut server = ServerProcess::new(name.to_string(), config);
        let id = server.id();
        tracing::debug!(server_id = %id, "Created ServerProcess instance");

        server.start().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to start server process");
            e
        })?;

        // Store server
        tracing::debug!(server_id = %id, "Storing running server process");
        self.servers.insert(id, server);
        self.server_names.insert(name.to_string(), id);

        // Notify SSE proxy about the new server if it's running
        if let Some(proxy) = &self.sse_proxy_handle {
            let status = format!("{:?}", ServerStatus::Running);
            if let Err(e) = proxy.update_server_info(name, Some(id), &status).await {
                tracing::warn!(
                    error = %e,
                    server = %name,
                    "Failed to update server info in SSE proxy"
                );

                // If the server wasn't in the proxy cache yet, try to add it
                let server_info = ServerInfo {
                    name: name.to_string(),
                    id: format!("{:?}", id),
                    status: status.clone(),
                };

                // Try to add the server information to the proxy
                if let Err(e) = proxy.add_server_info(name, server_info.clone()).await {
                    tracing::warn!(
                        error = %e,
                        server = %name,
                        "Failed to add server to SSE proxy cache"
                    );
                } else {
                    tracing::debug!(server = %name, "Added new server to SSE proxy cache");
                }
            } else {
                tracing::debug!(server = %name, "Updated SSE proxy with new server information");
            }
        }

        tracing::info!(server_id = %id, "Server started successfully");
        Ok(id)
    }

    /// Start all configured servers
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub async fn start_all_servers(&mut self) -> Result<Vec<ServerId>> {
        tracing::info!("Starting all configured servers");
        // Collect server names first to avoid borrowing issues
        let server_names: Vec<String> = self
            .config
            .mcp_servers
            .keys()
            .map(|k| k.to_string())
            .collect();
        tracing::debug!(servers_to_start = ?server_names);

        // Start servers sequentially
        let mut ids = Vec::new();
        let mut errors = Vec::new();

        for name in server_names {
            match self.start_server(&name).await {
                Ok(id) => ids.push(id),
                Err(e) => {
                    tracing::error!(server_name = %name, error = %e, "Failed to start server");
                    errors.push((name, e));
                }
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                num_failed = errors.len(),
                "Some servers failed to start: {:?}",
                errors
                    .iter()
                    .map(|(name, _): &(String, Error)| name.as_str())
                    .collect::<Vec<_>>()
            );
            // Create an aggregate error message including all failures
            if errors.len() == 1 {
                return Err(errors.remove(0).1);
            } else {
                let error_msg = errors
                    .iter()
                    .map(|(name, e)| format!("{}: {}", name, e))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(Error::Other(format!(
                    "Multiple servers failed to start: {}",
                    error_msg
                )));
            }
        }

        tracing::info!(num_started = ids.len(), "Finished starting all servers");
        Ok(ids)
    }

    /// Start all configured servers and the SSE proxy if configured
    ///
    /// This is a convenience method that starts all configured MCP servers
    /// and then starts the SSE proxy if it's configured. This ensures that
    /// all servers are available before the proxy begins accepting connections.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - A `Result<Vec<ServerId>>` with server IDs for all started servers, or an error
    /// - A `bool` indicating whether the SSE proxy was started
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     // Create runner from config
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///
    ///     // Start all servers and proxy if configured
    ///     let (server_ids, proxy_started) = runner.start_all_with_proxy().await;
    ///
    ///     // Check if servers started successfully
    ///     let server_ids = server_ids?;
    ///     println!("Started {} servers", server_ids.len());
    ///
    ///     if proxy_started {
    ///         println!("SSE proxy started successfully");
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub async fn start_all_with_proxy(&mut self) -> (Result<Vec<ServerId>>, bool) {
        // First start all servers
        let server_result = self.start_all_servers().await;

        // Only attempt to start proxy if servers started successfully
        let proxy_started = if server_result.is_ok() && self.is_sse_proxy_configured() {
            match self.start_sse_proxy().await {
                Ok(_) => {
                    tracing::info!("SSE proxy started automatically");
                    true
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to start SSE proxy");
                    false
                }
            }
        } else {
            if self.is_sse_proxy_configured() {
                tracing::warn!("Not starting SSE proxy because servers failed to start");
            }
            false
        };

        (server_result, proxy_started)
    }

    /// Stop a running server
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_id = %id))]
    pub async fn stop_server(&mut self, id: ServerId) -> Result<()> {
        tracing::info!("Attempting to stop server");
        if let Some(mut server) = self.servers.remove(&id) {
            let name = server.name().to_string();
            tracing::debug!(server_name = %name, "Found server process to stop");
            self.server_names.remove(&name);

            server.stop().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to stop server process");
                e
            })?;

            // Notify SSE proxy about the server being stopped
            if let Some(proxy) = &self.sse_proxy_handle {
                if let Err(e) = proxy.update_server_info(&name, None, "Stopped").await {
                    tracing::warn!(
                        error = %e,
                        server = %name,
                        "Failed to update SSE proxy with server stopped status"
                    );
                } else {
                    tracing::debug!(server = %name, "Updated SSE proxy with server stopped status");
                }
            }

            tracing::info!("Server stopped successfully");
            Ok(())
        } else {
            tracing::warn!("Attempted to stop a server that was not found or not running");
            Err(Error::ServerNotFound(format!("{:?}", id)))
        }
    }

    /// Stop all running servers and the SSE proxy if it's running
    ///
    /// This method stops all running servers and the SSE proxy (if running).
    /// It collects all errors but only returns the first one encountered.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or the first error encountered.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///     runner.start_all_with_proxy().await;
    ///
    ///     // Later, stop everything
    ///     runner.stop_all_servers().await?;
    ///     println!("All servers and proxy stopped");
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub async fn stop_all_servers(&mut self) -> Result<()> {
        tracing::info!("Stopping all servers and proxy if running");

        // Collect all server IDs first to avoid borrowing issues
        let server_ids: Vec<ServerId> = self.servers.keys().copied().collect();

        // Stop the SSE proxy first if it's running
        if let Some(proxy_handle) = self.sse_proxy_handle.take() {
            tracing::info!("Stopping SSE proxy");
            if let Err(e) = proxy_handle.shutdown().await {
                tracing::warn!(error = %e, "Error shutting down SSE proxy");
                // We continue anyway since we're in the process of clean-up
            }
            tracing::info!("SSE proxy stopped");
        }

        // Stop servers sequentially but with improved error handling
        let mut errors = Vec::new();

        for id in server_ids {
            match self.stop_server(id).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(server_id = ?id, error = %e, "Failed to stop server");
                    errors.push((id, e));
                }
            }
        }

        if errors.is_empty() {
            tracing::info!("All servers stopped successfully");
            Ok(())
        } else {
            tracing::warn!(error_count = errors.len(), "Some servers failed to stop");
            // Create an aggregate error message including all failures
            if errors.len() == 1 {
                return Err(errors.remove(0).1);
            } else {
                let error_msg = errors
                    .iter()
                    .map(|(id, e)| format!("{:?}: {}", id, e))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(Error::Other(format!(
                    "Multiple servers failed to stop: {}",
                    error_msg
                )));
            }
        }
    }

    /// Get server status
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_id = %id))]
    pub fn server_status(&self, id: ServerId) -> Result<ServerStatus> {
        tracing::debug!("Getting server status");
        self.servers
            .get(&id)
            .map(|server| {
                let status = server.status();
                tracing::trace!(status = ?status);
                status
            })
            .ok_or_else(|| {
                tracing::warn!("Status requested for unknown server");
                Error::ServerNotFound(format!("{:?}", id))
            })
    }

    /// Get server ID by name
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %name))]
    pub fn get_server_id(&self, name: &str) -> Result<ServerId> {
        tracing::debug!("Getting server ID by name");
        self.server_names.get(name).copied().ok_or_else(|| {
            tracing::warn!("Server ID requested for unknown server name");
            Error::ServerNotFound(name.to_string())
        })
    }

    /// Get a client for a server
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// If the client already exists in cache, a `ClientAlreadyCached` error is returned.
    /// In this case, you can retrieve the cached client using specialized methods like
    /// `get_server_tools` that handle the cache internally.
    #[tracing::instrument(skip(self), fields(server_id = %id))]
    pub fn get_client(&mut self, id: ServerId) -> Result<McpClient> {
        tracing::info!("Getting client for server");

        // First check if we already have a client for this server
        if let Some(Some(_client)) = self.clients.get(&id) {
            tracing::debug!("Client already exists in cache");
            // Return a specific error type for this case
            return Err(Error::ClientAlreadyCached);
        }

        // Check if we've already tried to get a client but it failed
        if let Some(None) = self.clients.get(&id) {
            tracing::warn!("Previously failed to create client for this server");
            return Err(Error::ServerNotFound(format!(
                "{:?} (client creation previously failed)",
                id
            )));
        }

        // Create a new client
        let server = self.servers.get_mut(&id).ok_or_else(|| {
            tracing::error!("Client requested for unknown or stopped server");
            Error::ServerNotFound(format!("{:?}", id))
        })?;
        let server_name = server.name().to_string();
        tracing::debug!(server_name = %server_name, "Found server process");

        tracing::debug!("Taking stdin/stdout from server process");
        let stdin = match server.take_stdin() {
            Ok(stdin) => stdin,
            Err(e) => {
                tracing::error!(error = %e, "Failed to take stdin from server");
                // Mark this server as failed in our clients cache
                self.clients.insert(id, None);
                return Err(e);
            }
        };

        let stdout = match server.take_stdout() {
            Ok(stdout) => stdout,
            Err(e) => {
                tracing::error!(error = %e, "Failed to take stdout from server");
                // Mark this server as failed in our clients cache
                self.clients.insert(id, None);
                return Err(e);
            }
        };

        tracing::debug!("Creating StdioTransport and McpClient");
        let transport = StdioTransport::new(server_name.clone(), stdin, stdout);
        let client = McpClient::new(server_name, transport);

        // Store the client in our cache
        self.clients.insert(id, Some(client.clone()));

        tracing::info!("Client created successfully");
        Ok(client)
    }

    /// Start the SSE proxy server if enabled in configuration
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub async fn start_sse_proxy(&mut self) -> Result<()> {
        if let Some(proxy_config) = &self.config.sse_proxy {
            tracing::info!("Initializing SSE proxy server");

            // Create the runner access functions
            let runner_access = SSEProxyRunnerAccess {
                get_server_id: Arc::new({
                    let self_clone = self.clone(); // Clone self to move into the closure
                    move |name: &str| self_clone.get_server_id(name)
                }),
                get_client: Arc::new({
                    let self_clone = self.clone(); // Clone self to move into the closure
                    move |id: ServerId| {
                        // We need a mutable reference to self, which we can't have in a closure
                        // Create a new client using the same logic as get_client, but without caching
                        let servers = &self_clone.servers;
                        if let Some(server) = servers.get(&id) {
                            // We can't actually take stdin/stdout from the server in this closure because we only
                            // have a shared reference. Instead, we'll create a new client to talk to the existing server.
                            // This is inefficient but necessary for the proxy's design.
                            let server_name = server.name().to_string();
                            match McpClient::connect(&server_name, &self_clone.config) {
                                Ok(client) => Ok(client),
                                Err(e) => {
                                    tracing::error!(error = %e, server_id = ?id, "Failed to create client for SSE proxy");
                                    Err(e)
                                }
                            }
                        } else {
                            Err(Error::ServerNotFound(format!("{:?}", id)))
                        }
                    }
                }),
                get_allowed_servers: Arc::new({
                    let config = self.config.clone(); // Clone config to move into the closure
                    move || {
                        // Extract the allowed_servers from the sse_proxy config if present
                        config
                            .sse_proxy
                            .as_ref()
                            .and_then(|proxy_config| proxy_config.allowed_servers.clone())
                    }
                }),
                get_server_config_keys: Arc::new({
                    let config = self.config.clone(); // Clone config to move into the closure
                    move || {
                        // Return all server names from the config
                        config.mcp_servers.keys().cloned().collect()
                    }
                }),
            };

            // Convert the config reference to an owned value
            let proxy_config_owned = proxy_config.clone();

            // Start the proxy with the runner access functions
            let proxy_handle = SSEProxy::start_proxy(runner_access, proxy_config_owned).await?;

            // Store the proxy handle
            self.sse_proxy_handle = Some(proxy_handle);

            tracing::info!(
                "SSE proxy server started on {}:{}",
                proxy_config.address,
                proxy_config.port
            );
            Ok(())
        } else {
            tracing::warn!("SSE proxy not configured, skipping start");
            Err(Error::Other(
                "SSE proxy not configured in config".to_string(),
            ))
        }
    }

    /// Check if the SSE proxy is enabled in configuration
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn is_sse_proxy_configured(&self) -> bool {
        self.config.sse_proxy.is_some()
    }

    /// Get the SSE proxy configuration if it exists
    ///
    /// Retrieves a reference to the SSE proxy configuration from the runner's config.
    /// This is useful for accessing proxy settings like address and port.
    ///
    /// # Returns
    ///
    /// A `Result` containing a reference to the `SSEProxyConfig` if configured,
    /// or an `Error::Other` if no SSE proxy is configured.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let runner = McpRunner::from_config_file("config.json")?;
    ///
    ///     if runner.is_sse_proxy_configured() {
    ///         let proxy_config = runner.get_sse_proxy_config()?;
    ///         println!("SSE proxy will listen on {}:{}", proxy_config.address, proxy_config.port);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn get_sse_proxy_config(&self) -> Result<&config::SSEProxyConfig> {
        tracing::debug!("Getting SSE proxy configuration");
        self.config.sse_proxy.as_ref().ok_or_else(|| {
            tracing::warn!("SSE proxy configuration requested but not configured");
            Error::Other("SSE proxy not configured".to_string())
        })
    }

    /// Get the running SSE proxy handle if it exists
    ///
    /// This method provides access to the running SSE proxy handle, which can be used
    /// to communicate with the proxy or control it.
    /// Note that this will only return a value if the proxy was previously started
    /// with `start_sse_proxy()` or `start_all_with_proxy()`.
    ///
    /// # Returns
    ///
    /// A `Result` containing a reference to the running `SSEProxyHandle` instance,
    /// or an `Error::Other` if no SSE proxy is running.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///
    ///     // Start servers and proxy
    ///     let (server_ids, proxy_started) = runner.start_all_with_proxy().await;
    ///     let _server_ids = server_ids?;
    ///
    ///     if proxy_started {
    ///         // Access the running proxy handle
    ///         let proxy_handle = runner.get_sse_proxy_handle()?;
    ///         let config = proxy_handle.config();
    ///         println!("SSE proxy is running on {}:{}", config.address, config.port);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn get_sse_proxy_handle(&self) -> Result<&SSEProxyHandle> {
        tracing::debug!("Getting SSE proxy handle");
        self.sse_proxy_handle.as_ref().ok_or_else(|| {
            tracing::warn!("SSE proxy handle requested but no proxy is running");
            Error::Other("SSE proxy not running".to_string())
        })
    }

    /// Get status for all running servers
    ///
    /// This method returns a HashMap of server names to their current status.
    /// This is a convenience method that can be called at any time to check on all servers.
    ///
    /// # Returns
    ///
    /// A `HashMap<String, ServerStatus>` containing the status of all currently running servers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///     runner.start_all_servers().await?;
    ///
    ///     // Check status of all servers
    ///     let statuses = runner.get_all_server_statuses();
    ///     for (name, status) in statuses {
    ///         println!("Server '{}' status: {:?}", name, status);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn get_all_server_statuses(&self) -> HashMap<String, ServerStatus> {
        tracing::debug!("Getting status for all running servers");
        let mut statuses = HashMap::new();

        for (server_name, server_id) in &self.server_names {
            if let Some(server) = self.servers.get(server_id) {
                let status = server.status();
                statuses.insert(server_name.clone(), status);
                tracing::trace!(server = %server_name, status = ?status);
            }
        }

        tracing::debug!(num_servers = statuses.len(), "Collected server statuses");
        statuses
    }

    /// Get a list of available tools for a specific server
    ///
    /// This is a convenience method that creates a temporary client to query tools from a server.
    /// Unlike `get_client().list_tools()`, this method handles all the client creation and cleanup internally.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of tools provided by the specified server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///     runner.start_server("fetch").await?;
    ///
    ///     // Get tools for a specific server
    ///     let tools = runner.get_server_tools("fetch").await?;
    ///     for tool in tools {
    ///         println!("Tool: {} - {}", tool.name, tool.description);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %name))]
    pub async fn get_server_tools(&mut self, name: &str) -> Result<Vec<client::Tool>> {
        tracing::info!("Getting tools for server '{}'", name);

        // Get server ID
        let server_id = self.get_server_id(name)?;

        // Check if we already have a client for this server
        let client_from_cache = if let Some(Some(_client)) = self.clients.get(&server_id) {
            tracing::debug!("Using cached client");
            // Return a specific error type for this case
            true
        } else {
            false
        };

        // Get or create client
        let result: Result<Vec<client::Tool>> = if client_from_cache {
            // Use cached client
            let client = self.clients.get(&server_id).unwrap().as_ref().unwrap();

            // Initialize the client
            client.initialize().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to initialize client");
                e
            })?;

            // List tools
            client.list_tools().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to list tools for server");
                e
            })
        } else {
            // Create a new client
            match self.get_client(server_id) {
                Ok(client) => {
                    // Initialize the client
                    client.initialize().await.map_err(|e| {
                        tracing::error!(error = %e, "Failed to initialize client");
                        e
                    })?;

                    // List tools
                    client.list_tools().await.map_err(|e| {
                        tracing::error!(error = %e, "Failed to list tools for server");
                        e
                    })
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get client");
                    Err(e)
                }
            }
        };

        match &result {
            Ok(tools) => {
                let tools_len = tools.len();
                tracing::info!(server = %name, num_tools = tools_len, "Successfully retrieved tools");
            }
            Err(e) => {
                tracing::error!(server = %name, error = %e, "Failed to get tools");
            }
        }

        result
    }

    /// Get tools for all running servers
    ///
    /// This method returns a HashMap of server names to their available tools.
    /// This is a convenience method that can be called at any time to check tools for all running servers.
    ///
    /// # Returns
    ///
    /// A `HashMap<String, Result<Vec<Tool>>>` containing the tools of all currently running servers.
    /// The Result indicates whether listing tools was successful for each server.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_runner::McpRunner;
    ///
    /// #[tokio::main]
    /// async fn main() -> mcp_runner::Result<()> {
    ///     let mut runner = McpRunner::from_config_file("config.json")?;
    ///     runner.start_all_servers().await?;
    ///
    ///     // Get tools for all servers
    ///     let all_tools = runner.get_all_server_tools().await;
    ///     for (server_name, tools_result) in all_tools {
    ///         match tools_result {
    ///             Ok(tools) => {
    ///                 println!("Server '{}' tools:", server_name);
    ///                 for tool in tools {
    ///                     println!(" - {}: {}", tool.name, tool.description);
    ///                 }
    ///             },
    ///             Err(e) => println!("Failed to get tools for '{}': {}", server_name, e),
    ///         }
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub async fn get_all_server_tools(&mut self) -> HashMap<String, Result<Vec<client::Tool>>> {
        tracing::debug!("Getting tools for all running servers");
        let mut all_tools = HashMap::new();

        // Need to collect keys to avoid borrowing issues
        let server_names: Vec<String> = self.server_names.keys().cloned().collect();

        // Process each server sequentially with timeout protection
        for name in server_names {
            tracing::debug!(server = %name, "Getting tools");
            // For each server, get its tools with a timeout to prevent hanging
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                self.get_server_tools(&name),
            )
            .await;

            // Process the result, handling timeout case separately
            let final_result = match result {
                Ok(inner_result) => inner_result,
                Err(_) => {
                    tracing::warn!(server = %name, "Timed out getting tools");
                    Err(Error::Timeout(format!(
                        "Tool listing for server '{}' timed out",
                        name
                    )))
                }
            };

            // Store the result (success or error) in the map
            all_tools.insert(name, final_result);
        }

        tracing::debug!(
            num_servers = all_tools.len(),
            "Collected tools for all servers"
        );
        all_tools
    }

    /// Create a snapshot of current server information
    ///
    /// This creates a HashMap of server names to their ServerInfo which can be used
    /// by the SSE proxy to report accurate server status information.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    fn get_server_info_snapshot(&self) -> HashMap<String, ServerInfo> {
        tracing::debug!("Creating server information snapshot for SSE proxy");
        let mut server_info = HashMap::new();

        for (name, id) in &self.server_names {
            if let Some(server) = self.servers.get(id) {
                let status = server.status();
                server_info.insert(
                    name.clone(),
                    ServerInfo {
                        name: name.clone(),
                        id: format!("{:?}", id),
                        status: format!("{:?}", status),
                    },
                );
                tracing::trace!(server = %name, status = ?status, "Added server to snapshot");
            }
        }

        server_info
    }
}

impl Clone for McpRunner {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            servers: self.servers.clone(),
            server_names: self.server_names.clone(),
            sse_proxy_handle: self.sse_proxy_handle.clone(),
            clients: HashMap::new(), // We don't clone clients as they can't be cleanly cloned
        }
    }
}
