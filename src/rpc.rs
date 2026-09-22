//! JSON-RPC 2.0 over the TrueNAS WebSocket at `/api/current`.

use crate::config::TrueNasConfig;
use crate::error::{Result, TrueNasError};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};
use tracing::{debug, warn};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Pending = HashMap<u64, oneshot::Sender<std::result::Result<Value, CallFail>>>;

#[derive(Debug)]
enum CallFail {
    Rpc(String),
    Disconnected(String),
    Timeout(String),
    Setup(String),
}

fn fail_to_error(fail: CallFail) -> TrueNasError {
    match fail {
        CallFail::Rpc(message) => TrueNasError::ApiError {
            status: 500,
            message,
        },
        CallFail::Disconnected(message) | CallFail::Setup(message) => {
            TrueNasError::ConfigError(message)
        }
        CallFail::Timeout(message) => TrueNasError::TimeoutError(message),
    }
}

enum Outbound {
    Text(String),
    Pong(Vec<u8>),
}

#[derive(Clone)]
struct Conn {
    tx: mpsc::UnboundedSender<Outbound>,
    pending: Arc<Mutex<Pending>>,
}

/// Shared WebSocket session. Clones share one connection.
#[derive(Clone)]
pub struct RpcSession {
    inner: Arc<SessionInner>,
}

struct SessionInner {
    config: TrueNasConfig,
    ws_url: String,
    conn: Mutex<Option<Conn>>,
    next_id: AtomicU64,
}

impl std::fmt::Debug for RpcSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RpcSession")
            .field("url", &self.inner.ws_url)
            .finish()
    }
}

impl RpcSession {
    pub fn new(config: TrueNasConfig) -> Result<Self> {
        let ws_url = websocket_url(&config.server_url)?;
        Ok(Self {
            inner: Arc::new(SessionInner {
                config,
                ws_url,
                conn: Mutex::new(None),
                next_id: AtomicU64::new(1),
            }),
        })
    }

    pub async fn call(&self, method: &str, params: &[Value]) -> Result<Value> {
        match self.call_once(method, params).await {
            Ok(value) => Ok(value),
            Err(CallFail::Disconnected(_)) => {
                self.disconnect().await;
                self.call_once(method, params).await.map_err(fail_to_error)
            }
            Err(other) => Err(fail_to_error(other)),
        }
    }

    async fn call_once(
        &self,
        method: &str,
        params: &[Value],
    ) -> std::result::Result<Value, CallFail> {
        let conn = self
            .connection()
            .await
            .map_err(|error| CallFail::Setup(error.to_string()))?;
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        conn.pending.lock().await.insert(id, tx);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        debug!(method, id, "JSON-RPC call");
        if conn.tx.send(Outbound::Text(payload.to_string())).is_err() {
            conn.pending.lock().await.remove(&id);
            self.disconnect().await;
            return Err(CallFail::Disconnected(
                "TrueNAS WebSocket closed before the call was sent".to_string(),
            ));
        }
        let wait = timeout(Duration::from_secs(self.inner.config.timeout_secs), rx).await;
        match wait {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(fail))) => {
                if matches!(fail, CallFail::Disconnected(_)) {
                    self.disconnect().await;
                }
                Err(fail)
            }
            Ok(Err(_)) | Err(_) => {
                conn.pending.lock().await.remove(&id);
                Err(CallFail::Timeout(format!(
                    "{method} timed out after {}s",
                    self.inner.config.timeout_secs
                )))
            }
        }
    }

    async fn connection(&self) -> Result<Conn> {
        let mut guard = self.inner.conn.lock().await;
        if let Some(conn) = guard.as_ref()
            && !conn.tx.is_closed()
        {
            return Ok(conn.clone());
        }
        let conn = connect(&self.inner.config, &self.inner.ws_url).await?;
        *guard = Some(conn.clone());
        Ok(conn)
    }

    async fn disconnect(&self) {
        let mut guard = self.inner.conn.lock().await;
        *guard = None;
    }
}

async fn connect(config: &TrueNasConfig, ws_url: &str) -> Result<Conn> {
    let connector = tls_connector(config.verify_ssl)?;
    debug!(url = ws_url, "connecting TrueNAS JSON-RPC WebSocket");
    let (socket, _) = connect_async_tls_with_config(ws_url, None, false, Some(connector))
        .await
        .map_err(|error| {
            TrueNasError::ConfigError(format!("WebSocket connect to {ws_url} failed: {error}"))
        })?;
    let (sink, stream) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel();
    let pending = Arc::new(Mutex::new(Pending::new()));
    spawn_writer(sink, rx);
    spawn_reader(stream, tx.clone(), Arc::clone(&pending));
    let conn = Conn { tx, pending };
    authenticate(config, &conn).await?;
    Ok(conn)
}

fn tls_connector(verify_ssl: bool) -> Result<Connector> {
    let mut builder = native_tls::TlsConnector::builder();
    if !verify_ssl {
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
    }
    let tls = builder
        .build()
        .map_err(|error| TrueNasError::ConfigError(format!("TLS connector: {error}")))?;
    Ok(Connector::NativeTls(tls))
}

fn spawn_writer(
    mut sink: futures_util::stream::SplitSink<WsStream, Message>,
    mut rx: mpsc::UnboundedReceiver<Outbound>,
) {
    tokio::spawn(async move {
        while let Some(outbound) = rx.recv().await {
            let message = match outbound {
                Outbound::Text(text) => Message::Text(text.into()),
                Outbound::Pong(payload) => Message::Pong(payload.into()),
            };
            if sink.send(message).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
}

fn spawn_reader(
    mut stream: futures_util::stream::SplitStream<WsStream>,
    tx: mpsc::UnboundedSender<Outbound>,
    pending: Arc<Mutex<Pending>>,
) {
    tokio::spawn(async move {
        while let Some(frame) = stream.next().await {
            match frame {
                Ok(Message::Text(text)) => dispatch_text(&text, &pending).await,
                Ok(Message::Ping(payload)) => {
                    if tx.send(Outbound::Pong(payload.to_vec())).is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "TrueNAS WebSocket read failed");
                    break;
                }
            }
        }
        let mut waiters = pending.lock().await;
        for (_, waiter) in waiters.drain() {
            let _ = waiter.send(Err(CallFail::Disconnected(
                "TrueNAS WebSocket closed".to_string(),
            )));
        }
    });
}

async fn dispatch_text(text: &str, pending: &Arc<Mutex<Pending>>) {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "ignoring non-JSON WebSocket frame");
            return;
        }
    };
    if value.get("method").is_some() && value.get("id").is_none() {
        return;
    }
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        return;
    };
    let result = if let Some(error) = value.get("error") {
        Err(CallFail::Rpc(format_rpc_error(error)))
    } else {
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    };
    if let Some(waiter) = pending.lock().await.remove(&id) {
        let _ = waiter.send(result);
    }
}

fn format_rpc_error(error: &Value) -> String {
    let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("JSON-RPC error");
    let reason = error
        .pointer("/data/reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    if reason.is_empty() {
        format!("{message} ({code})")
    } else {
        format!("{message} ({code}): {reason}")
    }
}

async fn authenticate(config: &TrueNasConfig, conn: &Conn) -> Result<()> {
    let (method, params) = if let Some(api_key) = &config.api_key {
        ("auth.login_with_api_key", vec![json!(api_key)])
    } else if let (Some(username), Some(password)) = (&config.username, &config.password) {
        ("auth.login", vec![json!(username), json!(password)])
    } else {
        return Err(TrueNasError::AuthError(
            "TRUENAS_API_KEY or TRUENAS_USERNAME and TRUENAS_PASSWORD is required".to_string(),
        ));
    };
    let id = 0_u64;
    let (tx, rx) = oneshot::channel();
    conn.pending.lock().await.insert(id, tx);
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    if conn.tx.send(Outbound::Text(payload.to_string())).is_err() {
        return Err(TrueNasError::AuthError(
            "WebSocket closed before authentication".to_string(),
        ));
    }
    let wait = timeout(Duration::from_secs(config.timeout_secs), rx).await;
    match wait {
        Ok(Ok(Ok(Value::Bool(false) | Value::Null))) => Err(TrueNasError::AuthError(
            "TrueNAS rejected the API key or username/password".to_string(),
        )),
        Ok(Ok(Ok(_))) => Ok(()),
        Ok(Ok(Err(CallFail::Rpc(message)))) => Err(TrueNasError::AuthError(message)),
        Ok(Ok(Err(CallFail::Disconnected(message)))) => Err(TrueNasError::AuthError(message)),
        _ => Err(TrueNasError::AuthError(
            "timed out waiting for TrueNAS authentication".to_string(),
        )),
    }
}

pub fn websocket_url(server_url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(server_url)
        .map_err(|error| TrueNasError::ConfigError(format!("invalid TrueNAS URL: {error}")))?;
    let (scheme, default_port) = match parsed.scheme() {
        "https" | "wss" => ("wss", 443),
        "http" | "ws" => ("ws", 80),
        other => {
            return Err(TrueNasError::ConfigError(format!(
                "unsupported TrueNAS URL scheme {other}"
            )));
        }
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| TrueNasError::ConfigError("TrueNAS URL has no host".to_string()))?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let authority = match parsed.port() {
        Some(port) if port != default_port => format!("{host}:{port}"),
        _ => host,
    };
    Ok(format!("{scheme}://{authority}/api/current"))
}
