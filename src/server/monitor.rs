use crate::error::{Error, Result};
use crate::server::{ServerId, ServerStatus};
use crate::server::lifecycle::{ServerLifecycleEvent, ServerLifecycleManager};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio::time;

/// Server health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerHealth {
    /// Server is healthy
    Healthy,
    /// Server is degraded
    Degraded,
    /// Server is unhealthy
    Unhealthy,
    /// Server health is unknown
    Unknown,
}

/// Server monitor configuration
#[derive(Debug, Clone)]
pub struct ServerMonitorConfig {
    /// Check interval
    pub check_interval: Duration,
    /// Health check timeout
    pub health_check_timeout: Duration,
    /// Maximum number of consecutive failures before marking as unhealthy
    pub max_consecutive_failures: u32,
    /// Auto-restart unhealthy servers
    pub auto_restart: bool,
}

impl Default for ServerMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            health_check_timeout: Duration::from_secs(5),
            max_consecutive_failures: 3,
            auto_restart: false,
        }
    }
}

/// Server monitor
pub struct ServerMonitor {
    /// Lifecycle manager
    lifecycle_manager: Arc<ServerLifecycleManager>,
    /// Server health statuses
    health_statuses: Arc<Mutex<HashMap<ServerId, ServerHealth>>>,
    /// Server failure counts
    failure_counts: Arc<Mutex<HashMap<ServerId, u32>>>,
    /// Server last checked times
    last_checked: Arc<Mutex<HashMap<ServerId, Instant>>>,
    /// Monitor configuration
    config: ServerMonitorConfig,
    /// Monitor task
    monitor_task: Option<JoinHandle<()>>,
    /// Running flag
    running: Arc<Mutex<bool>>,
}

impl ServerMonitor {
    /// Create a new server monitor
    pub fn new(lifecycle_manager: Arc<ServerLifecycleManager>, config: ServerMonitorConfig) -> Self {
        Self {
            lifecycle_manager,
            health_statuses: Arc::new(Mutex::new(HashMap::new())),
            failure_counts: Arc::new(Mutex::new(HashMap::new())),
            last_checked: Arc::new(Mutex::new(HashMap::new())),
            config,
            monitor_task: None,
            running: Arc::new(Mutex::new(false)),
        }
    }
    
    /// Start the monitor
    pub fn start(&mut self) -> Result<()> {
        {
            let mut running = self.running.lock()
                .map_err(|_| Error::Other("Failed to lock running flag".to_string()))?;
            
            if *running {
                return Ok(());
            }
            
            *running = true;
        }
        
        let lifecycle_manager = Arc::clone(&self.lifecycle_manager);
        let health_statuses = Arc::clone(&self.health_statuses);
        let failure_counts = Arc::clone(&self.failure_counts);
        let last_checked = Arc::clone(&self.last_checked);
        let running = Arc::clone(&self.running);
        let config = self.config.clone();
        
        let task = tokio::spawn(async move {
            let mut interval = time::interval(config.check_interval);
            
            loop {
                interval.tick().await;
                
                // Check if we should stop
                {
                    let running_guard = running.lock().unwrap();
                    if !*running_guard {
                        break;
                    }
                }
                
                // Get a snapshot of servers we need to check
                // For now, we'll just use the servers we already know about
                let server_ids_to_check = {
                    let health_guard = health_statuses.lock().unwrap();
                    health_guard.keys().cloned().collect::<Vec<_>>()
                };
                
                // Check each server
                for server_id in server_ids_to_check {
                    // Record check time
                    {
                        let mut checked = last_checked.lock().unwrap();
                        checked.insert(server_id, Instant::now());
                    }
                    
                    // Get current server status
                    if let Ok(status) = lifecycle_manager.get_status(server_id) {
                        // Determine health based on status
                        let health = match status {
                            ServerStatus::Running => ServerHealth::Healthy,
                            ServerStatus::Failed => ServerHealth::Unhealthy,
                            _ => ServerHealth::Unknown,
                        };
                        
                        // Update health status
                        {
                            let mut statuses = health_statuses.lock().unwrap();
                            statuses.insert(server_id, health);
                        }
                        
                        // Update failure counts for unhealthy servers
                        if health == ServerHealth::Unhealthy {
                            let mut counts = failure_counts.lock().unwrap();
                            let count = counts.entry(server_id).or_insert(0);
                            *count += 1;
                            
                            // TODO: Implement auto-restart logic here when needed
                        } else {
                            // Reset failure count for healthy servers
                            let mut counts = failure_counts.lock().unwrap();
                            counts.insert(server_id, 0);
                        }
                    }
                }
            }
        });
        
        self.monitor_task = Some(task);
        
        Ok(())
    }
    
    /// Stop the monitor
    pub fn stop(&mut self) -> Result<()> {
        {
            let mut running = self.running.lock()
                .map_err(|_| Error::Other("Failed to lock running flag".to_string()))?;
            
            if !*running {
                return Ok(());
            }
            
            *running = false;
        }
        
        if let Some(task) = self.monitor_task.take() {
            task.abort();
        }
        
        Ok(())
    }
    
    /// Get server health
    pub fn get_health(&self, id: ServerId) -> Result<ServerHealth> {
        let health_statuses = self.health_statuses.lock()
            .map_err(|_| Error::Other("Failed to lock health statuses".to_string()))?;
        
        health_statuses.get(&id)
            .copied()
            .ok_or_else(|| Error::ServerNotFound(format!("{:?}", id)))
    }
    
    /// Force health check for a server
    pub async fn check_health(&self, id: ServerId, name: &str) -> Result<ServerHealth> {
        // In a real implementation, this would perform an actual health check
        // For now, we'll just simulate a health check
        
        // Record the check time
        {
            let mut last_checked = self.last_checked.lock()
                .map_err(|_| Error::Other("Failed to lock last checked times".to_string()))?;
            
            last_checked.insert(id, Instant::now());
        }
        
        // Get current status
        let status = self.lifecycle_manager.get_status(id)?;
        
        let health = match status {
            ServerStatus::Running => ServerHealth::Healthy,
            ServerStatus::Starting => ServerHealth::Unknown,
            ServerStatus::Stopping => ServerHealth::Unknown,
            ServerStatus::Stopped => ServerHealth::Unknown,
            ServerStatus::Failed => ServerHealth::Unhealthy,
        };
        
        // Update health status
        {
            let mut health_statuses = self.health_statuses.lock()
                .map_err(|_| Error::Other("Failed to lock health statuses".to_string()))?;
            
            health_statuses.insert(id, health);
        }
        
        // Update failure count
        {
            let mut failure_counts = self.failure_counts.lock()
                .map_err(|_| Error::Other("Failed to lock failure counts".to_string()))?;
            
            if health == ServerHealth::Unhealthy {
                let count = failure_counts.entry(id).or_insert(0);
                *count += 1;
                
                // Auto-restart if needed
                if self.config.auto_restart && *count >= self.config.max_consecutive_failures {
                    // Reset count
                    *count = 0;
                    
                    // Record restart event
                    self.lifecycle_manager.record_event(
                        id,
                        name.to_string(),
                        ServerLifecycleEvent::Restarted,
                        Some("Auto-restart after consecutive failures".to_string()),
                    )?;
                    
                    // In a real implementation, we would trigger a restart here
                }
            } else {
                // Reset count on successful check
                failure_counts.insert(id, 0);
            }
        }
        
        Ok(health)
    }
    
    /// Get all health statuses
    pub fn get_all_health(&self) -> Result<HashMap<ServerId, ServerHealth>> {
        let health_statuses = self.health_statuses.lock()
            .map_err(|_| Error::Other("Failed to lock health statuses".to_string()))?;
        
        Ok(health_statuses.clone())
    }
}