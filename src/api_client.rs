// Integration layer for TrueNAS API
// Provides high-level methods using serde_json::Value for flexibility

use crate::config::TrueNasConfig;
use crate::error::{Result, TrueNasError};
use reqwest;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// High-level TrueNAS API client
#[derive(Debug, Clone)]
pub struct ApiClient {
    /// HTTP client
    client: reqwest::Client,
    /// Base URL
    base_url: String,
    /// Authorization header value
    auth_header: Option<String>,
}

impl ApiClient {
    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the auth header (if set)
    pub fn auth_header(&self) -> Option<&str> {
        self.auth_header.as_deref()
    }

    /// Create a new client from configuration
    pub fn new(config: &TrueNasConfig) -> Self {
        // Build auth header
        let auth_header = if let Some(api_key) = &config.api_key {
            Some(format!("Bearer {}", api_key))
        } else if let (Some(username), Some(password)) = (&config.username, &config.password) {
            use base64::prelude::*;
            let creds = format!("{}:{}", username, password);
            Some(format!("Basic {}", BASE64_STANDARD.encode(creds)))
        } else {
            None
        };

        // Build client with proper timeout settings
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(10));

        if !config.verify_ssl {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().expect("Failed to build HTTP client");

        Self {
            client,
            base_url: config.server_url.trim_end_matches('/').to_string(),
            auth_header,
        }
    }

    /// Make a request and parse response
    async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        body: Option<&impl serde::Serialize>,
    ) -> Result<T> {
        let _ = (&self.client, &self.auth_header, body);
        Err(TrueNasError::ConfigError(format!(
            "REST {method} {endpoint} on {} was removed in 1.0.0; this server speaks JSON-RPC over WebSocket",
            self.base_url
        )))
    }

    // === Pool Operations ===

    /// List all pools
    pub async fn list_pools(&self) -> Result<serde_json::Value> {
        let url = format!("{}/pool", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get pool by ID
    pub async fn get_pool(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/pool/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Scrub a pool
    pub async fn scrub_pool(&self, pool_name: &str) -> Result<serde_json::Value> {
        let url = format!("{}/pool/scrub?pool_name={}", self.base_url, pool_name);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    // === Dataset Operations ===

    /// List all datasets
    pub async fn list_datasets(&self) -> Result<serde_json::Value> {
        let url = format!("{}/pool/dataset", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create a dataset
    pub async fn create_dataset(&self, name: &str, pool: &str) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateDatasetRequest<'a> {
            name: &'a str,
            pool: &'a str,
        }
        let url = format!("{}/pool/dataset", self.base_url);
        let body = CreateDatasetRequest { name, pool };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Get dataset by ID/path
    pub async fn get_dataset(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/pool/dataset/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Delete dataset
    pub async fn delete_dataset(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/pool/dataset/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === User Operations ===

    /// List all users
    pub async fn list_users(&self) -> Result<serde_json::Value> {
        let url = format!("{}/user", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get user by ID
    pub async fn get_user(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/user/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create a new user
    pub async fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        full_name: Option<&str>,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateUserRequest<'a> {
            username: &'a str,
            email: Option<&'a str>,
            full_name: Option<&'a str>,
        }
        let url = format!("{}/user", self.base_url);
        let body = CreateUserRequest {
            username,
            email,
            full_name,
        };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Update user
    pub async fn update_user(
        &self,
        id: i32,
        email: Option<&str>,
        full_name: Option<&str>,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct UpdateUserRequest<'a> {
            email: Option<&'a str>,
            full_name: Option<&'a str>,
        }
        let url = format!("{}/user/{}", self.base_url, id);
        let body = UpdateUserRequest { email, full_name };
        self.request(reqwest::Method::PATCH, &url, Some(&body))
            .await
    }

    /// Delete user
    pub async fn delete_user(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/user/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === Group Operations ===

    /// List all groups
    pub async fn list_groups(&self) -> Result<serde_json::Value> {
        let url = format!("{}/group", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get group by ID
    pub async fn get_group(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/group/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create a new group
    pub async fn create_group(&self, name: &str) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateGroupRequest<'a> {
            name: &'a str,
        }
        let url = format!("{}/group", self.base_url);
        let body = CreateGroupRequest { name };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Delete group
    pub async fn delete_group(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/group/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === VM Operations ===

    /// List all VMs
    pub async fn list_vms(&self) -> Result<serde_json::Value> {
        let url = format!("{}/vm", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get VM by ID
    pub async fn get_vm(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/vm/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Start VM
    pub async fn start_vm(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/vm/{}/start", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    /// Stop VM
    pub async fn stop_vm(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/vm/{}/stop", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    /// Restart VM
    pub async fn restart_vm(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/vm/{}/restart", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    // === App Operations (SCALE) ===

    /// List all apps
    pub async fn list_apps(&self) -> Result<serde_json::Value> {
        let url = format!("{}/app", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get app by ID
    pub async fn get_app(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/app/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Start app
    pub async fn start_app(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/app/{}/start", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    /// Stop app
    pub async fn stop_app(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/app/{}/stop", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    // === Disk Operations ===

    /// List all disks
    pub async fn list_disks(&self) -> Result<serde_json::Value> {
        let url = format!("{}/disk", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get disk by ID
    pub async fn get_disk(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/disk/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    // === Service Operations ===

    /// List all services
    pub async fn list_services(&self) -> Result<serde_json::Value> {
        let url = format!("{}/service", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get service by ID
    pub async fn get_service(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/service/{}", self.base_url, id);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Start service
    pub async fn start_service(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/service/{}/start", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    /// Stop service
    pub async fn stop_service(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/service/{}/stop", self.base_url, id);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    // === Snapshot Operations ===

    /// List all snapshots
    pub async fn list_snapshots(&self) -> Result<serde_json::Value> {
        let url = format!("{}/pool/snapshot", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create a snapshot
    pub async fn create_snapshot(
        &self,
        dataset: &str,
        name: &str,
        recursive: bool,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateSnapshotRequest {
            dataset: String,
            name: String,
            recursive: bool,
        }
        let url = format!("{}/pool/snapshot", self.base_url);
        let body = CreateSnapshotRequest {
            dataset: dataset.to_string(),
            name: name.to_string(),
            recursive,
        };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Delete snapshot
    pub async fn delete_snapshot(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/pool/snapshot/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === SMB Share Operations ===

    /// List all SMB shares
    pub async fn list_smb_shares(&self) -> Result<serde_json::Value> {
        let url = format!("{}/smb/share", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create SMB share
    pub async fn create_smb_share(
        &self,
        path: &str,
        name: &str,
        comment: Option<&str>,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateShareRequest<'a> {
            path: &'a str,
            name: &'a str,
            comment: Option<&'a str>,
        }
        let url = format!("{}/smb/share", self.base_url);
        let body = CreateShareRequest {
            path,
            name,
            comment,
        };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Delete SMB share
    pub async fn delete_smb_share(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/smb/share/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === NFS Export Operations ===

    /// List all NFS exports
    pub async fn list_nfs_exports(&self) -> Result<serde_json::Value> {
        let url = format!("{}/nfs/export", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create NFS export
    pub async fn create_nfs_export(
        &self,
        path: &str,
        networks: Vec<&str>,
        ro: bool,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateExportRequest<'a> {
            path: &'a str,
            networks: Vec<&'a str>,
            ro: bool,
        }
        let url = format!("{}/nfs/export", self.base_url);
        let body = CreateExportRequest { path, networks, ro };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Delete NFS export
    pub async fn delete_nfs_export(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/nfs/export/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === iSCSI Operations ===

    /// List all iSCSI targets
    pub async fn list_iscsi_targets(&self) -> Result<serde_json::Value> {
        let url = format!("{}/iscsi/target", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Create iSCSI target
    pub async fn create_iscsi_target(
        &self,
        name: &str,
        alias: Option<&str>,
    ) -> Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        struct CreateTargetRequest<'a> {
            name: &'a str,
            alias: Option<&'a str>,
        }
        let url = format!("{}/iscsi/target", self.base_url);
        let body = CreateTargetRequest { name, alias };
        self.request(reqwest::Method::POST, &url, Some(&body)).await
    }

    /// Delete iSCSI target
    pub async fn delete_iscsi_target(&self, id: i32) -> Result<serde_json::Value> {
        let url = format!("{}/iscsi/target/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    // === System Operations ===

    /// Get system info
    pub async fn get_system_info(&self) -> Result<serde_json::Value> {
        let url = format!("{}/system/info", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Get alerts
    pub async fn get_alerts(&self) -> Result<serde_json::Value> {
        let url = format!("{}/system/alert", self.base_url);
        self.request(reqwest::Method::GET, &url, None::<&()>).await
    }

    /// Dismiss alert
    pub async fn dismiss_alert(&self, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/system/alert/{}", self.base_url, id);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    /// Clear all alerts
    pub async fn clear_alerts(&self) -> Result<serde_json::Value> {
        let url = format!("{}/system/alert", self.base_url);
        self.request(reqwest::Method::DELETE, &url, None::<&()>)
            .await
    }

    /// Reboot system
    pub async fn reboot_system(&self) -> Result<serde_json::Value> {
        let url = format!("{}/system/reboot", self.base_url);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }

    /// Shutdown system
    pub async fn shutdown_system(&self) -> Result<serde_json::Value> {
        let url = format!("{}/system/shutdown", self.base_url);
        self.request(reqwest::Method::POST, &url, None::<&()>).await
    }
}
