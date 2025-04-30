#![cfg(test)]

use mcp_runner::config::SSEProxyConfig;
use mcp_runner::config::ServerConfig;
use mcp_runner::error::Error;
use mcp_runner::proxy::sse_proxy::{SSEProxy, SSEProxyRunnerAccess};
use mcp_runner::server::{ServerId, ServerProcess};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

// Helper to get a valid ServerId from a server process - since ServerId::new() is private
fn create_test_server_id() -> ServerId {
    let config = ServerConfig {
        command: "echo".to_string(),
        args: vec!["test".to_string()],
        env: HashMap::new(),
    };
    let server = ServerProcess::new("test-server".to_string(), config);
    server.id() // Use the ID from the test server process
}

// Helper functions to replace redundant closures
fn get_none_allowed_servers() -> Option<Vec<String>> {
    None
}

fn get_empty_server_keys() -> Vec<String> {
    vec![]
}

// This is a simplified test that mocks just enough to create an SSEProxy instance
#[tokio::test]
async fn test_sse_proxy_creation() {
    // Create a simple config
    let config = SSEProxyConfig {
        address: "127.0.0.1".to_string(),
        port: 8090,
        authenticate: None,
        allowed_servers: None,
    };

    // Create mock runner access functions
    let runner_access = SSEProxyRunnerAccess {
        get_server_id: Arc::new(|_| Ok(create_test_server_id())),
        get_client: Arc::new(|_| Err(Error::ServerNotFound("Test".into()))),
        get_allowed_servers: Arc::new(get_none_allowed_servers),
        get_server_config_keys: Arc::new(get_empty_server_keys),
    };

    // Create communication channel
    let (_server_tx, server_rx) = mpsc::channel(32);

    // Create the SSE proxy
    let proxy = SSEProxy::new(runner_access, config, server_rx);

    // Verify basic properties
    assert_eq!(proxy.config().port, 8090);
    assert_eq!(proxy.config().address, "127.0.0.1");
}
