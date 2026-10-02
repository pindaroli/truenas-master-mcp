#![allow(clippy::unwrap_used)]

mod common;

use common::create_test_server;
use common::mcp_client::McpTestClient;
use common::mock_truenas::MockTrueNas;
use serde_json::json;
use truenas_master_mcp::server::{ToolCategory, ToolConfig};

#[tokio::test]
async fn test_readonly_mode_allows_queries_blocks_mutations() {
    let mock = MockTrueNas::start().await;
    let config = ToolConfig {
        readonly: true,
        enabled_categories: vec![ToolCategory::All],
        disabled_categories: vec![],
    };
    let server = create_test_server(&mock.url(), config);
    let client = McpTestClient::new_stdio(server);

    // Read tool: list_pools should SUCCEED
    let pools_res = client.call_tool("list_pools", json!({})).await.unwrap();
    assert_eq!(pools_res["isError"], false);

    // Read tool: list_users should SUCCEED
    let users_res = client.call_tool("list_users", json!({})).await.unwrap();
    assert_eq!(users_res["isError"], false);

    // Mutation tool: scrub_pool should be BLOCKED
    let scrub_res = client
        .call_tool("scrub_pool", json!({ "pool_name": "tank" }))
        .await
        .unwrap();
    assert_eq!(scrub_res["isError"], true);
    let scrub_text = scrub_res["content"][0]["text"].as_str().unwrap();
    assert!(scrub_text.contains("Server is in readonly mode - modification tools are disabled"));

    // Mutation tool: create_user should be BLOCKED
    let create_user_res = client
        .call_tool(
            "create_user",
            json!({ "username": "baduser", "password": "password123" }),
        )
        .await
        .unwrap();
    assert_eq!(create_user_res["isError"], true);
    let create_text = create_user_res["content"][0]["text"].as_str().unwrap();
    assert!(create_text.contains("Server is in readonly mode - modification tools are disabled"));
}

#[tokio::test]
async fn test_disable_category_blocks_tools_in_that_category() {
    let mock = MockTrueNas::start().await;
    let config = ToolConfig {
        readonly: false,
        enabled_categories: vec![ToolCategory::All],
        disabled_categories: vec![ToolCategory::Users],
    };
    let server = create_test_server(&mock.url(), config);
    let client = McpTestClient::new_stdio(server);

    // Tool in allowed category (Pools) should SUCCEED
    let pools_res = client.call_tool("list_pools", json!({})).await.unwrap();
    assert_eq!(pools_res["isError"], false);

    // Tool in disabled category (Users) should be BLOCKED
    let users_res = client.call_tool("list_users", json!({})).await.unwrap();
    assert_eq!(users_res["isError"], true);
    let users_text = users_res["content"][0]["text"].as_str().unwrap();
    assert!(users_text.contains("Category Users is not enabled"));

    // Mutation tool in disabled category should also be BLOCKED
    let create_user_res = client
        .call_tool(
            "create_user",
            json!({ "username": "blocked", "password": "secret" }),
        )
        .await
        .unwrap();
    assert_eq!(create_user_res["isError"], true);
    let create_text = create_user_res["content"][0]["text"].as_str().unwrap();
    assert!(create_text.contains("Category Users is not enabled"));
}

#[tokio::test]
async fn test_enable_only_specific_category() {
    let mock = MockTrueNas::start().await;
    let config = ToolConfig {
        readonly: false,
        enabled_categories: vec![ToolCategory::Pools],
        disabled_categories: vec![],
    };
    let server = create_test_server(&mock.url(), config);
    let client = McpTestClient::new_stdio(server);

    // Pools is explicitly enabled
    let pools_res = client.call_tool("list_pools", json!({})).await.unwrap();
    assert_eq!(pools_res["isError"], false);

    // Users was not enabled -> BLOCKED
    let users_res = client.call_tool("list_users", json!({})).await.unwrap();
    assert_eq!(users_res["isError"], true);
    assert!(
        users_res["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Category Users is not enabled")
    );

    // System was not enabled -> BLOCKED
    let sys_res = client
        .call_tool("get_system_info", json!({}))
        .await
        .unwrap();
    assert_eq!(sys_res["isError"], true);
    assert!(
        sys_res["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Category System is not enabled")
    );
}
