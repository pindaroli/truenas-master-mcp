use crate::config::TrueNasConfig;
use crate::error::{Result, TrueNasError};
use reqwest::{Client, header};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, instrument};

/// Connection pool settings
const MAX_IDLE_CONNECTIONS: usize = 20;
const IDLE_CONNECTION_TIMEOUT_SECS: u64 = 30;
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Rate limiter configuration
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 10;

/// TrueNAS API client with connection pooling and rate limiting
#[derive(Debug, Clone)]
pub struct TrueNasClient {
    client: Client,
    config: TrueNasConfig,
    base_url: String,
    /// Semaphore for limiting concurrent requests
    rate_limiter: Arc<Semaphore>,
}

impl TrueNasClient {
    /// Create a new TrueNAS client with connection pooling
    pub fn new(config: TrueNasConfig) -> Result<Self> {
        let mut builder = Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(config.timeout_secs))
            .pool_max_idle_per_host(MAX_IDLE_CONNECTIONS)
            .tcp_keepalive(Duration::from_secs(IDLE_CONNECTION_TIMEOUT_SECS));

        // SSL verification
        if !config.verify_ssl {
            builder = builder.danger_accept_invalid_certs(true);
        }



        let client = builder.build().map_err(|e| {
            TrueNasError::ConfigError(format!("Failed to build HTTP client: {}", e))
        })?;

        let base_url = config.server_url.trim_end_matches('/').to_string();

        // Initialize rate limiter with max concurrent requests
        let rate_limiter = Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT_REQUESTS));

        debug!(
            max_concurrent = DEFAULT_MAX_CONCURRENT_REQUESTS,
            "Initialized rate limiter"
        );

        Ok(Self {
            client,
            config,
            base_url,
            rate_limiter,
        })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the authorization header value
    fn get_auth_header(&self) -> Option<String> {
        if let Some(api_key) = &self.config.api_key {
            Some(format!("Bearer {}", api_key))
        } else if let (Some(username), Some(password)) =
            (&self.config.username, &self.config.password)
        {
            Some(format!("Basic {}", base64_encode(username, password)))
        } else {
            None
        }
    }

    /// Make a GET request with structured logging
    #[instrument(skip(self), fields(method = "GET", endpoint = %endpoint))]
    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.clone().acquire_owned().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let start = Instant::now();

        debug!(url = %url, "Starting GET request");

        let mut request = self.client.get(&url);

        if let Some(auth) = self.get_auth_header() {
            request = request.header(header::AUTHORIZATION, auth);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                error!(error = %e, "GET request failed");
                return Err(TrueNasError::RequestError(e));
            }
        };

        let status = response.status();
        let duration_ms = start.elapsed().as_millis();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            error!(status = %status, duration_ms = %duration_ms, message = %message, "GET request failed");
            return Err(TrueNasError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        info!(status = %status, duration_ms = %duration_ms, "GET request successful");

        response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse response");
            TrueNasError::RequestError(e)
        })
    }

    /// Make a POST request with structured logging
    #[instrument(skip(self, body), fields(method = "POST", endpoint = %endpoint))]
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.clone().acquire_owned().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let start = Instant::now();

        debug!(url = %url, "Starting POST request");

        let mut request = self.client.post(&url).json(body);

        if let Some(auth) = self.get_auth_header() {
            request = request.header(header::AUTHORIZATION, auth);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                error!(error = %e, "POST request failed");
                return Err(TrueNasError::RequestError(e));
            }
        };

        let status = response.status();
        let duration_ms = start.elapsed().as_millis();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            error!(status = %status, duration_ms = %duration_ms, message = %message, "POST request failed");
            return Err(TrueNasError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        info!(status = %status, duration_ms = %duration_ms, "POST request successful");

        response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse response");
            TrueNasError::RequestError(e)
        })
    }

    /// Make a PUT request with structured logging
    #[instrument(skip(self, body), fields(method = "PUT", endpoint = %endpoint))]
    #[allow(dead_code)]
    pub async fn put<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.clone().acquire_owned().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let start = Instant::now();

        debug!(url = %url, "Starting PUT request");

        let mut request = self.client.put(&url).json(body);

        if let Some(auth) = self.get_auth_header() {
            request = request.header(header::AUTHORIZATION, auth);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                error!(error = %e, "PUT request failed");
                return Err(TrueNasError::RequestError(e));
            }
        };

        let status = response.status();
        let duration_ms = start.elapsed().as_millis();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, duration_ms = %duration_ms, message = %message, "PUT request failed");
            return Err(TrueNasError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        info!(status = %status, duration_ms = %duration_ms, "PUT request successful");

        response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse response");
            TrueNasError::RequestError(e)
        })
    }

    /// Make a DELETE request with structured logging
    #[instrument(skip(self), fields(method = "DELETE", endpoint = %endpoint))]
    pub async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.clone().acquire_owned().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let start = Instant::now();

        debug!(url = %url, "Starting DELETE request");

        let mut request = self.client.delete(&url);

        if let Some(auth) = self.get_auth_header() {
            request = request.header(header::AUTHORIZATION, auth);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                error!(error = %e, "DELETE request failed");
                return Err(TrueNasError::RequestError(e));
            }
        };

        let status = response.status();
        let duration_ms = start.elapsed().as_millis();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, duration_ms = %duration_ms, message = %message, "DELETE request failed");
            return Err(TrueNasError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        info!(status = %status, duration_ms = %duration_ms, "DELETE request successful");

        response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse response");
            TrueNasError::RequestError(e)
        })
    }

    /// Make a DELETE request with body and structured logging
    #[instrument(skip(self, body), fields(method = "DELETE", endpoint = %endpoint))]
    #[allow(dead_code)]
    pub async fn delete_with_body<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.clone().acquire_owned().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let start = Instant::now();

        debug!(url = %url, "Starting DELETE with body request");

        let mut request = self.client.delete(&url).json(body);

        if let Some(auth) = self.get_auth_header() {
            request = request.header(header::AUTHORIZATION, auth);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                error!(error = %e, "DELETE with body request failed");
                return Err(TrueNasError::RequestError(e));
            }
        };

        let status = response.status();
        let duration_ms = start.elapsed().as_millis();

        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(status = %status, duration_ms = %duration_ms, message = %message, "DELETE with body request failed");
            return Err(TrueNasError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        info!(status = %status, duration_ms = %duration_ms, "DELETE with body request successful");

        response.json().await.map_err(|e| {
            error!(error = %e, "Failed to parse response");
            TrueNasError::RequestError(e)
        })
    }
}

/// Helper to create basic auth string
fn base64_encode(username: &str, password: &str) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(format!("{}:{}", username, password))
}
