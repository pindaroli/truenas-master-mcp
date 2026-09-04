use crate::error::Result;
use crate::error::TrueNasError;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;

/// Configuration file format
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "format")]
pub enum ConfigFormat {
    /// YAML configuration
    #[serde(alias = "yaml", alias = "YAML", alias = "yml")]
    Yaml,
    /// JSON configuration
    #[serde(alias = "json", alias = "JSON")]
    Json,
}

/// Configuration file with optional format hint
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFile {
    /// Configuration format (optional, auto-detected from extension)
    #[serde(default)]
    pub format: Option<ConfigFormat>,
    /// TrueNAS server URL
    pub server_url: String,
    /// API key for authentication
    #[serde(default)]
    pub api_key: Option<String>,
    /// Username for basic auth
    #[serde(default)]
    pub username: Option<String>,
    /// Password for basic auth
    #[serde(default)]
    pub password: Option<String>,
    /// Whether to verify SSL certificates
    #[serde(default = "default_true")]
    pub verify_ssl: bool,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// TrueNAS version
    #[serde(default)]
    pub version: Option<TrueNasVersion>,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

/// TrueNAS version type
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrueNasVersion {
    /// TrueNAS SCALE (Kubernetes-based apps)
    #[default]
    #[serde(alias = "scale", alias = "SCALE", alias = "sc", alias = "SC")]
    Scale,
    /// TrueNAS CORE (Jail-based apps)
    #[serde(alias = "core", alias = "CORE", alias = "cr", alias = "CR")]
    Core,
}

impl std::str::FromStr for TrueNasVersion {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "scale" | "sc" => Ok(TrueNasVersion::Scale),
            "core" | "cr" => Ok(TrueNasVersion::Core),
            _ => Err("Unknown TrueNAS version. Use 'scale' or 'core'"),
        }
    }
}

/// Configuration for the TrueNAS MCP server
#[derive(Clone, Deserialize)]
pub struct TrueNasConfig {
    /// TrueNAS server URL (e.g., https://truenas.local)
    pub server_url: String,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Username for basic auth (alternative to api_key)
    pub username: Option<String>,
    /// Password for basic auth (alternative to api_key)
    pub password: Option<String>,
    /// Whether to verify SSL certificates
    pub verify_ssl: bool,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// TrueNAS version (scale or core)
    pub version: TrueNasVersion,
}

impl Default for TrueNasConfig {
    fn default() -> Self {
        Self {
            server_url: env::var("TRUENAS_SERVER_URL")
                .unwrap_or_else(|_| "http://localhost".to_string()),
            api_key: env::var("TRUENAS_API_KEY").ok(),
            username: env::var("TRUENAS_USERNAME").ok(),
            password: env::var("TRUENAS_PASSWORD").ok(),
            verify_ssl: env::var("TRUENAS_VERIFY_SSL")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            timeout_secs: env::var("TRUENAS_TIMEOUT")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .unwrap_or(30),
            version: env::var("TRUENAS_VERSION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_default(),
        }
    }
}

impl std::fmt::Debug for TrueNasConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrueNasConfig")
            .field("server_url", &self.server_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***MASKED***"))
            .field("username", &self.username.as_ref().map(|_| "***MASKED***"))
            .field("password", &self.password.as_ref().map(|_| "***MASKED***"))
            .field("verify_ssl", &self.verify_ssl)
            .field("timeout_secs", &self.timeout_secs)
            .field("version", &self.version)
            .finish()
    }
}

impl TrueNasConfig {
    /// Load configuration from a file (YAML or JSON)
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(TrueNasError::ConfigError(format!(
                "Config file not found: {}",
                path.display()
            )));
        }

        let content = fs::read_to_string(path)
            .map_err(|e| TrueNasError::ConfigError(format!("Failed to read config file: {}", e)))?;

        let config_file: ConfigFile = if path.extension().and_then(|e| e.to_str()) == Some("yaml")
            || path.extension().and_then(|e| e.to_str()) == Some("yml")
        {
            serde_yaml::from_str(&content).map_err(|e| {
                TrueNasError::ConfigError(format!("Failed to parse YAML config: {}", e))
            })?
        } else {
            serde_json::from_str(&content).map_err(|e| {
                TrueNasError::ConfigError(format!("Failed to parse JSON config: {}", e))
            })?
        };

        // Convert ConfigFile to TrueNasConfig
        let mut config = Self {
            server_url: config_file.server_url,
            api_key: config_file.api_key,
            username: config_file.username,
            password: config_file.password,
            verify_ssl: config_file.verify_ssl,
            timeout_secs: config_file.timeout_secs,
            version: config_file.version.unwrap_or_default(),
        };

        // Allow environment variables to override config file
        config.apply_env_overrides();

        config.validate()?;
        Ok(config)
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let config = Self::default();
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from file with environment override
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(p) => Self::from_file(p),
            None => Self::from_env(),
        }
    }

    /// Apply environment variable overrides
    #[allow(clippy::collapsible_if)]
    fn apply_env_overrides(&mut self) {
        if let Ok(url) = env::var("TRUENAS_SERVER_URL") {
            if !url.is_empty() {
                self.server_url = url;
            }
        }
        if let Ok(key) = env::var("TRUENAS_API_KEY") {
            if !key.is_empty() {
                self.api_key = Some(key);
            }
        }
        if let Ok(user) = env::var("TRUENAS_USERNAME") {
            if !user.is_empty() {
                self.username = Some(user);
            }
        }
        if let Ok(pass) = env::var("TRUENAS_PASSWORD") {
            if !pass.is_empty() {
                self.password = Some(pass);
            }
        }
        if let Ok(ssl) = env::var("TRUENAS_VERIFY_SSL") {
            if ssl.parse::<bool>().unwrap_or(false) {
                self.verify_ssl = true;
            }
        }
        if let Ok(timeout) = env::var("TRUENAS_TIMEOUT") {
            if let Ok(t) = timeout.parse::<u64>() {
                self.timeout_secs = t;
            }
        }
        if let Ok(version) = env::var("TRUENAS_VERSION") {
            if let Ok(v) = version.parse::<TrueNasVersion>() {
                self.version = v;
            }
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server URL is not empty
        if self.server_url.is_empty() {
            return Err(TrueNasError::ConfigError(
                "TRUENAS_SERVER_URL must be set".to_string(),
            ));
        }

        // Validate URL format
        if !self.server_url.starts_with("http://") && !self.server_url.starts_with("https://") {
            return Err(TrueNasError::ConfigError(
                "TRUENAS_SERVER_URL must start with 'http://' or 'https://'".to_string(),
            ));
        }

        // Validate URL can be parsed
        let url = url::Url::parse(&self.server_url).map_err(|e| {
            TrueNasError::ConfigError(format!("Invalid TRUENAS_SERVER_URL format: {}", e))
        })?;

        if url.host_str().is_none() {
            return Err(TrueNasError::ConfigError(
                "TRUENAS_SERVER_URL must have a valid host".to_string(),
            ));
        }

        // Check if we have either API key or username/password
        if self.api_key.is_none() && (self.username.is_none() || self.password.is_none()) {
            return Err(TrueNasError::ConfigError(
                "Either TRUENAS_API_KEY or TRUENAS_USERNAME and TRUENAS_PASSWORD must be set"
                    .to_string(),
            ));
        }

        // Validate API key format if provided
        if let Some(key) = &self.api_key {
            if key.len() < 10 {
                return Err(TrueNasError::ConfigError(
                    "TRUENAS_API_KEY appears to be too short".to_string(),
                ));
            }
        }

        // Validate timeout bounds
        if self.timeout_secs == 0 {
            return Err(TrueNasError::ConfigError(
                "TRUENAS_TIMEOUT must be greater than 0".to_string(),
            ));
        }
        if self.timeout_secs > 3600 {
            return Err(TrueNasError::ConfigError(
                "TRUENAS_TIMEOUT must be less than or equal to 3600 (1 hour)".to_string(),
            ));
        }

        Ok(())
    }
}
