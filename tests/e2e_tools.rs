#![allow(clippy::unwrap_used)]

mod common;

use common::create_test_server;
use common::mcp_client::McpTestClient;
use common::mock_truenas::MockTrueNas;
use serde_json::{Value, json};
use truenas_master_mcp::server::ToolConfig;

#[tokio::test]
async fn test_tools_list_returns_complete_catalogue() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());

    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();

    // Check essential tools from various domains
    assert!(tool_names.contains(&"list_pools"));
    assert!(tool_names.contains(&"get_pool_status"));
    assert!(tool_names.contains(&"scrub_pool"));
    assert!(tool_names.contains(&"list_datasets"));
    assert!(tool_names.contains(&"create_dataset"));
    assert!(tool_names.contains(&"list_users"));
    assert!(tool_names.contains(&"create_user"));
    assert!(tool_names.contains(&"get_system_info"));
    assert!(tool_names.contains(&"list_apps"));

    // Verify inputSchema structure
    for tool in tools {
        assert!(tool.get("description").is_some());
        let schema = tool.get("inputSchema").expect("inputSchema must exist");
        assert_eq!(schema["type"], "object");
    }
}

#[tokio::test]
async fn test_query_tool_list_pools() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client.call_tool("list_pools", json!({})).await.unwrap();

    assert_eq!(res["isError"], false);
    let text = res["content"][0]["text"].as_str().unwrap();

    let pools: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0]["name"], "tank");
    assert_eq!(pools[0]["status"], "ONLINE");
}

#[tokio::test]
async fn test_query_tool_get_system_info() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client
        .call_tool("get_system_info", json!({}))
        .await
        .unwrap();

    assert_eq!(res["isError"], false);
    let text = res["content"][0]["text"].as_str().unwrap();
    let sys_info: Value = serde_json::from_str(text).unwrap();

    assert_eq!(sys_info["hostname"], "truenas.homelab");
    assert_eq!(sys_info["version"], "TrueNAS-SCALE-24.04.0");
}

#[tokio::test]
async fn test_mutating_tool_with_async_job_scrub_pool() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // Call scrub_pool which triggers TrueNAS job creation and polling
    let res = client
        .call_tool("scrub_pool", json!({ "pool_name": "tank" }))
        .await
        .unwrap();

    assert_eq!(res["isError"], false);
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("SCRUB_STARTED") || text.contains("tank"));
}

#[tokio::test]
async fn test_tool_error_propagation_from_backend() {
    let mock = MockTrueNas::start().await;
    // Inject backend failure
    mock.fail_method("pool.query", -32001, "Storage pool I/O error")
        .await;

    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client.call_tool("list_pools", json!({})).await.unwrap();

    assert_eq!(res["isError"], true);
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Error executing tool 'list_pools'"));
    assert!(text.contains("Storage pool I/O error"));
}

#[tokio::test]
async fn test_tool_missing_required_arguments() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // get_user requires user_id
    let res = client.call_tool("get_user", json!({})).await.unwrap();

    assert_eq!(res["isError"], true);
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Error executing tool 'get_user'"));
}
