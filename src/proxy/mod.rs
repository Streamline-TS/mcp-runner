//! Proxy module for handling HTTP and SSE communication with MCP servers.
//!
//! This module provides an HTTP and SSE (Server-Sent Events) proxy that allows web clients
//! to interact with MCP servers through a REST-like API. The proxy supports:
//!
//! - Server-Sent Events (SSE) for streaming updates from MCP servers
//! - JSON-RPC requests for tool calls and initialization
//! - Authentication via bearer tokens
//! - Cross-Origin Resource Sharing (CORS)
//! - Resource retrieval from MCP servers
//!
//! # Examples
//!
//! ```no_run
//! use mcp_runner::{McpRunner, config::SSEProxyConfig, error::Result};
//! use mcp_runner::proxy::{SSEProxy, sse_proxy::SSEProxyRunnerAccess};
//! use std::sync::{Arc, Mutex};
//!
//! # async fn run() -> Result<()> {
//! // Create a runner
//! let runner = McpRunner::from_config_file("config.json")?;
//!
//! // If the config file does not include "sseProxy", you can create it manually
//! let proxy_config = SSEProxyConfig {
//!     authenticate: None,
//!     allowed_servers: None,
//!     address: "127.0.0.1".to_string(),
//!     port: 8080,
//! };
//!
//! // Use Arc<Mutex<>> to allow shared mutable access to runner
//! let runner_mutex = Arc::new(Mutex::new(runner));
//! let runner_clone1 = Arc::clone(&runner_mutex);
//! let runner_clone2 = Arc::clone(&runner_mutex);
//! let allowed_servers = proxy_config.allowed_servers.clone();
//!
//! // Create the access functions for SSEProxy
//! let runner_access = SSEProxyRunnerAccess {
//!     get_server_id: Arc::new(move |name| {
//!         runner_clone1.lock().unwrap().get_server_id(name)
//!     }),
//!     get_client: Arc::new(move |id| {
//!         runner_clone2.lock().unwrap().get_client(id)
//!     }),
//!     get_allowed_servers: Arc::new(move || allowed_servers.clone()),
//!     get_server_config_keys: Arc::new(|| {
//!         // In a real application, you would use appropriate methods to get server names
//!         vec!["server1".to_string(), "server2".to_string()]
//!     }),
//! };
//!
//! // Start the SSE proxy
//! let proxy_handle = SSEProxy::start_proxy(runner_access, proxy_config).await?;
//!
//! // Later when you want to shut down the proxy:
//! // proxy_handle.shutdown().await?;
//! # Ok(())
//! # }
//! ```

// Re-exports
pub use self::sse_proxy::SSEProxy;
pub use self::types::{ResourceInfo, SSEEvent, SSEMessage, ServerInfo, ToolInfo};

// Submodules
pub mod events;
pub mod http;
pub mod sse_proxy;
pub mod types;
