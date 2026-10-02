#![allow(clippy::unwrap_used)]

mod common;

use common::create_test_server;
use common::mcp_client::McpTestClient;
use common::mock_truenas::MockTrueNas;
use serde_json::{Value, json};
use truenas_master_mcp::server::ToolConfig;

#[tokio::test]
async fn test_resources_list_and_read() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // 1. List resources
    let resources = client.list_resources().await.unwrap();
    assert!(!resources.is_empty());

    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(Value::as_str))
        .collect();

    assert!(uris.contains(&"truenas://system/info"));
    assert!(uris.contains(&"truenas://pools/list"));
    assert!(uris.contains(&"truenas://datasets/tree"));

    // 2. Read resource: truenas://system/info
    let read_res = client.read_resource("truenas://system/info").await.unwrap();
    let contents = read_res.get("contents").and_then(Value::as_array).unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], "truenas://system/info");
    assert_eq!(contents[0]["mimeType"], "application/json");

    let text = contents[0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["hostname"], "truenas.homelab");

    // 3. Read unknown resource
    let err = client
        .read_resource("truenas://unknown/resource")
        .await
        .unwrap_err();
    assert!(err.contains("Resource error") || err.contains("not found"));
}

#[tokio::test]
async fn test_prompts_list_and_get() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // 1. List prompts
    let prompts = client.list_prompts().await.unwrap();
    assert!(!prompts.is_empty());

    let prompt_names: Vec<&str> = prompts
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str))
        .collect();

    assert!(prompt_names.contains(&"system-overview"));
    assert!(prompt_names.contains(&"health-check"));
    assert!(prompt_names.contains(&"app-maintenance-guide"));

    // 2. Get prompt: health-check (aggregates pools, disks, services, apps)
    let health_prompt = client.get_prompt("health-check", None).await.unwrap();
    assert!(health_prompt["description"].is_string());
    let messages = health_prompt["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    let text = messages[0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("TrueNAS Health Check Report"));
    assert!(text.contains("Pools:"));

    // 3. Get prompt with missing required argument: app-maintenance-guide
    let err = client
        .get_prompt("app-maintenance-guide", None)
        .await
        .unwrap_err();
    assert!(err.contains("app_name argument is required"));

    // 4. Get prompt with valid argument: app-maintenance-guide
    let app_prompt = client
        .get_prompt("app-maintenance-guide", Some(json!({ "app_name": "plex" })))
        .await
        .unwrap();
    let app_msg = app_prompt["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(app_msg.contains("Maintenance Guide for plex"));
    assert!(app_msg.contains("RUNNING"));
}
