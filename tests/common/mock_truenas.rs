#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

type HandlerFn = Box<dyn Fn(Vec<Value>) -> Result<Value, (i32, String)> + Send + Sync>;

/// A hermetic, stateful mock of the TrueNAS WebSocket JSON-RPC 2.0 API.
pub struct MockTrueNas {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handlers: Arc<Mutex<HashMap<String, HandlerFn>>>,
    auth_allowed: Arc<Mutex<bool>>,
    jobs: Arc<Mutex<HashMap<u64, (String, Value)>>>,
}

impl MockTrueNas {
    /// Start a new mock TrueNAS server on an ephemeral loopback port.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let handlers: Arc<Mutex<HashMap<String, HandlerFn>>> = Arc::new(Mutex::new(HashMap::new()));
        let auth_allowed = Arc::new(Mutex::new(true));
        let jobs = Arc::new(Mutex::new(HashMap::new()));

        let handlers_clone = Arc::clone(&handlers);
        let auth_allowed_clone = Arc::clone(&auth_allowed);
        let jobs_clone = Arc::clone(&jobs);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_res = listener.accept() => {
                        let Ok((stream, _)) = accept_res else {
                            break;
                        };

                        let handlers = Arc::clone(&handlers_clone);
                        let auth_allowed = Arc::clone(&auth_allowed_clone);
                        let jobs = Arc::clone(&jobs_clone);

                        tokio::spawn(async move {
                            let mut socket = match accept_async(stream).await {
                                Ok(ws) => ws,
                                Err(_) => return,
                            };

                            while let Some(msg_res) = socket.next().await {
                                let Ok(msg) = msg_res else {
                                    break;
                                };

                                let Message::Text(text) = msg else {
                                    continue;
                                };

                                let req: Value = match serde_json::from_str(&text) {
                                    Ok(v) => v,
                                    Err(_) => continue,
                                };

                                let id = req.get("id").cloned().unwrap_or(Value::Null);
                                let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                                let params = req.get("params").and_then(Value::as_array).cloned().unwrap_or_default();

                                // Authentication handler
                                if method == "auth.login_with_api_key" || method == "auth.login" {
                                    let allowed = *auth_allowed.lock().await;
                                    let resp = if allowed {
                                        json!({ "jsonrpc": "2.0", "id": id, "result": true })
                                    } else {
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": { "code": -32000, "message": "Authentication failed" }
                                        })
                                    };
                                    let _ = socket.send(Message::Text(resp.to_string().into())).await;
                                    continue;
                                }

                                // Check custom handler first
                                let custom_result = {
                                    let guard = handlers.lock().await;
                                    guard.get(method).map(|handler| handler(params.clone()))
                                };

                                let resp = if let Some(res) = custom_result {
                                    match res {
                                        Ok(val) => json!({ "jsonrpc": "2.0", "id": id, "result": val }),
                                        Err((code, msg)) => json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": { "code": code, "message": msg, "data": { "reason": msg } }
                                        }),
                                    }
                                } else {
                                    // Default handlers
                                    Self::handle_default(method, &params, &jobs).await
                                        .map(|val| json!({ "jsonrpc": "2.0", "id": id, "result": val }))
                                        .unwrap_or_else(|(code, msg)| json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": { "code": code, "message": msg }
                                        }))
                                };

                                if socket.send(Message::Text(resp.to_string().into())).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                }
            }
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            handlers,
            auth_allowed,
            jobs,
        }
    }

    /// Return the HTTP base URL corresponding to this WebSocket server.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Set whether authentication attempts should succeed.
    pub async fn set_auth_allowed(&self, allowed: bool) {
        *self.auth_allowed.lock().await = allowed;
    }

    /// Register a custom handler for a TrueNAS JSON-RPC method.
    pub async fn set_handler<F>(&self, method: &str, handler: F)
    where
        F: Fn(Vec<Value>) -> Result<Value, (i32, String)> + Send + Sync + 'static,
    {
        self.handlers
            .lock()
            .await
            .insert(method.to_string(), Box::new(handler));
    }

    /// Register a method that immediately fails with a specific code and error message.
    pub async fn fail_method(&self, method: &str, code: i32, message: &str) {
        let msg = message.to_string();
        self.set_handler(method, move |_| Err((code, msg.clone())))
            .await;
    }

    /// Seed a TrueNAS Job for testing asynchronous job polling.
    pub async fn add_job(&self, job_id: u64, state: &str, result: Value) {
        self.jobs
            .lock()
            .await
            .insert(job_id, (state.to_string(), result));
    }

    async fn handle_default(
        method: &str,
        _params: &[Value],
        jobs: &Arc<Mutex<HashMap<u64, (String, Value)>>>,
    ) -> Result<Value, (i32, String)> {
        match method {
            "pool.query" => Ok(json!([
                {
                    "name": "tank",
                    "guid": "123456789",
                    "status": "ONLINE",
                    "size": 10_000_000_000_u64,
                    "free": 5_000_000_000_u64,
                    "description": "Main storage pool"
                }
            ])),
            "system.info" => Ok(json!({
                "version": "TrueNAS-SCALE-24.04.0",
                "hostname": "truenas.homelab",
                "uptime_seconds": 86400,
                "cpu_model": "AMD Ryzen 7 PRO",
                "physical_cores": 8
            })),
            "pool.dataset.query" => Ok(json!([
                {
                    "name": "tank/data",
                    "pool": "tank",
                    "mountpoint": "/mnt/tank/data",
                    "comments": "User dataset"
                }
            ])),
            "user.query" => Ok(json!([
                {
                    "id": 1000,
                    "username": "olindo",
                    "uid": 1000,
                    "home": "/home/olindo",
                    "full_name": "Olindo Lab"
                }
            ])),
            "core.get_jobs" => {
                let guard = jobs.lock().await;
                let mut list = Vec::new();
                for (id, (state, res)) in guard.iter() {
                    list.push(json!({
                        "id": id,
                        "state": state,
                        "result": res
                    }));
                }
                if list.is_empty() {
                    // Default fallback job if requested
                    list.push(json!({
                        "id": 1,
                        "state": "SUCCESS",
                        "result": { "status": "completed" }
                    }));
                }
                Ok(json!(list))
            }
            "core.job_wait" => {
                let job_id = _params.first().and_then(Value::as_u64).unwrap_or(42);
                let guard = jobs.lock().await;
                let (state, res) = guard.get(&job_id).cloned().unwrap_or_else(|| {
                    (
                        "SUCCESS".to_string(),
                        json!({"name": "tank", "status": "SCRUB_STARTED"}),
                    )
                });
                Ok(json!({
                    "id": job_id,
                    "state": state,
                    "result": res
                }))
            }
            "pool.scrub" | "pool.scrub.run" => {
                // Return a job id
                jobs.lock().await.insert(
                    42,
                    (
                        "SUCCESS".to_string(),
                        json!({"name": "tank", "status": "SCRUB_STARTED"}),
                    ),
                );
                Ok(json!(42))
            }
            "disk.query" => Ok(json!([
                {
                    "identifier": "{serial}12345",
                    "name": "sda",
                    "model": "Samsung SSD 870",
                    "serial": "12345678",
                    "type_field": "SSD",
                    "rotationrate": 0,
                    "advpowermode": "DISABLED",
                    "enclosure": null,
                    "size": 1_000_000_000_u64,
                    "crit": "OK"
                }
            ])),
            "alert.list" => Ok(json!([])),
            "service.query" => Ok(json!([
                {
                    "id": 1,
                    "service": "cifs",
                    "state": "RUNNING",
                    "enable": true
                }
            ])),
            "app.query" => Ok(json!([
                {
                    "name": "plex",
                    "state": "RUNNING",
                    "version": "1.32.0"
                }
            ])),
            "app.get_instance" => Ok(json!({
                "name": "plex",
                "state": "RUNNING",
                "version": "1.32.0"
            })),
            "vm.query" => Ok(json!([
                {
                    "id": 1,
                    "name": "talos-node-1",
                    "vcpus": 4,
                    "memory": 8589934592_u64,
                    "status": "RUNNING"
                }
            ])),
            _ => Err((
                -32601,
                format!("Method '{}' not implemented on mock", method),
            )),
        }
    }
}

impl Drop for MockTrueNas {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}
