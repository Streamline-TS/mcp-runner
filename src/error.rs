use thiserror::Error;

/// Errors that can occur in the mcp-runner library
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse configuration: {0}")]
    ConfigParse(String),

    #[error("Invalid configuration: {0}")]
    ConfigInvalid(String),

    #[error("Server process error: {0}")]
    Process(String),

    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Communication error: {0}")]
    Communication(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Already running")]
    AlreadyRunning,

    #[error("Not running")]
    NotRunning,

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Result type for mcp-runner operations
pub type Result<T> = std::result::Result<T, Error>;