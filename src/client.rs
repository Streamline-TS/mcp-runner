use crate::error::{Error, Result};
use crate::transport::StdioTransport;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::sync::Arc;

/// Represents an MCP tool with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Tool input schema
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<Value>,
    /// Tool output schema
    #[serde(rename = "outputSchema")]
    pub output_schema: Option<Value>,
}

/// Represents an MCP resource with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Resource type
    #[serde(rename = "type")]
    pub resource_type: Option<String>,
}

/// A client for interacting with an MCP server
pub struct McpClient {
    /// Server name
    name: String,
    /// Transport
    transport: Arc<StdioTransport>,
}

impl McpClient {
    /// Create a new MCP client
    pub fn new(name: String, transport: StdioTransport) -> Self {
        Self {
            name,
            transport: Arc::new(transport),
        }
    }
    
    /// Get server name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Initialize the MCP server
    pub async fn initialize(&self) -> Result<()> {
        self.transport.initialize().await
    }
    
    /// List available tools
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let tools_json = self.transport.list_tools().await?;
        
        let mut tools = Vec::new();
        for tool_value in tools_json {
            if let Ok(tool) = serde_json::from_value(tool_value) {
                tools.push(tool);
            }
        }
        
        Ok(tools)
    }
    
    /// Call a tool
    pub async fn call_tool<T, R>(&self, name: &str, args: &T) -> Result<R>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        // Serialize args to a Value
        let args_value = serde_json::to_value(args)
            .map_err(|e| Error::Serialization(format!("Failed to serialize tool arguments: {}", e)))?;
        
        // Call the tool
        let result_value = self.transport.call_tool(name, args_value).await?;
        
        // Deserialize the result
        serde_json::from_value(result_value)
            .map_err(|e| Error::Serialization(format!("Failed to deserialize tool result: {}", e)))
    }
    
    /// List available resources
    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        let resources_json = self.transport.list_resources().await?;
        
        let mut resources = Vec::new();
        for resource_value in resources_json {
            if let Ok(resource) = serde_json::from_value(resource_value) {
                resources.push(resource);
            }
        }
        
        Ok(resources)
    }
    
    /// Get a resource content
    pub async fn get_resource<R>(&self, uri: &str) -> Result<R>
    where
        R: for<'de> Deserialize<'de>,
    {
        let resource_value = self.transport.get_resource(uri).await?;
        
        serde_json::from_value(resource_value)
            .map_err(|e| Error::Serialization(format!("Failed to deserialize resource: {}", e)))
    }
    
    /// Close the client connection
    pub async fn close(&self) -> Result<()> {
        // We can't actually close the transport since it's behind an Arc
        // But we can notify the user that they should drop all references to the client
        Ok(())
    }
}