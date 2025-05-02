# Architecture Overview - mcp-runner

This document provides a high-level overview of the `mcp-runner` library's architecture, intended for developers contributing to the project.

## Goal

The primary goal of `mcp-runner` is to provide a robust and easy-to-use Rust interface for managing and communicating with local Model Context Protocol (MCP) servers.

## Core Modules

The library is structured into several core modules within the `src/` directory:

1.  **`config`**:
    *   Handles parsing and validating configuration files (e.g., `config.json`).
    *   Defines `Config` and `ServerConfig` structs to represent the server setup.
    *   Located in `src/config/`.

2.  **`server`**:
    *   Manages the lifecycle of individual MCP server processes (`ServerProcess`).
    *   Handles starting (`async_process::Command`), stopping, and monitoring server subprocesses.
    *   Defines `ServerId` for unique identification and `ServerStatus` for state tracking.
    *   Located in `src/server/`.

3.  **`transport`**:
    *   Abstracts the communication layer with MCP servers.
    *   Currently implements `StdioTransport` for JSON-RPC communication over standard input/output.
    *   Defines the `Transport` trait for potential future communication methods.
    *   Handles JSON-RPC message serialization/deserialization (`src/transport/json_rpc.rs`).
    *   Located in `src/transport/`.

4.  **`client`**:
    *   Provides the high-level API (`McpClient`) for interacting with a specific MCP server (via its `Transport`).
    *   Offers methods like `initialize`, `list_tools`, `call_tool`, `list_resources`, `get_resource`.
    *   Located in `src/client.rs`.

5.  **`error`**:
    *   Defines the unified error type (`Error`) and `Result` type used throughout the library.
    *   Categorizes errors originating from configuration, process management, communication, etc.
    *   Located in `src/error.rs`.

6.  **`lib.rs`**:
    *   The main library entry point.
    *   Defines the `McpRunner` struct, which acts as the central orchestrator, managing multiple `ServerProcess` instances based on the loaded `Config`.
    *   Provides methods to start/stop servers and obtain `McpClient` instances.

7.  **`proxy`**:
    *   Provides an HTTP and Server-Sent Events (SSE) proxy for MCP servers.
    *   Enables web clients to interact with MCP servers through a REST-like API.
    *   Located in `src/proxy/`.

## High-Level Flow (Example: Calling a Tool)

1.  **Initialization**: An `McpRunner` instance is created, typically from a `Config` (e.g., `McpRunner::from_config_file`).
2.  **Server Startup**: The user calls `runner.start_server("my_server")` or `runner.start_all_servers()`.
    *   `McpRunner` looks up the `ServerConfig` for "my_server".
    *   It creates a `ServerProcess`.
    *   `ServerProcess::start()` uses `async_process::Command` to spawn the server executable defined in the config, piping its stdin/stdout/stderr.
    *   The `ServerProcess` is stored in the `McpRunner`.
3.  **Client Acquisition**: The user calls `runner.get_client(server_id)`.
    *   `McpRunner` retrieves the corresponding `ServerProcess`.
    *   It takes ownership of the process's stdin/stdout handles.
    *   It creates an `StdioTransport` instance using these handles.
    *   It creates an `McpClient` instance, associating it with the `StdioTransport`.
4.  **Client Initialization**: The user calls `client.initialize().await`.
    *   `McpClient` delegates to `transport.initialize()`.
    *   `StdioTransport` sends a JSON-RPC `notifications/initialized` message via the server's stdin.
5.  **Tool Call**: The user calls `client.call_tool("tool_name", args).await`.
    *   `McpClient` delegates to `transport.call_tool()`.
    *   `StdioTransport` constructs a JSON-RPC request (`methods/callTool`) with a unique ID.
    *   It serializes the request to JSON and writes it to the server's stdin (followed by a newline).
    *   It registers a `oneshot` channel to wait for the response associated with the request ID.
    *   A background task (`reader_task` in `StdioTransport`) reads lines from the server's stdout, deserializes JSON-RPC responses, and sends the response through the correct `oneshot` channel based on the ID.
    *   `send_request` awaits the response on the `oneshot` channel.
    *   The result is returned to the user.
6.  **Server Shutdown**: The user calls `runner.stop_server(server_id)`.
    *   `McpRunner` retrieves and removes the `ServerProcess`.
    *   `ServerProcess::stop()` attempts to kill the child process and waits for it to exit.
    *   The `StdioTransport`'s `reader_task` eventually terminates as stdout closes.

## SSE Proxy Architecture

The SSE Proxy module (`src/sse_proxy/`) enables web clients to interact with MCP servers through an HTTP interface and Server-Sent Events (SSE). It serves as a bridge between web applications and the MCP protocol, translating HTTP requests into MCP operations. This implementation uses Actix Web for improved performance, reliability, and maintainability.

### Key Components

1.  **`SSEProxy` (`src/sse_proxy/proxy.rs`)**: 
    *   The main proxy server implementation built on Actix Web.
    *   Manages communication between the `McpRunner` and client connections.
    *   Handles server information updates and lifecycle management.
    *   Provides access to the runner for handling requests.

2.  **`EventManager` (`src/sse_proxy/events.rs`)**: 
    *   Manages Server-Sent Events (SSE) for broadcasting updates to connected clients.
    *   Handles event generation for tool call responses, errors, and server status changes.

3.  **`Handlers` (`src/sse_proxy/handlers.rs`)**: 
    *   Contains Actix Web handlers for different API endpoints (servers, tools, events, resources).
    *   Translates HTTP requests into MCP operations.

4.  **`Authentication` (`src/sse_proxy/auth.rs`)**: 
    *   Implements Actix Web middleware for securing access to the proxy.
    *   Currently supports bearer token authentication.

5.  **`Types` (`src/sse_proxy/types.rs`)**: 
    *   Contains shared data structures used across the SSE proxy components.

### Communication Flow

1.  **Proxy Startup**:
    *   An `SSEProxyHandle` is created by calling `SSEProxy::start_proxy` with the required configuration and access to the `McpRunner`.
    *   Actix Web server starts listening on the configured address and port.
    *   A communication channel is established between the `McpRunner` and the proxy for server status updates.

2.  **Client Connection**:
    *   When a client connects, Actix Web routes the request to the appropriate handler.
    *   Authentication is verified through middleware against the configured bearer token if enabled.

3.  **SSE Subscription**:
    *   Clients can subscribe to server events via the `/events` endpoint.
    *   The `EventManager` maintains a list of active subscribers and broadcasts events.
    *   Events include server status changes, tool call responses, and errors.

4.  **Tool Calls**:
    *   Clients can make tool calls to MCP servers via the `/tool` endpoint or the `/jsonrpc` endpoint.
    *   The proxy retrieves the corresponding `McpClient` from the runner.
    *   Tool calls are forwarded to the server, and responses are sent back to the client via HTTP response or as SSE events.

5.  **Server Updates**:
    *   The `McpRunner` sends server status updates to the proxy via the `SSEProxyHandle`.
    *   Updates are processed and broadcast as events to subscribed clients.

6.  **Proxy Shutdown**:
    *   The `SSEProxyHandle::shutdown()` method signals the proxy to stop accepting new connections.
    *   Actix Web server is gracefully shut down, terminating all active connections and tasks.

### Security Considerations

*   **Authentication**: The proxy supports bearer token authentication for securing access using Actix Web middleware.
*   **Server Allowlist**: Only servers explicitly allowed in the configuration can be accessed.
*   **CORS Support**: Built-in Cross-Origin Resource Sharing support for web browser clients.
*   **Error Handling**: Comprehensive error handling prevents information leakage and improves reliability.

### Configuration

The SSE proxy is configured via the `SSEProxyConfig` struct in the configuration file:

```json
{
  "sseProxy": {
    "address": "127.0.0.1",
    "port": 3000,
    "authenticate": {
      "bearer": {
        "token": "your-secret-token"
      }
    }
  }
}
```

## Key Concepts

*   **Model Context Protocol (MCP)**: The protocol standard defining how clients interact with context providers (servers).
*   **JSON-RPC 2.0**: The underlying message format used for communication over stdio.
*   **Async Rust**: The library heavily relies on `async/await` and the `tokio` runtime for managing asynchronous operations like process management and I/O.
*   **Stdio Transport**: Communication happens by writing JSON-RPC messages to the server's standard input and reading responses from its standard output.

## Contributing

Please refer to `CONTRIBUTING.md` for guidelines on submitting changes. Ensure code is formatted (`cargo fmt`), passes tests (`cargo test`), and passes clippy lints (`cargo clippy -- -D warnings`). Adding relevant tests and documentation for new features or fixes is encouraged.
