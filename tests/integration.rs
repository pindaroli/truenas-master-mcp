#![allow(clippy::unwrap_used)]

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use truenas_master_mcp::client::TrueNasClient;
use truenas_master_mcp::config::TrueNasConfig;

fn config_for(url: &str) -> TrueNasConfig {
    TrueNasConfig {
        server_url: url.to_string(),
        api_key: Some("test-api-key".to_string()),
        username: None,
        password: None,
        verify_ssl: false,
        timeout_secs: 5,
        version: Default::default(),
    }
}

async fn serve(handler: fn(Vec<Value>) -> Vec<Value>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let mut seen = Vec::new();
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.unwrap() else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).unwrap();
            seen.push(request.clone());
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let result = handler(seen.clone())
                .into_iter()
                .nth(seen.len() - 1)
                .unwrap();
            let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
            socket
                .send(Message::Text(response.to_string().into()))
                .await
                .unwrap();
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn pool_list_is_jsonrpc_query() {
    let url = serve(|seen| match seen.len() {
        1 => vec![json!(true)],
        _ => vec![json!(true), json!([{"name": "tank", "status": "ONLINE"}])],
    })
    .await;

    let client = TrueNasClient::new(config_for(&url)).unwrap();
    let pools = client.get::<Value>("/api/v2.0/pool").await.unwrap();
    assert_eq!(pools[0]["name"], "tank");
}

#[tokio::test]
async fn job_id_waits_and_unwraps_success() {
    let url = serve(|seen| match seen.len() {
        1 => vec![json!(true)],
        2 => vec![json!(true), json!(7)],
        _ => vec![
            json!(true),
            json!(7),
            json!({"id": 7, "state": "SUCCESS", "result": {"name": "stripe"}}),
        ],
    })
    .await;

    let client = TrueNasClient::new(config_for(&url)).unwrap();
    let result = client
        .post::<Value, _>("/api/v2.0/pool/scrub", &json!({"name": "stripe"}))
        .await
        .unwrap();
    assert_eq!(result["name"], "stripe");
}

#[tokio::test]
async fn rpc_error_is_returned() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        for _ in 0..2 {
            let frame = socket.next().await.unwrap().unwrap();
            let Message::Text(text) = frame else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).unwrap();
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let method = request.get("method").and_then(Value::as_str).unwrap_or("");
            let result = if method.starts_with("auth.") {
                json!({"jsonrpc": "2.0", "id": id, "result": true})
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32001, "message": "method call error", "data": {"reason": "missing"}}
                })
            };
            socket
                .send(Message::Text(result.to_string().into()))
                .await
                .unwrap();
        }
    });

    let client = TrueNasClient::new(config_for(&format!("http://{addr}"))).unwrap();
    let error = client.get::<Value>("/api/v2.0/pool").await.unwrap_err();
    assert!(error.to_string().contains("missing"));
}
