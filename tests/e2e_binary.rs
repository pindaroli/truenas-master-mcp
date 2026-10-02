#![allow(clippy::unwrap_used)]

mod common;

use common::mock_truenas::MockTrueNas;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

fn get_bin_path() -> String {
    env!("CARGO_BIN_EXE_truenas-master-mcp").to_string()
}

#[tokio::test]
async fn test_binary_help_and_version() {
    let bin = get_bin_path();

    // 1. Test --help
    let help_output = Command::new(&bin)
        .arg("--help")
        .output()
        .await
        .expect("failed to execute binary --help");
    assert!(help_output.status.success());
    let stdout = String::from_utf8_lossy(&help_output.stdout);
    assert!(stdout.contains("Official MCP server for TrueNAS API access"));
    assert!(stdout.contains("--transport"));
    assert!(stdout.contains("--readonly"));

    // 2. Test --version
    let ver_output = Command::new(&bin)
        .arg("--version")
        .output()
        .await
        .expect("failed to execute binary --version");
    assert!(ver_output.status.success());
    let ver_str = String::from_utf8_lossy(&ver_output.stdout);
    assert!(ver_str.contains("truenas-master-mcp"));
}

#[tokio::test]
async fn test_binary_stdio_process_lifecycle_and_tools() {
    let mock = MockTrueNas::start().await;
    let bin = get_bin_path();

    // Spawn child process in stdio mode
    let mut child = Command::new(&bin)
        .arg("--transport")
        .arg("stdio")
        .env("TRUENAS_SERVER_URL", mock.url())
        .env("TRUENAS_API_KEY", "test-api-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn truenas-master-mcp child process");

    let mut stdin = child.stdin.take().expect("Child stdin unavailable");
    let stdout = child.stdout.take().expect("Child stdout unavailable");
    let mut reader = BufReader::new(stdout);

    // 1. Send initialize
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "clientInfo": { "name": "e2e-subproc", "version": "1.0" },
            "capabilities": {}
        }
    });
    stdin
        .write_all(format!("{}\n", init_req).as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let mut line = String::new();
    let _ = timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("Timeout waiting for initialize response")
        .unwrap();

    let init_resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(init_resp["result"]["protocolVersion"], "2024-11-05");

    // 2. Send tools/call list_pools
    let tool_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "list_pools",
            "arguments": {}
        }
    });
    line.clear();
    stdin
        .write_all(format!("{}\n", tool_req).as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let _ = timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("Timeout waiting for list_pools response")
        .unwrap();

    let tool_resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(tool_resp["result"]["isError"], false);
    assert!(
        tool_resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("tank")
    );

    // 3. Close stdin to test graceful termination (EOF handling)
    drop(stdin);

    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("Child process did not exit in time")
        .unwrap();

    assert!(status.success(), "Child process should exit cleanly on EOF");
}

#[tokio::test]
async fn test_binary_cli_readonly_flag_enforcement() {
    let mock = MockTrueNas::start().await;
    let bin = get_bin_path();

    // Spawn child with --readonly flag
    let mut child = Command::new(&bin)
        .arg("--transport")
        .arg("stdio")
        .arg("--readonly")
        .env("TRUENAS_SERVER_URL", mock.url())
        .env("TRUENAS_API_KEY", "test-api-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn truenas-master-mcp with --readonly");

    let mut stdin = child.stdin.take().expect("Child stdin unavailable");
    let stdout = child.stdout.take().expect("Child stdout unavailable");
    let mut reader = BufReader::new(stdout);

    // Call mutating tool: scrub_pool
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "scrub_pool",
            "arguments": { "pool_name": "tank" }
        }
    });
    stdin
        .write_all(format!("{}\n", req).as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();

    let mut line = String::new();
    let _ = timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("Timeout waiting for scrub response")
        .unwrap();

    let resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Server is in readonly mode"));

    // Close stdin and verify graceful exit
    drop(stdin);
    let status = timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
}
