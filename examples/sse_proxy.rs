use mcp_runner::{McpRunner, error::Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::select;
use tokio::task;
use tokio::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

/// Handle keyboard input for interactive commands
async fn interactive_keyboard_handler(
    shutdown_flag: Arc<AtomicBool>,
    runner: Arc<tokio::sync::Mutex<McpRunner>>,
    server_names: Arc<Vec<String>>,
) {
    // Create channel for command passing
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

    // Spawn a blocking task for keyboard input
    task::spawn_blocking(move || {
        let mut buffer = String::new();

        // Initially show help message without cluttering log output
        println!("\nEnter a command ('h' for help):");

        loop {
            buffer.clear();
            if std::io::stdin().read_line(&mut buffer).is_ok() {
                let cmd = buffer.trim().to_string();
                if cmd == "q" {
                    println!("Quit command received");
                    shutdown_flag.store(true, Ordering::SeqCst);
                    break;
                } else if cmd == "s" || cmd == "t" || cmd == "h" || cmd == "help" {
                    // Send the command through the channel
                    if tx.blocking_send(cmd).is_err() {
                        // Channel closed, exit the loop
                        break;
                    }
                } else if !cmd.is_empty() {
                    println!("Unknown command: '{}'. Enter 'h' for help", cmd);
                }
            }
        }
    });

    // Process commands from the channel
    while let Some(cmd) = rx.recv().await {
        match cmd.as_str() {
            "h" | "help" => {
                println!("\nAvailable commands:");
                println!(" - 's' : Show server status");
                println!(" - 't' : Show available tools");
                println!(" - 'h' : Show this help message");
                println!(" - 'q' : Quit the application");
            }
            "s" => {
                println!("\nServer Status:");
                let runner_guard = runner.lock().await;

                // Use the built-in method to get all statuses at once
                let statuses = runner_guard.get_all_server_statuses();

                // Display statuses for all running servers
                if statuses.is_empty() {
                    println!(" - No servers are running");
                } else {
                    // Use reference to avoid moving statuses
                    for (name, status) in &statuses {
                        println!(" - Server '{}': {:?}", name, status);
                    }
                }

                // Also show servers from our list that aren't running
                for server_name in server_names.as_ref() {
                    if !statuses.contains_key(server_name) {
                        println!(" - Server '{}': Not started", server_name);
                    }
                }
            }
            "t" => {
                println!("\nAvailable Tools:");
                let mut runner_guard = runner.lock().await;

                // Use the built-in method to get all tools at once
                let all_tools = runner_guard.get_all_server_tools().await;

                if all_tools.is_empty() {
                    println!(" - No servers are running");
                } else {
                    // Collect server names from the results first to avoid ownership issues
                    let server_names_with_tools: Vec<String> = all_tools.keys().cloned().collect();

                    // Now iterate through the tools
                    for server_name in server_names_with_tools {
                        println!("Server: {}", server_name);

                        match &all_tools[&server_name] {
                            Ok(tools) => {
                                if tools.is_empty() {
                                    println!(" - No tools available");
                                } else {
                                    for tool in tools {
                                        println!(" - Tool: {} ({})", tool.name, tool.description);
                                    }
                                }
                            }
                            Err(e) => {
                                println!(" - Failed to list tools: {}", e);
                            }
                        }
                    }

                    // Check for servers from our list that aren't in the results
                    for server_name in server_names.as_ref() {
                        if !all_tools.contains_key(server_name) {
                            println!("Server: {}", server_name);
                            println!(" - Server not started");
                        }
                    }
                }
            }
            _ => {} // Ignore other commands
        }

        // Re-display the prompt after processing a command
        println!("\nEnter a command ('h' for help):");
    }
}

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

        // Extract server names
        let server_names = vec!["fetch".to_string(), "filesystem".to_string()];

        // Make sure all servers are properly registered before starting the proxy
        for name in &server_names {
            if let Ok(server_id) = runner.get_server_id(name) {
                let status = runner.server_status(server_id)?;
                info!("Server '{}' status: {:?}", name, status);
            }
        }

        // Start the SSE proxy server - now using address and port from config
        info!("Starting Actix Web-based SSE proxy with settings from config file");
        runner.start_sse_proxy().await?;

        info!("Actix Web SSE proxy started successfully!");
        info!("Available HTTP endpoints:");

        // Using values from the example config file
        let sse_proxy_config = runner.get_sse_proxy_config()?;
        let host = &sse_proxy_config.address;
        let port = &sse_proxy_config.port;

        info!(
            " - SSE events stream:           GET    http://{}:{}/sse",
            host, port
        );
        info!(
            " - JSON-RPC messages:           POST   http://{}:{}/sse/messages",
            host, port
        );

        info!("Example JSON-RPC tool call with curl:");
        info!("curl -X POST http://{}:{}/sse/messages \\", host, port);
        info!("  -H \"Content-Type: application/json\" \\");
        info!(
            "  -d '{{\"jsonrpc\":\"2.0\", \"id\":\"req-123\", \"method\":\"tools/call\", \"params\":{{\"server\":\"fetch\", \"tool\":\"fetch\", \"arguments\":{{\"url\":\"https://example.com\"}}}}}}' "
        );

        info!("Example SSE client with curl:");
        info!("curl -N http://{}:{}/sse", host, port);

        // Setup interactive keyboard handler for commands
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown_flag_clone = shutdown_flag.clone();
        let server_names = Arc::new(server_names);
        let runner_arc = Arc::new(tokio::sync::Mutex::new(runner));

        // Start the interactive keyboard handler in the background
        let keyboard_handle = tokio::spawn(interactive_keyboard_handler(
            shutdown_flag_clone,
            runner_arc.clone(),
            server_names.clone(),
        ));

        // Wait for shutdown signal from keyboard handler or Ctrl+C
        select! {
            _ = async {
                while !shutdown_flag.load(Ordering::SeqCst) {
                    sleep(Duration::from_millis(100)).await;
                }
            } => {
                info!("Shutdown requested via keyboard command");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown requested via Ctrl+C");
                shutdown_flag.store(true, Ordering::SeqCst);
            }
        }

        // Wait for the keyboard handler to finish
        if let Err(e) = keyboard_handle.await {
            warn!("Keyboard handler task error: {:?}", e);
        }

        info!("Shutting down");

        // Stop all servers and the proxy
        let mut runner_guard = runner_arc.lock().await;
        if let Err(e) = runner_guard.stop_all_servers().await {
            warn!("Error during shutdown: {}", e);
        }
    } else {
        warn!("SSE proxy not configured in {}", config_path);
        warn!("Please add sseProxy configuration to your config file");
    }

    info!("Example finished");
    Ok(())
}
