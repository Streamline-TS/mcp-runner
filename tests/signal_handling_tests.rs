#[cfg(unix)]
mod test {
    use mcp_runner::{Config, McpRunner};
    use std::time::Duration;
    use tokio::time::sleep;

    // Helper function to send a signal to the current process
    async fn send_signal_to_self(sig: i32) {
        use std::process;
        let pid = process::id();

        // Use tokio::process to send a signal
        let status = tokio::process::Command::new("kill")
            .arg(format!("-{}", sig))
            .arg(format!("{}", pid))
            .status()
            .await
            .expect("Failed to send signal");

        assert!(status.success(), "Failed to send signal");
    }

    #[tokio::test]
    async fn test_sigtstp_handling() {
        // Create a McpRunner instance which should install the signal handlers
        let config = Config::parse_from_str(
            r#"
        {
            "mcpServers": {
                "test": {
                    "command": "echo",
                    "args": ["test"]
                }
            }
        }
        "#,
        )
        .expect("Failed to parse config");

        let _runner = McpRunner::new(config);

        // Wait a moment for the signal handlers to be installed
        sleep(Duration::from_millis(500)).await;

        // Send SIGTSTP to ourselves (signal 20)
        send_signal_to_self(20).await;

        // Wait a moment to ensure we don't get stopped
        sleep(Duration::from_millis(500)).await;

        // If we reach this point, the test passes because our process wasn't stopped
        // by the SIGTSTP signal
    }
}
