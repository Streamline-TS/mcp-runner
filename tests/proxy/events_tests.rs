#![cfg(test)]

use mcp_runner::proxy::events::EventManager;
use mcp_runner::proxy::types::SSEMessage;
use mcp_runner::server::ServerId;
use mcp_runner::server::ServerProcess;
use mcp_runner::config::ServerConfig;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

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

#[test]
fn test_event_manager_creation() {
    let capacity = 10;
    let event_manager = EventManager::new(capacity);

    // Test that we can successfully create a subscription
    let _receiver = event_manager.subscribe();
}

#[tokio::test]
async fn test_event_subscription_receiving() {
    let capacity = 10;
    let event_manager = EventManager::new(capacity);
    let mut rx = event_manager.subscribe();

    // Send a tool response event
    let request_id = "req-123";
    let server_id = "server-1";
    let tool_name = "test-tool";
    let response = json!({"result": true});

    event_manager.send_tool_response(request_id, server_id, tool_name, response);

    // Try to receive the message with a timeout
    let received_message = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;

    // Check that we got a message
    assert!(
        received_message.is_ok(),
        "Should receive a message within timeout"
    );
    if let Ok(result) = received_message {
        assert!(result.is_ok(), "Should receive a valid message");
        if let Ok(message) = result {
            assert_eq!(message.event, "tool-response");
            assert!(message.data.contains("\"server_id\":\"server-1\""));
            assert!(message.data.contains("\"tool_name\":\"test-tool\""));
            assert!(message.data.contains("\"result\":true"));
            assert_eq!(message.id, Some(request_id.to_string()));
        }
    }
}

#[tokio::test]
async fn test_tool_error_event() {
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Send a tool error event
    let request_id = "err-req-456";
    let server_id = "server-2";
    let tool_name = "failing-tool";
    let error_message = "Operation failed";

    event_manager.send_tool_error(request_id, server_id, tool_name, error_message);

    // Receive and verify the message
    let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(received.event, "tool-error");
    assert!(received.data.contains("\"error\":\"Operation failed\""));
    assert_eq!(received.id, Some(request_id.to_string()));
}

#[tokio::test]
async fn test_server_status_event() {
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Create a ServerId for testing
    let server_id = create_test_server_id();
    let server_name = "TestServer";
    let status = "starting";

    event_manager.send_status_update(server_id, server_name, status);

    // Receive and verify the message
    let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(received.event, "server-status");
    assert!(received.data.contains("\"server_name\":\"TestServer\""));
    assert!(received.data.contains("\"status\":\"starting\""));
    assert_eq!(received.id, None); // Status events don't have an ID
}

// A simplified test of the SSEMessage format method
#[test]
fn test_sse_message_format() {
    let message = SSEMessage::new("test-event", "test-data", Some("test-id"));
    let formatted = message.format();

    assert!(formatted.contains("id: test-id\n"));
    assert!(formatted.contains("event: test-event\n"));
    assert!(formatted.contains("data: test-data\n\n"));
}
