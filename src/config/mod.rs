/// Configuration module for MCP Runner.
///
/// This module handles parsing, validation, and access to configuration
/// settings for MCP servers. It supports loading configurations from files
/// or strings in JSON format.
///
/// # Examples
///
/// Loading a configuration from a file:
///
/// ```no_run
/// use mcp_runner::config::Config;
///
/// let config = Config::from_file("config.json").unwrap();
/// println!("Loaded configuration with {} servers", config.mcp_servers.len());
/// ```
///
/// Creating a configuration programmatically:
///
/// ```
/// use mcp_runner::config::{Config, ServerConfig};
/// use std::collections::HashMap;
///
/// let mut servers = HashMap::new();
/// let gpt4_config = ServerConfig {
///     command: "python".to_string(),
///     args: vec!["-m".to_string(), "mcp_server".to_string()],
///     env: HashMap::new(),
/// };
/// 
/// servers.insert("gpt4".to_string(), gpt4_config);
/// let config = Config { mcp_servers: servers };
/// ```
mod parser;
pub mod validator;

pub use parser::{Config, ServerConfig};
pub use validator::validate_config;