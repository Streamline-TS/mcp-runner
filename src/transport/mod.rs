mod json_rpc;
mod stdio;

use crate::error::Result;
use async_trait::async_trait;
pub use json_rpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
pub use stdio::StdioTransport;

/// Transport is the core trait for communication with MCP servers.
///
/// This trait defines the interface for interacting with Model Context Protocol (MCP)
/// servers through various transport mechanisms. Implementations of this trait handle
/// the low-level communication details, allowing clients to focus on high-level interactions.
///
/// # Examples
///
/// Using a transport to list available tools:
///
/// ```no_run
/// use mcp_runner::transport::Transport;
/// use mcp_runner::error::Result;
/// use serde_json::Value;
///
/// async fn example<T: Transport>(transport: &T) -> Result<()> {
///     // Initialize the transport
///     transport.initialize().await?;
///     
///     // List available tools
///     let tools = transport.list_tools().await?;
///     println!("Available tools: {:?}", tools);
///     
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait Transport: Send + Sync {
    /// Initializes the transport connection to the MCP server.
    ///
    /// This method should be called before any other methods to ensure
    /// the transport is ready for communication.
    ///
    /// # Returns
    ///
    /// A `Result<()>` that is:
    /// - `Ok(())` if initialization was successful
    /// - `Err(Error)` if initialization failed
    async fn initialize(&self) -> Result<()>;

    /// Lists all available tools provided by the MCP server.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` that is:
    /// - `Ok(Vec<Value>)` containing a list of tool definitions if successful
    /// - `Err(Error)` if the request failed
    ///
    /// # Tool Definition Format
    ///
    /// Each tool definition is a JSON object with at least:
    /// - `name`: A string identifier for the tool
    /// - `description`: A human-readable description of the tool
    /// - Additional fields as specified by the MCP server
    async fn list_tools(&self) -> Result<Vec<Value>>;

    /// Calls a specific tool on the MCP server with the provided arguments.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `args` - Arguments to pass to the tool as a JSON value
    ///
    /// # Returns
    ///
    /// A `Result<Value>` that is:
    /// - `Ok(Value)` containing the tool's response if successful
    /// - `Err(Error)` if the tool call failed
    async fn call_tool(&self, name: &str, args: Value) -> Result<Value>;

    /// Lists all available resources provided by the MCP server.
    ///
    /// Resources can include model metadata, usage information, or other
    /// static or dynamic data exposed by the server.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Value>>` that is:
    /// - `Ok(Vec<Value>)` containing a list of resource definitions if successful
    /// - `Err(Error)` if the request failed
    async fn list_resources(&self) -> Result<Vec<Value>>;

    /// Retrieves a specific resource from the MCP server.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI identifying the resource to retrieve
    ///
    /// # Returns
    ///
    /// A `Result<Value>` that is:
    /// - `Ok(Value)` containing the resource data if successful
    /// - `Err(Error)` if the resource retrieval failed
    async fn get_resource(&self, uri: &str) -> Result<Value>;
}
