use crate::config::TrueNasConfig;
use crate::error::Result as TrueNasResult;
use crate::tools::TrueNasTools;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;

/// Tool categories for access control
#[derive(Debug, Clone, PartialEq, JsonSchema)]
pub enum ToolCategory {
    Users,
    Pools,
    Datasets,
    Shares,
    Snapshots,
    Iscsi,
    Apps,
    Network,
    System,
    All,
}

impl FromStr for ToolCategory {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "users" => Ok(ToolCategory::Users),
            "pools" => Ok(ToolCategory::Pools),
            "datasets" | "dataset" => Ok(ToolCategory::Datasets),
            "shares" | "share" => Ok(ToolCategory::Shares),
            "snapshots" | "snapshot" => Ok(ToolCategory::Snapshots),
            "iscsi" => Ok(ToolCategory::Iscsi),
            "apps" | "app" => Ok(ToolCategory::Apps),
            "network" => Ok(ToolCategory::Network),
            "system" => Ok(ToolCategory::System),
            "all" => Ok(ToolCategory::All),
            _ => Err("Unknown category"),
        }
    }
}

/// Tool access control configuration
#[derive(Debug, Clone, Default, JsonSchema)]
pub struct ToolConfig {
    pub readonly: bool,
    pub enabled_categories: Vec<ToolCategory>,
    pub disabled_categories: Vec<ToolCategory>,
}

impl ToolConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        let readonly = std::env::var("TRUENAS_READONLY")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let enabled_categories: Vec<ToolCategory> = std::env::var("TRUENAS_ENABLED_CATEGORIES")
            .ok()
            .map(|v| {
                v.split(',')
                    .filter_map(|c| ToolCategory::from_str(c).ok())
                    .collect()
            })
            .unwrap_or_default();

        let disabled_categories: Vec<ToolCategory> = std::env::var("TRUENAS_DISABLED_CATEGORIES")
            .ok()
            .map(|v| {
                v.split(',')
                    .filter_map(|c| ToolCategory::from_str(c).ok())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            readonly,
            enabled_categories: if enabled_categories.is_empty() {
                vec![ToolCategory::All]
            } else {
                enabled_categories
            },
            disabled_categories,
        }
    }

    /// Check if a category is allowed
    pub fn is_category_allowed(&self, category: &ToolCategory) -> bool {
        // Check if category is disabled
        if self.disabled_categories.contains(category)
            || self.disabled_categories.contains(&ToolCategory::All)
        {
            return false;
        }

        // Check if category is enabled (or All is enabled)
        if self.enabled_categories.contains(&ToolCategory::All) {
            return true;
        }

        self.enabled_categories.contains(category)
    }

    /// Check if a tool can be executed (considering readonly mode)
    pub fn can_execute(
        &self,
        category: &ToolCategory,
        is_modification: bool,
    ) -> Result<(), String> {
        // Check category access
        if !self.is_category_allowed(category) {
            return Err(format!("Category {:?} is not enabled", category));
        }

        // Check readonly mode for modification tools
        if self.readonly && is_modification {
            return Err("Server is in readonly mode - modification tools are disabled".to_string());
        }

        Ok(())
    }
}

// Request types for tools - each tool gets its own struct with JsonSchema
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetUserRequest {
    pub user_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetUserByUsernameRequest {
    pub username: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPoolStatusRequest {
    pub pool_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetDatasetRequest {
    pub dataset_path: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateDatasetRequest {
    pub pool_name: String,
    pub dataset_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteDatasetRequest {
    pub dataset_path: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateSmbShareRequest {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteSmbShareRequest {
    pub share_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateNfsExportRequest {
    pub paths: Vec<String>,
    pub comment: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteNfsExportRequest {
    pub export_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateSnapshotRequest {
    pub dataset: String,
    pub snapshot_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteSnapshotRequest {
    pub snapshot_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateIscsiTargetRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteIscsiTargetRequest {
    pub target_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetAppRequest {
    pub app_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StartAppRequest {
    pub app_name: String,
    #[serde(default)]
    pub options: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StopAppRequest {
    pub app_name: String,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RestartAppRequest {
    pub app_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateAppRequest {
    pub catalog: String,
    pub item: String,
    pub name: String,
    pub values: serde_json::Value,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct UpdateAppRequest {
    pub app_name: String,
    pub values: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteAppRequest {
    pub app_name: String,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RollbackAppRequest {
    pub app_name: String,
    #[serde(default)]
    pub rollback_version: Option<String>,
    #[serde(default)]
    pub snap_name: Option<String>,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetAppConfigRequest {
    pub app_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetAppUpgradeOptionsRequest {
    pub app_name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct UpgradeAppRequest {
    pub app_name: String,
    pub options: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetCatalogRequest {
    pub catalog_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetCatalogTrainsRequest {
    pub catalog_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetCatalogItemRequest {
    pub catalog_id: String,
    pub item: String,
    pub train: String,
}

// === New Request Types for Extended Tools ===

// Groups
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetGroupRequest {
    pub group_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetGroupByNameRequest {
    pub name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(default)]
    pub gid: Option<i32>,
    #[serde(default)]
    pub users: Option<Vec<i32>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteGroupRequest {
    pub group_id: i32,
}

// VMs
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetVmRequest {
    pub vm_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StartVmRequest {
    pub vm_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StopVmRequest {
    pub vm_id: i32,
    #[serde(default)]
    pub force: Option<bool>,
}

// Network
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListInterfacesRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListRoutesRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetDnsRequest {}

// Services
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetServiceRequest {
    pub service_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StartServiceRequest {
    pub service_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct StopServiceRequest {
    pub service_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RestartServiceRequest {
    pub service_id: i32,
}

// System
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetAlertsRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CheckUpdatesRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RebootSystemRequest {
    /// Must be true to confirm this destructive operation
    pub confirm: bool,
    /// Delay in seconds before rebooting (default: 10)
    #[serde(default)]
    pub delay_seconds: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ShutdownSystemRequest {
    /// Must be true to confirm this destructive operation
    pub confirm: bool,
    /// Delay in seconds before shutting down (default: 10)
    #[serde(default)]
    pub delay_seconds: Option<u32>,
}

// Pool Management
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ScrubPoolRequest {
    pub pool_name: String,
}

// VMs
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateVmRequest {
    pub name: String,
    pub vcpus: i32,
    pub memory: u64,
    #[serde(default)]
    pub disk_size: Option<u64>,
    #[serde(default)]
    pub iso: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct UpdateVmRequest {
    pub vm_id: i32,
    #[serde(default)]
    pub updates: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteVmRequest {
    pub vm_id: i32,
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CloneVmRequest {
    pub vm_id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RestartVmRequest {
    pub vm_id: i32,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PowercycleVmRequest {
    pub vm_id: i32,
}

// Disks
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListDisksRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetDiskRequest {
    pub disk_name: String,
}

// Certificates
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListCertificatesRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetCertificateRequest {
    pub cert_id: i32,
}

// Replication
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListReplicationTasksRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RunReplicationTaskRequest {
    pub task_id: i32,
}

// Cloud Sync
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListCloudSyncTasksRequest {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct RunCloudSyncTaskRequest {
    pub task_id: i32,
}

// User Management
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub uid: Option<i32>,
    #[serde(default)]
    pub group_ids: Option<Vec<i32>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct UpdateUserRequest {
    pub user_id: i32,
    #[serde(default)]
    pub updates: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteUserRequest {
    pub user_id: i32,
}

/// TrueNAS MCP Server
#[derive(Debug, Clone)]
pub struct TrueNasServer {
    tools: Arc<TrueNasTools>,
    tool_router: ToolRouter<Self>,
}

impl TrueNasServer {
    /// Create a new TrueNAS MCP server
    pub fn new(config: TrueNasConfig) -> TrueNasResult<Self> {
        let client = crate::client::TrueNasClient::new(config)?;
        let tools = Arc::new(TrueNasTools::new(client));
        Ok(Self {
            tools,
            tool_router: Self::tool_router(),
        })
    }

    /// Create a server without connecting (for testing)
    pub fn new_mock() -> Self {
        let config = crate::config::TrueNasConfig::default();
        let client = crate::client::TrueNasClient::new(config).unwrap();
        let tools = Arc::new(TrueNasTools::new(client));
        Self {
            tools,
            tool_router: Self::tool_router(),
        }
    }
}

/// Implement ServerHandler using the tool_router macro
#[tool_handler(router = self.tool_router)]
impl ServerHandler for TrueNasServer {}

/// Tool router with all tool definitions
#[tool_router(router = tool_router)]
impl TrueNasServer {
    #[tool(
        name = "list_users",
        description = "List all users on the TrueNAS system"
    )]
    async fn list_users(&self) {
        let result = self.tools.list_users().await;
        // Errors will be logged but not silently dropped - the MCP protocol
        // will handle errors at a higher level
        if let Err(e) = result {
            tracing::error!("list_users failed: {}", e);
        }
    }

    #[tool(
        name = "get_user",
        description = "Get details of a specific user by ID"
    )]
    async fn get_user(&self, _req: Parameters<GetUserRequest>) {
        let result = self.tools.get_user(_req.0.user_id).await;
        if let Err(e) = result {
            tracing::error!("get_user failed: {}", e);
        }
    }

    #[tool(
        name = "get_user_by_username",
        description = "Get details of a specific user by username"
    )]
    async fn get_user_by_username(&self, _req: Parameters<GetUserByUsernameRequest>) {
        let result = self.tools.get_user_by_username(&_req.0.username).await;
        if let Err(e) = result {
            tracing::error!("get_user_by_username failed: {}", e);
        }
    }

    #[tool(
        name = "create_user",
        description = "Create a new user on the TrueNAS system"
    )]
    async fn create_user(&self, _req: Parameters<CreateUserRequest>) {
        let result = self
            .tools
            .create_user(
                &_req.0.username,
                &_req.0.password,
                _req.0.uid,
                _req.0.group_ids,
            )
            .await;
        if let Err(e) = result {
            tracing::error!("create_user failed: {}", e);
        }
    }

    #[tool(
        name = "update_user",
        description = "Update an existing user on the TrueNAS system"
    )]
    async fn update_user(&self, _req: Parameters<UpdateUserRequest>) {
        let updates = _req.0.updates.unwrap_or_default();
        let result = self.tools.update_user(_req.0.user_id, updates).await;
        if let Err(e) = result {
            tracing::error!("update_user failed: {}", e);
        }
    }

    #[tool(
        name = "delete_user",
        description = "Delete a user from the TrueNAS system"
    )]
    async fn delete_user(&self, _req: Parameters<DeleteUserRequest>) {
        let result = self.tools.delete_user(_req.0.user_id).await;
        if let Err(e) = result {
            tracing::error!("delete_user failed: {}", e);
        }
    }

    #[tool(
        name = "list_pools",
        description = "List all storage pools on the TrueNAS system"
    )]
    async fn list_pools(&self) {
        let _ = self.tools.list_pools().await;
    }

    #[tool(
        name = "get_pool_status",
        description = "Get the status of a specific storage pool"
    )]
    async fn get_pool_status(&self, _req: Parameters<GetPoolStatusRequest>) {
        let _ = self.tools.get_pool_status(&_req.0.pool_name).await;
    }

    #[tool(name = "scrub_pool", description = "Start a scrub on a storage pool")]
    async fn scrub_pool(&self, _req: Parameters<ScrubPoolRequest>) {
        let _ = self.tools.scrub_pool(&_req.0.pool_name).await;
    }

    #[tool(
        name = "list_datasets",
        description = "List all datasets on the TrueNAS system"
    )]
    async fn list_datasets(&self) {
        let _ = self.tools.list_datasets().await;
    }

    #[tool(
        name = "get_dataset",
        description = "Get details of a specific dataset"
    )]
    async fn get_dataset(&self, _req: Parameters<GetDatasetRequest>) {
        let _ = self.tools.get_dataset(&_req.0.dataset_path).await;
    }

    #[tool(
        name = "create_dataset",
        description = "Create a new dataset in a pool"
    )]
    async fn create_dataset(&self, _req: Parameters<CreateDatasetRequest>) {
        let _ = self
            .tools
            .create_dataset(&_req.0.pool_name, &_req.0.dataset_name)
            .await;
    }

    #[tool(name = "delete_dataset", description = "Delete a dataset")]
    async fn delete_dataset(&self, _req: Parameters<DeleteDatasetRequest>) {
        let _ = self.tools.delete_dataset(&_req.0.dataset_path).await;
    }

    #[tool(
        name = "list_smb_shares",
        description = "List all SMB shares on the TrueNAS system"
    )]
    async fn list_smb_shares(&self) {
        let _ = self.tools.list_smb_shares().await;
    }

    #[tool(name = "create_smb_share", description = "Create a new SMB share")]
    async fn create_smb_share(&self, _req: Parameters<CreateSmbShareRequest>) {
        let _ = self
            .tools
            .create_smb_share(&_req.0.name, &_req.0.path, _req.0.comment.as_deref())
            .await;
    }

    #[tool(name = "delete_smb_share", description = "Delete an SMB share")]
    async fn delete_smb_share(&self, _req: Parameters<DeleteSmbShareRequest>) {
        let _ = self.tools.delete_smb_share(_req.0.share_id).await;
    }

    #[tool(
        name = "list_nfs_exports",
        description = "List all NFS exports on the TrueNAS system"
    )]
    async fn list_nfs_exports(&self) {
        let _ = self.tools.list_nfs_exports().await;
    }

    #[tool(name = "create_nfs_export", description = "Create a new NFS export")]
    async fn create_nfs_export(&self, _req: Parameters<CreateNfsExportRequest>) {
        let _ = self
            .tools
            .create_nfs_export(_req.0.paths, _req.0.comment)
            .await;
    }

    #[tool(name = "delete_nfs_export", description = "Delete an NFS export")]
    async fn delete_nfs_export(&self, _req: Parameters<DeleteNfsExportRequest>) {
        let _ = self.tools.delete_nfs_export(_req.0.export_id).await;
    }

    #[tool(
        name = "list_snapshots",
        description = "List all ZFS snapshots on the TrueNAS system"
    )]
    async fn list_snapshots(&self) {
        let _ = self.tools.list_snapshots().await;
    }

    #[tool(name = "create_snapshot", description = "Create a new ZFS snapshot")]
    async fn create_snapshot(&self, _req: Parameters<CreateSnapshotRequest>) {
        let _ = self
            .tools
            .create_snapshot(&_req.0.dataset, &_req.0.snapshot_name)
            .await;
    }

    #[tool(name = "delete_snapshot", description = "Delete a ZFS snapshot")]
    async fn delete_snapshot(&self, _req: Parameters<DeleteSnapshotRequest>) {
        let _ = self.tools.delete_snapshot(&_req.0.snapshot_id).await;
    }

    #[tool(
        name = "list_iscsi_targets",
        description = "List all iSCSI targets on the TrueNAS system"
    )]
    async fn list_iscsi_targets(&self) {
        let _ = self.tools.list_iscsi_targets().await;
    }

    #[tool(
        name = "create_iscsi_target",
        description = "Create a new iSCSI target"
    )]
    async fn create_iscsi_target(&self, _req: Parameters<CreateIscsiTargetRequest>) {
        let _ = self.tools.create_iscsi_target(&_req.0.name).await;
    }

    #[tool(name = "delete_iscsi_target", description = "Delete an iSCSI target")]
    async fn delete_iscsi_target(&self, _req: Parameters<DeleteIscsiTargetRequest>) {
        let _ = self.tools.delete_iscsi_target(_req.0.target_id).await;
    }

    #[tool(
        name = "get_system_info",
        description = "Get system information from TrueNAS"
    )]
    async fn get_system_info(&self) {
        let _ = self.tools.get_system_info().await;
    }

    #[tool(name = "list_apps", description = "List Docker apps on TrueNAS SCALE")]
    async fn list_apps(&self) {
        let _ = self.tools.list_apps().await;
    }

    #[tool(
        name = "get_app",
        description = "Get details of a specific application"
    )]
    async fn get_app(&self, _req: Parameters<GetAppRequest>) {
        let _ = self.tools.get_app(&_req.0.app_name).await;
    }

    #[tool(name = "start_app", description = "Start an application on TrueNAS")]
    async fn start_app(&self, _req: Parameters<StartAppRequest>) {
        let _ = self.tools.start_app(&_req.0.app_name, _req.0.options).await;
    }

    #[tool(name = "stop_app", description = "Stop an application on TrueNAS")]
    async fn stop_app(&self, _req: Parameters<StopAppRequest>) {
        let _ = self
            .tools
            .stop_app(&_req.0.app_name, _req.0.force.unwrap_or(false))
            .await;
    }

    #[tool(
        name = "restart_app",
        description = "Restart an application on TrueNAS"
    )]
    async fn restart_app(&self, _req: Parameters<RestartAppRequest>) {
        let _ = self.tools.restart_app(&_req.0.app_name).await;
    }

    #[tool(
        name = "create_app",
        description = "Create a new application from a catalog item"
    )]
    async fn create_app(&self, _req: Parameters<CreateAppRequest>) {
        let _ = self
            .tools
            .create_app(
                &_req.0.catalog,
                &_req.0.item,
                &_req.0.name,
                _req.0.values,
                _req.0.version.as_deref(),
            )
            .await;
    }

    #[tool(
        name = "update_app",
        description = "Update an existing application with new configuration"
    )]
    async fn update_app(&self, _req: Parameters<UpdateAppRequest>) {
        let _ = self.tools.update_app(&_req.0.app_name, _req.0.values).await;
    }

    #[tool(
        name = "delete_app",
        description = "Delete an application from TrueNAS"
    )]
    async fn delete_app(&self, _req: Parameters<DeleteAppRequest>) {
        let _ = self
            .tools
            .delete_app(&_req.0.app_name, _req.0.force.unwrap_or(false))
            .await;
    }

    #[tool(
        name = "rollback_app",
        description = "Rollback an application to a previous version"
    )]
    async fn rollback_app(&self, _req: Parameters<RollbackAppRequest>) {
        let _ = self
            .tools
            .rollback_app(
                &_req.0.app_name,
                _req.0.rollback_version.as_deref(),
                _req.0.snap_name.as_deref(),
                _req.0.force.unwrap_or(false),
            )
            .await;
    }

    #[tool(
        name = "get_app_config",
        description = "Get the configuration of an application"
    )]
    async fn get_app_config(&self, _req: Parameters<GetAppConfigRequest>) {
        let _ = self.tools.get_app_config(&_req.0.app_name).await;
    }

    #[tool(
        name = "get_app_upgrade_options",
        description = "Get available upgrade options for an application"
    )]
    async fn get_app_upgrade_options(&self, _req: Parameters<GetAppUpgradeOptionsRequest>) {
        let _ = self.tools.get_app_upgrade_options(&_req.0.app_name).await;
    }

    #[tool(
        name = "upgrade_app",
        description = "Upgrade an application to a newer version"
    )]
    async fn upgrade_app(&self, _req: Parameters<UpgradeAppRequest>) {
        let _ = self
            .tools
            .upgrade_app(&_req.0.app_name, _req.0.options)
            .await;
    }

    #[tool(
        name = "list_catalog_items",
        description = "List all available catalog items from TrueNAS catalog"
    )]
    async fn list_catalog_items(&self) {
        let _ = self.tools.list_catalog_items().await;
    }

    #[tool(
        name = "get_catalog",
        description = "Get details of a specific catalog"
    )]
    async fn get_catalog(&self, _req: Parameters<GetCatalogRequest>) {
        let _ = self.tools.get_catalog(&_req.0.catalog_id).await;
    }

    #[tool(
        name = "get_catalog_trains",
        description = "Get all available train versions from a catalog"
    )]
    async fn get_catalog_trains(&self, _req: Parameters<GetCatalogTrainsRequest>) {
        let _ = self.tools.get_catalog_trains(&_req.0.catalog_id).await;
    }

    #[tool(
        name = "get_catalog_item",
        description = "Get details of a specific item from a catalog"
    )]
    async fn get_catalog_item(&self, _req: Parameters<GetCatalogItemRequest>) {
        let _ = self
            .tools
            .get_catalog_item(&_req.0.catalog_id, &_req.0.item, &_req.0.train)
            .await;
    }

    // === Group Management Tools ===

    #[tool(
        name = "list_groups",
        description = "List all groups on the TrueNAS system"
    )]
    async fn list_groups(&self) {
        let _ = self.tools.list_groups().await;
    }

    #[tool(
        name = "get_group",
        description = "Get details of a specific group by ID"
    )]
    async fn get_group(&self, _req: Parameters<GetGroupRequest>) {
        let _ = self.tools.get_group(_req.0.group_id).await;
    }

    #[tool(
        name = "get_group_by_name",
        description = "Get details of a specific group by name"
    )]
    async fn get_group_by_name(&self, _req: Parameters<GetGroupByNameRequest>) {
        let _ = self.tools.get_group_by_name(&_req.0.name).await;
    }

    #[tool(name = "create_group", description = "Create a new group on TrueNAS")]
    async fn create_group(&self, _req: Parameters<CreateGroupRequest>) {
        let _ = self
            .tools
            .create_group(&_req.0.name, _req.0.gid, _req.0.users)
            .await;
    }

    #[tool(name = "delete_group", description = "Delete a group from TrueNAS")]
    async fn delete_group(&self, _req: Parameters<DeleteGroupRequest>) {
        let _ = self.tools.delete_group(_req.0.group_id).await;
    }

    // === VM Management Tools ===

    #[tool(
        name = "list_vms",
        description = "List all virtual machines on TrueNAS"
    )]
    async fn list_vms(&self) {
        let _ = self.tools.list_vms().await;
    }

    #[tool(
        name = "get_vm",
        description = "Get details of a specific virtual machine"
    )]
    async fn get_vm(&self, _req: Parameters<GetVmRequest>) {
        let _ = self.tools.get_vm(_req.0.vm_id).await;
    }

    #[tool(name = "start_vm", description = "Start a virtual machine")]
    async fn start_vm(&self, _req: Parameters<StartVmRequest>) {
        let _ = self.tools.start_vm(_req.0.vm_id).await;
    }

    #[tool(name = "stop_vm", description = "Stop a virtual machine")]
    async fn stop_vm(&self, _req: Parameters<StopVmRequest>) {
        let _ = self
            .tools
            .stop_vm(_req.0.vm_id, _req.0.force.unwrap_or(false))
            .await;
    }

    #[tool(name = "restart_vm", description = "Restart a virtual machine")]
    async fn restart_vm(&self, _req: Parameters<GetVmRequest>) {
        let _ = self.tools.restart_vm(_req.0.vm_id).await;
    }

    #[tool(
        name = "powercycle_vm",
        description = "Power cycle a virtual machine (hard reset)"
    )]
    async fn powercycle_vm(&self, _req: Parameters<PowercycleVmRequest>) {
        let _ = self.tools.powercycle_vm(_req.0.vm_id).await;
    }

    #[tool(
        name = "create_vm",
        description = "Create a new virtual machine on TrueNAS"
    )]
    async fn create_vm(&self, _req: Parameters<CreateVmRequest>) {
        let _ = self
            .tools
            .create_vm(
                &_req.0.name,
                _req.0.vcpus,
                _req.0.memory,
                _req.0.disk_size,
                _req.0.iso.as_deref(),
            )
            .await;
    }

    #[tool(
        name = "update_vm",
        description = "Update configuration of an existing virtual machine"
    )]
    async fn update_vm(&self, _req: Parameters<UpdateVmRequest>) {
        let _ = self
            .tools
            .update_vm(_req.0.vm_id, _req.0.updates.unwrap_or_default())
            .await;
    }

    #[tool(
        name = "delete_vm",
        description = "Delete a virtual machine from TrueNAS"
    )]
    async fn delete_vm(&self, _req: Parameters<DeleteVmRequest>) {
        let _ = self
            .tools
            .delete_vm(_req.0.vm_id, _req.0.force.unwrap_or(false))
            .await;
    }

    #[tool(name = "clone_vm", description = "Clone an existing virtual machine")]
    async fn clone_vm(&self, _req: Parameters<CloneVmRequest>) {
        let _ = self.tools.clone_vm(_req.0.vm_id, &_req.0.name).await;
    }

    // === Network Management Tools ===

    #[tool(
        name = "list_interfaces",
        description = "List all network interfaces on TrueNAS"
    )]
    async fn list_interfaces(&self) {
        let _ = self.tools.list_interfaces().await;
    }

    #[tool(
        name = "list_routes",
        description = "List all network routes on TrueNAS"
    )]
    async fn list_routes(&self) {
        let _ = self.tools.list_routes().await;
    }

    #[tool(name = "get_dns", description = "Get DNS configuration for TrueNAS")]
    async fn get_dns(&self) {
        let _ = self.tools.get_dns().await;
    }

    // === Services Management Tools ===

    #[tool(name = "list_services", description = "List all services on TrueNAS")]
    async fn list_services(&self) {
        let _ = self.tools.list_services().await;
    }

    #[tool(
        name = "get_service",
        description = "Get details of a specific service"
    )]
    async fn get_service(&self, _req: Parameters<GetServiceRequest>) {
        let _ = self.tools.get_service(_req.0.service_id).await;
    }

    #[tool(name = "start_service", description = "Start a service on TrueNAS")]
    async fn start_service(&self, _req: Parameters<StartServiceRequest>) {
        let _ = self.tools.start_service(_req.0.service_id).await;
    }

    #[tool(name = "stop_service", description = "Stop a service on TrueNAS")]
    async fn stop_service(&self, _req: Parameters<StopServiceRequest>) {
        let _ = self.tools.stop_service(_req.0.service_id).await;
    }

    #[tool(name = "restart_service", description = "Restart a service on TrueNAS")]
    async fn restart_service(&self, _req: Parameters<RestartServiceRequest>) {
        let _ = self.tools.restart_service(_req.0.service_id).await;
    }

    // === System Management Tools ===

    #[tool(name = "get_alerts", description = "Get system alerts from TrueNAS")]
    async fn get_alerts(&self) {
        let _ = self.tools.get_alerts().await;
    }

    #[tool(name = "check_for_updates", description = "Check for system updates")]
    async fn check_for_updates(&self) {
        let _ = self.tools.check_for_updates().await;
    }

    #[tool(
        name = "reboot_system",
        description = "Reboot the TrueNAS system. Requires confirm=true for safety."
    )]
    async fn reboot_system(&self, _req: Parameters<RebootSystemRequest>) {
        let _ = self
            .tools
            .reboot_system(_req.0.confirm, _req.0.delay_seconds)
            .await;
    }

    #[tool(
        name = "shutdown_system",
        description = "Shutdown the TrueNAS system. Requires confirm=true for safety."
    )]
    async fn shutdown_system(&self, _req: Parameters<ShutdownSystemRequest>) {
        let _ = self
            .tools
            .shutdown_system(_req.0.confirm, _req.0.delay_seconds)
            .await;
    }

    // === Disk Management Tools ===

    #[tool(name = "list_disks", description = "List all disks on TrueNAS")]
    async fn list_disks(&self) {
        let _ = self.tools.list_disks().await;
    }

    #[tool(name = "get_disk", description = "Get details of a specific disk")]
    async fn get_disk(&self, _req: Parameters<GetDiskRequest>) {
        let _ = self.tools.get_disk(&_req.0.disk_name).await;
    }

    // === Certificate Management Tools ===

    #[tool(
        name = "list_certificates",
        description = "List all certificates on TrueNAS"
    )]
    async fn list_certificates(&self) {
        let _ = self.tools.list_certificates().await;
    }

    #[tool(
        name = "get_certificate",
        description = "Get details of a specific certificate"
    )]
    async fn get_certificate(&self, _req: Parameters<GetCertificateRequest>) {
        let _ = self.tools.get_certificate(_req.0.cert_id).await;
    }

    // === Replication Tools ===

    #[tool(
        name = "list_replication_tasks",
        description = "List all replication tasks on TrueNAS"
    )]
    async fn list_replication_tasks(&self) {
        let _ = self.tools.list_replication_tasks().await;
    }

    #[tool(name = "run_replication_task", description = "Run a replication task")]
    async fn run_replication_task(&self, _req: Parameters<RunReplicationTaskRequest>) {
        let _ = self.tools.run_replication_task(_req.0.task_id).await;
    }

    // === Cloud Sync Tools ===

    #[tool(
        name = "list_cloudsync_tasks",
        description = "List all cloud sync tasks on TrueNAS"
    )]
    async fn list_cloudsync_tasks(&self) {
        let _ = self.tools.list_cloudsync_tasks().await;
    }

    #[tool(name = "run_cloudsync_task", description = "Run a cloud sync task")]
    async fn run_cloudsync_task(&self, _req: Parameters<RunCloudSyncTaskRequest>) {
        let _ = self.tools.run_cloudsync_task(_req.0.task_id).await;
    }

    // === Enclosure Tools ===

    #[tool(name = "get_enclosure", description = "Get enclosure information")]
    async fn get_enclosure(&self) {
        let _ = self.tools.get_enclosure().await;
    }

    // === Support Tools ===

    #[tool(name = "get_support", description = "Get support information")]
    async fn get_support(&self) {
        let _ = self.tools.get_support().await;
    }
}
