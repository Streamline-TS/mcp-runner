/// Error handling module for MCP Runner.
///
/// This module defines the error types used throughout the library.
/// It provides a comprehensive set of errors that can occur when
/// working with MCP servers, along with helpful context for debugging.
///
/// # Example
///
/// ```
/// use mcp_runner::{McpRunner, error::{Error, Result}};
///
/// fn handle_error(result: Result<()>) {
///     match result {
///         Ok(_) => println!("Operation succeeded"),
///         Err(Error::ServerNotFound(name)) => println!("Server '{}' not found in configuration", name),
///         Err(Error::Communication(msg)) => println!("Communication error: {}", msg),
///         Err(Error::Timeout(msg)) => println!("Operation timed out: {}", msg),
///         Err(e) => println!("Other error: {}", e),
///     }
/// }
/// ```
use thiserror::Error;

/// Errors that can occur in the mcp-runner library.
///
/// This enum represents all possible error types that can be returned from
/// operations in the MCP Runner library. Each variant includes context
/// information to help diagnose and handle the error appropriately.
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to parse configuration from a file or string.
    ///
    /// This error occurs when:
    /// - The configuration JSON is malformed
    /// - Required fields are missing
    /// - Field types are incorrect
    #[error("Failed to parse configuration: {0}")]
    ConfigParse(String),

    /// Configuration is valid JSON but contains invalid values.
    ///
    /// This error occurs when:
    /// - A command doesn't exist or isn't executable
    /// - Environment variables have invalid values
    /// - Conflicting settings are specified
    #[error("Invalid configuration: {0}")]
    ConfigInvalid(String),

    /// Error when starting, stopping, or communicating with a server process.
    ///
    /// This error occurs when:
    /// - The process fails to start
    /// - The process exits unexpectedly
    /// - The process fails to respond correctly
    #[error("Server process error: {0}")]
    Process(String),

    /// Error in the JSON-RPC protocol.
    ///
    /// This error occurs when:
    /// - The server returns an error response
    /// - The method doesn't exist
    /// - Invalid parameters are provided
    /// - The server response doesn't match the request
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    /// Error in the transport layer.
    ///
    /// This error occurs when:
    /// - The transport fails to initialize
    /// - The transport encounters an error during operation
    /// - The transport fails to close properly
    #[error("Transport error: {0}")]
    Transport(String),

    /// Requested server was not found in the configuration.
    ///
    /// This error occurs when:
    /// - A server name is passed that doesn't exist in the config
    /// - A server ID is used that doesn't match any running server
    #[error("Server not found: {0}")]
    ServerNotFound(String),

    /// Requested tool was not found on the MCP server.
    ///
    /// This error occurs when:
    /// - The tool name doesn't exist on the server
    /// - The tool is disabled or unavailable
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Requested resource was not found on the MCP server.
    ///
    /// This error occurs when:
    /// - The resource URI doesn't exist
    /// - The resource is not accessible to the client
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    /// Error in communication with the MCP server.
    ///
    /// This error occurs when:
    /// - The server doesn't respond
    /// - The response is malformed
    /// - The connection is lost
    #[error("Communication error: {0}")]
    Communication(String),

    /// Operation timed out.
    ///
    /// This error occurs when:
    /// - A server takes too long to start
    /// - A server takes too long to respond
    /// - A response is expected but doesn't arrive within the timeout period
    #[error("Timeout: {0}")]
    Timeout(String),

    /// The server is already running.
    ///
    /// This error occurs when:
    /// - Attempting to start a server that's already running
    #[error("Already running")]
    AlreadyRunning,

    /// The server is not running.
    ///
    /// This error occurs when:
    /// - Attempting to stop a server that's not running
    /// - Attempting to get a client for a server that's not running
    #[error("Not running")]
    NotRunning,

    /// Error in serializing or deserializing data.
    ///
    /// This error occurs when:
    /// - Arguments can't be serialized to JSON
    /// - Results can't be deserialized from JSON
    /// - Types don't match expected schema
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Configuration is valid but contains values that fail validation checks.
    ///
    /// This error occurs when:
    /// - A specified file path doesn't exist.
    /// - A required field is missing based on context.
    /// - A value is outside the allowed range or set.
    #[error("Config validation error: {0}")]
    ConfigValidation(String),

    /// Unauthorized access error.
    ///
    /// This error occurs when:
    /// - An operation requires authentication but none was provided.
    /// - The provided authentication token is invalid.
    /// - The authenticated user does not have permission for the operation.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Any other error not covered by the above categories.
    ///
    /// This is a catch-all error for cases not explicitly handled elsewhere.
    #[error("Other error: {0}")]
    Other(String),
}

/// Result type for mcp-runner operations.
///
/// This is a convenience type alias for `std::result::Result` with the `Error` type
/// from this module. Use this throughout the library and in client code to handle
/// errors in a consistent way.
pub type Result<T> = std::result::Result<T, Error>;
