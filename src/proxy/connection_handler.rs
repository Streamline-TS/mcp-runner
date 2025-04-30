//! HTTP connection handling for the SSE proxy.
//!
//! This module contains the connection handling logic for the SSE proxy,
//! including request parsing, routing, and response handling.

use crate::Error;
use crate::error::Result;
use crate::proxy::events::EventManager;
use crate::proxy::http::HttpResponse;
use crate::proxy::http_handlers::HttpHandlers;
use crate::proxy::sse_proxy::SSEProxy;

use std::collections::HashMap;
use std::net::SocketAddr;

use tokio::io::AsyncBufReadExt;
use tokio::net::TcpStream;
use tracing;

/// Connection handler for the SSE proxy
///
/// Handles parsing HTTP requests, routing to appropriate handlers,
/// and authentication.
pub struct ConnectionHandler;

impl ConnectionHandler {
    /// Handle an incoming HTTP connection
    ///
    /// Processes an incoming HTTP connection, parsing the request and routing it
    /// to the appropriate handler based on the path and method.
    ///
    /// # Arguments
    ///
    /// * `stream` - TCP stream for the client connection
    /// * `addr` - Socket address of the client
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// A `Result<()>` indicating success or an error
    pub async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        proxy: SSEProxy,
    ) -> Result<()> {
        // Create a buffered reader for the stream
        let (reader, mut writer) = tokio::io::split(stream);
        let mut buf_reader = tokio::io::BufReader::new(reader);

        // Read the request line and headers
        let (method, path, headers) = match Self::parse_request(&mut buf_reader).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(client_addr = %addr, error = %e, "Failed to parse HTTP request");
                return HttpResponse::send_bad_request_response(
                    &mut writer,
                    "Invalid HTTP request format",
                )
                .await;
            }
        };

        // Check for authentication if required
        if !Self::authenticate(&headers, &proxy).await {
            return HttpResponse::send_unauthorized_response(&mut writer).await;
        }

        // Route based on the path and method
        match (method.as_str(), path.as_str()) {
            // SSE event stream endpoint
            ("GET", "/events") => {
                // Use the cloned Arc<EventManager>
                EventManager::handle_sse_stream(&mut writer, proxy.event_manager().subscribe())
                    .await
            }
            // JSON-RPC initialize endpoint
            ("POST", "/initialize") => {
                const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MB

                match HttpHandlers::read_body(&mut buf_reader, &headers, MAX_BODY_SIZE).await {
                    Ok(body) => HttpHandlers::handle_initialize(&mut writer, &body).await,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to read request body");
                        HttpResponse::send_bad_request_response(
                            &mut writer,
                            &format!("Failed to read request body: {}", e),
                        )
                        .await
                    }
                }
            }
            // Tool call endpoint (JSON-RPC enforced)
            ("POST", "/tool") => {
                const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10 MB

                match HttpHandlers::read_body(&mut buf_reader, &headers, MAX_BODY_SIZE).await {
                    Ok(body) => {
                        HttpHandlers::handle_tool_call_jsonrpc(&mut writer, &body, proxy).await
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to read request body");
                        HttpResponse::send_bad_request_response(
                            &mut writer,
                            &format!("Failed to read request body: {}", e),
                        )
                        .await
                    }
                }
            }
            // List available servers endpoint
            ("GET", "/servers") => HttpHandlers::handle_list_servers(&mut writer, proxy).await,
            // List tools for a specific server
            ("GET", p) if p.starts_with("/servers/") && p.ends_with("/tools") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() == 4 {
                    let server_name = parts[2];
                    HttpHandlers::handle_list_tools(&mut writer, server_name, proxy).await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // List resources for a specific server
            ("GET", p) if p.starts_with("/servers/") && p.ends_with("/resources") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() == 4 {
                    let server_name = parts[2];
                    HttpHandlers::handle_list_resources(&mut writer, server_name, proxy).await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // Get a specific resource
            ("GET", p) if p.starts_with("/resource/") => {
                let parts: Vec<&str> = p.split('/').collect();
                if parts.len() >= 4 {
                    let server_name = parts[2];
                    let resource_uri = parts[3..].join("/");
                    HttpHandlers::handle_get_resource(
                        &mut writer,
                        server_name,
                        &resource_uri,
                        proxy,
                    )
                    .await
                } else {
                    HttpResponse::send_not_found_response(&mut writer).await
                }
            }
            // OPTIONS for CORS
            ("OPTIONS", _) => HttpResponse::handle_options_request(&mut writer).await,
            // Not found for other paths
            _ => HttpResponse::send_not_found_response(&mut writer).await,
        }
    }

    /// Parse an HTTP request from a buffered reader
    ///
    /// Extracts the method, path, and headers from an HTTP request.
    ///
    /// # Arguments
    ///
    /// * `buf_reader` - Buffered reader containing the HTTP request
    ///
    /// # Returns
    ///
    /// A `Result` containing a tuple of the method, path, and headers
    async fn parse_request(
        buf_reader: &mut tokio::io::BufReader<tokio::io::ReadHalf<TcpStream>>,
    ) -> Result<(String, String, HashMap<String, String>)> {
        let mut headers = HashMap::new();

        // Read the request line
        let mut line = String::new();
        buf_reader
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Communication(format!("Failed to read request line: {}", e)))?;
        tracing::debug!(request = %line.trim(), "Received HTTP request");

        // Parse request line
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(Error::Other("Invalid HTTP request line".to_string()));
        }
        let method = parts[0].to_string();
        let path = parts[1].to_string();

        // Read headers
        loop {
            let mut header_line = String::new();
            buf_reader
                .read_line(&mut header_line)
                .await
                .map_err(|e| Error::Communication(format!("Failed to read header: {}", e)))?;

            let line = header_line.trim();
            if line.is_empty() {
                break;
            }

            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_lowercase(), value.trim().to_string());
            }
        }

        Ok((method, path, headers))
    }

    /// Check authentication for a request
    ///
    /// Validates the authentication credentials in the request headers against
    /// the proxy's authentication settings.
    ///
    /// # Arguments
    ///
    /// * `headers` - Request headers
    /// * `proxy` - SSE proxy instance
    ///
    /// # Returns
    ///
    /// `true` if authentication is valid or not required, `false` otherwise
    async fn authenticate(headers: &HashMap<String, String>, proxy: &SSEProxy) -> bool {
        if let Some(auth) = &proxy.config().authenticate {
            if let Some(_bearer) = &auth.bearer {
                let token = if let Some(auth_header) = headers.get("authorization") {
                    if let Some(stripped) = auth_header.strip_prefix("Bearer ") {
                        stripped.to_string()
                    } else {
                        return false;
                    }
                } else {
                    return false;
                };

                return proxy.is_valid_token(&token);
            }
        }

        // If no authentication is configured, allow the request
        true
    }
}
