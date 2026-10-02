#![allow(clippy::unwrap_used)]

mod common;

use common::create_test_server;
use common::mcp_client::McpTestClient;
use common::mock_truenas::MockTrueNas;
use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use truenas_master_mcp::server::ToolConfig;
use truenas_master_mcp::service::{build_http_router, build_sse_router};

#[tokio::test]
async fn test_transport_http_end_to_end() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());

    // Bind HTTP router to an ephemeral port
    let app = build_http_router(server);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let http_client = HttpClient::new();
    let base_url = format!("http://{}", addr);

    // 1. Health check GET /
    let health_res = http_client
        .get(&base_url)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(health_res["status"], "TrueNAS MCP Server running");

    // 2. MCP Client over HTTP
    let mcp = McpTestClient::new_http(format!("{}/mcp", base_url));

    // Initialize
    let init_res = mcp.initialize().await.unwrap();
    assert_eq!(init_res["protocolVersion"], "2024-11-05");

    // Tools call
    let pool_res = mcp.call_tool("list_pools", json!({})).await.unwrap();
    assert_eq!(pool_res["isError"], false);
    let text = pool_res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("tank"));

    // 3. CORS verification
    let cors_res = http_client
        .post(format!("{}/mcp", base_url))
        .header("Origin", "http://localhost:5173")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping"
        }))
        .send()
        .await
        .unwrap();

    assert!(
        cors_res
            .headers()
            .contains_key("access-control-allow-origin")
    );
}

#[tokio::test]
async fn test_transport_sse_end_to_end() {
    let mock = MockTrueNas::start().await;
    let server = create_test_server(&mock.url(), ToolConfig::default());

    // Bind SSE router to an ephemeral port
    let app = build_sse_router(server);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let http_client = HttpClient::new();
    let base_url = format!("http://{}", addr);

    // 1. Health check GET /
    let health_res = http_client
        .get(&base_url)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(health_res["status"], "TrueNAS MCP Server running");
    assert!(
        health_res["endpoints"]
            .as_array()
            .unwrap()
            .contains(&json!("/sse"))
    );
    assert!(
        health_res["endpoints"]
            .as_array()
            .unwrap()
            .contains(&json!("/messages"))
    );

    // 2. Connect to GET /sse to verify SSE stream handshake
    let sse_res = http_client
        .get(format!("{}/sse", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(sse_res.status(), reqwest::StatusCode::OK);
    let content_type = sse_res
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));

    // 3. MCP Client over SSE POST /messages
    let mcp = McpTestClient::new_sse(base_url);

    // Initialize
    let init_res = mcp.initialize().await.unwrap();
    assert_eq!(init_res["protocolVersion"], "2024-11-05");

    // Call get_system_info
    let sys_res = mcp.call_tool("get_system_info", json!({})).await.unwrap();
    assert_eq!(sys_res["isError"], false);
    let text = sys_res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("truenas.homelab"));
}
