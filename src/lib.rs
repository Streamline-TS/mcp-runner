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
pub mod proxy;
pub mod server;
pub mod transport;

pub use client::McpClient; // Re-export McpClient as public
pub use config::Config;
pub use error::{Error, Result};
pub use proxy::SSEProxy;
pub use server::{ServerId, ServerProcess, ServerStatus};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use transport::StdioTransport;

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
    /// SSE proxy instance
    sse_proxy: Option<SSEProxy>,
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
            sse_proxy: None,
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
            tracing::warn!(num_failed = errors.len(), "Some servers failed to start");
            return Err(errors.remove(0).1);
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

            tracing::info!("Server stopped successfully");
            Ok(())
        } else {
            tracing::warn!("Attempted to stop a server that was not found or not running");
            Err(Error::ServerNotFound(format!("{:?}", id)))
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
    #[tracing::instrument(skip(self), fields(server_id = %id))]
    pub fn get_client(&mut self, id: ServerId) -> Result<McpClient> {
        tracing::info!("Getting client for server");
        let server = self.servers.get_mut(&id).ok_or_else(|| {
            tracing::error!("Client requested for unknown or stopped server");
            Error::ServerNotFound(format!("{:?}", id))
        })?;
        let server_name = server.name().to_string();
        tracing::debug!(server_name = %server_name, "Found server process");

        tracing::debug!("Taking stdin/stdout from server process");
        let stdin = server.take_stdin().map_err(|e| {
            tracing::error!(error = %e, "Failed to take stdin from server");
            e
        })?;
        let stdout = server.take_stdout().map_err(|e| {
            tracing::error!(error = %e, "Failed to take stdout from server");
            e
        })?;

        tracing::debug!("Creating StdioTransport and McpClient");
        let transport = StdioTransport::new(server_name.clone(), stdin, stdout);
        let client = McpClient::new(server_name, transport);

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

            // Parse the address from config
            let addr_str = format!("{}:{}", proxy_config.address, proxy_config.port);
            let address: SocketAddr = addr_str.parse().map_err(|e| {
                tracing::error!(error = %e, address = %addr_str, "Failed to parse SSE proxy address");
                Error::ConfigInvalid(format!("Invalid SSE proxy address {}: {}", addr_str, e))
            })?;

            tracing::info!(address = %address, "Configured SSE proxy address");

            // Create a clone of self for the proxy
            let runner_clone = Self {
                config: self.config.clone(),
                servers: HashMap::new(),
                server_names: HashMap::new(),
                sse_proxy: None,
            };

            // Create and start the proxy
            let proxy = SSEProxy::new(runner_clone, proxy_config.clone(), address);

            // Store the proxy instance
            self.sse_proxy = Some(proxy.clone());

            // Start the proxy in a background task
            let proxy_arc = std::sync::Arc::new(proxy);
            let proxy_clone = std::sync::Arc::clone(&proxy_arc);

            tokio::spawn(async move {
                if let Err(e) = (*proxy_clone).start().await {
                    tracing::error!(error = %e, "SSE proxy server failed");
                }
            });

            tracing::info!("SSE proxy server started on {}", address);
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

    /// Get the running SSE proxy instance if it exists
    ///
    /// This method provides access to the running SSE proxy instance, which can be used
    /// for more advanced operations or to get runtime information about the proxy.
    /// Note that this will only return a value if the proxy was previously started
    /// with `start_sse_proxy()` or `start_all_with_proxy()`.
    ///
    /// # Returns
    ///
    /// A `Result` containing a reference to the running `SSEProxy` instance,
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
    ///         // Access the running proxy instance
    ///         let proxy = runner.get_sse_proxy()?;
    ///         let config = proxy.config();
    ///         println!("SSE proxy is running on {}:{}", config.address, config.port);
    ///         
    ///         // Could perform additional operations with the proxy instance
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn get_sse_proxy(&self) -> Result<&SSEProxy> {
        tracing::debug!("Getting SSE proxy instance");
        self.sse_proxy.as_ref().ok_or_else(|| {
            tracing::warn!("SSE proxy instance requested but no proxy is running");
            Error::Other("SSE proxy not running".to_string())
        })
    }
}
