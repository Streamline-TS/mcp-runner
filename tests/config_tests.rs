use mcp_runner::config::{Config, ServerConfig};
use mcp_runner::error::Result;
use std::collections::HashMap;

#[test]
fn test_parse_config() -> Result<()> {
    let config_str = r#"{
        "mcpServers": {
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/files"]
            },
            "github": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-github"],
                "env": {
                    "GITHUB_TOKEN": "your_token_here"
                }
            }
        }
    }"#;

    let config = Config::parse_from_str(config_str)?;

    assert_eq!(config.mcp_servers.len(), 2);
    assert!(config.mcp_servers.contains_key("filesystem"));
    assert!(config.mcp_servers.contains_key("github"));

    let fs_config = &config.mcp_servers["filesystem"];
    assert_eq!(fs_config.command, "npx");
    assert_eq!(
        fs_config.args,
        vec![
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "/path/to/files"
        ]
    );
    assert!(fs_config.env.is_empty());

    let gh_config = &config.mcp_servers["github"];
    assert_eq!(gh_config.command, "npx");
    assert_eq!(
        gh_config.args,
        vec!["-y", "@modelcontextprotocol/server-github"]
    );
    assert_eq!(
        gh_config.env.get("GITHUB_TOKEN"),
        Some(&"your_token_here".to_string())
    );

    Ok(())
}

#[test]
fn test_validate_config() -> Result<()> {
    let mut config_map = HashMap::new();

    let fs_config = ServerConfig {
        command: "npx".to_string(),
        args: vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-filesystem".to_string(),
            "/path".to_string(),
        ],
        env: HashMap::new(),
    };

    config_map.insert("filesystem".to_string(), fs_config);

    let gh_config = ServerConfig {
        command: "npx".to_string(),
        args: vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-github".to_string(),
        ],
        env: {
            let mut env = HashMap::new();
            env.insert("GITHUB_TOKEN".to_string(), "token".to_string());
            env
        },
    };

    config_map.insert("github".to_string(), gh_config);

    // Import the validator
    use mcp_runner::config::validate_config;

    // Validate the config
    validate_config(&config_map)?;

    // Test invalid config
    let mut invalid_config = HashMap::new();
    let invalid_server = ServerConfig {
        command: "".to_string(), // Empty command is invalid
        args: vec![],
        env: HashMap::new(),
    };

    invalid_config.insert("invalid".to_string(), invalid_server);

    // This should fail
    assert!(validate_config(&invalid_config).is_err());

    Ok(())
}

#[test]
fn test_parse_claude_config() -> Result<()> {
    let config_str = r#"{
        "mcpServers": {
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/files"]
            }
        }
    }"#;

    let config = Config::parse_from_str(config_str)?;

    assert_eq!(config.mcp_servers.len(), 1);
    assert!(config.mcp_servers.contains_key("filesystem"));

    let fs_config = &config.mcp_servers["filesystem"];
    assert_eq!(fs_config.command, "npx");
    assert_eq!(
        fs_config.args,
        vec![
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "/path/to/allowed/files"
        ]
    );

    Ok(())
}
