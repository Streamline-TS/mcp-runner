/// Client module for interacting with MCP servers.
///
/// This module provides the `McpClient` class which serves as the main interface
/// for communicating with Model Context Protocol servers. It allows applications to:
/// - List available tools provided by an MCP server
/// - Call tools with arguments
/// - List available resources
/// - Retrieve resource content
///
/// The client is transport-agnostic and can work with any implementation of the
/// `Transport` trait, though the library primarily focuses on StdioTransport.
use crate::error::{Error, Result};
use crate::transport::Transport;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing; // Import tracing

/// Represents an MCP tool with its metadata.
///
/// Tools are the primary way to interact with MCP servers. Each tool has
/// a name, description, and optional schema information for its inputs and outputs.
///
/// # Examples
///
/// ```
/// # use serde_json::json;
/// use mcp_runner::client::Tool;
///
/// let tool = Tool {
///     name: "fetch".to_string(),
///     description: "Fetch data from a URL".to_string(),
///     input_schema: Some(json!({
///         "type": "object",
///         "properties": {
///             "url": {
///                 "type": "string",
///                 "description": "The URL to fetch data from"
///             }
///         },
///         "required": ["url"]
///     })),
///     output_schema: Some(json!({
///         "type": "object",
///         "properties": {
///             "content": {
///                 "type": "string"
///             }
///         }
///     })),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name used when calling the tool.
    pub name: String,
    /// Human-readable description of the tool's purpose and functionality.
    pub description: String,
    /// JSON Schema defining the expected format of tool inputs.
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<Value>,
    /// JSON Schema defining the expected format of tool outputs.
    #[serde(rename = "outputSchema")]
    pub output_schema: Option<Value>,
}

/// Represents an MCP resource with its metadata.
///
/// Resources are data exposed by the MCP server that can be retrieved by clients.
/// Each resource has a URI that uniquely identifies it, along with metadata.
///
/// # Examples
///
/// ```
/// use mcp_runner::client::Resource;
///
/// let resource = Resource {
///     uri: "res:fetch/settings".to_string(),
///     name: "Fetch Settings".to_string(),
///     description: Some("Configuration settings for fetch operations".to_string()),
///     resource_type: Some("application/json".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Unique URI identifying the resource.
    pub uri: String,
    /// Human-readable name of the resource.
    pub name: String,
    /// Optional description of the resource's content or purpose.
    pub description: Option<String>,
    /// Optional MIME type or format of the resource.
    #[serde(rename = "type")]
    pub resource_type: Option<String>,
}

/// A client for interacting with an MCP server.
///
/// The `McpClient` provides a high-level interface for communicating with
/// Model Context Protocol servers. It abstracts away the details of the
/// transport layer and JSON-RPC protocol, offering a simple API for listing
/// and calling tools, and accessing resources.
/// All public methods are instrumented with `tracing` spans.
///
/// # Examples
///
/// Basic usage:
///
/// ```no_run
/// # // This example is marked no_run because it doesn't actually run the code,
/// # // it just verifies that it compiles correctly.
/// use mcp_runner::{McpClient, transport::StdioTransport, error::Result};
/// use serde_json::{json, Value};
/// use async_process::{ChildStdin, ChildStdout};
///
/// # // Mock implementation for the example
/// # fn get_mock_stdin_stdout() -> (ChildStdin, ChildStdout) {
/// #     unimplemented!("This is just for doctest and won't be called")
/// # }
///
/// # async fn example() -> Result<()> {
/// # // In a real app, you would get these from a server process
/// # // Here we just declare them but don't initialize to make the doctest pass
/// # let (stdin, stdout) = get_mock_stdin_stdout();
///
/// // Create a transport
/// let transport = StdioTransport::new("fetch-server".to_string(), stdin, stdout);
///
/// // Create a client
/// let client = McpClient::new("fetch-server".to_string(), transport);
///
/// // Initialize
/// client.initialize().await?;
///
/// // List tools
/// let tools = client.list_tools().await?;
/// for tool in tools {
///     println!("Tool: {} - {}", tool.name, tool.description);
/// }
///
/// // Call the fetch tool
/// #[derive(serde::Serialize, Debug)]
/// struct FetchInput {
///     url: String,
/// }
///
/// let input = FetchInput {
///     url: "https://modelcontextprotocol.io".to_string(),
/// };
///
/// let output: Value = client.call_tool("fetch", &input).await?;
/// println!("Fetch result: {}", output);
/// # Ok(())
/// # }
/// ```
pub struct McpClient {
    /// Server name for identification.
    name: String,
    /// Transport implementation for communication.
    transport: Arc<dyn Transport>,
}

impl McpClient {
    /// Creates a new MCP client with the specified name and transport.
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// # Arguments
    ///
    /// * `name` - A name for this client, typically the server name
    /// * `transport` - The transport implementation to use for communication
    ///
    /// # Returns
    ///
    /// A new `McpClient` instance
    #[tracing::instrument(skip(transport), fields(client_name = %name))]
    pub fn new(name: String, transport: impl Transport + 'static) -> Self {
        tracing::info!("Creating new McpClient");
        Self {
            name,
            transport: Arc::new(transport),
        }
    }

    /// Gets the name of the client (usually the same as the server name).
    ///
    /// # Returns
    ///
    /// A string slice containing the client name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Initializes the connection to the MCP server.
    ///
    /// This method should be called before any other methods to ensure
    /// the server is ready to accept requests.
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or failure
    #[tracing::instrument(skip(self), fields(client_name = %self.name))]
    pub async fn initialize(&self) -> Result<()> {
        tracing::info!("Initializing client connection");
        self.transport.initialize().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to initialize transport");
            e
        })
    }

    /// Lists all available tools provided by the MCP server.
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Tool>>` containing descriptions of available tools if successful
    #[tracing::instrument(skip(self), fields(client_name = %self.name))]
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        tracing::debug!("Listing tools via transport");
        let tools_json = self.transport.list_tools().await?;
        tracing::trace!(raw_tools = ?tools_json, "Received raw tools list");

        let mut tools = Vec::new();
        for tool_value in tools_json {
            match serde_json::from_value::<Tool>(tool_value.clone()) {
                Ok(tool) => {
                    tracing::trace!(tool_name = %tool.name, "Successfully deserialized tool");
                    tools.push(tool);
                }
                Err(e) => {
                    tracing::warn!(error = %e, value = ?tool_value, "Failed to deserialize tool from value");
                }
            }
        }
        tracing::debug!(num_tools = tools.len(), "Finished listing tools");
        Ok(tools)
    }

    /// Calls a tool on the MCP server with the given arguments.
    ///
    /// This method provides a strongly-typed interface for tool calls,
    /// where the input and output types are specified as generic parameters.
    /// This method is instrumented with `tracing`.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The input type, which must be serializable to JSON and implement `Debug`
    /// * `R` - The output type, which must be deserializable from JSON
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `args` - The arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// A `Result<R>` containing the tool's response if successful
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The tool call fails
    /// * The arguments cannot be serialized
    /// * The result cannot be deserialized to type R
    #[tracing::instrument(skip(self, args), fields(client_name = %self.name, tool_name = %name))]
    pub async fn call_tool<T, R>(&self, name: &str, args: &T) -> Result<R>
    where
        T: Serialize + std::fmt::Debug,
        R: for<'de> Deserialize<'de>,
    {
        tracing::debug!(args = ?args, "Calling tool");
        let args_value = serde_json::to_value(args).map_err(|e| {
            tracing::error!(error = %e, "Failed to serialize tool arguments");
            Error::Serialization(format!("Failed to serialize tool arguments: {}", e))
        })?;
        tracing::trace!(args_json = ?args_value, "Serialized arguments");

        let result_value = self.transport.call_tool(name, args_value).await?;
        tracing::trace!(result_json = ?result_value, "Received raw tool result");

        serde_json::from_value(result_value.clone()).map_err(|e| {
            tracing::error!(error = %e, value = ?result_value, "Failed to deserialize tool result");
            Error::Serialization(format!("Failed to deserialize tool result: {}", e))
        })
    }

    /// Lists all available resources provided by the MCP server.
    ///
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Resource>>` containing descriptions of available resources if successful
    #[tracing::instrument(skip(self), fields(client_name = %self.name))]
    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        tracing::debug!("Listing resources via transport");
        let resources_json = self.transport.list_resources().await?;
        tracing::trace!(raw_resources = ?resources_json, "Received raw resources list");

        let mut resources = Vec::new();
        for resource_value in resources_json {
            match serde_json::from_value::<Resource>(resource_value.clone()) {
                Ok(resource) => {
                    tracing::trace!(resource_uri = %resource.uri, "Successfully deserialized resource");
                    resources.push(resource);
                }
                Err(e) => {
                    tracing::warn!(error = %e, value = ?resource_value, "Failed to deserialize resource from value");
                }
            }
        }
        tracing::debug!(
            num_resources = resources.len(),
            "Finished listing resources"
        );
        Ok(resources)
    }

    /// Gets a specific resource from the MCP server.
    ///
    /// This method provides a strongly-typed interface for resource retrieval,
    /// where the expected resource type is specified as a generic parameter.
    /// This method is instrumented with `tracing`.
    ///
    /// # Type Parameters
    ///
    /// * `R` - The resource type, which must be deserializable from JSON
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to retrieve
    ///
    /// # Returns
    ///
    /// A `Result<R>` containing the resource data if successful
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// * The resource retrieval fails
    /// * The result cannot be deserialized to type R
    #[tracing::instrument(skip(self), fields(client_name = %self.name, resource_uri = %uri))]
    pub async fn get_resource<R>(&self, uri: &str) -> Result<R>
    where
        R: for<'de> Deserialize<'de>,
    {
        tracing::debug!("Getting resource via transport");
        let resource_value = self.transport.get_resource(uri).await?;
        tracing::trace!(raw_resource = ?resource_value, "Received raw resource value");

        serde_json::from_value(resource_value.clone()).map_err(|e| {
            tracing::error!(error = %e, value = ?resource_value, "Failed to deserialize resource");
            Error::Serialization(format!("Failed to deserialize resource: {}", e))
        })
    }

    /// Closes the client connection.
    ///
    /// This is a placeholder method since the transport is behind an Arc and can't actually
    /// be closed by the client directly. Users should drop all references to the client
    /// to properly clean up resources.
    /// This method is instrumented with `tracing`.
    ///
    /// # Returns
    ///
    /// A `Result<()>` that is always Ok
    #[tracing::instrument(skip(self), fields(client_name = %self.name))]
    pub async fn close(&self) -> Result<()> {
        tracing::info!(
            "Close called on McpClient (Note: Transport closure depends on Arc references)"
        );
        Ok(())
    }
}
