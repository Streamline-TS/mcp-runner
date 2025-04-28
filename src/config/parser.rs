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

/// Authentication configuration for SSE Proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Bearer authentication configuration
    #[serde(default)]
    pub bearer: Option<BearerAuthConfig>,
}

/// Bearer token authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BearerAuthConfig {
    /// Authentication token
    pub token: String,
}

/// Server-Sent Events (SSE) Proxy configuration
///
/// This structure defines the configuration for the SSE proxy server, which allows
/// web clients to connect to MCP servers via HTTP and receive real-time updates
/// through Server-Sent Events. The proxy provides authentication, server access control,
/// and network binding options.
///
/// # Examples
///
/// Basic SSE proxy configuration with default address and port:
///
/// ```
/// use mcp_runner::config::SSEProxyConfig;
///
/// let proxy_config = SSEProxyConfig {
///     allowed_servers: None,         // Allow all servers
///     authenticate: None,            // No authentication required
///     address: "127.0.0.1".to_string(),
///     port: 3000,
/// };
/// ```
///
/// Secure SSE proxy configuration with restrictions:
///
/// ```
/// use mcp_runner::config::{SSEProxyConfig, AuthConfig, BearerAuthConfig};
///
/// let auth_config = AuthConfig {
///     bearer: Some(BearerAuthConfig {
///         token: "secure_token_string".to_string(),
///     }),
/// };
///
/// let proxy_config = SSEProxyConfig {
///     allowed_servers: Some(vec!["fetch-server".to_string(), "embedding-server".to_string()]),
///     authenticate: Some(auth_config),
///     address: "0.0.0.0".to_string(),  // Listen on all interfaces
///     port: 8080,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SSEProxyConfig {
    /// List of allowed server names that clients can access
    ///
    /// When specified, only servers in this list can be accessed through the proxy.
    /// If `None`, all servers defined in the configuration are accessible.
    #[serde(default)]
    pub allowed_servers: Option<Vec<String>>,

    /// Authentication configuration for securing the proxy
    ///
    /// When specified, clients must provide valid authentication credentials.
    /// If `None`, the proxy accepts all connections without authentication.
    #[serde(default)]
    pub authenticate: Option<AuthConfig>,

    /// Network address the proxy server will bind to
    ///
    /// Use "127.0.0.1" to allow only local connections, or "0.0.0.0" to accept
    /// connections from any network interface.
    #[serde(default = "default_address")]
    pub address: String,

    /// TCP port the proxy server will listen on
    ///
    /// The port must be available and not require elevated privileges (typically
    /// ports above 1024 unless running with administrator/root privileges).
    #[serde(default = "default_port")]
    pub port: u16,
}

/// Default address for the SSE proxy
fn default_address() -> String {
    "127.0.0.1".to_string()
}

/// Default port for the SSE proxy
fn default_port() -> u16 {
    3000
}

impl Default for SSEProxyConfig {
    fn default() -> Self {
        Self {
            allowed_servers: None,
            authenticate: None,
            address: default_address(),
            port: default_port(),
        }
    }
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
///   },
///   "sseProxy": {
///     "allowedServers": ["server1"],
///     "authenticate": {
///       "bearer": {
///         "token": "your_token"
///       }
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
/// # let config = Config { mcp_servers: servers, sse_proxy: None };
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

    /// SSE Proxy configuration, if None the proxy is disabled
    #[serde(rename = "sseProxy", default)]
    pub sse_proxy: Option<SSEProxyConfig>,
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
