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
//! use mcp_runner::{McpRunner, error::Result};
//!
//! # async fn run() -> Result<()> {
//! // Create a runner from a config file that includes SSE proxy settings
//! let mut runner = McpRunner::from_config_file("config.json")?;
//!
//! // Check if SSE proxy is configured in the config file
//! if runner.is_sse_proxy_configured() {
//!     // Start the servers first (optional)
//!     runner.start_all_servers().await?;
//!
//!     // Start the SSE proxy - this automatically sets up all necessary components
//!     runner.start_sse_proxy().await?;
//!
//!     // The proxy is now running and will serve HTTP/SSE requests
//!     
//!     // You can get the proxy handle if needed for shutdown later
//!     let proxy_handle = runner.get_sse_proxy_handle()?;
//!
//!     // Later when you want to shut down the proxy:
//!     // proxy_handle.shutdown().await?;
//! }
//! # Ok(())
//! # }
//! ```

// Re-exports
pub use self::sse_proxy::SSEProxy;
pub use self::types::{ResourceInfo, SSEEvent, SSEMessage, ServerInfo, ToolInfo};

// Submodules
pub mod connection_handler;
pub mod error_helpers;
pub mod events;
pub mod http;
pub mod http_handlers;
pub mod server_manager;
pub mod sse_proxy;
pub mod types;
