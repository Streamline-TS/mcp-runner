//! HTTP endpoint handlers for the SSE proxy.
//!
//! This module contains handlers for various HTTP endpoints supported by the SSE proxy.
//! These handlers were extracted from the SSEProxy implementation to improve modularity
//! and maintainability.

use crate::Error;
use crate::error::Result;
use crate::proxy::http::HttpResponse;
use crate::proxy::sse_proxy::SSEProxy;
use crate::proxy::types::{ResourceInfo, ToolInfo};
use crate::transport::json_rpc::{JSON_RPC_VERSION, JsonRpcRequest, JsonRpcResponse};

use std::collections::HashMap;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

/// Handlers for HTTP endpoints used by the SSE proxy
pub struct HttpHandlers;

impl HttpHandlers {
    /// Handle JSON-RPC initialize request
    ///
    /// Processes a JSON-RPC initialize request, validating it and returning
    /// information about the proxy capabilities.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `body` - Request body bytes
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_initialize(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        body: &[u8],
    ) -> Result<()> {
        let req: JsonRpcRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::json!(null),
                    -32700, // Parse error
                    format!("Parse error: {}", e),
                    None,
                );
                // Use HttpResponse helper for error
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        // Fallback to generic 500 if serialization fails
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        if req.method != "initialize" {
            let resp = JsonRpcResponse::error(
                req.id,
                -32601, // Method not found
                "Method not found (expected 'initialize')",
                None,
            );
            // Use HttpResponse helper for error
            match serde_json::to_string(&resp) {
                Ok(json) => return HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                Err(serialize_err) => {
                    tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                    return HttpResponse::send_error_response(
                        writer,
                        500,
                        "Internal server error during error reporting",
                    )
                    .await;
                }
            }
        }

        // Respond with protocol version and capabilities
        let result = serde_json::json!({
            "protocolVersion": JSON_RPC_VERSION,
            "capabilities": {
                "sse": true,
                "tools": true,
                "resources": true
            }
        });
        let resp = JsonRpcResponse::success(req.id, result);

        // Use HttpResponse helper for success
        match serde_json::to_string(&resp) {
            Ok(json) => HttpResponse::send_json_response(writer, &json).await,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize JSON-RPC success response");
                HttpResponse::send_error_response(
                    writer,
                    500,
                    "Internal server error during response serialization",
                )
                .await
            }
        }
    }

    /// Handle tool call request as JSON-RPC
    ///
    /// Processes a JSON-RPC tool call request, parsing and validating the parameters,
    /// and then forwarding the call to the appropriate MCP server through the runner.
    /// The actual tool response is sent asynchronously via SSE.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `body` - Request body bytes
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_tool_call_jsonrpc(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        body: &[u8],
        proxy: SSEProxy,
    ) -> Result<()> {
        let req: JsonRpcRequest = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(
                    serde_json::json!(null),
                    -32700, // Parse error
                    format!("Parse error: {}", e),
                    None,
                );
                // Use HttpResponse helper for error
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        if req.method != "tools/call" {
            let resp = JsonRpcResponse::error(
                req.id.clone(), // Clone id for error response
                -32601,         // Method not found
                "Method not found (expected 'tools/call')",
                None,
            );
            // Use HttpResponse helper for error
            match serde_json::to_string(&resp) {
                Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                Err(serialize_err) => {
                    tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                    return HttpResponse::send_error_response(
                        writer,
                        500,
                        "Internal server error during error reporting",
                    )
                    .await;
                }
            }
        }

        // Extract params
        let (server, tool, args) = match &req.params {
            Some(params) => {
                // Improved parameter validation with explicit error handling
                let server = match params.get("server").and_then(|v| v.as_str()) {
                    Some(s) => s.to_string(),
                    None => {
                        let resp = JsonRpcResponse::error(
                            req.id.clone(),
                            -32602, // Invalid params
                            "Invalid 'server' parameter: must be a string",
                            None,
                        );
                        match serde_json::to_string(&resp) {
                            Ok(json) => {
                                return HttpResponse::send_json_response(writer, &json).await;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                                return HttpResponse::send_error_response(
                                    writer,
                                    500,
                                    "Internal server error during error reporting",
                                )
                                .await;
                            }
                        }
                    }
                };

                let tool = match params.get("tool").and_then(|v| v.as_str()) {
                    Some(t) => t.to_string(),
                    None => {
                        let resp = JsonRpcResponse::error(
                            req.id.clone(),
                            -32602, // Invalid params
                            "Invalid 'tool' parameter: must be a string",
                            None,
                        );
                        match serde_json::to_string(&resp) {
                            Ok(json) => {
                                return HttpResponse::send_json_response(writer, &json).await;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                                return HttpResponse::send_error_response(
                                    writer,
                                    500,
                                    "Internal server error during error reporting",
                                )
                                .await;
                            }
                        }
                    }
                };

                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                (server, tool, args)
            }
            None => {
                let resp = JsonRpcResponse::error(
                    req.id.clone(),
                    -32602, // Invalid params
                    "Missing required parameters",
                    None,
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => return HttpResponse::send_json_response(writer, &json).await,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize JSON-RPC error response");
                        return HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await;
                    }
                }
            }
        };

        // Call the tool and respond
        let request_id_str = req.id.to_string(); // Convert JsonRpcId to string for process_tool_call
        let result = proxy
            .process_tool_call(&server, &tool, args, &request_id_str)
            .await;

        match result {
            Ok(_) => {
                // Send success response (tool result is sent via SSE)
                let resp = JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({ "status": "accepted", "request_id": request_id_str }),
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => HttpResponse::send_json_response(writer, &json).await,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize JSON-RPC success response");
                        HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during response serialization",
                        )
                        .await
                    }
                }
            }
            Err(e) => {
                // Send error response
                let resp = JsonRpcResponse::error(
                    req.id,
                    -32000, // Server error
                    format!("Tool call failed: {}", e),
                    None, // No additional data for this error
                );
                match serde_json::to_string(&resp) {
                    Ok(json) => HttpResponse::send_json_response(writer, &json).await, // Send as 200 OK with JSON error body
                    Err(serialize_err) => {
                        tracing::error!(error = %serialize_err, "Failed to serialize JSON-RPC error response");
                        HttpResponse::send_error_response(
                            writer,
                            500,
                            "Internal server error during error reporting",
                        )
                        .await
                    }
                }
            }
        }
    }

    /// Handle a request to list available servers
    ///
    /// Returns a JSON array of server information, including name, ID, and status.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_list_servers(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::info!("Handling list servers request");

        // Debug the entire proxy config structure
        if let Ok(config_json) = serde_json::to_string(&proxy.config()) {
            tracing::info!("Full proxy config: {}", config_json);
        } else {
            tracing::warn!("Unable to serialize proxy config for debugging");
        }

        let server_info_cache = proxy.get_server_info().lock().await;
        let mut servers = Vec::new();

        // Log allowed servers configuration
        if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
            tracing::info!("Allowed servers configured: {:?}", allowed_servers);
        } else {
            tracing::info!("No allowed servers list configured - all servers will be visible");
        }

        // First check if we have any cached server info
        if !server_info_cache.is_empty() {
            tracing::info!("Using cached server information for /servers endpoint");
            // Log all servers in cache
            tracing::info!(
                "Servers in cache: {:?}",
                server_info_cache.keys().collect::<Vec<_>>()
            );

            // Use the cached info directly
            for (name, info) in server_info_cache.iter() {
                // Check if this server is in the allowed list (if we have one)
                if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
                    if !allowed_servers.contains(name) {
                        tracing::info!(server = %name, "Server '{}' not in allowed list, excluding from response", name);
                        continue;
                    }
                    tracing::info!(server = %name, "Server '{}' is in allowed list, including in response", name);
                }
                servers.push(info.clone());
            }
        } else {
            tracing::info!("No cached server info available, using config-only information");
            // Get all server names from config
            let config_servers = (proxy.get_runner_access().get_server_config_keys)();
            tracing::info!("Servers in config: {:?}", config_servers);

            // Fall back to the original behavior if no cache is available
            // Collect information about all servers in the config
            for name in config_servers {
                // Check if this server is in the allowed list (if we have one)
                if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
                    if !allowed_servers.contains(&name) {
                        tracing::info!(server = %name, "Server '{}' not in allowed list, excluding from response", name);
                        continue;
                    }
                    tracing::info!(server = %name, "Server '{}' is in allowed list, including in response", name);
                }

                // Try to get server information
                let server_info = match (proxy.get_runner_access().get_server_id)(&name) {
                    Ok(id) => crate::proxy::types::ServerInfo {
                        name: name.clone(),
                        id: format!("{:?}", id),
                        status: "Running".to_string(),
                    },
                    Err(_) => {
                        // Server not started yet
                        crate::proxy::types::ServerInfo {
                            name: name.clone(),
                            id: "not_started".to_string(),
                            status: "Stopped".to_string(),
                        }
                    }
                };

                servers.push(server_info);
            }
        }

        tracing::info!(
            "Returning list of {} servers: {:?}",
            servers.len(),
            servers.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        // Convert to JSON
        let json = serde_json::to_string(&servers)
            .map_err(|e| Error::Serialization(format!("Failed to serialize server list: {}", e)))?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to list tools for a specific server
    ///
    /// Returns a JSON array of tool information for the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to list tools for
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_list_tools(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, "Handling list tools request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.get_runner_access().get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.get_runner_access().get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // List tools
        let tools = match client.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(server = %server_name, error = %e, "Failed to list tools");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to list tools: {}", e),
                )
                .await;
            }
        };

        // Convert to response format
        let tool_infos: Vec<ToolInfo> = tools
            .into_iter()
            .map(|t| ToolInfo {
                name: t.name,
                description: t.description,
                // Handle Option for parameters_schema, defaulting to json!(null)
                parameters_schema: t.input_schema.unwrap_or(serde_json::Value::Null),
            })
            .collect();

        // Convert to JSON
        let json = serde_json::to_string(&tool_infos)
            .map_err(|e| Error::Serialization(format!("Failed to serialize tool list: {}", e)))?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to list resources for a specific server
    ///
    /// Returns a JSON array of resource information for the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to list resources for
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_list_resources(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, "Handling list resources request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.get_runner_access().get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.get_runner_access().get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // List resources
        let resources = match client.list_resources().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(server = %server_name, error = %e, "Failed to list resources");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to list resources: {}", e),
                )
                .await;
            }
        };

        // Convert to response format
        let resource_infos: Vec<ResourceInfo> = resources
            .into_iter()
            .map(|r| ResourceInfo {
                name: r.name,
                uri: r.uri,
                // Handle Option for description, defaulting to empty string
                description: r.description.unwrap_or_default(),
            })
            .collect();

        // Convert to JSON
        let json = serde_json::to_string(&resource_infos).map_err(|e| {
            Error::Serialization(format!("Failed to serialize resource list: {}", e))
        })?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Handle a request to get a specific resource
    ///
    /// Retrieves and returns a specific resource from the specified server.
    ///
    /// # Arguments
    ///
    /// * `writer` - TCP stream writer for sending the response
    /// * `server_name` - Name of the server to get resource from
    /// * `resource_uri` - URI of the resource to retrieve
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_get_resource(
        writer: &mut tokio::io::WriteHalf<TcpStream>,
        server_name: &str,
        resource_uri: &str,
        proxy: SSEProxy,
    ) -> Result<()> {
        tracing::debug!(server = %server_name, uri = %resource_uri, "Handling get resource request");

        // Check if server is allowed
        if let Some(allowed_servers) = (proxy.get_runner_access().get_allowed_servers)() {
            if !allowed_servers.contains(&server_name.to_string()) {
                tracing::warn!(server = %server_name, "Server not in allowed list");
                return HttpResponse::send_forbidden_response(writer, "Server not in allowed list")
                    .await;
            }
        }

        // Get server ID
        let server_id = match (proxy.get_runner_access().get_server_id)(server_name) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(server = %server_name, error = %e, "Server not found");
                return HttpResponse::send_not_found_response(writer).await;
            }
        };

        // Get client
        let client = match (proxy.get_runner_access().get_client)(server_id) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(server_id = ?server_id, error = %e, "Failed to get client");
                return HttpResponse::send_error_response(
                    writer,
                    500,
                    &format!("Failed to get client: {}", e),
                )
                .await;
            }
        };

        // Get resource (explicitly use Value as the return type)
        let resource_data: serde_json::Value = match client.get_resource(resource_uri).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(server = %server_name, uri = %resource_uri, error = %e, "Failed to get resource");
                // Use 404 for resource not found specifically
                return HttpResponse::send_error_response(
                    writer,
                    404,
                    &format!("Resource not found or error getting resource: {}", e),
                )
                .await;
            }
        };

        // Convert to JSON
        let json = serde_json::to_string(&resource_data).map_err(|e| {
            Error::Serialization(format!("Failed to serialize resource data: {}", e))
        })?;

        // Send response using helper
        HttpResponse::send_json_response(writer, &json).await
    }

    /// Helper method to read body content from a request
    ///
    /// This utility reads the body content from a buffered reader based on the
    /// content length header.
    ///
    /// # Arguments
    ///
    /// * `buf_reader` - Buffered reader containing the request
    /// * `headers` - Request headers map
    /// * `max_size` - Maximum allowed body size
    ///
    /// # Returns
    ///
    /// A `Result` containing either the body bytes or an error
    pub async fn read_body(
        buf_reader: &mut tokio::io::BufReader<tokio::io::ReadHalf<TcpStream>>,
        headers: &HashMap<String, String>,
        max_size: usize,
    ) -> Result<Vec<u8>> {
        let content_length = headers
            .get("content-length")
            .and_then(|len| len.parse::<usize>().ok())
            .unwrap_or(0);

        if content_length > max_size {
            tracing::warn!(length = content_length, max_size, "Request body too large");
            return Err(Error::Other(format!(
                "Request body too large: {} bytes (max: {} bytes)",
                content_length, max_size
            )));
        }

        if content_length > 0 {
            let mut body = vec![0; content_length];
            match AsyncReadExt::read_exact(buf_reader, &mut body).await {
                Ok(_) => Ok(body),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to read request body");
                    Err(Error::Communication(format!(
                        "Failed to read request body: {}",
                        e
                    )))
                }
            }
        } else {
            Ok(vec![])
        }
    }
}
