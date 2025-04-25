# MCP Runner

A Rust library for running and interacting with Model Context Protocol (MCP) servers locally.

[![Crates.io](https://img.shields.io/crates/v/mcp-runner.svg)](https://crates.io/crates/mcp-runner)
[![Documentation](https://docs.rs/mcp-runner/badge.svg)](https://docs.rs/mcp-runner)

## Overview

MCP Runner provides a complete solution for managing Model Context Protocol servers in Rust applications. It enables:

- Starting and managing MCP server processes
- Configuring multiple servers through a unified interface
- Communicating with MCP servers using JSON-RPC
- Listing and calling tools exposed by MCP servers
- Accessing resources provided by MCP servers

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
mcp-runner = "0.1.0"
```

## Quick Start

Here's a simple example of using MCP Runner to start a server and call a tool:

```rust
use mcp_runner::{McpRunner, error::Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    // Create runner from config file
    let mut runner = McpRunner::from_config_file("config.json")?;
    
    // Start server
    let server_id = runner.start_server("fetch").await?;
    
    // Get client for interacting with the server
    let client = runner.get_client(server_id)?;
    
    // Initialize the client
    client.initialize().await?;
    
    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools:");
    for tool in tools {
        println!("  - {}: {}", tool.name, tool.description);
    }
    
    // Call the fetch tool with structured input
    let fetch_result = client.call_tool("fetch", &json!({
        "url": "https://modelcontextprotocol.io"
    })).await?;
    println!("Fetch result: {}", fetch_result);
    
    // Stop the server when done
    runner.stop_server(server_id).await?;
    
    Ok(())
}
```

## Observability

This library uses the `tracing` crate for logging and diagnostics. To enable logging, ensure you have a `tracing_subscriber` configured in your application and set the `RUST_LOG` environment variable. For example:

```bash
# Show info level logs for all crates
RUST_LOG=info cargo run --example simple_client

# Show trace level logs specifically for mcp_runner
RUST_LOG=mcp_runner=trace cargo run --example simple_client
```

## Configuration

MCP Runner uses JSON configuration to define MCP servers.

```json
{
  "mcpServers": {
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"]
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/files"]
    }
  }
}
```

You can load configurations in three different ways:

### 1. Load from a file

```rust
use mcp_runner::McpRunner;

let runner = McpRunner::from_config_file("config.json")?;
```

### 2. Load from a JSON string

```rust
use mcp_runner::McpRunner;

let config_json = r#"{
  "mcpServers": {
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"]
    }
  }
}"#;
let runner = McpRunner::from_config_str(config_json)?;
```

### 3. Create programmatically

```rust
use mcp_runner::{McpRunner, config::{Config, ServerConfig}};
use std::collections::HashMap;

let mut servers = HashMap::new();

let server_config = ServerConfig {
    command: "uvx".to_string(),
    args: vec!["mcp-server-fetch".to_string()],
    env: HashMap::new(),
};

servers.insert("fetch".to_string(), server_config);
let config = Config { mcp_servers: servers };

// Initialize the runner
let runner = McpRunner::new(config);
```

## Core Components

### McpRunner

The main entry point for managing MCP servers:

```rust
let mut runner = McpRunner::from_config_file("config.json")?;
let server_ids = runner.start_all_servers().await?;
```

### McpClient

For interacting with MCP servers:

```rust
let client = runner.get_client(server_id)?;
client.initialize().await?;

// Call tools
let result = client.call_tool("fetch", &json!({
    "url": "https://example.com",
})).await?;
```

## Error Handling

MCP Runner uses a custom error type that covers:
- Configuration errors
- Server lifecycle errors
- Communication errors
- Serialization errors

```rust
match result {
    Ok(value) => println!("Success: {:?}", value),
    Err(Error::ServerNotFound(name)) => println!("Server not found: {}", name),
    Err(Error::Communication(msg)) => println!("Communication error: {}", msg),
    Err(e) => println!("Other error: {}", e),
}
```

## Examples

Check the `examples/` directory for more usage examples:

- `simple_client.rs`: Basic usage of the client API
  ```bash
  # Run with info level logging
  RUST_LOG=info cargo run --example simple_client
  ```  
- more to come

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the terms in the [LICENSE](LICENSE) file.