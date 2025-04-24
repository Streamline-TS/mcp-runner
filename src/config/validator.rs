use crate::config::ServerConfig;
use crate::error::{Error, Result};
use std::collections::HashMap;

/// Validates a server configuration
fn validate_server_config(name: &str, config: &ServerConfig) -> Result<()> {
    // Check command is not empty
    if config.command.is_empty() {
        return Err(Error::ConfigInvalid(format!(
            "Server '{}' has empty command",
            name
        )));
    }

    // TODO: Add more validation rules as needed

    Ok(())
}

/// Validates a map of server configurations
fn validate_server_configs(configs: &HashMap<String, ServerConfig>) -> Result<()> {
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

    Ok(())
}
