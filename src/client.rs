use crate::config::TrueNasConfig;
use crate::error::{Result, TrueNasError};
use crate::rest_map::{self, RpcCall};
use crate::rpc::RpcSession;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, instrument};

/// Rate limiter configuration
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 10;

/// TrueNAS API client. Calls are JSON-RPC 2.0 over WebSocket.
/// The `get`/`post`/`put`/`delete` methods still accept historical `/api/v2.0`
/// paths so existing tools keep their names.
#[derive(Debug, Clone)]
pub struct TrueNasClient {
    config: TrueNasConfig,
    base_url: String,
    rate_limiter: Arc<Semaphore>,
    session: RpcSession,
}

impl TrueNasClient {
    /// Create a client. The WebSocket connects on the first call.
    pub fn new(config: TrueNasConfig) -> Result<Self> {
        let base_url = config.server_url.trim_end_matches('/').to_string();
        let session = RpcSession::new(config.clone())?;
        let rate_limiter = Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT_REQUESTS));
        debug!(
            max_concurrent = DEFAULT_MAX_CONCURRENT_REQUESTS,
            "Initialized rate limiter"
        );
        Ok(Self {
            config,
            base_url,
            rate_limiter,
            session,
        })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Make a GET request
    #[instrument(skip(self), fields(method = "GET", endpoint = %endpoint))]
    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        self.invoke("GET", endpoint, None).await
    }

    /// Make a POST request
    #[instrument(skip(self, body), fields(method = "POST", endpoint = %endpoint))]
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        self.invoke("POST", endpoint, Some(serde_json::to_value(body)?))
            .await
    }

    /// Make a PUT request
    #[instrument(skip(self, body), fields(method = "PUT", endpoint = %endpoint))]
    #[allow(dead_code)]
    pub async fn put<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        self.invoke("PUT", endpoint, Some(serde_json::to_value(body)?))
            .await
    }

    /// Make a DELETE request
    #[instrument(skip(self), fields(method = "DELETE", endpoint = %endpoint))]
    pub async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        self.invoke("DELETE", endpoint, None).await
    }

    /// Make a DELETE request with a body
    #[instrument(skip(self, body), fields(method = "DELETE", endpoint = %endpoint))]
    #[allow(dead_code)]
    pub async fn delete_with_body<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        self.invoke("DELETE", endpoint, Some(serde_json::to_value(body)?))
            .await
    }

    async fn invoke<T: DeserializeOwned>(
        &self,
        http: &str,
        endpoint: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let _permit = self
            .rate_limiter
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TrueNasError::ConfigError("rate limiter closed".to_string()))?;
        let call = rest_map::translate(http, endpoint, body.as_ref())?;
        debug!(method = %call.method, "translated REST path to JSON-RPC");
        let mut result = self.session.call(&call.method, &call.params).await?;
        if call.job {
            result = self.wait_for_job(result).await?;
        }
        if call.unwrap_one {
            result = unwrap_one(&call, result)?;
        }
        parse_response(result)
    }

    async fn wait_for_job(&self, result: Value) -> Result<Value> {
        let Some(job_id) = result.as_i64() else {
            return unwrap_job(result);
        };
        let timeout_secs = self.config.timeout_secs;
        let waited = self
            .session
            .call(
                "core.job_wait",
                &[Value::from(job_id), Value::from(timeout_secs)],
            )
            .await?;
        unwrap_job(waited)
    }
}

fn unwrap_job(value: Value) -> Result<Value> {
    let Some(state) = value.get("state").and_then(Value::as_str) else {
        return Ok(value);
    };
    match state {
        "SUCCESS" => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        "FAILED" | "ABORTED" | "ERROR" => {
            let message = value
                .get("error")
                .or_else(|| value.get("exception"))
                .map(|item| item.to_string())
                .unwrap_or_else(|| format!("job {state}"));
            Err(TrueNasError::ApiError {
                status: 500,
                message,
            })
        }
        _ => Ok(value),
    }
}

fn unwrap_one(call: &RpcCall, value: Value) -> Result<Value> {
    let Some(rows) = value.as_array() else {
        return Ok(value);
    };
    match rows.len() {
        1 => Ok(rows[0].clone()),
        0 => Err(TrueNasError::NotFound(format!(
            "{} returned no rows",
            call.method
        ))),
        count => Err(TrueNasError::ApiError {
            status: 500,
            message: format!("{} returned {count} rows", call.method),
        }),
    }
}

fn parse_response<T: DeserializeOwned>(value: Value) -> Result<T> {
    match serde_json::from_value::<T>(value.clone()) {
        Ok(parsed) => Ok(parsed),
        Err(error) if value.is_null() || value.is_boolean() => {
            serde_json::from_value(Value::Null).map_err(|_| TrueNasError::SerializationError(error))
        }
        Err(error) => Err(TrueNasError::SerializationError(error)),
    }
}
