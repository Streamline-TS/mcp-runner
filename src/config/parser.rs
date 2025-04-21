use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Command to execute
    pub command: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Optional environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// MCP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Map of server names to their configurations
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, ServerConfig>,
}

impl Config {
    /// Load a configuration from a file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::ConfigParse(format!("Failed to read config file: {}", e)))?;
        
        Self::from_str(&content)
    }

    /// Parse a configuration from a string
    pub fn from_str(content: &str) -> Result<Self> {
        serde_json::from_str(content)
            .map_err(|e| Error::ConfigParse(format!("Failed to parse JSON config: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_claude_config() {
        let config_str = r#"{
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/files"]
                }
            }
        }"#;
        
        let config = Config::from_str(config_str).unwrap();
        
        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("filesystem"));
        
        let fs_config = &config.mcp_servers["filesystem"];
        assert_eq!(fs_config.command, "npx");
        assert_eq!(fs_config.args, vec!["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/files"]);
    }
}