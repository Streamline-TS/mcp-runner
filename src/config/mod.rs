//! Configuration module for MCP Runner.
//!
//! This module handles parsing, validation, and access to configuration
//! settings for MCP servers. It supports loading configurations from files
//! or strings in JSON format.
//!
//! # Examples
//!
//! Loading a configuration from a file:
//!
//! ```no_run
//! use mcp_runner::config::Config;
//!
//! let config = Config::from_file("config.json").unwrap();
//! println!("Loaded configuration with {} servers", config.mcp_servers.len());
//! ```
//!
//! Creating a configuration programmatically:
//! ```
//! use mcp_runner::{McpRunner, config::{Config, ServerConfig}};
//! use std::collections::HashMap;
//!
//! let mut servers = HashMap::new();
//!
//! let server_config = ServerConfig {
//!     command: "uvx".to_string(),
//!     args: vec!["mcp-server-fetch".to_string()],
//!     env: HashMap::new(),
//! };
//!
//! servers.insert("fetch".to_string(), server_config);
//! // sse_proxy is optional, so we can use None
//! let config = Config { 
//!     mcp_servers: servers,
//!     sse_proxy: None,
//! };
//! let runner = McpRunner::new(config);
//! ```
mod parser;
pub mod validator;

pub use parser::{Config, ServerConfig, SSEProxyConfig, AuthConfig, BearerAuthConfig};
pub use validator::validate_config;
