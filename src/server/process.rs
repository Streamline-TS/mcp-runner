// src/server/process.rs
use crate::config::ServerConfig;
use crate::error::{Error, Result};
use async_process::{Child, Command, Stdio};
use std::fmt;
use uuid::Uuid; // Import fmt

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

/// A running MCP server process
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
    /// Create a new server process from configuration
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

    /// Get the server status
    pub fn status(&self) -> ServerStatus {
        self.status
    }

    /// Start the server process
    pub async fn start(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Err(Error::AlreadyRunning);
        }

        self.status = ServerStatus::Starting;

        let mut command = Command::new(&self.config.command);
        command.args(&self.config.args);

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
        let child = command
            .spawn()
            .map_err(|e| Error::Process(format!("Failed to start process: {}", e)))?;

        self.child = Some(child);
        self.status = ServerStatus::Running;

        Ok(())
    }

    /// Stop the server process
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            self.status = ServerStatus::Stopping;

            // Try to kill the process gracefully
            if let Err(e) = child.kill() {
                return Err(Error::Process(format!("Failed to kill process: {}", e)));
            }

            // Wait for the process to exit
            let _ = child.status().await;

            self.status = ServerStatus::Stopped;
            Ok(())
        } else {
            Err(Error::NotRunning)
        }
    }

    /// Take the stdin pipe from the process
    pub fn take_stdin(&mut self) -> Result<async_process::ChildStdin> {
        if let Some(child) = &mut self.child {
            child.stdin.take().ok_or_else(|| {
                Error::Process("Failed to get stdin pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }

    /// Take the stdout pipe from the process
    pub fn take_stdout(&mut self) -> Result<async_process::ChildStdout> {
        if let Some(child) = &mut self.child {
            child.stdout.take().ok_or_else(|| {
                Error::Process("Failed to get stdout pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }

    /// Take the stderr pipe from the process
    pub fn take_stderr(&mut self) -> Result<async_process::ChildStderr> {
        if let Some(child) = &mut self.child {
            child.stderr.take().ok_or_else(|| {
                Error::Process("Failed to get stderr pipe from child process".to_string())
            })
        } else {
            Err(Error::NotRunning)
        }
    }
}
