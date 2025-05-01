#![cfg(test)]

use mcp_runner::sse_proxy::events::EventManager;
use mcp_runner::transport::json_rpc::{JSON_RPC_VERSION, JsonRpcResponse};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// Mock server access for testing
struct MockServerAccess {
    servers: HashMap<String, String>,
}

impl MockServerAccess {
    fn new() -> Self {
        let mut servers = HashMap::new();
        servers.insert("test-server".to_string(), "server-id-123".to_string());
        servers.insert("fetch".to_string(), "server-id-456".to_string());
        Self { servers }
    }

    fn get_server_id(&self, name: &str) -> Result<String, String> {
        self.servers
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Server not found: {}", name))
    }
}

// A simulated client to verify tool calls work
struct MockClient {
    calls: Arc<Mutex<Vec<(String, String, serde_json::Value)>>>, // (tool, request_id, args)
    responses: HashMap<String, serde_json::Value>,               // tool -> response
}

impl MockClient {
    fn new() -> Self {
        let mut responses = HashMap::new();
        responses.insert("echo".to_string(), json!({"echoed": true}));
        responses.insert(
            "fetch".to_string(),
            json!({"url": "https://example.com", "content": "Example website content"}),
        );

        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            responses,
        }
    }

    fn call_tool(
        &self,
        tool: &str,
        request_id: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push((tool.to_string(), request_id.to_string(), args));
        }

        if let Some(response) = self.responses.get(tool) {
            Ok(response.clone())
        } else {
            Err(format!("Tool not found: {}", tool))
        }
    }

    fn get_calls(&self) -> Vec<(String, String, serde_json::Value)> {
        self.calls.lock().unwrap().clone()
    }
}

// A test proxy implementation that mimics SSEProxy behavior
struct TestProxy {
    server_access: MockServerAccess,
    clients: HashMap<String, MockClient>,
    event_manager: EventManager,
}

impl TestProxy {
    fn new() -> Self {
        let mut clients = HashMap::new();
        clients.insert("test-server".to_string(), MockClient::new());
        clients.insert("fetch".to_string(), MockClient::new());

        Self {
            server_access: MockServerAccess::new(),
            clients,
            event_manager: EventManager::new(100),
        }
    }

    // Get a reference to a client for checking calls
    fn get_client(&self, name: &str) -> Option<&MockClient> {
        self.clients.get(name)
    }

    fn get_server_id(&self, name: &str) -> Result<String, String> {
        self.server_access.get_server_id(name)
    }

    fn call_tool(
        &self,
        req_id: &str,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<(), String> {
        if let Some(client) = self.clients.get(server) {
            match client.call_tool(tool, req_id, args.clone()) {
                Ok(result) => {
                    // Simulate the async tool call completing and sending an event
                    let server_id = self.get_server_id(server)?;
                    self.event_manager
                        .send_tool_response(req_id, &server_id, tool, result);
                    Ok(())
                }
                Err(e) => {
                    let server_id = self.get_server_id(server)?;
                    self.event_manager
                        .send_tool_error(req_id, &server_id, tool, &e);
                    Err(e)
                }
            }
        } else {
            Err(format!("Server not found: {}", server))
        }
    }

    fn event_manager(&self) -> &EventManager {
        &self.event_manager
    }

    fn list_servers(&self) -> Vec<serde_json::Value> {
        self.server_access
            .servers
            .iter()
            .map(|(name, id)| {
                json!({
                    "name": name,
                    "id": id,
                    "status": "Running"
                })
            })
            .collect()
    }
}

#[test]
fn test_proxy_initialization() {
    let proxy = TestProxy::new();

    // Check server list
    let servers = proxy.list_servers();
    assert_eq!(servers.len(), 2);

    // Check that we can get server IDs
    let test_id = proxy.get_server_id("test-server");
    assert!(test_id.is_ok());
    assert_eq!(test_id.unwrap(), "server-id-123");

    // Check event manager
    let _rx = proxy.event_manager().subscribe();
}

#[tokio::test]
async fn test_proxy_tool_call() {
    let proxy = TestProxy::new();
    let mut event_rx = proxy.event_manager().subscribe();

    // Make a tool call
    let req_id = "tool-call-123";
    let server = "test-server";
    let tool = "echo";
    let args = json!({"message": "Hello, world!"});

    // Call should succeed
    let result = proxy.call_tool(req_id, server, tool, args.clone());
    assert!(result.is_ok());

    // Check that the client received the call
    let client = proxy.get_client(server).unwrap();
    let calls = client.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, tool); // tool name
    assert_eq!(calls[0].1, req_id); // request ID
    assert_eq!(calls[0].2, args); // arguments

    // Check that an event was sent
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv())
        .await
        .unwrap()
        .unwrap();

    // Event should be a "message" type with JSON-RPC format
    assert_eq!(event.event, "message");
    assert_eq!(event.id.unwrap(), req_id);

    // Parse response and verify structure
    let response: JsonRpcResponse = serde_json::from_str(&event.data).unwrap();

    assert_eq!(response.jsonrpc, JSON_RPC_VERSION);
    assert_eq!(response.id, json!(req_id));
    assert!(response.result.is_some());
    assert!(response.error.is_none());
}

#[tokio::test]
async fn test_proxy_tool_error() {
    let proxy = TestProxy::new();
    let mut event_rx = proxy.event_manager().subscribe();

    // Call a non-existent tool
    let req_id = "error-call-456";
    let server = "test-server";
    let tool = "non_existent_tool"; // This doesn't exist in our mock responses
    let args = json!({});

    // Call should fail and send error event
    let result = proxy.call_tool(req_id, server, tool, args);
    assert!(result.is_err());

    // Check that an error event was sent
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv())
        .await
        .unwrap()
        .unwrap();

    // Event should be a "message" type with JSON-RPC format
    assert_eq!(event.event, "message");
    assert_eq!(event.id.unwrap(), req_id);

    // Parse response and verify it's an error
    let response: JsonRpcResponse = serde_json::from_str(&event.data).unwrap();

    assert_eq!(response.jsonrpc, JSON_RPC_VERSION);
    assert_eq!(response.id, json!(req_id));
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    let error = response.error.unwrap();
    assert_eq!(error.code, -32000);
    assert!(error.message.contains("not found"));
    assert!(error.data.is_some());
}

#[test]
fn test_server_not_found() {
    let proxy = TestProxy::new();

    // Try to get a non-existent server
    let result = proxy.get_server_id("non_existent_server");
    assert!(result.is_err());

    // Try to call a tool on a non-existent server
    let call_result = proxy.call_tool("req-789", "non_existent_server", "echo", json!({}));
    assert!(call_result.is_err());
}
