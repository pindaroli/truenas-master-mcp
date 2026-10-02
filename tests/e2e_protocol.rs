#![allow(clippy::unwrap_used)]

mod common;

use common::create_test_server;
use common::mcp_client::McpTestClient;
use common::mock_truenas::MockTrueNas;
use serde_json::Value;
use truenas_master_mcp::server::ToolConfig;

#[tokio::test]
async fn test_handshake_initialize_and_capabilities() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client.initialize().await.unwrap();

    // Verify protocol version conforms to MCP spec 2024-11-05
    assert_eq!(res["protocolVersion"], "2024-11-05");

    // Verify server info
    assert_eq!(res["serverInfo"]["name"], "truenas-master-mcp");
    assert!(!res["serverInfo"]["version"].as_str().unwrap().is_empty());
    assert_eq!(res["serverInfo"]["readonly"], false);

    // Verify capabilities
    assert!(res["capabilities"]["tools"].is_object());
    assert_eq!(res["capabilities"]["resources"]["list"], true);
    assert_eq!(res["capabilities"]["resources"]["read"], true);
    assert_eq!(res["capabilities"]["prompts"]["list"], true);
    assert_eq!(res["capabilities"]["prompts"]["get"], true);
}

#[tokio::test]
async fn test_lifecycle_initialized_notification_and_ping() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // Handshake
    client.initialize().await.unwrap();

    // Notification: initialized
    client.initialized().await.unwrap();

    // Ping
    client.ping().await.unwrap();
}

#[tokio::test]
async fn test_protocol_error_parse_error() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client.send_raw("THIS IS NOT JSON").await.unwrap();

    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], Value::Null);
    assert_eq!(res["error"]["code"], -32700);
    assert!(
        res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Parse error")
    );
}

#[tokio::test]
async fn test_protocol_error_missing_method() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let res = client
        .send_raw(r#"{"jsonrpc": "2.0", "id": 42}"#)
        .await
        .unwrap();

    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], 42);
    assert_eq!(res["error"]["code"], -32600);
    assert!(
        res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Missing method")
    );
}

#[tokio::test]
async fn test_protocol_error_method_not_found() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    let err = client
        .send_request("non_existent_method_xyz", None)
        .await
        .unwrap_err();

    assert!(err.contains("-32601"));
    assert!(err.contains("Method not found: non_existent_method_xyz"));
}

#[tokio::test]
async fn test_unknown_notification_silently_ignored() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());
    let client = McpTestClient::new_stdio(server);

    // Notifications without id should be safely ignored without generating responses
    client
        .send_notification("custom/random_notification", None)
        .await
        .unwrap();

    // The stream should still be fully functional for following requests
    let ping_res = client.ping().await;
    assert!(ping_res.is_ok());
}
