mod stdio;
mod json_rpc;

pub use stdio::StdioTransport;
pub use json_rpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use async_trait::async_trait;
use serde_json::Value;
use crate::error::Result;

/// Trait defining transport operations for MCP communication
#[async_trait]
pub trait Transport: Send + Sync {
    /// Initialize the transport
    async fn initialize(&self) -> Result<()>;
    
    /// List available tools
    async fn list_tools(&self) -> Result<Vec<Value>>;
    
    /// Call a tool
    async fn call_tool(&self, name: &str, args: Value) -> Result<Value>;
    
    /// List available resources
    async fn list_resources(&self) -> Result<Vec<Value>>;
    
    /// Get a resource
    async fn get_resource(&self, uri: &str) -> Result<Value>;
}