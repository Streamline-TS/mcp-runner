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
//! use std::net::SocketAddr;
//! use std::str::FromStr;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a runner
//! let runner = McpRunner::from_config_file("config.json")?;
//!
//! // Create proxy configuration
//! let proxy_config = SSEProxyConfig {
//!     authenticate: None,
//!     allowed_servers: None,
//!     address: "127.0.0.1".to_string(),
//!     port: 8080,
//! };
//!
//! // Create socket address to listen on
//! let addr = SocketAddr::from_str("127.0.0.1:8080")?;
//!
//! // Create and start the SSE proxy
//! let proxy = SSEProxy::new(runner, proxy_config, addr);
//! proxy.start().await?;
//! # Ok(())
//! # }
//! ```

// Re-exports
pub use self::sse_proxy::SSEProxy;
pub use self::types::{ServerInfo, ToolInfo, ResourceInfo, SSEMessage, SSEEvent};

// Submodules
pub mod events;
pub mod http;
pub mod sse_proxy;
pub mod types;