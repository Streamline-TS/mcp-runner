use mcp_runner::{McpRunner, error::Result};
use std::net::SocketAddr;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true)
        .init();

    info!("Starting SSE proxy example");

    // Load config with SSE proxy settings
    let config_path = "examples/sse_config.json";
    let mut runner = McpRunner::from_config_file(config_path)?;
    
    // Check if SSE proxy is configured
    if runner.is_sse_proxy_configured() {
        info!("SSE proxy is configured in config");
        
        // Start both servers first
        info!("Starting MCP servers");
        let server_ids = runner.start_all_servers().await?;
        info!("Started {} servers", server_ids.len());
        
        // Start the SSE proxy server
        let proxy_addr: SocketAddr = "127.0.0.1:3000".parse()
            .expect("Valid socket address");
            
        info!("Starting SSE proxy on {}", proxy_addr);
        runner.start_sse_proxy(proxy_addr).await?;
        
        info!("SSE proxy started successfully!");
        info!("Available HTTP endpoints:");
        info!(" - SSE events stream:           GET    http://localhost:3000/events");
        info!(" - Call a tool:                 POST   http://localhost:3000/tool");
        info!(" - List all servers:            GET    http://localhost:3000/servers");
        info!(" - List tools for a server:     GET    http://localhost:3000/servers/SERVER_NAME/tools");
        info!(" - List resources for a server: GET    http://localhost:3000/servers/SERVER_NAME/resources");
        info!(" - Get resource content:        GET    http://localhost:3000/resource/SERVER_NAME/RESOURCE_URI");
        
        info!("Example tool call with curl:");
        info!("curl -X POST http://localhost:3000/tool \\");
        info!("  -H \"Content-Type: application/json\" \\");
        // Remove direct access to private config field
        // For a real application, you would need to add an API method to get the auth token
        info!("  -d '{{\"server\":\"fetch\", \"tool\":\"fetch\", \"args\":{{\"url\":\"https://example.com\"}}}}' ");
        info!("");
        
        info!("Example SSE client with curl:");
        info!("curl -N http://localhost:3000/events");
        
        info!("");
        info!("Press Ctrl+C to exit");
        
        // Keep the server running until Ctrl+C
        // Fixed signal handling with proper import
        tokio::signal::ctrl_c().await.expect("Failed to wait for Ctrl+C");
        
        info!("Shutting down");
        
        // Stop all servers
        for id in server_ids {
            if let Err(e) = runner.stop_server(id).await {
                warn!("Failed to stop server: {}", e);
            }
        }
    } else {
        warn!("SSE proxy not configured in {}", config_path);
        warn!("Please add sseProxy configuration to your config file");
    }
    
    info!("Example finished");
    Ok(())
}