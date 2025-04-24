use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Configuration for a single MCP server instance.
///
/// This structure defines how to start and configure a specific MCP server process.
/// It includes the command to execute, any arguments to pass, and optional environment
/// variables to set when launching the server.
///
/// # Examples
///
/// Basic server configuration:
///
/// ```
/// use mcp_runner::config::ServerConfig;
/// use std::collections::HashMap;
///
/// let server_config = ServerConfig {
///     command: "node".to_string(),
///     args: vec!["server.js".to_string()],
///     env: HashMap::new(),
/// };
/// ```
///
/// Configuration with environment variables:
///
/// ```
/// use mcp_runner::config::ServerConfig;
/// use std::collections::HashMap;
///
/// let mut env = HashMap::new();
/// env.insert("MODEL_PATH".to_string(), "/path/to/model".to_string());
/// env.insert("DEBUG".to_string(), "true".to_string());
///
/// let server_config = ServerConfig {
///     command: "python".to_string(),
///     args: vec!["-m".to_string(), "mcp_server".to_string()],
///     env,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Command to execute when starting the MCP server.
    /// This can be an absolute path or a command available in the PATH.
    pub command: String,

    /// Command-line arguments to pass to the server.
    pub args: Vec<String>,

    /// Environment variables to set when launching the server.
    /// These will be combined with the current environment.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Main configuration for the MCP Runner.
///
/// This structure holds configurations for multiple MCP servers that can be
/// managed by the runner. Each server has a unique name and its own configuration.
///
/// # JSON Schema
///
/// The configuration follows this JSON schema:
///
/// ```json
/// {
///   "mcpServers": {
///     "server1": {
///       "command": "node",
///       "args": ["server.js"],
///       "env": {
///         "PORT": "3000",
///         "DEBUG": "true"
///       }
///     },
///     "server2": {
///       "command": "python",
///       "args": ["-m", "mcp_server"],
///       "env": {}
///     }
///   }
/// }
/// ```
///
/// # Examples
///
/// Loading a configuration from a file:
///
/// ```no_run
/// use mcp_runner::config::Config;
///
/// let config = Config::from_file("config.json").unwrap();
/// ```
///
/// Accessing a server configuration:
///
/// ```
/// use mcp_runner::config::{Config, ServerConfig};
/// # use std::collections::HashMap;
/// # let mut servers = HashMap::new();
/// # let server_config = ServerConfig {
/// #    command: "uvx".to_string(),
/// #    args: vec!["mcp-server-fetch".to_string()],
/// #    env: HashMap::new(),
/// # };
/// # servers.insert("fetch".to_string(), server_config);
/// # let config = Config { mcp_servers: servers };
///
/// if let Some(server_config) = config.mcp_servers.get("fetch") {
///     println!("Command: {}", server_config.command);
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Map of server names to their configurations.
    /// The key is a unique identifier for each server.
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, ServerConfig>,
}

impl Config {
    /// Loads a configuration from a file path.
    ///
    /// This method reads the file at the specified path and parses its contents
    /// as a JSON configuration.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the configuration file
    ///
    /// # Returns
    ///
    /// A `Result<Config>` that contains the parsed configuration or an error
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The file cannot be read
    /// * The file contents are not valid JSON
    /// * The JSON does not conform to the expected schema
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::ConfigParse(format!("Failed to read config file: {}", e)))?;

        Self::parse_from_str(&content)
    }

    /// Parses a configuration from a JSON string.
    ///
    /// # Arguments
    ///
    /// * `content` - A string containing JSON configuration
    ///
    /// # Returns
    ///
    /// A `Result<Config>` that contains the parsed configuration or an error
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The string is not valid JSON
    /// * The JSON does not conform to the expected schema
    pub fn parse_from_str(content: &str) -> Result<Self> {
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

        let config = Config::parse_from_str(config_str).unwrap();

        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("filesystem"));

        let fs_config = &config.mcp_servers["filesystem"];
        assert_eq!(fs_config.command, "npx");
        assert_eq!(
            fs_config.args,
            vec![
                "-y",
                "@modelcontextprotocol/server-filesystem",
                "/path/to/allowed/files"
            ]
        );
    }
}
