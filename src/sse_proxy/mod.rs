//! SSE proxy implementation for MCP servers using Actix Web.
//!
//! This module provides an HTTP and Server-Sent Events (SSE) proxy for MCP servers
//! built on the Actix Web framework, allowing web clients to interact with MCP servers
//! through a unified REST-like API.
//!
//! The proxy handles HTTP routing, authentication, and SSE event streaming.
//! It enables clients to:
//! - Subscribe to SSE events from MCP servers
//! - Make tool calls to MCP servers via JSON-RPC
//! - List available servers, tools, and resources
//! - Retrieve resources from MCP servers
//!
//! This implementation uses Actix Web for improved performance, reliability,
//! and maintainability compared to the custom HTTP implementation.

// Re-export the main types
pub use self::proxy::{SSEProxy, SSEProxyHandle, SSEProxyRunnerAccess};
pub use self::types::{SSEEvent, SSEMessage, ServerInfo, ServerInfoUpdate};

// Submodules
pub mod actix_error;
pub mod auth;
pub mod events;
pub mod handlers;
pub mod proxy;
pub mod types;
