#![cfg(test)]

use mcp_runner::sse_proxy::events::EventManager;
use mcp_runner::sse_proxy::types::SSEMessage;
use mcp_runner::transport::json_rpc::{JSON_RPC_VERSION, JsonRpcResponse};
use serde_json::json;
use std::time::Duration;

#[test]
fn test_event_manager_creation() {
    let capacity = 10;
    let event_manager = EventManager::new(capacity);

    // Test that we can successfully create a subscription
    let _receiver = event_manager.subscribe();
}

#[tokio::test]
async fn test_tool_response_as_jsonrpc() {
    // Create event manager and subscribe
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Test data
    let request_id = "req-123";
    let server_id = "server-1";
    let tool_name = "test-tool";
    let response_data = json!({"result": "success", "value": 42});

    // Send the tool response
    event_manager.send_tool_response(request_id, server_id, tool_name, response_data.clone());

    // Receive the message with timeout
    let received_message = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        received_message.is_ok(),
        "Should receive a message within timeout"
    );

    // Unwrap and verify message structure
    let message = received_message.unwrap().unwrap();

    // The message should be using the "message" event type not "tool-response"
    assert_eq!(message.event, "message", "Event type should be 'message'");

    // The message ID should match our request ID
    assert_eq!(
        message.id,
        Some(request_id.to_string()),
        "Message ID should match request ID"
    );

    // Parse the data field as a JSON-RPC response
    let json_rpc: JsonRpcResponse =
        serde_json::from_str(&message.data).expect("Data should be a valid JSON-RPC response");

    // Verify the JSON-RPC response structure
    assert_eq!(
        json_rpc.jsonrpc, JSON_RPC_VERSION,
        "JSON-RPC version should match"
    );
    assert_eq!(
        json_rpc.id,
        json!(request_id),
        "JSON-RPC ID should match request ID"
    );
    assert!(json_rpc.result.is_some(), "Result should be present");
    // Since send_tool_response extracts the "result" field from the response_data
    // we should only be comparing the inner data, which is response_data["result"]
    assert_eq!(
        json_rpc.result.unwrap(),
        response_data["result"],
        "Result data should match"
    );
    assert!(
        json_rpc.error.is_none(),
        "Error should not be present in a success response"
    );
}

#[tokio::test]
async fn test_tool_error_as_jsonrpc() {
    // Create event manager and subscribe
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Test data
    let request_id = "err-456";
    let server_id = "server-2";
    let tool_name = "failing-tool";
    let error_msg = "Test error message";

    // Send the tool error
    event_manager.send_tool_error(request_id, server_id, tool_name, error_msg);

    // Receive the message with timeout
    let received_message = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        received_message.is_ok(),
        "Should receive a message within timeout"
    );

    // Unwrap and verify message structure
    let message = received_message.unwrap().unwrap();

    // The message should be using the "message" event type not "tool-error"
    assert_eq!(message.event, "message", "Event type should be 'message'");

    // The message ID should match our request ID
    assert_eq!(
        message.id,
        Some(request_id.to_string()),
        "Message ID should match request ID"
    );

    // Parse the data field as a JSON-RPC response
    let json_rpc: JsonRpcResponse =
        serde_json::from_str(&message.data).expect("Data should be a valid JSON-RPC response");

    // Verify the JSON-RPC error response structure
    assert_eq!(
        json_rpc.jsonrpc, JSON_RPC_VERSION,
        "JSON-RPC version should match"
    );
    assert_eq!(
        json_rpc.id,
        json!(request_id),
        "JSON-RPC ID should match request ID"
    );
    assert!(
        json_rpc.result.is_none(),
        "Result should not be present in an error response"
    );
    assert!(json_rpc.error.is_some(), "Error should be present");

    // Verify error details
    let error = json_rpc.error.unwrap();
    assert_eq!(error.code, -32000, "Error code should match");
    assert_eq!(error.message, error_msg, "Error message should match");
    assert!(error.data.is_some(), "Error data should be present");

    // Verify the error data includes server and tool information
    let error_data = error.data.unwrap();
    assert_eq!(
        error_data["serverId"],
        json!(server_id),
        "Server ID should match"
    );
    assert_eq!(
        error_data["toolName"],
        json!(tool_name),
        "Tool name should match"
    );
}

#[tokio::test]
async fn test_server_status_event() {
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Test data
    let server_id = "server-3";
    let server_name = "test-server";
    let status = "running";

    // Send status update
    event_manager.send_server_status(server_name, server_id, status);

    // Receive and verify the message
    let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();

    // Server status events still use their specific event type
    assert_eq!(received.event, "server-status");
    assert_eq!(
        received.id, None,
        "Server status events should not have an ID"
    );

    // Check data contents
    let data = serde_json::from_str::<serde_json::Value>(&received.data).unwrap();
    assert_eq!(data["server_id"], json!(server_id));
    assert_eq!(data["server_name"], json!(server_name));
    assert_eq!(data["status"], json!(status));
}

#[tokio::test]
async fn test_notification_event() {
    let event_manager = EventManager::new(10);
    let mut rx = event_manager.subscribe();

    // Test data
    let title = "Test Notification";
    let message = "This is a test notification";
    let level = "info";

    // Send notification
    event_manager.send_notification(title, message, level);

    // Receive and verify the message
    let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .unwrap()
        .unwrap();

    // Notification events still use their specific event type
    assert_eq!(received.event, "notification");
    assert_eq!(
        received.id, None,
        "Notification events should not have an ID"
    );

    // Check data contents
    let data = serde_json::from_str::<serde_json::Value>(&received.data).unwrap();
    assert_eq!(data["title"], json!(title));
    assert_eq!(data["message"], json!(message));
    assert_eq!(data["level"], json!(level));
}

#[test]
fn test_sse_message_format() {
    // Test a standard message with ID
    let message = SSEMessage::new("test-event", "test-data", Some("test-id"));
    let formatted = EventManager::format_sse_message(&message);
    let formatted_str = std::str::from_utf8(&formatted).unwrap();

    assert!(formatted_str.contains("id: test-id\n"));
    assert!(formatted_str.contains("event: test-event\n"));
    assert!(formatted_str.contains("data: test-data\n\n"));

    // Test a message without ID
    let message_no_id = SSEMessage::new("event-only", "some-data", None);
    let formatted_no_id = EventManager::format_sse_message(&message_no_id);
    let formatted_no_id_str = std::str::from_utf8(&formatted_no_id).unwrap();

    assert!(!formatted_no_id_str.contains("id:"));
    assert!(formatted_no_id_str.contains("event: event-only\n"));
    assert!(formatted_no_id_str.contains("data: some-data\n\n"));
}
