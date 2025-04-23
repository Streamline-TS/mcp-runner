use async_trait::async_trait;
use mcp_runner::error::{Error, Result};
use mcp_runner::transport::Transport;
use serde_json::{Value, json};
use std::io::{self};
use std::sync::{Arc, Mutex};

/// MockStdio simulates stdin/stdout interactions for testing without actual process I/O.
///
/// This struct allows tests to provide predefined responses and capture sent messages,
/// making it possible to test the StdioTransport implementation without spawning actual
/// processes.
///
/// # Fields
///
/// * `recv_buffer` - A buffer of mock responses to return when reading
/// * `sent_messages` - A record of messages sent to this mock
struct MockStdio {
    recv_buffer: Vec<String>,
    sent_messages: Vec<String>,
}

impl MockStdio {
    /// Creates a new MockStdio with a predefined set of responses.
    ///
    /// # Arguments
    ///
    /// * `recv_buffer` - A vector of strings that will be returned in reverse order
    ///                    when `read_line` is called.
    ///
    /// # Returns
    ///
    /// A new `MockStdio` instance
    fn new(recv_buffer: Vec<String>) -> Self {
        Self {
            recv_buffer,
            sent_messages: Vec::new(),
        }
    }

    /// Simulates reading a line from stdout.
    ///
    /// This method pops a string from the recv_buffer and returns it,
    /// or returns an IO error if the buffer is empty.
    ///
    /// # Returns
    ///
    /// An `io::Result<String>` containing either the next mock response or an error
    fn read_line(&mut self) -> io::Result<String> {
        if let Some(line) = self.recv_buffer.pop() {
            Ok(line)
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "No more mock input",
            ))
        }
    }

    /// Simulates writing a line to stdin.
    ///
    /// This method records the provided line in the sent_messages vector
    /// for later inspection.
    ///
    /// # Arguments
    ///
    /// * `line` - The line to record as being written
    ///
    /// # Returns
    ///
    /// An `io::Result<()>` that is always Ok in this mock implementation
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.sent_messages.push(line.to_string());
        Ok(())
    }

    /// Gets the messages that have been "sent" to this mock.
    ///
    /// # Returns
    ///
    /// A vector of strings that were passed to `write_line`
    fn get_sent_messages(&self) -> Vec<String> {
        self.sent_messages.clone()
    }
}

/// TestableStdioTransport is a test implementation of the Transport trait.
///
/// This struct wraps a MockStdio to provide a testable implementation of the
/// Transport trait that can be used in unit tests without requiring actual process I/O.
/// It allows tests to verify that correct JSON-RPC requests are sent and to provide
/// mock responses.
///
/// # Example
///
/// ```no_run
/// #[tokio::test]
/// async fn test_list_tools() {
///     // Create mock responses
///     let mock_responses = vec![
///         r#"{"jsonrpc":"2.0","id":"1","result":{"tools":[]}}"#.to_string(),
///     ];
///     
///     // Create transport with mocks
///     let transport = TestableStdioTransport::new(mock_responses);
///     
///     // Call the method and verify results
///     let tools = transport.list_tools().await.unwrap();
///     assert_eq!(tools.len(), 0);
///     
///     // Verify the request format
///     let sent = transport.get_sent_messages();
///     // Assert on sent messages...
/// }
/// ```
struct TestableStdioTransport {
    mock_stdio: Arc<Mutex<MockStdio>>,
}

impl TestableStdioTransport {
    fn new(mock_responses: Vec<String>) -> Self {
        Self {
            mock_stdio: Arc::new(Mutex::new(MockStdio::new(mock_responses))),
        }
    }

    fn get_sent_messages(&self) -> Vec<String> {
        self.mock_stdio.lock().unwrap().get_sent_messages()
    }
}

#[async_trait]
impl Transport for TestableStdioTransport {
    async fn initialize(&self) -> Result<()> {
        // Simple pass-through that doesn't do anything for testing
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<Value>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "listTools",
            "params": {}
        });

        let request_str = match serde_json::to_string(&request) {
            Ok(s) => s,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Send request to mock stdout
        {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.write_line(&request_str)?;
        }

        // Read response from mock stdin
        let response_str = {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.read_line()?
        };

        // Parse the response
        let response: Value = match serde_json::from_str(&response_str) {
            Ok(v) => v,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Check if we have an error
        if let Some(error) = response.get("error") {
            return Err(Error::JsonRpc(format!("JSON-RPC error: {:?}", error)));
        }

        // Extract the tools from the result
        if let Some(result) = response.get("result") {
            if let Some(tools) = result.get("tools") {
                if let Some(tools_array) = tools.as_array() {
                    return Ok(tools_array.clone());
                }
            }
        }

        Err(Error::JsonRpc(
            "Invalid listTools response format".to_string(),
        ))
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "callTool",
            "params": {
                "name": name,
                "args": args
            }
        });

        let request_str = match serde_json::to_string(&request) {
            Ok(s) => s,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Send request to mock stdout
        {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.write_line(&request_str)?;
        }

        // Read response from mock stdin
        let response_str = {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.read_line()?
        };

        // Parse the response
        let response: Value = match serde_json::from_str(&response_str) {
            Ok(v) => v,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Check if we have an error
        if let Some(error) = response.get("error") {
            return Err(Error::JsonRpc(format!("JSON-RPC error: {:?}", error)));
        }

        // Extract the result
        if let Some(result) = response.get("result") {
            return Ok(result.clone());
        }

        Err(Error::JsonRpc(
            "Invalid callTool response format".to_string(),
        ))
    }

    async fn list_resources(&self) -> Result<Vec<Value>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "listResources",
            "params": {}
        });

        let request_str = match serde_json::to_string(&request) {
            Ok(s) => s,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Send request to mock stdout
        {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.write_line(&request_str)?;
        }

        // Read response from mock stdin
        let response_str = {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.read_line()?
        };

        // Parse the response
        let response: Value = match serde_json::from_str(&response_str) {
            Ok(v) => v,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Check if we have an error
        if let Some(error) = response.get("error") {
            return Err(Error::JsonRpc(format!("JSON-RPC error: {:?}", error)));
        }

        // Extract the resources from the result
        if let Some(result) = response.get("result") {
            if let Some(resources) = result.get("resources") {
                if let Some(resources_array) = resources.as_array() {
                    return Ok(resources_array.clone());
                }
            }
        }

        Err(Error::JsonRpc(
            "Invalid listResources response format".to_string(),
        ))
    }

    async fn get_resource(&self, uri: &str) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "getResource",
            "params": {
                "uri": uri
            }
        });

        let request_str = match serde_json::to_string(&request) {
            Ok(s) => s,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Send request to mock stdout
        {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.write_line(&request_str)?;
        }

        // Read response from mock stdin
        let response_str = {
            let mut stdio = self.mock_stdio.lock().unwrap();
            stdio.read_line()?
        };

        // Parse the response
        let response: Value = match serde_json::from_str(&response_str) {
            Ok(v) => v,
            Err(e) => return Err(Error::Serialization(e.to_string())),
        };

        // Check if we have an error
        if let Some(error) = response.get("error") {
            return Err(Error::JsonRpc(format!("JSON-RPC error: {:?}", error)));
        }

        // Extract the result
        if let Some(result) = response.get("result") {
            if let Some(resource) = result.get("resource") {
                return Ok(resource.clone());
            }
        }

        Err(Error::JsonRpc(
            "Invalid getResource response format".to_string(),
        ))
    }
}

#[tokio::test]
async fn test_list_tools_request_format() {
    // Prepare mock response
    let mock_responses = vec![
        r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"tool1","description":"First tool"}]}}"#.to_string(),
    ];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call the method
    let _ = transport.list_tools().await;

    // Check the format of the request sent
    let sent_messages = transport.get_sent_messages();
    assert_eq!(sent_messages.len(), 1);

    // Parse the sent message to verify format
    let sent_request: Value = serde_json::from_str(&sent_messages[0]).unwrap();
    assert_eq!(sent_request["jsonrpc"], "2.0");
    assert_eq!(sent_request["method"], "listTools");
    assert!(sent_request["params"].is_object());
    assert_eq!(sent_request["id"], 1);
}

#[tokio::test]
async fn test_call_tool_request_format() {
    // Prepare mock response
    let mock_responses =
        vec![r#"{"jsonrpc":"2.0","id":2,"result":{"output":"success"}}"#.to_string()];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call the method
    let _ = transport
        .call_tool("test-tool", json!({"param": "value"}))
        .await;

    // Check the format of the request sent
    let sent_messages = transport.get_sent_messages();
    assert_eq!(sent_messages.len(), 1);

    // Parse the sent message to verify format
    let sent_request: Value = serde_json::from_str(&sent_messages[0]).unwrap();
    assert_eq!(sent_request["jsonrpc"], "2.0");
    assert_eq!(sent_request["method"], "callTool");
    assert_eq!(sent_request["params"]["name"], "test-tool");
    assert_eq!(sent_request["params"]["args"]["param"], "value");
    assert_eq!(sent_request["id"], 2);
}

#[tokio::test]
async fn test_list_resources_request_format() {
    // Prepare mock response
    let mock_responses = vec![
        r#"{"jsonrpc":"2.0","id":3,"result":{"resources":[{"uri":"res:1","name":"First Resource"}]}}"#.to_string(),
    ];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call the method
    let _ = transport.list_resources().await;

    // Check the format of the request sent
    let sent_messages = transport.get_sent_messages();
    assert_eq!(sent_messages.len(), 1);

    // Parse the sent message to verify format
    let sent_request: Value = serde_json::from_str(&sent_messages[0]).unwrap();
    assert_eq!(sent_request["jsonrpc"], "2.0");
    assert_eq!(sent_request["method"], "listResources");
    assert!(sent_request["params"].is_object());
    assert_eq!(sent_request["id"], 3);
}

#[tokio::test]
async fn test_get_resource_request_format() {
    // Prepare mock response
    let mock_responses = vec![
        r#"{"jsonrpc":"2.0","id":4,"result":{"resource":{"content":"test content"}}}"#.to_string(),
    ];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call the method
    let _ = transport.get_resource("res:test").await;

    // Check the format of the request sent
    let sent_messages = transport.get_sent_messages();
    assert_eq!(sent_messages.len(), 1);

    // Parse the sent message to verify format
    let sent_request: Value = serde_json::from_str(&sent_messages[0]).unwrap();
    assert_eq!(sent_request["jsonrpc"], "2.0");
    assert_eq!(sent_request["method"], "getResource");
    assert_eq!(sent_request["params"]["uri"], "res:test");
    assert_eq!(sent_request["id"], 4);
}

#[tokio::test]
async fn test_error_handling() {
    // Prepare mock error response
    let mock_responses = vec![
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#
            .to_string(),
    ];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call should return an error
    let result = transport.list_tools().await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.to_string().contains("JSON-RPC error"));
        assert!(e.to_string().contains("Method not found"));
    }
}

#[tokio::test]
async fn test_malformed_response() {
    // Prepare invalid response
    let mock_responses = vec![
        r#"{"jsonrpc":"2.0","id":1,"result":{"not_tools":[]}}"#.to_string(), // Missing "tools" field
    ];

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call should return a protocol error
    let result = transport.list_tools().await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.to_string().contains("Invalid listTools response format"));
    }
}

#[tokio::test]
async fn test_io_error() {
    // Create an empty response buffer to cause an IO error when reading
    let mock_responses = Vec::new();

    // Create transport with mock
    let transport = TestableStdioTransport::new(mock_responses);

    // Call should return an IO error
    let result = transport.list_tools().await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(matches!(e, Error::Io(_)));
        assert!(e.to_string().contains("No more mock input"));
    }
}
