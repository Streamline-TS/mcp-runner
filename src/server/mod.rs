/// Server management module for MCP Runner.
///
/// This module handles the lifecycle, monitoring, and process management of MCP servers.
/// It provides functionality to start, stop, and monitor the health of MCP server processes.
/// All public components are instrumented with `tracing` spans.
///
/// # Components
///
/// * `lifecycle` - Manages server lifecycle events and statuses
/// * `monitor` - Health monitoring and automatic recovery of servers  
/// * `process` - Core process management for server instances
///
/// # Examples
///
/// Managing server lifecycle:
///
/// ```no_run
/// use mcp_runner::server::{ServerLifecycleManager, ServerLifecycleEvent, ServerProcess};
/// use mcp_runner::config::ServerConfig;
/// use std::sync::Arc;
/// use std::collections::HashMap;
///
/// // Create a server process to get a valid ServerId
/// let config = ServerConfig {
///     command: "sample".to_string(),
///     args: vec![],
///     env: HashMap::new(),
/// };
/// let server_process = ServerProcess::new("fetch-server".to_string(), config);
/// let server_id = server_process.id();
/// let server_name = "fetch-server".to_string();
///
/// // Create lifecycle manager and record an event
/// let manager = Arc::new(ServerLifecycleManager::new());
/// manager.record_event(
///     server_id,
///     server_name,
///     ServerLifecycleEvent::Started,
///     Some("Server started successfully".to_string())
/// ).unwrap();
/// ```
///
/// Monitoring server health:
///
/// ```no_run
/// use mcp_runner::server::{ServerMonitor, ServerMonitorConfig, ServerLifecycleManager};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let lifecycle_manager = Arc::new(ServerLifecycleManager::new());
/// let config = ServerMonitorConfig {
///     check_interval: Duration::from_secs(30),
///     health_check_timeout: Duration::from_secs(5),
///     auto_restart: true,
///     max_consecutive_failures: 3,
/// };
///
/// // Create and start the monitor
/// let mut monitor = ServerMonitor::new(lifecycle_manager, config);
/// monitor.start().unwrap();
/// ```
pub mod lifecycle;
pub mod monitor;
mod process;

pub use lifecycle::{ServerEvent, ServerLifecycleEvent, ServerLifecycleManager};
pub use monitor::{ServerHealth, ServerMonitor, ServerMonitorConfig};
pub use process::{ServerId, ServerProcess, ServerStatus};
