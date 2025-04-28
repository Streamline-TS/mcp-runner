use mcp_runner::McpRunner;
use mcp_runner::error::Result;
use serde_json::json;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    // Reads the RUST_LOG environment variable to set the log level.
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true) // Show module targets
        .init();

    info!("Starting simple_client example");

    // Create a runner from a config file
    let config_path = "examples/config.json";
    let mut runner = McpRunner::from_config_file(config_path)?;

    // Start all servers and the SSE proxy (if configured) in one call
    info!("Starting all servers and proxy if configured...");
    let (server_ids_result, proxy_started) = runner.start_all_with_proxy().await;

    // Check if servers started successfully
    let server_ids = server_ids_result?;
    info!("Started {} servers", server_ids.len());

    if proxy_started {
        info!("SSE proxy started successfully");
    }

    // Get the server IDs for our specific servers
    let fetch_server_id = runner.get_server_id("fetch")?;
    let fs_server_id = runner.get_server_id("filesystem")?;

    // First, work with the fetch server
    info!("=== Fetch Server ===");
    let fetch_client = runner.get_client(fetch_server_id)?;

    // Initialize the fetch server
    fetch_client.initialize().await?;

    // List available tools for fetch server
    let fetch_tools = fetch_client.list_tools().await?;
    info!("Available tools in fetch server:");
    for tool in &fetch_tools {
        info!("- {}: {}", tool.name, tool.description);
    }

    // Call the fetch tool
    if let Some(tool) = fetch_tools.iter().find(|t| t.name == "fetch") {
        let args = json!({
            "url": "https://modelcontextprotocol.io",
        });

        info!("Calling fetch tool...");
        let result: serde_json::Value = fetch_client.call_tool(&tool.name, &args).await?;
        info!("fetch result: {}", result);
    } else {
        warn!("fetch tool not available");
    }

    // Now, work with the filesystem server
    info!("\n=== Filesystem Server ===");
    let fs_client = runner.get_client(fs_server_id)?;

    // Initialize the filesystem server
    fs_client.initialize().await?;

    // List available tools for filesystem server
    let fs_tools = fs_client.list_tools().await?;
    info!("Available tools in filesystem server:");
    for tool in &fs_tools {
        info!("- {}: {}", tool.name, tool.description);
    }

    // Call the list_directory tool
    if let Some(tool) = fs_tools.iter().find(|t| t.name == "list_directory") {
        let args = json!({
            "path": "/tmp"
        });

        info!("Calling list_directory tool...");
        let result: serde_json::Value = fs_client.call_tool(&tool.name, &args).await?;
        info!("list_directory result: {}", result);
    } else {
        warn!("list_directory tool not available");
    }

    // Stop both servers
    info!("Stopping servers...");
    if let Err(e) = runner.stop_server(fetch_server_id).await {
        error!("Failed to stop fetch server: {}", e);
    }
    if let Err(e) = runner.stop_server(fs_server_id).await {
        error!("Failed to stop filesystem server: {}", e);
    }

    info!("simple_client example finished");
    Ok(())
}
