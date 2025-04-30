#![cfg(test)]

use mcp_runner::proxy::error_helpers;
use serde_json::json;

// Test the helper function for extracting string params
#[test]
fn test_get_string_param() {
    let params = json!({
        "name": "Alice",
        "age": 30,
        "city": null,
        "country": "Wonderland"
    });

    // Test successful string extraction
    assert_eq!(
        error_helpers::get_string_param(&params, "name"),
        Some("Alice".to_string())
    );
    assert_eq!(
        error_helpers::get_string_param(&params, "country"),
        Some("Wonderland".to_string())
    );

    // Test non-string values
    assert_eq!(error_helpers::get_string_param(&params, "age"), None); // Not a string
    assert_eq!(error_helpers::get_string_param(&params, "city"), None); // Null is not a string

    // Test missing keys
    assert_eq!(
        error_helpers::get_string_param(&params, "missing_key"),
        None
    );

    // Test with empty object
    let params_empty = json!({});
    assert_eq!(error_helpers::get_string_param(&params_empty, "name"), None);

    // Test with non-object JSON
    let params_not_object = json!("not an object");
    assert_eq!(
        error_helpers::get_string_param(&params_not_object, "name"),
        None
    );
}

// The error sending functions require TCP stream/writer mocking
// A full test suite would use a proper mocking framework like mockall
// For illustration, we'll include simpler test cases that can run without mocking

#[test]
fn test_jsonrpc_error_structure() {
    // Test that the JsonRpcResponse::error function creates the correct structure
    // This indirectly tests the send_jsonrpc_error function's payload structure

    let id = json!(123);
    let code = -32700;
    let message = "Parse error";
    let data = Some(json!({"line": 5, "column": 10}));

    let response = mcp_runner::transport::json_rpc::JsonRpcResponse::error(
        id.clone(),
        code,
        message,
        data.clone(),
    );

    // Verify the structure
    assert_eq!(response.id, id);
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    if let Some(error) = response.error {
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(error.data, data);
    }
}
