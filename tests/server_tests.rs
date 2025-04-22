use mcp_runner::server::{ServerProcess, ServerStatus};
use mcp_runner::server::lifecycle::{ServerLifecycleManager, ServerLifecycleEvent};
use mcp_runner::server::monitor::{ServerMonitor, ServerMonitorConfig, ServerHealth};
use mcp_runner::config::ServerConfig;
use mcp_runner::error::Result;
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn test_server_process() -> Result<()> {
    // Create a simple echo server configuration
    let config = ServerConfig {
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        env: HashMap::new(),
    };
    
    // Create a server process
    let mut server = ServerProcess::new("echo".to_string(), config);
    
    // Check initial state
    assert_eq!(server.status(), ServerStatus::Stopped);
    
    // Start the server
    server.start().await?;
    
    // Check status after starting
    assert_eq!(server.status(), ServerStatus::Running);
    
    // Stop the server
    server.stop().await?;
    
    // Check status after stopping
    assert_eq!(server.status(), ServerStatus::Stopped);
    
    Ok(())
}

#[tokio::test]
async fn test_server_io() -> Result<()> {
    // This test would ideally test the I/O capabilities of the server
    // However, this is hard to do in a unit test without actual MCP servers
    // For a more complete test, we'd need to mock the MCP server
    
    Ok(())
}

#[tokio::test]
async fn test_server_lifecycle() -> Result<()> {
    // Create a lifecycle manager
    let lifecycle_manager = ServerLifecycleManager::new();
    
    // Create a server process to get a valid ID
    let config = ServerConfig {
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        env: HashMap::new(),
    };
    let server = ServerProcess::new("test-server".to_string(), config);
    let id = server.id();  // Get the ID from the server process
    
    // Record some events
    lifecycle_manager.record_event(
        id,
        "test-server".to_string(),
        ServerLifecycleEvent::Started,
        Some("Test start".to_string()),
    )?;
    
    lifecycle_manager.record_event(
        id,
        "test-server".to_string(),
        ServerLifecycleEvent::Stopped,
        Some("Test stop".to_string()),
    )?;
    
    // Get server events
    let events = lifecycle_manager.get_server_events(id, None)?;
    
    // Check events
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, ServerLifecycleEvent::Stopped);
    assert_eq!(events[1].event, ServerLifecycleEvent::Started);
    
    // Check server status
    let status = lifecycle_manager.get_status(id)?;
    assert_eq!(status, ServerStatus::Stopped);
    
    Ok(())
}

#[tokio::test]
async fn test_server_monitor() -> Result<()> {
    // Create a lifecycle manager
    let lifecycle_manager = Arc::new(ServerLifecycleManager::new());
    
    // Create a monitor configuration
    let config = ServerMonitorConfig::default();
    
    // Create a server monitor
    let mut monitor = ServerMonitor::new(Arc::clone(&lifecycle_manager), config);
    
    // Start the monitor
    monitor.start()?;
    
    // Create a server process to get a valid ID
    let server_config = ServerConfig {
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        env: HashMap::new(),
    };
    let server = ServerProcess::new("test-server".to_string(), server_config);
    let id = server.id();  // Get the ID from the server process
    
    // Record a started event
    lifecycle_manager.record_event(
        id,
        "test-server".to_string(),
        ServerLifecycleEvent::Started,
        None,
    )?;
    
    // Check health
    let health = monitor.check_health(id, "test-server").await?;
    
    // Should be healthy since we recorded a Started event
    assert_eq!(health, ServerHealth::Healthy);
    
    // Record a failed event
    lifecycle_manager.record_event(
        id,
        "test-server".to_string(),
        ServerLifecycleEvent::Failed,
        None,
    )?;
    
    // Check health again
    let health = monitor.check_health(id, "test-server").await?;
    
    // Should be unhealthy since we recorded a Failed event
    assert_eq!(health, ServerHealth::Unhealthy);
    
    // Stop the monitor
    monitor.stop()?;
    
    Ok(())
}