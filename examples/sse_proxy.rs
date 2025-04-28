use mcp_runner::{McpRunner, error::Result};
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

        // Start the SSE proxy server - now using address and port from config
        info!("Starting SSE proxy with settings from config file");
        runner.start_sse_proxy().await?;

        info!("SSE proxy started successfully!");
        info!("Available HTTP endpoints:");

        // Using values from the example config file
        let sse_proxy_config = runner.get_sse_proxy_config()?;
        let host = &sse_proxy_config.address;
        let port = &sse_proxy_config.port;

        info!(
            " - SSE events stream:           GET    http://{}:{}/events",
            host, port
        );
        info!(
            " - Call a tool:                 POST   http://{}:{}/tool",
            host, port
        );
        info!(
            " - List all servers:            GET    http://{}:{}/servers",
            host, port
        );
        info!(
            " - List tools for a server:     GET    http://{}:{}/servers/SERVER_NAME/tools",
            host, port
        );
        info!(
            " - List resources for a server: GET    http://{}:{}/servers/SERVER_NAME/resources",
            host, port
        );
        info!(
            " - Get resource content:        GET    http://{}:{}/resource/SERVER_NAME/RESOURCE_URI",
            host, port
        );

        info!("Example tool call with curl:");
        info!("curl -X POST http://{}:{}/tool \\", host, port);
        info!("  -H \"Content-Type: application/json\" \\");
        info!(
            "  -d '{{\"server\":\"fetch\", \"tool\":\"fetch\", \"args\":{{\"url\":\"https://example.com\"}}}}' "
        );
        info!("");

        info!("Example SSE client with curl:");
        info!("curl -N http://{}:{}/events", host, port);

        info!("");
        info!("Press Ctrl+C to exit");

        // Keep the server running until Ctrl+C
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to wait for Ctrl+C");

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
