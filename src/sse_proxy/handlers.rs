//! HTTP request handlers for the Actix Web-based SSE proxy.
//!
//! This module contains the Actix Web handlers for the SSE proxy endpoints:
//! - `/sse` for SSE connection and streaming
//! - `/sse/messages` for receiving client JSON-RPC messages

use crate::error::{Error, Result};
use crate::sse_proxy::events::EventManager;
use crate::sse_proxy::proxy::SSEProxy;
use crate::transport::json_rpc::{error_codes, JsonRpcResponse};

use actix_web::{
    HttpRequest, HttpResponse, Responder,
    web::{Bytes, Data},
};

use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;



/// Helper function to create a JsonRPC error response
fn create_jsonrpc_error(request_id: Value, code: i32, message: String) -> HttpResponse {
    let error_response = JsonRpcResponse::error(request_id, code, message, None);

    HttpResponse::InternalServerError().json(error_response)
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

    // Special handling for tools/list method
    if method == "tools/list" {
        tracing::info!("Processing tools/list request");

        let proxy_lock = proxy.lock().await;
        let event_manager = proxy_lock.event_manager();
        let server_info = proxy_lock.get_server_info().lock().await;
        let runner_access = proxy_lock.get_runner_access();

        // Collect tools from all servers dynamically
        let mut all_tools = Vec::new();

        // For each server in the server info map
        for (server_name, _info) in server_info.iter() {
            // Try to get a client for this server
            match (runner_access.get_server_id)(server_name) {
                Ok(server_id) => {
                    match (runner_access.get_client)(server_id) {
                        Ok(client) => {
                            // Initialize client if needed
                            if let Err(e) = client.initialize().await {
                                tracing::warn!(
                                    server = %server_name,
                                    error = %e,
                                    "Failed to initialize client, continuing to next server"
                                );
                                continue;
                            }

                            // List tools for this server
                            match client.list_tools().await {
                                Ok(tools) => {
                                    for tool in tools {
                                        all_tools.push(json!({
                                            "name": tool.name,
                                            "description": tool.description,
                                            "server": server_name,
                                            "inputSchema": tool.input_schema.unwrap_or(json!({})),
                                            "outputSchema": tool.output_schema.unwrap_or(json!({}))
                                        }));
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        server = %server_name,
                                        error = %e,
                                        "Failed to list tools for server"
                                    );
                                    // Continue to next server
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                server = %server_name,
                                error = %e,
                                "Failed to get client for server"
                            );
                            // Continue to next server
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        server = %server_name,
                        error = %e,
                        "Failed to get server ID"
                    );
                    // Continue to next server
                }
            }
        }

        // Format response as Python client expects
        let tools_response = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "tools": all_tools
            }
        });

        tracing::debug!(
            response = ?tools_response,
            "Sending tools/list response"
        );

        event_manager.send_tool_response(
            &request_id.to_string(),
            "system",
            "tools/list",
            tools_response,
        );

        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "id": request_id,
            "message": "Tools list request received and processed"
        })));
    }

    // Special handling for tools/call method
    if method == "tools/call" {
        tracing::info!("Processing tools/call request");

        // Extract tool name and arguments from the params
        let params = match json_value.get("params") {
            Some(p) => p.clone(),
            None => {
                tracing::error!("Missing params in tools/call request");
                return Err(Error::JsonRpc(
                    "Missing params in tools/call request".to_string(),
                ));
            }
        };

        // Extract the tool name and arguments
        let tool_name = match params.get("name") {
            Some(Value::String(name)) => name.clone(),
            _ => {
                tracing::error!("Missing or invalid tool name in tools/call request");
                return Err(Error::JsonRpc("Missing or invalid tool name".to_string()));
            }
        };

        let arguments = match params.get("arguments") {
            Some(args) => args.clone(),
            None => {
                tracing::error!("Missing arguments in tools/call request");
                return Err(Error::JsonRpc("Missing arguments".to_string()));
            }
        };

        // Check if server is provided, if not we'll try to determine it automatically
        let server_name = match params.get("server") {
            Some(Value::String(s)) => s.clone(),
            _ => {
                // Server name not provided, try to determine it based on tool name
                tracing::info!(
                    tool = %tool_name,
                    "Server name not provided, attempting to determine from tool name"
                );

                // Get server info and runner access
                let proxy_lock = proxy.lock().await;
                let server_info = proxy_lock.get_server_info().lock().await;
                let runner_access = proxy_lock.get_runner_access();

                // Try to find the server that has this tool
                let mut found_server: Option<String> = None;

                // For each server, check if it has the tool we want
                for (server_name, _info) in server_info.iter() {
                    if let Ok(server_id) = (runner_access.get_server_id)(server_name) {
                        if let Ok(client) = (runner_access.get_client)(server_id) {
                            // Initialize client if needed
                            if let Err(e) = client.initialize().await {
                                tracing::warn!(
                                    server = %server_name,
                                    error = %e,
                                    "Failed to initialize client, continuing to next server"
                                );
                                continue;
                            }

                            // List tools for this server
                            if let Ok(tools) = client.list_tools().await {
                                for tool in tools {
                                    if tool.name == tool_name {
                                        found_server = Some(server_name.clone());
                                        break;
                                    }
                                }
                            }

                            if found_server.is_some() {
                                break;
                            }
                        }
                    }
                }

                match found_server {
                    Some(server) => {
                        tracing::info!(
                            tool = %tool_name,
                            server = %server,
                            "Automatically determined server for tool"
                        );
                        server
                    }
                    None => {
                        tracing::error!(
                            tool = %tool_name,
                            "Could not determine which server provides this tool"
                        );
                        return Err(Error::JsonRpc(
                            "Could not determine which server provides this tool".to_string(),
                        ));
                    }
                }
            }
        };

        tracing::debug!(
            req_id = ?request_id,
            tool_name = %tool_name,
            server_name = %server_name,
            "Processing tool call"
        );

        // Process the tool call asynchronously
        let proxy_lock = Arc::clone(&proxy);
        let request_id_str = request_id.to_string();
        let server_name_clone = server_name.clone();
        let tool_name_clone = tool_name.clone();
        let arguments_clone = arguments.clone();

        tokio::spawn(async move {
            // Acquire the lock on the proxy
            let proxy = proxy_lock.lock().await;

            // Process the tool call
            if let Err(e) = proxy
                .process_tool_call(
                    &server_name_clone,
                    &tool_name_clone,
                    arguments_clone,
                    &request_id_str,
                )
                .await
            {
                tracing::error!(
                    req_id = %request_id_str,
                    server = %server_name_clone,
                    tool = %tool_name_clone,
                    error = %e,
                    "Failed to process tool call from tools/call method"
                );
                // Error handling is done in process_tool_call which will send error events
            }
        });

        // Return immediate acceptance
        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "id": request_id,
            "message": "Tool call received and being processed"
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

        // Send the response through the SSE stream
        event_manager.send_tool_response(&request_id.to_string(), "system", "ping", ping_response);

        // Return immediate acceptance
        return Ok(HttpResponse::Accepted().json(json!({
            "status": "accepted",
            "id": request_id,
            "message": "Ping request received and processed"
        })));
    }

    // Default response for unknown methods
    tracing::warn!(
        req_id = ?request_id,
        method = %method,
        "Unknown method received"
    );

    let error_response =
        create_jsonrpc_error(request_id, error_codes::METHOD_NOT_FOUND, format!("Method '{}' not found", method));

    Ok(error_response)
}
