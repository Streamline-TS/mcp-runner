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
- Proxying Server-Sent Events (SSE) to enable clients to connect to MCP servers

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
mcp-runner = "0.2.2"
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
    
    // Start all servers and the SSE proxy if configured
    let (server_ids, proxy_started) = runner.start_all_with_proxy().await;
    let server_ids = server_ids?;
    
    if proxy_started {
        println!("SSE proxy started successfully");
    }
    
    // Get client for interacting with a specific server
    let server_id = runner.get_server_id("fetch")?;
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

MCP Runner uses JSON configuration to define MCP servers and optional SSE proxy settings.

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
  },
  "sseProxy": {
    "address": "127.0.0.1",
    "port": 3000,
    "allowedServers": ["fetch", "filesystem"],
    "authenticate": {
      "bearer": {
        "token": "your-secure-token"
      }
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

## SSE Proxy

The SSE (Server-Sent Events) proxy allows clients to connect to MCP servers through HTTP and receive real-time updates using the Server-Sent Events protocol. It is implemented using Actix Web for high performance, reliability, and maintainability.

### Features

- **HTTP API Gateway**: Provides REST-like HTTP endpoints for accessing MCP server functionality
- **Authentication**: Optional Bearer token authentication for secure access
- **Server Access Control**: Restrict which servers can be accessed through the proxy
- **Event Streaming**: Real-time updates from MCP servers to clients via SSE
- **Cross-Origin Support**: Built-in CORS support for web browser clients
- **Performance**: High-performance HTTP server built on Actix Web

### Starting the Proxy

You can start the SSE proxy automatically when starting your servers:

```rust
// Start all servers and the proxy if configured
let (server_ids, proxy_started) = runner.start_all_with_proxy().await;
let server_ids = server_ids?;

if proxy_started {
    println!("SSE proxy started successfully");
}
```

Or manually start it after configuring your servers:

```rust
if runner.is_sse_proxy_configured() {
    runner.start_sse_proxy().await?;
    println!("SSE proxy started manually");
}
```

### Proxy Configuration

Configure the SSE proxy in your configuration file:

```json
{
  "mcpServers": { /* server configs */ },
  "sseProxy": {
    "address": "127.0.0.1",  // Listen address (localhost only) - default if omitted
    "port": 3000,            // Port to listen on - default if omitted
    "workers": 4,            // Number of worker threads - default is 4 if omitted
    "allowedServers": [      // Optional: restrict which servers can be accessed
      "fetch", 
      "embedding"
    ],
    "authenticate": {        // Optional: require authentication
      "bearer": {
        "token": "your-secure-token-here"
      }
    }
  }
}
```

### Proxy API Endpoints

The SSE proxy exposes the following HTTP endpoints:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/events` | GET | SSE event stream endpoint for receiving real-time updates |
| `/initialize` | POST | JSON-RPC initialize endpoint for client initialization |
| `/tool` | POST | Tool call endpoint for invoking MCP server tools |
| `/servers` | GET | List available MCP servers and their status |
| `/servers/{name}/tools` | GET | List available tools for a specific server |
| `/servers/{name}/resources` | GET | List available resources for a specific server |
| `/resource/{server}/{uri}` | GET | Get a specific resource from a server |

## Examples

Check the `examples/` directory for more usage examples:

- `simple_client.rs`: Basic usage of the client API
  ```bash
  # Run with info level logging
  RUST_LOG=info cargo run --example simple_client
  ```  
- `sse_proxy.rs`: Example of using the SSE proxy to expose MCP servers to web clients
  ```bash
  # Run with info level logging
  RUST_LOG=info cargo run --example sse_proxy
  ```
  
  This example uses the config in `examples/sse_config.json` to start servers and an SSE proxy,
  allowing web clients to connect and interact with MCP servers through HTTP and SSE.
  
  JavaScript client example:
  ```javascript
  // Connect to the event stream
  const eventSource = new EventSource('http://localhost:3000/events');
  eventSource.addEventListener('tool-response', (event) => {
    const response = JSON.parse(event.data);
    console.log('Received tool response:', response);
  });

  // Make a tool call
  async function callTool() {
    const response = await fetch('http://localhost:3000/tool', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': 'Bearer your-secure-token-here'
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'tools/call',
        params: {
          server: 'fetch',
          tool: 'fetch',
          arguments: {
            url: 'https://modelcontextprotocol.io'
          }
        }
      })
    });
    const result = await response.json();
    console.log('Tool call initiated:', result);
    // Actual response will come through the SSE event stream
  }
  ```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the terms in the [LICENSE](LICENSE) file.