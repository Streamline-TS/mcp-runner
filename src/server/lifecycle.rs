use crate::error::{Error, Result};
use crate::server::{ServerId, ServerStatus};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;

/// Server lifecycle event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerLifecycleEvent {
    /// Server started
    Started,
    /// Server stopped
    Stopped,
    /// Server failed
    Failed,
    /// Server restarted
    Restarted,
}

/// Server lifecycle event
#[derive(Debug, Clone)]
pub struct ServerEvent {
    /// Server ID
    pub id: ServerId,
    /// Server name
    pub name: String,
    /// Event type
    pub event: ServerLifecycleEvent,
    /// Event timestamp
    pub timestamp: Instant,
    /// Event details
    pub details: Option<String>,
}

/// Server lifecycle manager
pub struct ServerLifecycleManager {
    /// Server events
    events: Arc<Mutex<Vec<ServerEvent>>>,
    /// Server statuses
    statuses: Arc<Mutex<HashMap<ServerId, ServerStatus>>>,
}

impl ServerLifecycleManager {
    /// Create a new server lifecycle manager
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            statuses: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Record a server event
    pub fn record_event(&self, id: ServerId, name: String, event: ServerLifecycleEvent, details: Option<String>) -> Result<()> {
        let server_event = ServerEvent {
            id,
            name,
            event,
            timestamp: Instant::now(),
            details,
        };
        
        // Update status
        {
            let mut statuses = self.statuses.lock()
                .map_err(|_| Error::Other("Failed to lock server statuses".to_string()))?;
            
            let status = match event {
                ServerLifecycleEvent::Started => ServerStatus::Running,
                ServerLifecycleEvent::Stopped => ServerStatus::Stopped,
                ServerLifecycleEvent::Failed => ServerStatus::Failed,
                ServerLifecycleEvent::Restarted => ServerStatus::Running,
            };
            
            statuses.insert(id, status);
        }
        
        // Record event
        {
            let mut events = self.events.lock()
                .map_err(|_| Error::Other("Failed to lock server events".to_string()))?;
            
            events.push(server_event);
            
            // Limit event history
            if events.len() > 1000 {
                events.remove(0);
            }
        }
        
        Ok(())
    }
    
    /// Get server status
    pub fn get_status(&self, id: ServerId) -> Result<ServerStatus> {
        let statuses = self.statuses.lock()
            .map_err(|_| Error::Other("Failed to lock server statuses".to_string()))?;
        
        statuses.get(&id)
            .copied()
            .ok_or_else(|| Error::ServerNotFound(format!("{:?}", id)))
    }
    
    /// Get recent events for a server
    pub fn get_server_events(&self, id: ServerId, limit: Option<usize>) -> Result<Vec<ServerEvent>> {
        let events = self.events.lock()
            .map_err(|_| Error::Other("Failed to lock server events".to_string()))?;
        
        let mut server_events: Vec<ServerEvent> = events.iter()
            .filter(|e| e.id == id)
            .cloned()
            .collect();
        
        // Sort by timestamp (newest first)
        server_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        // Apply limit
        if let Some(limit) = limit {
            server_events.truncate(limit);
        }
        
        Ok(server_events)
    }
    
    /// Get all events
    pub fn get_all_events(&self, limit: Option<usize>) -> Result<Vec<ServerEvent>> {
        let events = self.events.lock()
            .map_err(|_| Error::Other("Failed to lock server events".to_string()))?;
        
        let mut all_events = events.clone();
        
        // Sort by timestamp (newest first)
        all_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        // Apply limit
        if let Some(limit) = limit {
            all_events.truncate(limit);
        }
        
        Ok(all_events)
    }
    
    /// Clear events
    pub fn clear_events(&self) -> Result<()> {
        let mut events = self.events.lock()
            .map_err(|_| Error::Other("Failed to lock server events".to_string()))?;
        
        events.clear();
        
        Ok(())
    }
}

impl Default for ServerLifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}