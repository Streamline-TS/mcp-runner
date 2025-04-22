use crate::config::ServerConfig;
use crate::error::{Error, Result};
use std::collections::HashMap;

/// Validates a server configuration
pub fn validate_server_config(name: &str, config: &ServerConfig) -> Result<()> {
    // Check command is not empty
    if config.command.is_empty() {
        return Err(Error::ConfigInvalid(format!("Server '{}' has empty command", name)));
    }
    
    // Check command exists
    // For simplicity, we'll just check it's not empty, but in a more robust implementation
    // we could check if the command exists on the system
    
    // Validate configuration based on server type
    // We could add specific validation for known server types
    
    Ok(())
}

/// Validates a map of server configurations
pub fn validate_server_configs(configs: &HashMap<String, ServerConfig>) -> Result<()> {
    if configs.is_empty() {
        return Err(Error::ConfigInvalid("No servers configured".to_string()));
    }
    
    for (name, config) in configs {
        validate_server_config(name, config)?;
    }
    
    Ok(())
}

/// Full configuration validation
pub fn validate_config(configs: &HashMap<String, ServerConfig>) -> Result<()> {
    // Validate server configurations
    validate_server_configs(configs)?;
    
    // Add any global validation here
    
    Ok(())
}