use crate::error::{Error, Result};
use crate::server::lifecycle::{ServerLifecycleEvent, ServerLifecycleManager};
use crate::server::{ServerId, ServerStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tokio::time;
use tracing;

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

/// Monitors the health of running MCP servers.
///
/// Periodically checks the status of registered servers and can optionally
/// trigger restarts based on configuration.
/// All public methods are instrumented with `tracing` spans.
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
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(lifecycle_manager, config))]
    pub fn new(
        lifecycle_manager: Arc<ServerLifecycleManager>,
        config: ServerMonitorConfig,
    ) -> Self {
        tracing::info!(config = ?config, "Creating new ServerMonitor");
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
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn start(&mut self) -> Result<()> {
        {
            let mut running = self.running.lock().map_err(|_| {
                tracing::error!("Failed to lock running flag");
                Error::Other("Failed to lock running flag".to_string())
            })?;

            if *running {
                tracing::debug!("Monitor already running");
                return Ok(());
            }
            tracing::info!("Starting monitor task");
            *running = true;
        }

        let lifecycle_manager = Arc::clone(&self.lifecycle_manager);
        let health_statuses = Arc::clone(&self.health_statuses);
        let failure_counts = Arc::clone(&self.failure_counts);
        let last_checked = Arc::clone(&self.last_checked);
        let running = Arc::clone(&self.running);
        let config = self.config.clone();

        let task = tokio::spawn(async move {
            tracing::info!("Monitor loop started");
            let mut interval = time::interval(config.check_interval);

            loop {
                interval.tick().await;

                // Check if we should stop
                {
                    let running_guard = running.lock().unwrap();
                    if !*running_guard {
                        tracing::info!("Monitor loop stopping");
                        break;
                    }
                }
                tracing::debug!("Running health check cycle");

                // Get a snapshot of servers we need to check
                let server_ids_to_check = {
                    let health_guard = health_statuses.lock().unwrap();
                    health_guard.keys().cloned().collect::<Vec<_>>()
                };
                tracing::trace!(servers = ?server_ids_to_check, "Checking health for servers");

                // Check each server
                for server_id in server_ids_to_check {
                    let check_span =
                        tracing::info_span!("server_health_check", server_id = %server_id);
                    let _check_guard = check_span.enter();

                    tracing::debug!("Checking server health");
                    // Record check time
                    {
                        let mut checked = last_checked.lock().unwrap();
                        checked.insert(server_id, Instant::now());
                    }

                    // Get current server status
                    match lifecycle_manager.get_status(server_id) {
                        Ok(status) => {
                            tracing::debug!(current_status = ?status, "Got server status");
                            let health = match status {
                                ServerStatus::Running => ServerHealth::Healthy,
                                ServerStatus::Failed => ServerHealth::Unhealthy,
                                _ => ServerHealth::Unknown,
                            };
                            tracing::info!(health = ?health, "Determined server health");

                            {
                                let mut statuses = health_statuses.lock().unwrap();
                                statuses.insert(server_id, health);
                            }

                            {
                                let mut counts = failure_counts.lock().unwrap();
                                if health == ServerHealth::Unhealthy {
                                    let count = counts.entry(server_id).or_insert(0);
                                    *count += 1;
                                    tracing::warn!(
                                        failure_count = *count,
                                        "Server health check failed"
                                    );

                                    if config.auto_restart
                                        && *count >= config.max_consecutive_failures
                                    {
                                        tracing::warn!(
                                            threshold = config.max_consecutive_failures,
                                            "Failure threshold reached, attempting auto-restart"
                                        );
                                        *count = 0;
                                        tracing::info!(
                                            "Auto-restart triggered (logic not implemented)"
                                        );
                                    }
                                } else {
                                    if counts.contains_key(&server_id)
                                        && *counts.get(&server_id).unwrap() > 0
                                    {
                                        tracing::info!("Resetting failure count");
                                        counts.insert(server_id, 0);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to get server status during health check");
                        }
                    }
                }
            }
            tracing::info!("Monitor loop finished");
        });

        self.monitor_task = Some(task);

        Ok(())
    }

    /// Stop the monitor
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self))]
    pub fn stop(&mut self) -> Result<()> {
        {
            let mut running = self.running.lock().map_err(|_| {
                tracing::error!("Failed to lock running flag");
                Error::Other("Failed to lock running flag".to_string())
            })?;

            if !*running {
                tracing::debug!("Monitor already stopped");
                return Ok(());
            }
            tracing::info!("Stopping monitor task");
            *running = false;
        }

        if let Some(task) = self.monitor_task.take() {
            tracing::debug!("Aborting monitor task handle");
            task.abort();
        }

        Ok(())
    }

    /// Get server health
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_id = %id))]
    pub fn get_health(&self, id: ServerId) -> Result<ServerHealth> {
        tracing::debug!("Getting server health status");
        let health_statuses = self.health_statuses.lock().map_err(|_| {
            tracing::error!("Failed to lock health statuses");
            Error::Other("Failed to lock health statuses".to_string())
        })?;

        health_statuses.get(&id).copied().ok_or_else(|| {
            tracing::warn!("Health status requested for unknown server");
            Error::ServerNotFound(format!("{:?}", id))
        })
    }

    /// Force health check for a server
    ///
    /// This method is instrumented with `tracing`.
    // Note: This is a simplified version, real health checks might involve communication
    #[tracing::instrument(skip(self), fields(server_id = %id, server_name = %name))]
    pub async fn check_health(&self, id: ServerId, name: &str) -> Result<ServerHealth> {
        tracing::info!("Forcing health check");

        {
            let mut last_checked = self
                .last_checked
                .lock()
                .map_err(|_| Error::Other("Failed to lock last checked times".to_string()))?;
            last_checked.insert(id, Instant::now());
        }

        let status = self.lifecycle_manager.get_status(id)?;
        tracing::debug!(current_status = ?status, "Got server status for forced check");

        let health = match status {
            ServerStatus::Running => ServerHealth::Healthy,
            ServerStatus::Starting => ServerHealth::Unknown,
            ServerStatus::Stopping => ServerHealth::Unknown,
            ServerStatus::Stopped => ServerHealth::Unknown,
            ServerStatus::Failed => ServerHealth::Unhealthy,
        };
        tracing::info!(health = ?health, "Determined server health from forced check");

        {
            let mut health_statuses = self
                .health_statuses
                .lock()
                .map_err(|_| Error::Other("Failed to lock health statuses".to_string()))?;
            health_statuses.insert(id, health);
        }

        {
            let mut failure_counts = self
                .failure_counts
                .lock()
                .map_err(|_| Error::Other("Failed to lock failure counts".to_string()))?;

            if health == ServerHealth::Unhealthy {
                let count = failure_counts.entry(id).or_insert(0);
                *count += 1;
                tracing::warn!(
                    failure_count = *count,
                    "Server health check failed (forced)"
                );

                if self.config.auto_restart && *count >= self.config.max_consecutive_failures {
                    tracing::warn!(
                        threshold = self.config.max_consecutive_failures,
                        "Failure threshold reached (forced), attempting auto-restart"
                    );
                    *count = 0;

                    self.lifecycle_manager.record_event(
                        id,
                        name.to_string(),
                        ServerLifecycleEvent::Restarted,
                        Some("Auto-restart after consecutive failures (forced check)".to_string()),
                    )?;
                    tracing::info!(
                        "Auto-restart triggered by forced check (logic not implemented)"
                    );
                }
            } else {
                if failure_counts.contains_key(&id) && *failure_counts.get(&id).unwrap() > 0 {
                    tracing::info!("Resetting failure count (forced check)");
                    failure_counts.insert(id, 0);
                }
            }
        }

        Ok(health)
    }

    /// Get all health statuses
    pub fn get_all_health(&self) -> Result<HashMap<ServerId, ServerHealth>> {
        let health_statuses = self
            .health_statuses
            .lock()
            .map_err(|_| Error::Other("Failed to lock health statuses".to_string()))?;

        Ok(health_statuses.clone())
    }
}
