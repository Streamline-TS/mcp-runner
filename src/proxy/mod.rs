// Proxy module for handling HTTP and SSE communication

// Re-export the main proxy structure
pub use self::sse_proxy::SSEProxy;

// Re-export common types (remove ToolCallRequest)
pub use self::types::{ServerInfo, ToolInfo, ResourceInfo, SSEMessage, SSEEvent};

// Define submodules
pub mod events;
pub mod http;
pub mod sse_proxy;
pub mod types;