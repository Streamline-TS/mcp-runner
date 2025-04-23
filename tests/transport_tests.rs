use mcp_runner::transport::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use serde_json::json;

#[test]
fn test_json_rpc_request_serialization() {
    // Create a JSON-RPC request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(123),
        method: "test_method".to_string(),
        params: Some(json!({"key": "value"})),
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&request).unwrap();
    
    // Expected JSON string
    let expected = r#"{"jsonrpc":"2.0","id":123,"method":"test_method","params":{"key":"value"}}"#;
    
    assert_eq!(json, expected);
}

#[test]
fn test_json_rpc_request_deserialization() {
    // JSON-RPC request as string
    let json = r#"{"jsonrpc":"2.0","id":123,"method":"test_method","params":{"key":"value"}}"#;
    
    // Deserialize
    let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
    
    // Verify fields
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.id, json!(123));
    assert_eq!(request.method, "test_method");
    assert_eq!(request.params, Some(json!({"key": "value"})));
}

#[test]
fn test_json_rpc_response_serialization() {
    // Create a successful JSON-RPC response
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(123),
        result: Some(json!({"answer": 42})),
        error: None,
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&response).unwrap();
    
    // Expected JSON string
    let expected = r#"{"jsonrpc":"2.0","id":123,"result":{"answer":42}}"#;
    
    assert_eq!(json, expected);
    
    // Create an error JSON-RPC response with error details
    // We'll use the JSON directly since JsonRpcError doesn't implement PartialEq
    let error_response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(456),
        result: None,
        error: Some(serde_json::from_str(r#"{"code":-32600,"message":"Invalid Request"}"#).unwrap()),
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&error_response).unwrap();
    
    // Expected JSON string
    let expected = r#"{"jsonrpc":"2.0","id":456,"error":{"code":-32600,"message":"Invalid Request"}}"#;
    
    assert_eq!(json, expected);
}

#[test]
fn test_json_rpc_response_deserialization() {
    // Success response as string
    let json = r#"{"jsonrpc":"2.0","id":123,"result":{"answer":42}}"#;
    
    // Deserialize
    let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
    
    // Verify fields
    assert_eq!(response.jsonrpc, "2.0");
    assert_eq!(response.id, json!(123));
    assert_eq!(response.result, Some(json!({"answer": 42})));
    assert!(response.error.is_none());
    
    // Error response as string
    let json = r#"{"jsonrpc":"2.0","id":456,"error":{"code":-32600,"message":"Invalid Request"}}"#;
    
    // Deserialize
    let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
    
    // Verify fields
    assert_eq!(response.jsonrpc, "2.0");
    assert_eq!(response.id, json!(456));
    assert!(response.result.is_none());
    assert!(response.error.is_some());
    
    // Since JsonRpcError doesn't implement PartialEq, check its fields individually
    if let Some(error) = response.error {
        assert_eq!(error.code, -32600);
        assert_eq!(error.message, "Invalid Request");
    } else {
        panic!("Expected error to be Some");
    }
}

#[test]
fn test_json_rpc_message_enum() {
    // Create a request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(123),
        method: "test_method".to_string(),
        params: Some(json!({"key": "value"})),
    };
    
    // Wrap in message enum
    let message = JsonRpcMessage::Request(request.clone());
    
    // Check type
    match &message {
        JsonRpcMessage::Request(r) => {
            assert_eq!(r.jsonrpc, request.jsonrpc);
            assert_eq!(r.id, request.id);
            assert_eq!(r.method, request.method);
            assert_eq!(r.params, request.params);
        },
        _ => panic!("Expected Request variant"),
    }
    
    // Create a response
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: json!(123),
        result: Some(json!({"answer": 42})),
        error: None,
    };
    
    // Wrap in message enum
    let message = JsonRpcMessage::Response(response.clone());
    
    // Check type
    match &message {
        JsonRpcMessage::Response(r) => {
            assert_eq!(r.jsonrpc, response.jsonrpc);
            assert_eq!(r.id, response.id);
            assert_eq!(r.result, response.result);
            
            // For the error field, just check if both are None or both are Some
            assert_eq!(r.error.is_some(), response.error.is_some());
        },
        _ => panic!("Expected Response variant"),
    }
}

// Additional tests for edge cases and error handling
#[test]
fn test_json_rpc_request_without_params() {
    // Some methods don't require parameters
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(123),
        method: "no_params_method".to_string(),
        params: None,
    };
    
    // Serialize to JSON
    let json = serde_json::to_string(&request).unwrap();
    
    // Expected JSON string (no params field)
    let expected = r#"{"jsonrpc":"2.0","id":123,"method":"no_params_method"}"#;
    
    assert_eq!(json, expected);
}

#[test]
fn test_json_rpc_response_with_error_data() {
    // Create an error with additional data
    let response = serde_json::from_str::<JsonRpcResponse>(
        r#"{
            "jsonrpc": "2.0",
            "id": null,
            "error": {
                "code": -32700,
                "message": "Parse error",
                "data": {
                    "line": 1,
                    "column": 34
                }
            }
        }"#
    ).unwrap();
    
    // Instead of comparing JSON strings, which might have different field orders,
    // verify each field individually
    assert_eq!(response.jsonrpc, "2.0");
    assert_eq!(response.id, serde_json::Value::Null);
    assert!(response.result.is_none());
    assert!(response.error.is_some());
    
    if let Some(error) = response.error {
        assert_eq!(error.code, -32700);
        assert_eq!(error.message, "Parse error");
        assert!(error.data.is_some());
        
        if let Some(data) = error.data {
            assert_eq!(data["line"], 1);
            assert_eq!(data["column"], 34);
        } else {
            panic!("Expected data to be Some");
        }
    } else {
        panic!("Expected error to be Some");
    }
}

#[test]
fn test_json_rpc_factory_methods() {
    // Test factory methods for creating requests
    let list_tools = JsonRpcRequest::list_tools(1);
    assert_eq!(list_tools.method, "tools/list");
    assert_eq!(list_tools.params, None);
    
    // Test call_tool
    let call_tool = JsonRpcRequest::call_tool(2, "echo", json!({"message": "hello"}));
    assert_eq!(call_tool.method, "tools/call");
    assert!(call_tool.params.is_some());
    if let Some(params) = call_tool.params {
        assert_eq!(params["name"], "echo");
        assert_eq!(params["arguments"]["message"], "hello");
    } else {
        panic!("Expected params to be Some");
    }
    
    // Test list_resources
    let list_resources = JsonRpcRequest::list_resources(3);
    assert_eq!(list_resources.method, "resources/list");
    assert_eq!(list_resources.params, None);
    
    // Test get_resource
    let get_resource = JsonRpcRequest::get_resource(4, "resource:test");
    assert_eq!(get_resource.method, "resources/get");
    assert!(get_resource.params.is_some());
    if let Some(params) = get_resource.params {
        assert_eq!(params["uri"], "resource:test");
    } else {
        panic!("Expected params to be Some");
    }
    
    // Test response factory methods
    let success = JsonRpcResponse::success(5, json!({"result": "ok"}));
    assert_eq!(success.id, json!(5));
    assert!(success.result.is_some());
    assert!(success.error.is_none());
    assert_eq!(success.result.unwrap()["result"], "ok");
    
    // Test error factory method
    let error = JsonRpcResponse::error(6, -32601, "Method not found", None);
    assert_eq!(error.id, json!(6));
    assert!(error.result.is_none());
    assert!(error.error.is_some());
    
    // Check error fields individually
    if let Some(err) = error.error {
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
        assert!(err.data.is_none());
    } else {
        panic!("Expected error to be Some");
    }
}