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
use uuid::Uuid;

/// Structure representing a parsed JSON-RPC request
struct ParsedMessage {
    /// The full JSON-RPC request as a Value
    json_value: Value,
    /// The method name extracted from the request
    method: String,
    /// The request ID, which might be generated if not provided
    request_id: Value,
}

/// Parse a raw message into a structured ParsedMessage with method and request ID
fn parse_message(body: &[u8]) -> Result<ParsedMessage> {
    // Parse message as a JSON value
    let json_value: Value = match serde_json::from_slice(body) {
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

    // Extract the method
    let method = match json_value.get("method") {
        Some(m) => m.as_str().unwrap_or("unknown").to_string(),
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
                json!(format!("generated-{}", Uuid::new_v4()))
            }
        }
    };

    Ok(ParsedMessage {
        json_value,
        method,
        request_id,
    })
}

/// Create a standard HTTP acceptance response for async processing
fn create_acceptance_response(request_id: &Value, message: &str) -> HttpResponse {
    HttpResponse::Accepted().json(json!({
        "status": "accepted",
        "id": request_id,
        "message": message
    }))
}

/// Helper function to create a JsonRPC error response
fn create_jsonrpc_error(request_id: Value, code: i32, message: String) -> HttpResponse {
    let error_response = JsonRpcResponse::error(request_id, code, message, None);

    HttpResponse::InternalServerError().json(error_response)
}

/// Helper function to create a standard JsonRPC response
fn create_jsonrpc_response(request_id: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": result
    })
}

/// Handle notification messages (methods starting with notifications/)
async fn handle_notification_message(method: &str, request_id: &Value) -> Result<HttpResponse> {
    tracing::info!("Processing notification: {}", method);

    // Handle 'initialized' notification
    if method == "notifications/initialized" {
        tracing::debug!("Client sent initialized notification");
    }

    // Just accept the notification, no response needed
    Ok(create_acceptance_response(request_id, "Notification acknowledged"))
}

/// Handle initialize method
async fn handle_initialize_message(
    proxy: &Arc<Mutex<SSEProxy>>, 
    request_id: &Value
) -> Result<HttpResponse> {
    tracing::info!("Processing initialize request");

    // Get necessary services
    let proxy_lock = proxy.lock().await;
    let event_manager = proxy_lock.event_manager();
    let server_info = proxy_lock.get_server_info().lock().await;
    
    // Build servers map
    let mut servers_map = serde_json::Map::new();
    for (_id, info) in server_info.iter() {
        servers_map.insert(info.name.clone(), json!(format!("/sse/{}", info.name)));
    }

    // Create response
    let initialize_response = create_jsonrpc_response(
        request_id, 
        json!({
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
        })
    );

    // Log what we're sending back
    tracing::info!(
        response = ?initialize_response,
        "Sending initialize response"
    );

    // Send response through event manager
    event_manager.send_tool_response(
        &request_id.to_string(),
        "system",
        "initialize",
        initialize_response
    );

    tracing::debug!("Sent initialization response");

    // Return immediate acceptance
    Ok(create_acceptance_response(request_id, "Initialization request received and being processed"))
}

/// Handle tools/list method
async fn handle_tools_list_message(
    proxy: &Arc<Mutex<SSEProxy>>, 
    request_id: &Value
) -> Result<HttpResponse> {
    tracing::info!("Processing tools/list request");

    // Get necessary services
    let proxy_lock = proxy.lock().await;
    let event_manager = proxy_lock.event_manager();
    let server_info = proxy_lock.get_server_info().lock().await;
    let runner_access = proxy_lock.get_runner_access();

    // Collect tools from all servers
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

    // Format response as client expects
    let tools_response = create_jsonrpc_response(
        request_id,
        json!({
            "tools": all_tools
        })
    );

    tracing::debug!(
        response = ?tools_response,
        "Sending tools/list response"
    );

    // Send response through event manager
    event_manager.send_tool_response(
        &request_id.to_string(),
        "system",
        "tools/list",
        tools_response
    );

    // Return immediate acceptance
    Ok(create_acceptance_response(request_id, "Tools list request received and processed"))
}

/// Extract server name from tool call request, trying to determine it automatically if not provided
async fn determine_server_for_tool(
    proxy: &Arc<Mutex<SSEProxy>>,
    tool_name: &str,
    params: &Value,
) -> Result<String> {
    // Check if server is provided
    if let Some(Value::String(s)) = params.get("server") {
        return Ok(s.clone());
    }
    
    // Server name not provided, try to determine it based on tool name
    tracing::info!(
        tool = %tool_name,
        "Server name not provided, attempting to determine from tool name"
    );

    // Get server info and runner access
    let proxy_lock = proxy.lock().await;
    let server_info = proxy_lock.get_server_info().lock().await;
    let runner_access = proxy_lock.get_runner_access();

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
                            tracing::info!(
                                tool = %tool_name,
                                server = %server_name,
                                "Automatically determined server for tool"
                            );
                            return Ok(server_name.clone());
                        }
                    }
                }
            }
        }
    }

    // Couldn't find a server providing this tool
    tracing::error!(
        tool = %tool_name,
        "Could not determine which server provides this tool"
    );
    Err(Error::JsonRpc(
        "Could not determine which server provides this tool".to_string(),
    ))
}

/// Handle ping method
async fn handle_ping_message(
    proxy: &Arc<Mutex<SSEProxy>>, 
    request_id: &Value
) -> Result<HttpResponse> {
    tracing::info!("Processing ping request");

    // Get necessary services
    let proxy_lock = proxy.lock().await;
    let event_manager = proxy_lock.event_manager();

    // Create ping response
    let ping_response = create_jsonrpc_response(
        request_id,
        json!({
            "type": "pong"
        })
    );

    // Log what we're sending back
    tracing::debug!(
        response = ?ping_response,
        "Sending ping response"
    );

    // Send response through event manager
    event_manager.send_tool_response(
        &request_id.to_string(), 
        "system", 
        "ping", 
        ping_response
    );

    // Return immediate acceptance
    Ok(create_acceptance_response(request_id, "Ping request received and processed"))
}

/// Handle tools/call method
async fn handle_tools_call_message(
    proxy: &Arc<Mutex<SSEProxy>>,
    request_id: &Value,
    json_value: &Value,
) -> Result<HttpResponse> {
    tracing::info!("Processing tools/call request");

    // Extract params
    let params = match json_value.get("params") {
        Some(p) => p,
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

    // Determine which server to use for this tool
    let server_name = determine_server_for_tool(proxy, &tool_name, params).await?;

    tracing::debug!(
        req_id = ?request_id,
        tool_name = %tool_name,
        server_name = %server_name,
        "Processing tool call"
    );

    // Process the tool call asynchronously
    let proxy_lock = Arc::clone(proxy);
    let request_id_str = request_id.to_string();
    let server_name_clone = server_name;
    let tool_name_clone = tool_name;
    let arguments_clone = arguments;

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
    Ok(create_acceptance_response(request_id, "Tool call received and being processed"))
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
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    req: HttpRequest,
) -> Result<impl Responder> {
    tracing::info!(
        path = ?req.path(),
        remote_addr = ?req.connection_info().peer_addr(),
        "New SSE connection request"
    );

    // Access the proxy to get the event manager
    let proxy = proxy.lock().await;
    let event_manager = proxy.event_manager();

    // Create a channel for SSE messages
    let mut receiver = event_manager.subscribe();

    // Create server configuration for client
    let server_info = proxy.get_server_info().lock().await;
    let mut servers_map = serde_json::Map::new();

    for (_id, info) in server_info.iter() {
        servers_map.insert(info.name.clone(), json!(format!("/sse/{}", info.name)));
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
        .insert_header(("Content-Type", "text/event-stream"))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .streaming(stream);

    Ok(response)
}

/// Main handler for processing JSON-RPC messages from the client
///
/// This handler receives JSON-RPC requests and notifications from the client,
/// processes them, and returns appropriate responses.
pub async fn sse_messages(
    proxy: Data<Arc<Mutex<SSEProxy>>>,
    body: Bytes,
    _req: HttpRequest,
) -> Result<impl Responder> {
    tracing::debug!("Received message from client");

    // Parse the message to extract method and request ID
    let parsed = match parse_message(&body) {
        Ok(parsed) => parsed,
        Err(e) => return Err(e),
    };

    // Log information about the message
    tracing::debug!(
        req_id = ?parsed.request_id,
        method = %parsed.method,
        "Processing client message"
    );
    
    // Dispatch to the appropriate handler based on the method
    if parsed.method.starts_with("notifications/") {
        handle_notification_message(&parsed.method, &parsed.request_id).await
    } 
    else if parsed.method == "initialize" {
        handle_initialize_message(&proxy, &parsed.request_id).await
    } 
    else if parsed.method == "tools/list" {
        handle_tools_list_message(&proxy, &parsed.request_id).await
    } 
    else if parsed.method == "tools/call" {
        handle_tools_call_message(&proxy, &parsed.request_id, &parsed.json_value).await
    } 
    else if parsed.method == "ping" {
        handle_ping_message(&proxy, &parsed.request_id).await
    } 
    else {
        // Handle unknown methods
        tracing::warn!(
            req_id = ?parsed.request_id,
            method = %parsed.method,
            "Unknown method received"
        );

        // Create and return an error response
        Ok(create_jsonrpc_error(
            parsed.request_id, 
            error_codes::METHOD_NOT_FOUND, 
            format!("Method '{}' not found", parsed.method)
        ))
    }
}