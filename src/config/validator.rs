//! Configuration validation module.
//!
//! This module provides functions for validating MCP Runner configurations,
//! ensuring that configuration values are valid and consistent before they
//! are used to create and manage server processes.

use crate::config::{Config, SSEProxyConfig, ServerConfig};
use crate::error::{Error, Result};
use std::collections::HashMap;

/// Validates a server configuration.
///
/// This function checks that the server configuration is valid by ensuring:
/// - The command is not empty
/// - Any required arguments are present
///
/// # Arguments
///
/// * `config` - The server configuration to validate
///
/// # Returns
///
/// A `Result` indicating success or failure
pub fn validate_server_config(config: &ServerConfig) -> Result<()> {
    // Command must not be empty
    if config.command.is_empty() {
        return Err(Error::ConfigValidation(
            "Server command cannot be empty".to_string(),
        ));
    }

    Ok(())
}

/// Validates a collection of server configurations.
///
/// This function validates each server configuration in the collection.
///
/// # Arguments
///
/// * `configs` - The server configurations to validate
///
/// # Returns
///
/// A `Result` indicating success or failure
pub fn validate_server_configs(configs: &HashMap<String, ServerConfig>) -> Result<()> {
    for (name, config) in configs {
        validate_server_config(config).map_err(|e| {
            Error::ConfigValidation(format!(
                "Invalid configuration for server '{}': {}",
                name, e
            ))
        })?;
    }

    Ok(())
}

/// Validates an SSE proxy configuration.
///
/// This function checks that the SSE proxy configuration is valid.
/// Currently, there's no specific validation needed, but this function
/// exists for future validation requirements.
///
/// # Arguments
///
/// * `config` - The SSE proxy configuration to validate
///
/// # Returns
///
/// A `Result` indicating success or failure
pub fn validate_sse_proxy_config(_config: &SSEProxyConfig) -> Result<()> {
    // Currently, there's no validation needed for SSE proxy config
    // This function exists for future validation needs

    Ok(())
}

/// Validates a complete configuration.
///
/// This function validates the entire configuration by checking:
/// - Server configurations are valid
/// - SSE proxy configuration is valid if present
///
/// # Arguments
///
/// * `config` - The configuration to validate
///
/// # Returns
///
/// A `Result` indicating success or failure
pub fn validate_config(config: &HashMap<String, ServerConfig>) -> Result<()> {
    validate_server_configs(config)
}

/// Validates a complete Config object
///
/// This validates both the server configurations and SSE proxy config if present
///
/// # Arguments
///
/// * `config` - The Config object to validate
///
/// # Returns
///
/// A `Result` indicating success or failure
pub fn validate_full_config(config: &Config) -> Result<()> {
    // Validate server configs
    validate_server_configs(&config.mcp_servers)?;

    // Validate SSE proxy config if present
    if let Some(proxy_config) = &config.sse_proxy {
        validate_sse_proxy_config(proxy_config)?;
    }

    Ok(())
}
