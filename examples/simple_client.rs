// examples/simple_client.rs
use mcp_runner::McpRunner;
use mcp_runner::error::Result;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a runner from a config file
    let config_path = "examples/config.json";
    let mut runner = McpRunner::from_config_file(config_path)?;
    
    // Start a specific server
    let server_id = runner.start_server("shell").await?;
    
    // Get a client for the server
    let client = runner.get_client(server_id)?;
    
    // Initialize the server
    client.initialize().await?;
    
    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools:");
    for tool in &tools {
        println!("- {}: {}", tool.name, tool.description);
    }
    
    // Call a tool
    if let Some(tool) = tools.iter().find(|t| t.name == "shell_execute") {
        let args = json!({
            "ls": "/"
        });
        
        let result: serde_json::Value = client.call_tool(&tool.name, &args).await?;
        println!("Tool result: {}", result);
    }
    
    // List resources
    let resources = client.list_resources().await?;
    println!("Available resources:");
    for resource in &resources {
        println!("- {} ({}): {}", resource.name, resource.uri, resource.description.as_deref().unwrap_or(""));
    }
    
    // Stop the server
    runner.stop_server(server_id).await?;
    
    Ok(())
}