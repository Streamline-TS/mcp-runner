// src/server/process.rs
use crate::config::ServerConfig;
use crate::error::{Error, Result};
use std::fmt;
use std::process::Stdio;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tracing;
use uuid::Uuid; // Import tracing

/// Unique identifier for a server process
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerId(Uuid);

impl ServerId {
    // Private constructor, only usable within our crate
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

// Implement Display trait
impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a server process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    /// Server is starting
    Starting,
    /// Server is running
    Running,
    /// Server is stopping
    Stopping,
    /// Server has stopped
    Stopped,
    /// Server failed to start or crashed
    Failed,
}

/// Represents a running MCP server process.
///
/// Manages the lifecycle of a single MCP server, including starting, stopping,
/// and providing access to its standard I/O streams.
/// All public methods are instrumented with `tracing` spans.
pub struct ServerProcess {
    /// Server configuration
    config: ServerConfig,
    /// Server name
    name: String,
    /// Server ID
    id: ServerId,
    /// Child process
    child: Option<Child>,
    /// Server status
    status: ServerStatus,
}

impl ServerProcess {
    /// Create a new server process instance.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(config), fields(server_name = %name))]
    pub fn new(name: String, config: ServerConfig) -> Self {
        Self {
            config,
            name,
            id: ServerId::new(),
            child: None,
            status: ServerStatus::Stopped,
        }
    }

    /// Get the server ID
    pub fn id(&self) -> ServerId {
        self.id
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the current status of the server.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %self.name, server_id = %self.id))]
    pub fn status(&self) -> ServerStatus {
        self.status
    }

    /// Start the server process.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %self.name, server_id = %self.id))]
    pub async fn start(&mut self) -> Result<()> {
        if self.child.is_some() {
            tracing::warn!("Attempted to start an already running server");
            return Err(Error::AlreadyRunning);
        }

        tracing::info!("Starting server process");
        self.status = ServerStatus::Starting;

        let mut command = Command::new(&self.config.command);
        command.args(&self.config.args);

        // Set the process group ID to 0 (use new process group)
        // This prevents SIGTSTP/SIGSTOP from propagating to parent
        command.process_group(0);

        // Set environment variables
        for (key, value) in &self.config.env {
            command.env(key, value);
        }

        // Configure stdio
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Start the process
        tracing::debug!(command = ?self.config.command, args = ?self.config.args, env = ?self.config.env, "Spawning process");
        let child = command.spawn().map_err(|e| {
            tracing::error!("Failed to spawn process: {}", e);
            Error::Process(format!("Failed to start process: {}", e))
        })?;

        self.child = Some(child);
        self.status = ServerStatus::Running;
        tracing::info!("Server process started successfully");

        Ok(())
    }

    /// Stop the server process.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %self.name, server_id = %self.id))]
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            tracing::info!("Stopping server process");
            self.status = ServerStatus::Stopping;

            // Try to kill the process gracefully
            if let Err(e) = child.kill().await {
                tracing::error!("Failed to kill process: {}", e);
                // We still attempt to wait for the process below, so don't return early
                // return Err(Error::Process(format!("Failed to kill process: {}", e)));
            }

            // Wait for the process to exit
            match child.wait().await {
                Ok(status) => tracing::info!(exit_status = ?status, "Server process stopped"),
                Err(e) => tracing::warn!("Failed to get exit status after stopping: {}", e),
            }

            self.status = ServerStatus::Stopped;
            Ok(())
        } else {
            tracing::warn!("Attempted to stop a server that was not running");
            Err(Error::NotRunning)
        }
    }

    /// Take ownership of the server's stdin handle.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %self.name, server_id = %self.id))]
    pub fn take_stdin(&mut self) -> Result<ChildStdin> {
        if let Some(child) = &mut self.child {
            child.stdin.take().ok_or_else(|| {
                Error::Process("Failed to get stdin pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }

    /// Take ownership of the server's stdout handle.
    ///
    /// This method is instrumented with `tracing`.
    #[tracing::instrument(skip(self), fields(server_name = %self.name, server_id = %self.id))]
    pub fn take_stdout(&mut self) -> Result<ChildStdout> {
        if let Some(child) = &mut self.child {
            child.stdout.take().ok_or_else(|| {
                Error::Process("Failed to get stdout pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }

    /// Take the stderr pipe from the process
    pub fn take_stderr(&mut self) -> Result<ChildStderr> {
        if let Some(child) = &mut self.child {
            child.stderr.take().ok_or_else(|| {
                Error::Process("Failed to get stderr pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }
}

impl Clone for ServerProcess {
    fn clone(&self) -> Self {
        // Note: We can't clone the actual running process, so when cloning a ServerProcess,
        // we create a new instance with the same configuration but no running child process.
        // This is acceptable for our use case since the clone is only used for the SSE proxy
        // to check status information, not to control the actual process.
        Self {
            config: self.config.clone(),
            name: self.name.clone(),
            id: self.id,
            child: None, // We can't clone a running process
            status: self.status,
        }
    }
}
