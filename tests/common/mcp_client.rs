#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

use reqwest::Client as HttpClient;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::Mutex;
use tokio::time::timeout;
use truenas_master_mcp::service::{TrueNasServerImpl, run_stdio_with_io};

pub enum TransportBackend {
    Stdio {
        reader: Arc<Mutex<BufReader<DuplexStream>>>,
        writer: Arc<Mutex<DuplexStream>>,
    },
    Http {
        client: HttpClient,
        endpoint: String,
    },
    Sse {
        client: HttpClient,
        messages_endpoint: String,
        sse_endpoint: String,
    },
}

pub struct McpTestClient {
    backend: TransportBackend,
    next_id: AtomicU64,
}

impl McpTestClient {
    /// Create an in-process MCP client running over stdio via tokio duplex channels.
    pub fn new_stdio(server: Arc<TrueNasServerImpl>) -> Self {
        let (client_write, server_read) = tokio::io::duplex(65536);
        let (server_write, client_read) = tokio::io::duplex(65536);

        tokio::spawn(async move {
            let _ = run_stdio_with_io(server, server_read, server_write).await;
        });

        Self {
            backend: TransportBackend::Stdio {
                reader: Arc::new(Mutex::new(BufReader::new(client_read))),
                writer: Arc::new(Mutex::new(client_write)),
            },
            next_id: AtomicU64::new(1),
        }
    }

    /// Create an MCP client targeting an HTTP server endpoint (e.g. `http://127.0.0.1:3000/mcp`).
    pub fn new_http(endpoint: String) -> Self {
        Self {
            backend: TransportBackend::Http {
                client: HttpClient::new(),
                endpoint,
            },
            next_id: AtomicU64::new(1),
        }
    }

    /// Create an MCP client targeting an SSE server (`/sse` and `/messages`).
    pub fn new_sse(base_url: String) -> Self {
        Self {
            backend: TransportBackend::Sse {
                client: HttpClient::new(),
                messages_endpoint: format!("{}/messages", base_url.trim_end_matches('/')),
                sse_endpoint: format!("{}/sse", base_url.trim_end_matches('/')),
            },
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    pub async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });

        if let Some(p) = params {
            req["params"] = p;
        }

        match &self.backend {
            TransportBackend::Stdio { reader, writer } => {
                let mut line = req.to_string();
                line.push('\n');

                let mut w = writer.lock().await;
                w.write_all(line.as_bytes())
                    .await
                    .map_err(|e| format!("write error: {e}"))?;
                w.flush().await.map_err(|e| format!("flush error: {e}"))?;
                drop(w);

                let mut r = reader.lock().await;
                let mut resp_line = String::new();
                let read_future = r.read_line(&mut resp_line);
                let bytes_read = timeout(Duration::from_secs(5), read_future)
                    .await
                    .map_err(|_| "stdio response timeout (5s)".to_string())?
                    .map_err(|e| format!("read error: {e}"))?;

                if bytes_read == 0 {
                    return Err("EOF from stdio server".to_string());
                }

                let resp: Value = serde_json::from_str(&resp_line)
                    .map_err(|e| format!("invalid JSON from server: {e} (got: {resp_line})"))?;

                if let Some(err) = resp.get("error") {
                    return Err(err.to_string());
                }

                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
            TransportBackend::Http { client, endpoint } => {
                let res = client
                    .post(endpoint)
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| format!("HTTP post failed: {e}"))?;

                if !res.status().is_success() {
                    return Err(format!("HTTP status error: {}", res.status()));
                }

                let resp: Value = res
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse HTTP JSON: {e}"))?;

                if let Some(err) = resp.get("error") {
                    return Err(err.to_string());
                }

                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
            TransportBackend::Sse {
                client,
                messages_endpoint,
                ..
            } => {
                let res = client
                    .post(messages_endpoint)
                    .json(&req)
                    .send()
                    .await
                    .map_err(|e| format!("SSE POST /messages failed: {e}"))?;

                let resp: Value = res
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse SSE JSON: {e}"))?;

                if let Some(err) = resp.get("error") {
                    return Err(err.to_string());
                }

                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
        }
    }

    /// Send raw string without structured JSON-RPC (for testing parse error -32700).
    pub async fn send_raw(&self, raw_content: &str) -> Result<Value, String> {
        match &self.backend {
            TransportBackend::Stdio { reader, writer } => {
                let mut line = raw_content.to_string();
                if !line.ends_with('\n') {
                    line.push('\n');
                }

                let mut w = writer.lock().await;
                w.write_all(line.as_bytes())
                    .await
                    .map_err(|e| format!("write error: {e}"))?;
                w.flush().await.map_err(|e| format!("flush error: {e}"))?;
                drop(w);

                let mut r = reader.lock().await;
                let mut resp_line = String::new();
                let bytes_read = timeout(Duration::from_secs(5), r.read_line(&mut resp_line))
                    .await
                    .map_err(|_| "stdio response timeout (5s)".to_string())?
                    .map_err(|e| format!("read error: {e}"))?;

                if bytes_read == 0 {
                    return Err("EOF from server".to_string());
                }

                let resp: Value = serde_json::from_str(&resp_line)
                    .map_err(|e| format!("invalid JSON: {e} (got: {resp_line})"))?;
                Ok(resp)
            }
            TransportBackend::Http { client, endpoint } => {
                let res = client
                    .post(endpoint)
                    .header("content-type", "application/json")
                    .body(raw_content.to_string())
                    .send()
                    .await
                    .map_err(|e| format!("HTTP post failed: {e}"))?;

                let resp: Value = res
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse HTTP JSON: {e}"))?;
                Ok(resp)
            }
            TransportBackend::Sse {
                client,
                messages_endpoint,
                ..
            } => {
                let res = client
                    .post(messages_endpoint)
                    .header("content-type", "application/json")
                    .body(raw_content.to_string())
                    .send()
                    .await
                    .map_err(|e| format!("SSE POST failed: {e}"))?;

                let resp: Value = res
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse JSON: {e}"))?;
                Ok(resp)
            }
        }
    }

    /// Send a notification (no response expected).
    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let mut req = json!({
            "jsonrpc": "2.0",
            "method": method,
        });

        if let Some(p) = params {
            req["params"] = p;
        }

        match &self.backend {
            TransportBackend::Stdio { writer, .. } => {
                let mut line = req.to_string();
                line.push('\n');
                let mut w = writer.lock().await;
                w.write_all(line.as_bytes())
                    .await
                    .map_err(|e| format!("write error: {e}"))?;
                w.flush().await.map_err(|e| format!("flush error: {e}"))?;
                Ok(())
            }
            TransportBackend::Http { client, endpoint } => {
                let _ = client.post(endpoint).json(&req).send().await;
                Ok(())
            }
            TransportBackend::Sse {
                client,
                messages_endpoint,
                ..
            } => {
                let _ = client.post(messages_endpoint).json(&req).send().await;
                Ok(())
            }
        }
    }

    /// Perform MCP initialize handshake.
    pub async fn initialize(&self) -> Result<Value, String> {
        self.send_request(
            "initialize",
            Some(json!({
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "mcp-test-harness",
                    "version": "1.0.0"
                },
                "capabilities": {}
            })),
        )
        .await
    }

    /// Send `notifications/initialized`.
    pub async fn initialized(&self) -> Result<(), String> {
        self.send_notification("notifications/initialized", None)
            .await
    }

    /// Send `ping`.
    pub async fn ping(&self) -> Result<(), String> {
        let _ = self.send_request("ping", None).await?;
        Ok(())
    }

    /// List tools via `tools/list`.
    pub async fn list_tools(&self) -> Result<Vec<Value>, String> {
        let res = self.send_request("tools/list", None).await?;
        let tools = res
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools)
    }

    /// Call a tool via `tools/call`.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let params = json!({
            "name": name,
            "arguments": arguments,
        });
        self.send_request("tools/call", Some(params)).await
    }

    /// List resources via `resources/list`.
    pub async fn list_resources(&self) -> Result<Vec<Value>, String> {
        let res = self.send_request("resources/list", None).await?;
        let resources = res
            .get("resources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(resources)
    }

    /// Read resource via `resources/read`.
    pub async fn read_resource(&self, uri: &str) -> Result<Value, String> {
        let params = json!({ "uri": uri });
        self.send_request("resources/read", Some(params)).await
    }

    /// List prompts via `prompts/list`.
    pub async fn list_prompts(&self) -> Result<Vec<Value>, String> {
        let res = self.send_request("prompts/list", None).await?;
        let prompts = res
            .get("prompts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(prompts)
    }

    /// Get prompt via `prompts/get`.
    pub async fn get_prompt(&self, name: &str, arguments: Option<Value>) -> Result<Value, String> {
        let mut params = json!({ "name": name });
        if let Some(args) = arguments {
            params["arguments"] = args;
        }
        self.send_request("prompts/get", Some(params)).await
    }
}
