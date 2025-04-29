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
//! use mcp_runner::{McpRunner, config::SSEProxyConfig};
//! use mcp_runner::proxy::SSEProxy;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
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
//! // Create and start the SSE proxy
//! // The address is automatically derived from the proxy_config
//! // Pass None for server_info to have the proxy discover servers on-demand
//! let proxy = SSEProxy::new(runner, proxy_config, None);
//! proxy.start().await?;
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
