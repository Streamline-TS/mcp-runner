use mcp_runner::McpRunner;
use mcp_runner::error::Result;
use serde_json::json;
use tracing_subscriber::{EnvFilter, fmt}; // Import tracing subscriber components

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    // This configures how logs are collected and formatted.
    // `with_env_filter` reads the RUST_LOG environment variable to set the log level.
    // `with_target(true)` includes the module path in the log output.
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true) // Show module targets
        .init();

    tracing::info!("Starting simple_client example");

    // Create a runner from a config file
    let config_path = "examples/config.json";
    let mut runner = McpRunner::from_config_file(config_path)?;

    // Start both servers
    println!("Starting the fetch server...");
    let fetch_server_id = runner.start_server("fetch").await?;
    println!("Starting the filesystem server...");
    let fs_server_id = runner.start_server("filesystem").await?;

    // First, work with the fetch server
    println!("\n=== Fetch Server ===");
    let fetch_client = runner.get_client(fetch_server_id)?;

    // Initialize the fetch server
    fetch_client.initialize().await?;

    // List available tools for fetch server
    let fetch_tools = fetch_client.list_tools().await?;
    println!("Available tools in fetch server:");
    for tool in &fetch_tools {
        println!("- {}: {}", tool.name, tool.description);
    }

    // Call the fetch tool
    if let Some(tool) = fetch_tools.iter().find(|t| t.name == "fetch") {
        let args = json!({
            "url": "https://modelcontextprotocol.io",
        });

        println!("Calling fetch tool...");
        let result: serde_json::Value = fetch_client.call_tool(&tool.name, &args).await?;
        println!("fetch result: {}", result);
    } else {
        println!("fetch tool not available");
    }

    // Server resources
    match fetch_client.list_resources().await {
        Ok(resources) => {
            println!("Fetch server resources:");
            for resource in &resources {
                println!(
                    "- {} ({}): {}",
                    resource.name,
                    resource.uri,
                    resource.description.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => println!("Fetch server doesn't support resources: {}", e),
    }

    // Now, work with the filesystem server
    println!("\n=== Filesystem Server ===");
    let fs_client = runner.get_client(fs_server_id)?;

    // Initialize the filesystem server
    fs_client.initialize().await?;

    // List available tools for filesystem server
    let fs_tools = fs_client.list_tools().await?;
    println!("Available tools in filesystem server:");
    for tool in &fs_tools {
        println!("- {}: {}", tool.name, tool.description);
    }

    // Call the list_directory tool
    if let Some(tool) = fs_tools.iter().find(|t| t.name == "list_directory") {
        let args = json!({
            "path": "/tmp"
        });

        println!("Calling list_directory tool...");
        let result: serde_json::Value = fs_client.call_tool(&tool.name, &args).await?;
        println!("list_directory result: {}", result);
    } else {
        println!("list_directory tool not available");
    }

    // Server resources
    match fs_client.list_resources().await {
        Ok(resources) => {
            println!("Filesystem server resources:");
            for resource in &resources {
                println!(
                    "- {} ({}): {}",
                    resource.name,
                    resource.uri,
                    resource.description.as_deref().unwrap_or("")
                );
            }
        }
        Err(e) => println!("Filesystem server doesn't support resources: {}", e),
    }

    // Stop both servers
    println!("\nStopping servers...");
    if let Err(e) = runner.stop_server(fetch_server_id).await {
        println!("Warning: Failed to stop fetch server: {}", e);
    }
    if let Err(e) = runner.stop_server(fs_server_id).await {
        println!("Warning: Failed to stop filesystem server: {}", e);
    }

    tracing::info!("simple_client example finished");
    Ok(())
}
