//! HTTP request handlers for the Actix Web-based SSE proxy.
//!
//! This module contains the Actix Web handlers for the various endpoints
//! supported by the SSE proxy, including SSE events streaming, tool calls,
//! and server information retrieval.

use crate::client::McpClient;
use crate::error::{Error, Result};
use crate::server::ServerId;
use crate::sse_proxy::events::EventManager;
use crate::sse_proxy::proxy::SSEProxy;
use crate::sse_proxy::types::{ResourceInfo, ToolInfo};
use crate::transport::json_rpc::{JSON_RPC_VERSION, JsonRpcRequest, JsonRpcResponse};

use actix_web::{
    HttpRequest, HttpResponse, Responder,
    web::{Bytes, Data, Json, Path},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing;

// Define a struct to deserialize the resource response from the client
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceResponse {
    content_type: String,
    // Assuming data is returned as a string (potentially base64 for binary)
    data: String,
}

/// Helper function to validate server, retrieve client and initialize it
/// Returns a tuple of (client, server_id, server_id_str) or an Error
async fn get_validated_client(
    proxy: &SSEProxy,
    server_name: &str,
) -> Result<(McpClient, ServerId, String)> {
    // Check if server is allowed
    if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
        if !allowed_servers.contains(&server_name.to_string()) {
            return Err(Error::Unauthorized(format!(
                "Server '{}' not in allowed list",
                server_name
            )));
        }
    }

    // Get server ID
    let server_id = match (proxy.get_runner_access().get_server_id)(server_name) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(server = %server_name, error = %e, "Server not found");
            return Err(e);
        }
    };
    let server_id_str = format!("{:?}", server_id);

    // Get client
    let client = match (proxy.get_runner_access().get_client)(server_id) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
            return Err(e);
        }
    };

    // Initialize client
    client.initialize().await?;

    Ok((client, server_id, server_id_str))
}

/// Helper function to create a JsonRPC error response
fn create_jsonrpc_error(request_id: Value, code: i32, message: String) -> HttpResponse {
    let error_response = JsonRpcResponse::error(request_id, code, message, None);

    HttpResponse::InternalServerError().json(error_response)
}

/// Request body for tool calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Tool name to call
    pub tool: String,
    /// Server name to call the tool on
    pub server: String,
    /// Arguments to pass to the tool
    pub args: Value,
    /// Request ID for correlation
    #[serde(rename = "requestId")]
    pub request_id: String,
}

/// Main SSE entrypoint handler
///
/// Establishes the primary SSE connection for client-server communication
/// and returns initial configuration with namespaced endpoints for backend MCP servers.
///
/// # Returns
///
/// An HTTP response with an SSE stream and initial configuration
pub async fn sse_main_endpoint(
    event_manager: Data<Arc<EventManager>>,
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    req: HttpRequest,
) -> impl Responder {
    tracing::debug!(
        method = %req.method(),
        path = %req.path(),
        headers = ?req.headers(),
        "Client connected to main SSE entrypoint"
    );

    // Create a receiver from the event manager
    let mut receiver = event_manager.subscribe();

    // Get server information
    let proxy_lock = proxy.lock().await;
    let server_info = proxy_lock.get_server_info().lock().await;

    // Build server endpoints mapping with correct paths
    let mut servers_map = serde_json::Map::new();
    for (_id, info) in server_info.iter() {
        // Use the correct /sse/{server_name} pattern for server endpoints
        let server_endpoint = format!("/sse/{}", info.name);
        servers_map.insert(info.name.clone(), json!(server_endpoint));
        tracing::debug!(
            server_name = %info.name,
            endpoint = %server_endpoint,
            "Adding server endpoint to SSE configuration"
        );
    }
    let servers = Value::Object(servers_map);

    // Define the message URL
    let message_url = "/sse/messages";

    // Log the configuration being sent
    tracing::debug!(
        message_url = %message_url,
        servers = ?servers,
        "Preparing SSE endpoint configuration response"
    );

    // Clone for use in the async block
    let event_manager_clone = Arc::clone(&event_manager);

    // Send initial configuration asynchronously with the correct message_url
    tokio::spawn(async move {
        // Use /sse/messages as the correct message endpoint URL
        event_manager_clone.send_initial_config(message_url, &servers);

        tracing::info!(
            message_url = %message_url,
            servers = ?servers,
            "Sent SSE endpoint configuration to client"
        );
    });

    // Prepare the event stream
    let stream = async_stream::stream! {
        loop {
            // Check for new events
            match receiver.recv().await {
                Ok(msg) => {
                    tracing::debug!(
                        event_type = %msg.event,
                        event_id = ?msg.id,
                        "Sending SSE event to client"
                    );
                    yield Ok::<_, actix_web::Error>(EventManager::format_sse_message(&msg));
                },
                Err(e) => {
                    tracing::error!(error = %e, "Error receiving SSE event");
                    break;
                }
            }
        }
    };

    // Return the HTTP response with the SSE stream
    let response = HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("Access-Control-Allow-Origin", "*"))
        .streaming(stream);

    tracing::debug!("SSE connection response initiated");
    response
}

/// SSE stream handler
///
/// This handler creates and returns an SSE stream for clients to receive events.
///
/// # Returns
///
/// An HTTP response with an SSE stream
pub async fn sse_events(
    event_manager: Data<Arc<EventManager>>,
    _req: HttpRequest,
) -> impl Responder {
    tracing::debug!("Client connected to SSE stream");

    // Create a receiver from the event manager
    let mut receiver = event_manager.subscribe();

    // Prepare the event stream
    let stream = async_stream::stream! {
        // Send an initial event to confirm connection
        let initial_event = json!({
            "type": "notification",
            "title": "Connected",
            "message": "SSE connection established",
            "level": "info"
        });

        if let Ok(data) = serde_json::to_string(&initial_event) {
            let msg = crate::sse_proxy::types::SSEMessage::new("notification", &data, None);
            yield Ok::<_, actix_web::Error>(EventManager::format_sse_message(&msg));
        }

        // Create a heartbeat interval (every 30 seconds)
        let mut heartbeat_interval = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Check for new events
                event = receiver.recv() => {
                    match event {
                        Ok(msg) => {
                            yield Ok::<_, actix_web::Error>(EventManager::format_sse_message(&msg));
                        },
                        Err(e) => {
                            tracing::error!(error = %e, "Error receiving SSE event");
                            break;
                        }
                    }
                },
                // Send heartbeat
                _ = heartbeat_interval.tick() => {
                    yield Ok::<_, actix_web::Error>(Bytes::from(":\n\n")); // Colon comment for heartbeat
                }
            }
        }
    };

    // Return the HTTP response with the SSE stream
    HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("Access-Control-Allow-Origin", "*"))
        .streaming(stream)
}

/// Initialize the connection
///
/// This handler initializes the connection with the client.
///
/// # Returns
///
/// A JSON response with initialization information
pub async fn initialize(_req: HttpRequest) -> impl Responder {
    let response = json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "serverType": "mcp-runner-actix",
    });

    HttpResponse::Ok().json(response)
}

/// Process tool calls
///
/// This handler processes tool calls from clients and streams the response
/// through the SSE channel.
///
/// # Returns
///
/// An HTTP response indicating the tool call was accepted
pub async fn tool_call(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    body: Json<ToolCallRequest>,
) -> impl Responder {
    let request = body.into_inner();
    let tool_name = request.tool.clone();
    let server_name = request.server.clone();
    let args = request.args.clone();
    let request_id = request.request_id.clone();

    tracing::debug!(
        req_id = %request_id,
        server = %server_name,
        tool = %tool_name,
        "Tool call request received"
    );

    // Clone the proxy for async processing
    let proxy_lock = Arc::clone(&proxy);
    // Clone request_id again for use in the async block
    let request_id_clone = request_id.clone();

    // Process the tool call asynchronously
    tokio::spawn(async move {
        // Acquire the lock on the proxy
        let proxy = proxy_lock.lock().await;

        // Process the tool call using the cloned request_id
        if let Err(e) = proxy
            .process_tool_call(&server_name, &tool_name, args, &request_id_clone)
            .await
        {
            tracing::error!(
                req_id = %request_id_clone, // Use cloned request_id here
                server = %server_name,
                tool = %tool_name,
                error = %e,
                "Failed to process tool call"
            );
        }
    });

    // Return a successful response immediately using the original request_id
    HttpResponse::Accepted().json(json!({
        "status": "accepted",
        "requestId": request_id, // Use original request_id here
        "message": "Tool call accepted and processing asynchronously"
    }))
}

/// List available servers
///
/// This handler returns a list of all available servers with their status.
/// Only servers in the allowedServers list (if specified) are included.
///
/// # Returns
///
/// A JSON response with server information
pub async fn list_servers(proxy: Data<Arc<Mutex<SSEProxy>>>) -> impl Responder {
    let proxy = proxy.lock().await;
    let servers = proxy.get_server_info().lock().await;

    // Get the allowed servers list if configured
    let allowed_servers = (proxy.get_runner_access().get_allowed_servers)();

    // Filter servers based on allowedServers if configured
    let server_list: Vec<_> = match &allowed_servers {
        Some(allowed) => servers
            .values()
            .filter(|server| allowed.contains(&server.name))
            .cloned()
            .collect(),
        None => servers.values().cloned().collect(),
    };

    HttpResponse::Ok().json(server_list)
}

/// List tools for a specific server
///
/// This handler returns a list of all tools available on a specific server.
///
/// # Returns
///
/// A JSON response with tool information
pub async fn list_server_tools(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    path: Path<(String,)>,
) -> Result<impl Responder> {
    let server_name = &path.0;
    let proxy = proxy.lock().await;

    let (client, _, _) = get_validated_client(&proxy, server_name).await?;

    // List tools
    let tools = client.list_tools().await?;

    // Convert to ToolInfo for response
    let tool_infos: Vec<ToolInfo> = tools
        .into_iter()
        .map(|tool| ToolInfo {
            name: tool.name,
            description: tool.description,
            parameters: tool.input_schema, // Map input_schema to parameters
            return_type: tool.output_schema.map(|v| v.to_string()), // Map output_schema to return_type string
        })
        .collect();

    Ok(HttpResponse::Ok().json(tool_infos))
}

/// List resources for a specific server
///
/// This handler returns a list of all resources available on a specific server.
///
/// # Returns
///
/// A JSON response with resource information
pub async fn list_server_resources(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    path: Path<(String,)>,
) -> Result<impl Responder> {
    let server_name = &path.0;
    let proxy = proxy.lock().await;

    let (client, _, _) = get_validated_client(&proxy, server_name).await?;

    // List resources
    let resources_result = client.list_resources().await;

    match resources_result {
        Ok(resources) => {
            // Convert to ResourceInfo for response
            let resource_infos: Vec<ResourceInfo> = resources
                .into_iter()
                .map(|r| ResourceInfo {
                    name: r.name,
                    description: r.description,
                    metadata: None, // No metadata field in client::Resource
                })
                .collect();

            Ok(HttpResponse::Ok().json(resource_infos))
        }
        Err(e) => {
            // Check if the error indicates an unsupported operation (e.g., method not found)
            if matches!(&e, Error::JsonRpc(s) if s.contains("method not found") || s.contains("unsupported"))
            {
                // Return empty list if resources not supported
                tracing::debug!(server = %server_name, "Server does not support resources (method not found)");
                Ok(HttpResponse::Ok().json(Vec::<ResourceInfo>::new()))
            } else {
                Err(e)
            }
        }
    }
}

/// Get a specific resource from a server
///
/// This handler retrieves a specific resource from a server.
///
/// # Returns
///
/// The resource content with appropriate content type
pub async fn get_server_resource(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    path: Path<(String, String)>,
) -> Result<impl Responder> {
    let (server_name, resource_name) = (&path.0, &path.1);
    let proxy = proxy.lock().await;

    let (client, _, _) = get_validated_client(&proxy, server_name).await?;

    // Get the resource by deserializing into ResourceResponse
    let resource_response: ResourceResponse = client.get_resource(resource_name).await?;

    // Convert the data string to Bytes.
    // TODO: Handle potential base64 decoding if necessary based on content_type
    let data_bytes = Bytes::from(resource_response.data);

    // Determine how to return the resource based on content type
    Ok(HttpResponse::Ok()
        .content_type(resource_response.content_type)
        .body(data_bytes))
}

/// Process JsonRPC tool calls
///
/// This handler processes JsonRPC tool calls and returns the response directly.
///
/// # Returns
///
/// A JSON response with the tool call result
pub async fn tool_call_jsonrpc(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    body: Bytes,
) -> Result<impl Responder> {
    // Parse the JsonRPC request
    let json_rpc_req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse JsonRPC request");
            // Use Error::JsonRpc instead of Error::InvalidRequest
            return Err(Error::JsonRpc(format!("Invalid JsonRPC request: {}", e)));
        }
    };

    // Check JsonRPC version
    if json_rpc_req.jsonrpc != JSON_RPC_VERSION {
        // Use Error::JsonRpc instead of Error::InvalidRequest
        return Err(Error::JsonRpc(format!(
            "Unsupported JsonRPC version: {}",
            json_rpc_req.jsonrpc
        )));
    }

    // Extract method and params
    let request_id = json_rpc_req.id.clone();
    let method = json_rpc_req.method.clone();

    // Parse method to get server name and tool name
    let parts: Vec<&str> = method.splitn(2, '.').collect();
    if parts.len() != 2 {
        // Use Error::JsonRpc instead of Error::InvalidRequest
        return Err(Error::JsonRpc(format!(
            "Invalid method format. Expected 'server.tool', got '{}'",
            method
        )));
    }

    let server_name = parts[0];
    let tool_name = parts[1];
    let args = json_rpc_req.params.unwrap_or(json!({}));

    tracing::debug!(
        req_id = ?request_id,
        server = %server_name,
        tool = %tool_name,
        "JsonRPC tool call received"
    );

    // Process the tool call
    let proxy_instance = proxy.lock().await;

    let (client, _, _) = match get_validated_client(&proxy_instance, server_name).await {
        Ok(result) => result,
        Err(e) => {
            let error_response =
                create_jsonrpc_error(request_id, -32000, format!("Validation failed: {}", e));
            return Ok(error_response);
        }
    };

    // Call the tool
    match client.call_tool(tool_name, &args).await {
        Ok(result) => {
            // Use JsonRpcResponse::success instead of ::result
            let json_rpc_resp = JsonRpcResponse::success(request_id, result);
            Ok(HttpResponse::Ok().json(json_rpc_resp))
        }
        Err(e) => {
            tracing::error!(
                req_id = ?request_id,
                error = %e,
                "Tool call failed"
            );
            let error_response =
                create_jsonrpc_error(request_id, -32000, format!("Tool call failed: {}", e));
            Ok(error_response)
        }
    }
}

/// Handle messages from clients
///
/// This handler receives messages sent by clients to the SSE endpoint.
/// These messages are typically requests that will be processed and
/// responded to via the SSE stream.
///
/// # Returns
///
/// An HTTP response acknowledging receipt of the message
pub async fn sse_messages(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    body: Bytes,
    _req: HttpRequest,
) -> Result<impl Responder> {
    tracing::debug!("Received message from client");

    // First try to parse message as a JSON value to see its structure
    let json_value: Value = match serde_json::from_slice(&body) {
        Ok(val) => val,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse client message as JSON");
            return Err(Error::JsonRpc(format!("Invalid JSON format: {}", e)));
        }
    };

    // Log the raw message for debugging
    tracing::debug!(
        raw_message = ?json_value,
        "Raw client message"
    );

    // Try to extract the method and determine message type
    let method = match json_value.get("method") {
        Some(m) => m.as_str().unwrap_or("unknown"),
        None => {
            tracing::error!("Message missing 'method' field");
            return Err(Error::JsonRpc("Missing 'method' field".to_string()));
        }
    };

    // Extract the request ID if present (might be missing in notifications)
    let request_id = match json_value.get("id") {
        Some(id) => id.clone(),
        None => {
            // For ping and other special messages, provide a default ID
            if method == "ping" {
                json!("ping-default-id")
            } else {
                // For other messages, generate a random ID
                json!(format!("generated-{}", uuid::Uuid::new_v4()))
            }
        }
    };

    // Log information about the message
    tracing::debug!(
        req_id = ?request_id,
        method = %method,
        "Processing client message"
    );

    // Special handling for notification messages
    if method.starts_with("notifications/") {
        tracing::info!("Processing notification: {}", method);

        // Handle 'initialized' notification
        if method == "notifications/initialized" {
            tracing::debug!("Client sent initialized notification");

            // Just accept the notification, no response needed
            return Ok(HttpResponse::Accepted().json(json!({
                "status": "accepted",
                "message": "Notification acknowledged"
            })));
        }

        // Handle other notifications in the future
        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "message": "Notification received"
        })));
    }

    // Special handling for initialize method
    if method == "initialize" {
        tracing::info!("Processing initialize request");

        // Create a simple response for initialize
        let proxy_lock = proxy.lock().await;
        let event_manager = proxy_lock.event_manager();

        // Get server information to include in response
        let server_info = proxy_lock.get_server_info().lock().await;
        let mut servers_map = serde_json::Map::new();

        for (_id, info) in server_info.iter() {
            servers_map.insert(info.name.clone(), json!(format!("/sse/{}", info.name)));
        }

        // Format exactly as Python client expects - a standard JSON-RPC response
        let initialize_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "servers": servers_map,
                "capabilities": {
                    "streaming": true,
                    "roots": {
                        "listChanged": true
                    },
                    "sampling": {}
                },
                "serverInfo": {
                    "name": "mcp-runner",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "protocolVersion": "2024-11-05"
            }
        });

        // Log what we're sending back for debugging
        tracing::info!(
            response = ?initialize_response,
            "Sending initialize response"
        );

        // Send the full JSON-RPC response (not just the result)
        event_manager.send_tool_response(
            &request_id.to_string(),
            "system",
            "initialize",
            initialize_response, // Send the entire JSON-RPC response object
        );

        tracing::debug!("Sent initialization response");

        // Return immediate acceptance
        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "id": request_id,
            "message": "Initialization request received and being processed"
        })));
    }

    // Special handling for ping method
    if method == "ping" {
        tracing::info!("Processing ping request");

        // Create a simple response for ping
        let proxy_lock = proxy.lock().await;
        let event_manager = proxy_lock.event_manager();

        // Format exactly as Python client expects - a standard JSON-RPC response
        let ping_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "type": "pong"
            }
        });

        // Log what we're sending back
        tracing::debug!(
            response = ?ping_response,
            "Sending ping response"
        );

        // Send the response
        let request_id_str = request_id.to_string();
        event_manager.send_tool_response(&request_id_str, "system", "ping", ping_response);

        // Return immediate acceptance
        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "id": request_id,
            "message": "Ping request received and processed"
        })));
    }

    // Try to parse as a standard JSON-RPC request now
    let json_rpc_req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse as standard JsonRPC request");
            // Return acceptance for special messages we've already handled
            if method == "ping" || method == "initialize" {
                return Ok(HttpResponse::Accepted().json(json!({
                    "status": "accepted",
                    "message": "Message processed with special handling"
                })));
            }
            return Err(Error::JsonRpc(format!("Invalid message format: {}", e)));
        }
    };

    // For standard JSON-RPC requests, continue with normal processing
    // Parse method to get server name and tool name
    let parts: Vec<&str> = method.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(Error::JsonRpc(format!(
            "Invalid method format. Expected 'server.tool', got '{}'",
            method
        )));
    }

    let server_name = parts[0];
    let tool_name = parts[1];
    let args = json_rpc_req.params.unwrap_or(json!({}));

    tracing::debug!(
        req_id = ?request_id,
        server = %server_name,
        tool = %tool_name,
        "SSE message received"
    );

    // Process the tool call asynchronously
    let proxy_lock = Arc::clone(&proxy);
    let request_id_str = match &request_id {
        Value::String(s) => s.clone(),
        _ => request_id.to_string(),
    };

    // Clone data for the async task
    let server_name_clone = server_name.to_string();
    let tool_name_clone = tool_name.to_string();
    let args_clone = args.clone();

    tokio::spawn(async move {
        // Acquire the lock on the proxy
        let proxy = proxy_lock.lock().await;

        // Process the tool call
        if let Err(e) = proxy
            .process_tool_call(
                &server_name_clone,
                &tool_name_clone,
                args_clone,
                &request_id_str,
            )
            .await
        {
            tracing::error!(
                req_id = %request_id_str,
                server = %server_name_clone,
                tool = %tool_name_clone,
                error = %e,
                "Failed to process tool call from SSE message"
            );
            // Error handling is done in process_tool_call which will send error events
        }
    });

    // Return a successful response immediately
    Ok(HttpResponse::Accepted().json(json!({
        "status": "accepted",
        "id": request_id,
        "message": "Message received and being processed"
    })))
}
