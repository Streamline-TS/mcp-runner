/*!
 # MCP Runner

 A Rust library for running and interacting with Model Context Protocol (MCP) servers.

 ## Overview
 
 MCP Runner provides functionality to:
 - Start and manage MCP server processes
 - Communicate with MCP servers using JSON-RPC
 - Configure MCP servers through config files
 - List and call tools provided by MCP servers
 - Access resources exposed by MCP servers

 ## Basic Usage

 ```no_run
 use mcp_runner::{McpRunner, Result};
 use serde_json::{json, Value};
 
 #[tokio::main]
 async fn main() -> Result<()> {
     // Create a runner from config file
     let mut runner = McpRunner::from_config_file("config.json")?;
     
     // Start all configured servers
     let server_ids = runner.start_all_servers().await?;
     
     // Or start a specific server
     let server_id = runner.start_server("fetch").await?;
     
     // Get a client to interact with the server
     let client = runner.get_client(server_id)?;
     
     // Initialize the client
     client.initialize().await?;
     
     // List available tools
     let tools = client.list_tools().await?;
     println!("Available tools: {:?}", tools);
     
     // Call a tool
     let args = json!({
         "url": "https://modelcontextprotocol.io"
     });
     let result: Value = client.call_tool("fetch", &args).await?;
     
     println!("Result: {:?}", result);
     
     Ok(())
 }
 ```

 ## Features
 
 - **Server Management**: Start, stop, and monitor MCP servers
 - **JSON-RPC Communication**: Communicate with MCP servers using JSON-RPC
 - **Configuration**: Configure servers through JSON config files
 - **Error Handling**: Comprehensive error handling
 - **Async Support**: Full async/await support

 ## License

 This project is licensed under the terms in the LICENSE file.
*/

pub mod config;
pub mod server;
pub mod transport;
pub mod error;
pub mod client;

// Re-export key types for better API ergonomics
pub use config::Config;
pub use error::{Error, Result};
pub use server::{ServerId, ServerProcess, ServerStatus};
pub use client::McpClient;

use std::collections::HashMap;
use std::path::Path;
use transport::StdioTransport;

/// Configure and run MCP servers
pub struct McpRunner {
    /// Configuration
    config: Config,
    /// Running server processes
    servers: HashMap<ServerId, ServerProcess>,
    /// Map of server names to server IDs
    server_names: HashMap<String, ServerId>,
}

impl McpRunner {
    /// Create a new MCP runner from a configuration file path
    pub fn from_config_file(path: impl AsRef<Path>) -> Result<Self> {
        let config = Config::from_file(path)?;
        Ok(Self::new(config))
    }
    
    /// Create a new MCP runner from a configuration string
    pub fn from_config_str(config: &str) -> Result<Self> {
        let config = Config::from_str(config)?;
        Ok(Self::new(config))
    }
    
    /// Create a new MCP runner from a configuration
    pub fn new(config: Config) -> Self {
        Self {
            config,
            servers: HashMap::new(),
            server_names: HashMap::new(),
        }
    }
    
    /// Start a specific MCP server
    pub async fn start_server(&mut self, name: &str) -> Result<ServerId> {
        // Check if server is already running
        if let Some(id) = self.server_names.get(name) {
            return Ok(*id);
        }
        
        // Get server configuration
        let config = self.config.mcp_servers.get(name)
            .ok_or_else(|| Error::ServerNotFound(name.to_string()))?
            .clone();
        
        // Create and start server process
        let mut server = ServerProcess::new(name.to_string(), config);
        let id = server.id();
        
        server.start().await?;
        
        // Store server
        self.servers.insert(id, server);
        self.server_names.insert(name.to_string(), id);
        
        Ok(id)
    }
    
    /// Start all configured servers
    pub async fn start_all_servers(&mut self) -> Result<Vec<ServerId>> {
        // Collect server names first to avoid borrowing issues
        let server_names: Vec<String> = self.config.mcp_servers.keys()
            .map(|k| k.to_string())
            .collect();
        
        let mut ids = Vec::new();
        
        for name in server_names {
            let id = self.start_server(&name).await?;
            ids.push(id);
        }
        
        Ok(ids)
    }
    
    /// Stop a running server
    pub async fn stop_server(&mut self, id: ServerId) -> Result<()> {
        if let Some(mut server) = self.servers.remove(&id) {
            // Remove from server_names
            self.server_names.remove(server.name());
            
            // Stop the server
            server.stop().await?;
            
            Ok(())
        } else {
            Err(Error::ServerNotFound(format!("{:?}", id)))
        }
    }
    
    /// Get server status
    pub fn server_status(&self, id: ServerId) -> Result<ServerStatus> {
        self.servers.get(&id)
            .map(|server| server.status())
            .ok_or_else(|| Error::ServerNotFound(format!("{:?}", id)))
    }
    
    /// Get server ID by name
    pub fn get_server_id(&self, name: &str) -> Result<ServerId> {
        self.server_names.get(name)
            .copied()
            .ok_or_else(|| Error::ServerNotFound(name.to_string()))
    }
    
    /// Get a client for a server
    pub fn get_client(&mut self, id: ServerId) -> Result<McpClient> {
        let server = self.servers.get_mut(&id)
            .ok_or_else(|| Error::ServerNotFound(format!("{:?}", id)))?;
        
        // Take the stdin and stdout from the server
        let stdin = server.take_stdin()?;
        let stdout = server.take_stdout()?;
        
        // Create the transport and client
        let transport = StdioTransport::new(server.name().to_string(), stdin, stdout);
        let client = McpClient::new(server.name().to_string(), transport);
        
        Ok(client)
    }
}