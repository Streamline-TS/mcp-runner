use async_trait::async_trait;
use mcp_runner::McpClient; // Updated to use the re-exported McpClient
use mcp_runner::error::{Error, Result};
use mcp_runner::transport::Transport;
use mockall::mock;
use mockall::predicate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Define a mock for the Transport trait
mock! {
    pub TransportMock {}

    #[async_trait]
    impl Transport for TransportMock {
        async fn initialize(&self) -> Result<()>;
        async fn list_tools(&self) -> Result<Vec<Value>>;
        async fn call_tool(&self, name: &str, args: Value) -> Result<Value>;
        async fn list_resources(&self) -> Result<Vec<Value>>;
        async fn get_resource(&self, uri: &str) -> Result<Value>;
    }
}

// Helper function to create a client with a mock transport
fn create_test_client(mock_transport: MockTransportMock) -> McpClient {
    McpClient::new("test".to_string(), mock_transport)
}

#[tokio::test]
async fn test_list_tools() -> Result<()> {
    // Create a mock that expects list_tools and returns a sample response
    let mut mock_transport = MockTransportMock::new();
    mock_transport.expect_list_tools().times(1).returning(|| {
        // Return a sample list of tools
        Ok(vec![serde_json::json!({
            "name": "test_tool",
            "description": "A test tool",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "output": {"type": "string"}
                }
            }
        })])
    });

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Call the method being tested
    let tools = client.list_tools().await?;

    // Assertions
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test_tool");
    assert_eq!(tools[0].description, "A test tool");
    assert!(tools[0].input_schema.is_some());
    assert!(tools[0].output_schema.is_some());

    Ok(())
}

#[tokio::test]
async fn test_call_tool() -> Result<()> {
    // Define test input and expected output
    #[derive(Serialize)]
    struct TestInput {
        message: String,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestOutput {
        response: String,
    }

    // Create a mock transport that expects a specific tool call
    let mut mock_transport = MockTransportMock::new();
    mock_transport
        .expect_call_tool()
        .with(eq("echo"), eq(serde_json::json!({"message": "hello"})))
        .times(1)
        .returning(|_, _| {
            Ok(serde_json::json!({
                "response": "hello from echo"
            }))
        });

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Execute the test
    let input = TestInput {
        message: "hello".to_string(),
    };
    let output: TestOutput = client.call_tool("echo", &input).await?;

    // Verify the result
    assert_eq!(
        output,
        TestOutput {
            response: "hello from echo".to_string()
        }
    );

    Ok(())
}

#[tokio::test]
async fn test_list_resources() -> Result<()> {
    // Create a mock transport that returns sample resources
    let mut mock_transport = MockTransportMock::new();
    mock_transport
        .expect_list_resources()
        .times(1)
        .returning(|| {
            Ok(vec![serde_json::json!({
                "uri": "resource:test",
                "name": "test_resource",
                "description": "A test resource",
                "type": "text"
            })])
        });

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Call the method being tested
    let resources = client.list_resources().await?;

    // Assertions
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].uri, "resource:test");
    assert_eq!(resources[0].name, "test_resource");
    assert_eq!(
        resources[0].description,
        Some("A test resource".to_string())
    );
    assert_eq!(resources[0].resource_type, Some("text".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_get_resource() -> Result<()> {
    // Define expected resource output structure
    #[derive(Deserialize, Debug, PartialEq)]
    struct TestResource {
        content: String,
        metadata: TestResourceMetadata,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestResourceMetadata {
        created: String,
    }

    // Create a mock transport that expects a specific resource request
    let mut mock_transport = MockTransportMock::new();
    mock_transport
        .expect_get_resource()
        .with(eq("resource:test"))
        .times(1)
        .returning(|_| {
            Ok(serde_json::json!({
                "content": "Sample content",
                "metadata": {
                    "created": "2025-04-23T12:00:00Z"
                }
            }))
        });

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Execute the test
    let resource: TestResource = client.get_resource("resource:test").await?;

    // Verify the result
    assert_eq!(
        resource,
        TestResource {
            content: "Sample content".to_string(),
            metadata: TestResourceMetadata {
                created: "2025-04-23T12:00:00Z".to_string()
            }
        }
    );

    Ok(())
}

#[tokio::test]
async fn test_initialize() -> Result<()> {
    // Create a mock that expects initialize to be called once
    let mut mock_transport = MockTransportMock::new();
    mock_transport
        .expect_initialize()
        .times(1)
        .returning(|| Ok(()));

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Call the initialize method
    client.initialize().await?;

    // The test passes if the mock's expectations are met
    Ok(())
}

#[tokio::test]
async fn test_serialization_error() -> Result<()> {
    use std::collections::HashMap;

    // Create a type that will fail to serialize
    struct Unserializable;

    impl Serialize for Unserializable {
        fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("Test serialization error"))
        }
    }

    let mock_transport = MockTransportMock::new();
    // No expectations set - we don't expect call_tool to be called

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Call should fail with serialization error
    let result = client
        .call_tool::<_, HashMap<String, String>>("test", &Unserializable)
        .await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(
            e.to_string().contains("serialization"),
            "Expected serialization error, got: {}",
            e
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_deserialization_error() -> Result<()> {
    // Create a mock that returns an incompatible result
    let mut mock_transport = MockTransportMock::new();
    mock_transport.expect_call_tool().returning(|_, _| {
        // Return a value that can't be deserialized to the expected type
        Ok(serde_json::json!({
            "invalid": "structure"
        }))
    });

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Define a struct that won't match the returned JSON
    #[derive(Deserialize)]
    #[allow(dead_code)] // We intentionally never read this field; it's just for testing deserialization
    struct ExpectedOutput {
        required_field: String,
    }

    // Call should fail with deserialization error
    let result: std::result::Result<ExpectedOutput, _> =
        client.call_tool("test", &serde_json::json!({})).await;

    assert!(result.is_err());
    if let Err(e) = result {
        assert!(
            e.to_string().contains("deserialize"),
            "Expected deserialization error, got: {}",
            e
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_transport_error_propagation() -> Result<()> {
    // Create a mock that simulates a transport error
    let mut mock_transport = MockTransportMock::new();
    mock_transport
        .expect_list_tools()
        .returning(|| Err(Error::Communication("Transport error".to_string())));

    // Create the client with the mock transport
    let client = create_test_client(mock_transport);

    // Call should propagate the transport error
    let result = client.list_tools().await;

    assert!(result.is_err());
    if let Err(e) = result {
        // Check if the error is a Communication error without using matches! macro
        if let Error::Communication(msg) = &e {
            assert!(msg.contains("Transport error"));
        } else {
            panic!("Expected Communication error, got: {:?}", e);
        }
    }

    Ok(())
}
