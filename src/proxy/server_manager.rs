//! Server information management for the SSE proxy.
//!
//! This module contains the server information management logic for the SSE proxy,
//! handling server status updates and caching server information.

use crate::server::ServerId;
use crate::proxy::events::EventManager;
use crate::proxy::types::ServerInfo;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing;

/// Manager for server information in the SSE proxy
pub struct ServerManager {
    /// Server information cache
    server_info: Arc<Mutex<HashMap<String, ServerInfo>>>,
    /// Event manager for broadcasting events
    event_manager: Arc<EventManager>,
}

impl ServerManager {
    /// Create a new server information manager
    pub fn new(event_manager: Arc<EventManager>) -> Self {
        Self {
            server_info: Arc::new(Mutex::new(HashMap::new())),
            event_manager,
        }
    }

    /// Get the server information cache
    pub fn server_info(&self) -> &Arc<Mutex<HashMap<String, ServerInfo>>> {
        &self.server_info
    }

    /// Update server information
    ///
    /// Updates the cached information for a server and broadcasts a status update event.
    pub async fn update_server_info(
        &self,
        server_name: &str,
        server_id: Option<ServerId>,
        status: &str,
    ) {
        let mut server_info_cache = self.server_info.lock().await;

        if let Some(info) = server_info_cache.get_mut(server_name) {
            if let Some(id) = server_id {
                // Update with new information
                info.id = format!("{:?}", id);
                info.status = status.to_string();
                tracing::debug!(
                    server = %server_name,
                    server_id = ?id,
                    status = %status,
                    "Updated server info in SSE proxy cache"
                );

                // Send a status update event to clients
                self.event_manager.send_status_update(id, server_name, status);
            } else {
                // Server was removed or stopped
                info.id = "not_running".to_string();
                info.status = "Stopped".to_string();
                tracing::debug!(
                    server = %server_name,
                    "Marked server as stopped in SSE proxy cache"
                );
            }
        } else {
            tracing::warn!(
                server = %server_name,
                "Attempted to update server info in SSE proxy cache, but server not found"
            );
        }
    }

    /// Add new server information
    ///
    /// Adds information about a new server to the cache and broadcasts a status update.
    pub async fn add_server_info(
        &self,
        server_name: &str,
        server_info: ServerInfo,
        get_server_id_fn: impl FnOnce(&str) -> Result<ServerId, crate::Error>,
    ) {
        let mut server_info_cache = self.server_info.lock().await;

        if server_info_cache.contains_key(server_name) {
            tracing::warn!(
                server = %server_name,
                "Attempted to add server to SSE proxy cache, but server already exists"
            );
        } else {
            // Add the new server info
            server_info_cache.insert(server_name.to_string(), server_info.clone());
            tracing::info!(
                server = %server_name,
                "Added new server to SSE proxy cache"
            );

            // Send a status update event to clients if we can get the ID
            if let Ok(id) = get_server_id_fn(server_name) {
                self.event_manager.send_status_update(id, server_name, &server_info.status);
            }
        }
    }
}