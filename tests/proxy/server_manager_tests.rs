#![cfg(test)]

use mcp_runner::proxy::events::EventManager;
use mcp_runner::proxy::server_manager::ServerManager;
use mcp_runner::proxy::types::ServerInfo;
use mcp_runner::server::{ServerId, ServerProcess};
use mcp_runner::config::ServerConfig;
use std::sync::Arc;
use std::collections::HashMap;

// Helper to get a valid ServerId from a server process - since ServerId::new() is private
fn create_test_server_id() -> ServerId {
    let config = ServerConfig {
        command: "echo".to_string(),
        args: vec!["test".to_string()],
        env: HashMap::new(),
    };
    let server = ServerProcess::new("test-server".to_string(), config);
    server.id() // Use the ID from the test server process
}

#[tokio::test]
async fn test_server_manager_creation() {
    let event_manager = Arc::new(EventManager::new(10));
    let server_manager = ServerManager::new(event_manager);
    
    // Verify we can access the server_info cache
    let server_info = server_manager.server_info();
    let cache = server_info.lock().await;
    assert!(cache.is_empty(), "New server manager should have empty cache");
}

#[tokio::test]
async fn test_add_server_info() {
    let event_manager = Arc::new(EventManager::new(10));
    let server_manager = ServerManager::new(event_manager);
    
    // Create a test server info
    let server_name = "Test Server";
    let server_info = ServerInfo {
        name: server_name.to_string(),
        id: "pending".to_string(),
        status: "Starting".to_string(),
    };
    
    // Add server info
    server_manager
        .add_server_info(
            server_name,
            server_info.clone(),
            |_| Ok(create_test_server_id())
        )
        .await;
    
    // Verify the server was added
    let cache = server_manager.server_info().lock().await;
    assert_eq!(cache.len(), 1, "Cache should contain one server");
    assert!(cache.contains_key(server_name), "Cache should contain our test server");
    
    // Check server info matches what we added
    if let Some(cached_info) = cache.get(server_name) {
        assert_eq!(cached_info.status, "Starting");
    }
}

#[tokio::test]
async fn test_update_server_info() {
    let event_manager = Arc::new(EventManager::new(10));
    let server_manager = ServerManager::new(event_manager);
    
    // Add a test server first
    let server_name = "Test Server";
    let server_info = ServerInfo {
        name: server_name.to_string(),
        id: "pending".to_string(),
        status: "Starting".to_string(),
    };
    
    // Add server info
    server_manager
        .add_server_info(
            server_name, 
            server_info.clone(),
            |_| Ok(create_test_server_id()) 
        )
        .await;
    
    // Now update the server status
    let server_id = create_test_server_id();
    server_manager.update_server_info(server_name, Some(server_id), "Running").await;
    
    // Verify the update worked
    let cache = server_manager.server_info().lock().await;
    if let Some(updated_info) = cache.get(server_name) {
        assert_eq!(updated_info.status, "Running");
        assert_ne!(updated_info.id, "pending"); // ID should be updated
    } else {
        panic!("Server info not found after update");
    }
}

#[tokio::test]
async fn test_update_nonexistent_server() {
    let event_manager = Arc::new(EventManager::new(10));
    let server_manager = ServerManager::new(event_manager);
    
    // Try to update a server that doesn't exist - should not crash
    let server_id = create_test_server_id();
    server_manager.update_server_info("NonexistentServer", Some(server_id), "Running").await;
    
    // Cache should still be empty
    let cache = server_manager.server_info().lock().await;
    assert!(cache.is_empty(), "Cache should still be empty after updating nonexistent server");
}