use crate::api_client::ApiClient;
use crate::client::TrueNasClient;
use crate::error::Result;
use crate::error::TrueNasError;
use serde::{Deserialize, Serialize};

/// Validate a path to prevent path traversal attacks
pub(crate) fn validate_path(path: &str, field_name: &str) -> Result<()> {
    // Check for path traversal attempts
    if path.contains("..") {
        return Err(TrueNasError::ValidationError(format!(
            "{} contains path traversal sequence '..'",
            field_name
        )));
    }
    // Check for absolute paths (should be relative to pool/dataset)
    if path.starts_with('/') {
        return Err(TrueNasError::ValidationError(format!(
            "{} must be a relative path, not absolute",
            field_name
        )));
    }
    // Check for null bytes
    if path.contains('\0') {
        return Err(TrueNasError::ValidationError(format!(
            "{} contains null bytes",
            field_name
        )));
    }
    Ok(())
}

/// Represents a background task on TrueNAS
#[derive(Debug, Deserialize, Serialize)]
pub struct Task {
    /// Unique identifier for the task
    pub id: i32,
    /// Task method name
    pub method: String,
    /// Task arguments
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    /// Current state (e.g., "WAITING", "RUNNING", "SUCCESS", "FAILED")
    pub state: String,
    /// Task progress (0-100)
    #[serde(default)]
    pub progress: Option<u32>,
    /// Error message if failed
    #[serde(default)]
    pub error: Option<String>,
    /// When the task was created
    #[serde(default)]
    pub created_at: Option<i64>,
    /// When the task finished (if completed)
    #[serde(default)]
    pub finished_at: Option<i64>,
}

/// TrueNAS API response types
/// Represents a user account on TrueNAS
#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    /// Unique identifier for the user
    pub id: i32,
    /// Login username
    pub username: String,
    /// User ID number
    pub uid: i32,
    /// Home directory path
    #[serde(default)]
    pub home: Option<String>,
    /// Email address
    #[serde(default)]
    pub email: Option<String>,
    /// Full name for display
    #[serde(default)]
    pub full_name: Option<String>,
}

/// Pagination parameters for list operations
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    /// Offset in the list (default: 0)
    #[serde(default)]
    pub offset: Option<u32>,
    /// Limit number of results (default: 50, 0 means all)
    #[serde(default)]
    pub limit: Option<u32>,
    /// Order by field (e.g., "name", "-created", "status")
    #[serde(default)]
    pub order_by: Option<String>,
}

/// Filter condition for list operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCondition {
    /// Field name to filter on
    pub field: String,
    /// Operator: eq, ne, gt, gte, lt, lte, contains, startswith, endswith, in
    pub operator: String,
    /// Value to compare against
    pub value: serde_json::Value,
}

/// Filter parameters for list operations
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FilterParams {
    /// AND conditions (all must match)
    #[serde(default)]
    pub and_conditions: Vec<FilterCondition>,
    /// OR conditions (any must match)
    #[serde(default)]
    pub or_conditions: Vec<FilterCondition>,
}

/// Combined pagination and filter parameters
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ListParams {
    /// Pagination settings
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Filter settings
    #[serde(flatten)]
    pub filters: FilterParams,
}

/// Paginated response wrapper
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    /// Total number of items matching the query
    pub total: u64,
    /// Items in this page
    pub items: Vec<T>,
    /// Current offset
    pub offset: u32,
    /// Limit that was applied
    pub limit: u32,
    /// Whether there are more results
    pub has_more: bool,
}

impl<T> PaginatedResponse<T> {
    /// Create a new paginated response
    pub fn new(items: Vec<T>, total: u64, offset: u32, limit: u32) -> Self {
        let has_more = (offset as u64 + items.len() as u64) < total;
        Self {
            total,
            items,
            offset,
            limit,
            has_more,
        }
    }
}

/// Helper methods for filtering and pagination
impl FilterCondition {
    /// Compare two JSON values for ordering
    fn compare_values(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
        match (a, b) {
            (serde_json::Value::Number(an), serde_json::Value::Number(bn)) => {
                an.as_f64().and_then(|a_f| {
                    bn.as_f64()
                        .map(|b_f| a_f.partial_cmp(&b_f).unwrap_or(std::cmp::Ordering::Equal))
                })
            }
            (serde_json::Value::String(as_str), serde_json::Value::String(bs_str)) => {
                Some(as_str.cmp(bs_str))
            }
            (serde_json::Value::Bool(ab), serde_json::Value::Bool(bb)) => Some(ab.cmp(bb)),
            _ => None,
        }
    }

    /// Check if a JSON value matches this filter condition
    pub fn matches(&self, value: &serde_json::Value) -> bool {
        let target = match value.get(&self.field) {
            Some(v) => v,
            None => return false,
        };

        match self.operator.as_str() {
            "eq" => target == &self.value,
            "ne" => target != &self.value,
            "gt" => Self::compare_values(target, &self.value) == Some(std::cmp::Ordering::Greater),
            "gte" => {
                let cmp = Self::compare_values(target, &self.value);
                cmp == Some(std::cmp::Ordering::Greater) || cmp == Some(std::cmp::Ordering::Equal)
            }
            "lt" => Self::compare_values(target, &self.value) == Some(std::cmp::Ordering::Less),
            "lte" => {
                let cmp = Self::compare_values(target, &self.value);
                cmp == Some(std::cmp::Ordering::Less) || cmp == Some(std::cmp::Ordering::Equal)
            }
            "contains" => target
                .as_str()
                .map(|s| s.contains(self.value.as_str().unwrap_or("")))
                .unwrap_or(false),
            "startswith" => target
                .as_str()
                .map(|s| s.starts_with(self.value.as_str().unwrap_or("")))
                .unwrap_or(false),
            "endswith" => target
                .as_str()
                .map(|s| s.ends_with(self.value.as_str().unwrap_or("")))
                .unwrap_or(false),
            "in" => self
                .value
                .as_array()
                .map(|arr| arr.iter().any(|v| v == target))
                .unwrap_or(false),
            _ => false,
        }
    }
}

impl FilterParams {
    /// Check if an item matches all filter conditions
    pub fn matches(&self, item: &serde_json::Value) -> bool {
        // Check AND conditions
        for cond in &self.and_conditions {
            if !cond.matches(item) {
                return false;
            }
        }
        // If there are OR conditions, at least one must match
        if !self.or_conditions.is_empty() {
            let or_matches = self.or_conditions.iter().any(|cond| cond.matches(item));
            if !or_matches {
                return false;
            }
        }
        true
    }
}

impl PaginationParams {
    /// Apply ordering to a slice
    pub fn apply_ordering<T>(&self, items: &mut [T])
    where
        T: Clone + Serialize,
    {
        if let Some(ref order_by) = self.order_by {
            let mut descending = false;
            let field = if let Some(stripped) = order_by.strip_prefix('-') {
                descending = true;
                stripped
            } else {
                order_by.as_str()
            };

            let field = field.to_string();
            items.sort_by(|a, b| {
                let a_val = serde_json::to_value(a)
                    .ok()
                    .and_then(|v| v.get(&field).cloned());
                let b_val = serde_json::to_value(b)
                    .ok()
                    .and_then(|v| v.get(&field).cloned());
                match (a_val, b_val) {
                    (Some(av), Some(bv)) => {
                        let cmp = FilterCondition::compare_values(&av, &bv)
                            .unwrap_or(std::cmp::Ordering::Equal);
                        if descending { cmp.reverse() } else { cmp }
                    }
                    _ => std::cmp::Ordering::Equal,
                }
            });
        }
    }

    /// Apply pagination to a slice and return a subset
    pub fn apply_pagination<T: Clone>(&self, items: &[T]) -> (Vec<T>, u32, u32) {
        let offset = self.offset.unwrap_or(0);
        let limit = self.limit.unwrap_or(50);

        let total_len = items.len() as u32;
        let start = offset.min(total_len) as usize;
        let end_usize = items.len();
        let end = if limit == 0 {
            end_usize
        } else {
            ((offset + limit).min(total_len) as usize).min(end_usize)
        };

        (items[start..end].to_vec(), offset, limit)
    }
}

/// Represents a storage pool on TrueNAS
#[derive(Debug, Deserialize, Serialize)]
pub struct Pool {
    /// Pool name (e.g., "tank")
    pub name: String,
    /// Unique GUID identifier
    pub guid: String,
    /// Current status (e.g., "ONLINE", "OFFLINE", "DEGRADED")
    pub status: String,
    /// Total pool size in bytes
    pub size: u64,
    /// Available free space in bytes
    pub free: u64,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
}

/// Represents a ZFS dataset on TrueNAS
#[derive(Debug, Deserialize, Serialize)]
pub struct Dataset {
    /// Full dataset path (e.g., "tank/data")
    pub name: String,
    /// Parent pool name
    pub pool: String,
    /// Mount point path
    #[serde(default)]
    pub mountpoint: Option<String>,
    /// Optional comments
    #[serde(default)]
    pub comments: Option<String>,
}

/// Represents an SMB/CIFS share on TrueNAS
#[derive(Debug, Deserialize, Serialize)]
pub struct SmbShare {
    pub id: i32,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NfsExport {
    pub id: i32,
    pub paths: Vec<String>,
    pub comment: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub name: String,
    pub pool: String,
    pub dataset: String,
    pub creation: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IscsiTarget {
    pub id: i32,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub cpu_model: Option<String>,
    #[serde(default)]
    pub uptime_seconds: Option<f64>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// App information for TrueNAS apps/jails
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppInfo {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub image: Option<String>,
}

// === New response types for extended API ===

/// Group response type
#[derive(Debug, Deserialize, Serialize)]
pub struct Group {
    pub id: i32,
    pub gid: i32,
    pub name: String,
    #[serde(default)]
    pub users: Option<Vec<i32>>,
}

/// VM response type
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Vm {
    pub id: i32,
    pub name: String,
    pub vcpus: i32,
    pub memory: u64,
    pub status: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Network interface response
#[derive(Debug, Deserialize, Serialize)]
pub struct NetworkInterface {
    pub id: String,
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub ipaddr: Option<String>,
    #[serde(default)]
    pub netmask: Option<String>,
}

/// Network route response
#[derive(Debug, Deserialize, Serialize)]
pub struct NetworkRoute {
    pub destination: String,
    pub gateway: String,
    pub interface: String,
}

/// DNS configuration
#[derive(Debug, Deserialize, Serialize)]
pub struct DnsConfig {
    pub nameservers: Vec<String>,
    pub domains: Vec<String>,
}

/// Replication task response
#[derive(Debug, Deserialize, Serialize)]
pub struct ReplicationTask {
    pub id: i32,
    pub name: String,
    pub source: String,
    pub target: String,
    pub direction: String,
    pub state: String,
}

/// Cloud sync task response
#[derive(Debug, Deserialize, Serialize)]
pub struct CloudSyncTask {
    pub id: i32,
    pub description: String,
    pub direction: String,
    pub transport: String,
    pub state: String,
}

/// Cloud credentials
#[derive(Debug, Deserialize, Serialize)]
pub struct CloudCredential {
    pub id: i32,
    pub name: String,
    pub provider: String,
}

/// Service response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Service {
    pub id: i32,
    pub service: String,
    pub state: String,
    pub enable: bool,
}

/// System alerts
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Alert {
    pub id: String,
    pub level: String,
    pub message: String,
    pub timestamp: i64,
}

/// Check for updates
#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateCheck {
    pub status: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Certificate response
#[derive(Debug, Deserialize, Serialize)]
pub struct Certificate {
    pub id: i32,
    pub name: String,
    pub cert_type: String,
    pub state: String,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub until: Option<i64>,
}

/// Kubernetes status
#[derive(Debug, Deserialize, Serialize)]
pub struct KubernetesStatus {
    pub node_ip: String,
    pub cluster_ip: String,
    pub cluster_cidr: String,
    pub service_cidr: String,
    pub status: String,
}

/// Jail response
#[derive(Debug, Deserialize, Serialize)]
pub struct Jail {
    pub id: i32,
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub ip4_addr: Option<String>,
    #[serde(default)]
    pub ip6_addr: Option<String>,
}

/// Enclosure info
#[derive(Debug, Deserialize, Serialize)]
pub struct EnclosureInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub status: String,
}

/// Support info
#[derive(Debug, Deserialize, Serialize)]
pub struct SupportInfo {
    pub name: String,
    pub email: String,
    pub phone: String,
    #[serde(default)]
    pub secondary_name: Option<String>,
    #[serde(default)]
    pub secondary_email: Option<String>,
    #[serde(default)]
    pub secondary_phone: Option<String>,
}

/// Disk response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Disk {
    pub identifier: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub model: Option<String>,
    pub serial: String,
    pub type_field: String,
    #[serde(default)]
    pub temperature: Option<u64>,
    pub rotationrate: i32,
    pub crit: String,
    pub advpowermode: String,
    pub enclosure: Option<String>,
}

/// SMART test result
#[derive(Debug, Deserialize, Serialize)]
pub struct SmartTest {
    pub id: i32,
    pub type_field: String,
    pub description: String,
    pub disks: Vec<String>,
    #[serde(default)]
    pub schedule: Option<serde_json::Value>,
}

/// SMART config
#[derive(Debug, Deserialize, Serialize)]
pub struct SmartConfig {
    pub interval: Vec<i32>,
    pub critical: i32,
    pub diff: i32,
    pub informational: i32,
    pub email: Vec<String>,
    pub mode: String,
}

/// Tunable
#[derive(Debug, Deserialize, Serialize)]
pub struct Tunable {
    pub id: i32,
    pub var: String,
    pub value: String,
    pub type_field: String,
    pub comment: String,
}

/// NTP Server
#[derive(Debug, Deserialize, Serialize)]
pub struct NtpServer {
    pub id: i32,
    pub address: String,
    pub burst: bool,
    pub iburst: bool,
    pub prefer: bool,
    pub minpoll: i32,
    pub maxpoll: i32,
}

/// Alert Filter
#[derive(Debug, Deserialize, Serialize)]
pub struct AlertFilter {
    pub id: i32,
    pub name: String,
    pub program: String,
    pub level: String,
    pub message: String,
    pub enabled: bool,
}

/// Alert Service
#[derive(Debug, Deserialize, Serialize)]
pub struct AlertService {
    pub id: i32,
    pub name: String,
    pub type_field: String,
    pub enabled: bool,
}

/// Catalog
#[derive(Debug, Deserialize, Serialize)]
pub struct Catalog {
    pub id: String,
    pub label: String,
    pub repository: String,
    pub branch: String,
    #[serde(default)]
    pub status: Option<String>,
}

/// SSH Key
#[derive(Debug, Deserialize, Serialize)]
pub struct SshKey {
    pub id: i32,
    pub name: String,
    pub fingerprint: String,
    pub key: String,
}

/// SSH Config
#[derive(Debug, Deserialize, Serialize)]
pub struct SshConfig {
    pub port: i32,
    pub bindiface: Vec<String>,
    pub rootlogin: bool,
    pub passwordauth: bool,
    pub keyboardval: String,
    pub tcp_nodelay: bool,
    pub compression: bool,
    pub privatekey: String,
    pub remote_host: String,
}

/// rsync Task
#[derive(Debug, Deserialize, Serialize)]
pub struct RsyncTask {
    pub id: i32,
    pub description: String,
    pub path: String,
    pub user: String,
    pub remotehost: String,
    pub mode: String,
    pub direction: String,
    pub schedule: Option<serde_json::Value>,
}

/// rsync Module
#[derive(Debug, Deserialize, Serialize)]
pub struct RsyncModule {
    pub id: i32,
    pub name: String,
    pub comment: String,
    pub path: String,
    pub hostsallow: Vec<String>,
    pub hostsdeny: Vec<String>,
    pub uid: String,
    pub gid: String,
    pub read_only: bool,
}

/// FTP Config
#[derive(Debug, Deserialize, Serialize)]
pub struct FtpConfig {
    pub port: i32,
    #[serde(rename = "loginAttempts")]
    pub login_attempts: i32,
    #[serde(rename = "clientAlive")]
    pub client_alive: i32,
    pub rootlogin: bool,
    pub anonyuser: String,
    pub anonypass: String,
    pub only_local: bool,
}

/// SNMP Config
#[derive(Debug, Deserialize, Serialize)]
pub struct SnmpConfig {
    pub location: String,
    pub contact: String,
    pub community: String,
    pub v3: bool,
    pub traps: bool,
}

/// Active Directory Config
#[derive(Debug, Deserialize, Serialize)]
pub struct AdConfig {
    pub domainname: String,
    pub bindname: String,
    pub bindpw: String,
    pub timeout: i32,
    pub dns_timeout: i32,
    pub kerberos_realm: Option<String>,
    pub site: Option<String>,
    pub domaincontroller: Option<String>,
}

/// LDAP Config
#[derive(Debug, Deserialize, Serialize)]
pub struct LdapConfig {
    pub hostname: Vec<String>,
    pub basedn: String,
    pub binddn: String,
    pub bindpw: String,
    pub tls_ssl: bool,
    pub ldap_timeout: i32,
    #[serde(rename = "machineOu")]
    pub machine_ou: String,
}

/// Network Global Config
#[derive(Debug, Deserialize, Serialize)]
pub struct NetworkGlobalConfig {
    pub domain: String,
    pub hostname: String,
    pub ipv4gateway: String,
    pub ipv6gateway: String,
    pub nameservers: Vec<String>,
    pub httpproxy: String,
    pub netwait: Vec<String>,
}

/// Interface IP
#[derive(Debug, Deserialize, Serialize)]
pub struct InterfaceIp {
    pub id: i32,
    pub interface: String,
    pub ipaddr: String,
    pub netmask: u32,
    pub v4: bool,
    pub v6: bool,
}

/// Static Route
#[derive(Debug, Deserialize, Serialize)]
pub struct StaticRoute {
    pub id: i32,
    pub destination: String,
    pub gateway: String,
    pub description: String,
}

/// Reporting
#[derive(Debug, Deserialize, Serialize)]
pub struct Reporting {
    pub graph: Vec<String>,
}

/// Dataset Quota
#[derive(Debug, Deserialize, Serialize)]
pub struct DatasetQuota {
    pub id: String,
    pub name: String,
    pub quota: u64,
    pub used: u64,
}

/// Tool handlers for TrueNAS API operations
///
/// This struct provides methods for interacting with the TrueNAS REST API.
/// Each method corresponds to a specific TrueNAS management operation.
///
/// # Example
///
/// ```ignore
/// use truenas_master_mcp::{TrueNasClient, TrueNasConfig, TrueNasTools};
///
/// let config = TrueNasConfig {
///     server_url: "https://truenas.local".to_string(),
///     api_key: Some("your-api-key".to_string()),
///     ..Default::default()
/// };
///
/// let client = TrueNasClient::new(config).unwrap();
/// let tools = TrueNasTools::new(client);
///
/// // Example: List users
/// let users = tools.list_users().await.unwrap();
/// ```
#[derive(Debug)]
pub struct TrueNasTools {
    client: TrueNasClient,
    /// Optional API client for additional operations
    api_client: Option<ApiClient>,
}

impl TrueNasTools {
    /// Create a new TrueNasTools instance
    pub fn new(client: TrueNasClient) -> Self {
        Self {
            client,
            api_client: None,
        }
    }

    /// Create with an API client
    pub fn with_api_client(client: TrueNasClient, api_client: ApiClient) -> Self {
        Self {
            client,
            api_client: Some(api_client),
        }
    }

    /// Check if using API client
    pub fn has_api_client(&self) -> bool {
        self.api_client.is_some()
    }

    /// Get access to the API client if available
    pub fn api_client(&self) -> Option<&ApiClient> {
        self.api_client.as_ref()
    }

    // === User Management ===

    /// List all user accounts on the TrueNAS system
    pub async fn list_users(&self) -> Result<Vec<User>> {
        self.client.get("/api/v2.0/user").await
    }

    pub async fn get_user(&self, user_id: i32) -> Result<User> {
        self.client
            .get(&format!("/api/v2.0/user/{}", user_id))
            .await
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let users: Vec<User> = self.client.get("/api/v2.0/user").await?;
        users
            .into_iter()
            .find(|u| u.username == username)
            .ok_or_else(|| {
                crate::error::TrueNasError::NotFound(format!("User '{}' not found", username))
            })
    }

    // === Pool Management ===

    pub async fn list_pools(&self) -> Result<Vec<Pool>> {
        self.client.get("/api/v2.0/pool").await
    }

    pub async fn get_pool_status(&self, pool_name: &str) -> Result<Pool> {
        self.client
            .get(&format!("/api/v2.0/pool/{}", pool_name))
            .await
    }

    /// Scrub a pool
    #[allow(dead_code)]
    pub async fn scrub_pool(&self, pool_name: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct ScrubRequest {
            name: String,
        }
        self.client
            .post(
                "/api/v2.0/pool/scrub",
                &ScrubRequest {
                    name: pool_name.to_string(),
                },
            )
            .await
    }

    // === Dataset Management ===

    pub async fn list_datasets(&self) -> Result<Vec<Dataset>> {
        self.client.get("/api/v2.0/pool/dataset").await
    }

    pub async fn get_dataset(&self, dataset_path: &str) -> Result<Dataset> {
        validate_path(dataset_path, "dataset_path")?;
        let encoded = urlencoding::encode(dataset_path);
        self.client
            .get(&format!("/api/v2.0/pool/dataset/{}", encoded))
            .await
    }

    /// Get dataset by path (alias for get_dataset)
    #[allow(dead_code)]
    pub async fn get_dataset_by_path(&self, path: &str) -> Result<Dataset> {
        self.get_dataset(path).await
    }

    pub async fn create_dataset(&self, pool_name: &str, dataset_name: &str) -> Result<Dataset> {
        validate_path(dataset_name, "dataset_name")?;
        #[derive(Serialize)]
        struct CreateDatasetRequest {
            name: String,
        }
        let full_name = format!("{}/{}", pool_name, dataset_name);
        self.client
            .post(
                "/api/v2.0/pool/dataset",
                &CreateDatasetRequest { name: full_name },
            )
            .await
    }

    pub async fn delete_dataset(&self, dataset_path: &str) -> Result<()> {
        validate_path(dataset_path, "dataset_path")?;
        let encoded = urlencoding::encode(dataset_path);
        self.client
            .delete(&format!("/api/v2.0/pool/dataset/{}", encoded))
            .await
    }

    /// Update a dataset's properties
    #[allow(dead_code)]
    pub async fn update_dataset(
        &self,
        dataset_path: &str,
        updates: serde_json::Value,
    ) -> Result<Dataset> {
        validate_path(dataset_path, "dataset_path")?;
        let encoded = urlencoding::encode(dataset_path);
        self.client
            .put(&format!("/api/v2.0/pool/dataset/{}", encoded), &updates)
            .await
    }

    // === SMB Shares ===

    pub async fn list_smb_shares(&self) -> Result<Vec<SmbShare>> {
        self.client.get("/api/v2.0/sharing/smb").await
    }

    pub async fn create_smb_share(
        &self,
        name: &str,
        path: &str,
        comment: Option<&str>,
    ) -> Result<SmbShare> {
        validate_path(path, "path")?;
        #[derive(Serialize)]
        struct CreateSmbRequest {
            name: String,
            path: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            comment: Option<String>,
        }
        self.client
            .post(
                "/api/v2.0/sharing/smb",
                &CreateSmbRequest {
                    name: name.to_string(),
                    path: path.to_string(),
                    comment: comment.map(|c| c.to_string()),
                },
            )
            .await
    }

    pub async fn delete_smb_share(&self, share_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/sharing/smb/{}", share_id))
            .await
    }

    // === NFS Exports ===

    pub async fn list_nfs_exports(&self) -> Result<Vec<NfsExport>> {
        self.client.get("/api/v2.0/sharing/nfs").await
    }

    pub async fn create_nfs_export(
        &self,
        paths: Vec<String>,
        comment: String,
    ) -> Result<NfsExport> {
        // Validate all paths
        for path in &paths {
            validate_path(path, "path")?;
        }
        #[derive(Serialize)]
        struct CreateNfsRequest {
            paths: Vec<String>,
            comment: String,
        }
        self.client
            .post(
                "/api/v2.0/sharing/nfs",
                &CreateNfsRequest { paths, comment },
            )
            .await
    }

    pub async fn delete_nfs_export(&self, export_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/sharing/nfs/{}", export_id))
            .await
    }

    // === Snapshots ===

    pub async fn list_snapshots(&self) -> Result<Vec<Snapshot>> {
        self.client.get("/api/v2.0/zfs/snapshot").await
    }

    pub async fn create_snapshot(&self, dataset: &str, snapshot_name: &str) -> Result<Snapshot> {
        validate_path(dataset, "dataset")?;
        validate_path(snapshot_name, "snapshot_name")?;
        #[derive(Serialize)]
        struct CreateSnapshotRequest {
            dataset: String,
            name: String,
        }
        self.client
            .post(
                "/api/v2.0/zfs/snapshot",
                &CreateSnapshotRequest {
                    dataset: dataset.to_string(),
                    name: snapshot_name.to_string(),
                },
            )
            .await
    }

    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/zfs/snapshot/{}", snapshot_id))
            .await
    }

    /// Rollback a dataset to a specific snapshot
    #[allow(dead_code)]
    pub async fn rollback_snapshot(&self, dataset: &str, snapshot_name: &str) -> Result<()> {
        #[derive(Serialize)]
        struct RollbackRequest {
            dataset: String,
            name: String,
            force: bool,
        }
        self.client
            .post(
                "/api/v2.0/zfs/snapshot/rollback",
                &RollbackRequest {
                    dataset: dataset.to_string(),
                    name: snapshot_name.to_string(),
                    force: true,
                },
            )
            .await
    }

    /// Clone a snapshot to a new dataset
    #[allow(dead_code)]
    pub async fn clone_snapshot(&self, snapshot_id: &str, target_name: &str) -> Result<Dataset> {
        #[derive(Serialize)]
        struct CloneRequest {
            snapshot: String,
            dataset_dst: String,
        }
        self.client
            .post(
                "/api/v2.0/zfs/snapshot/clone",
                &CloneRequest {
                    snapshot: snapshot_id.to_string(),
                    dataset_dst: target_name.to_string(),
                },
            )
            .await
    }

    /// Get all snapshots for a specific dataset
    #[allow(dead_code)]
    pub async fn get_dataset_snapshots(&self, dataset: &str) -> Result<Vec<Snapshot>> {
        let encoded = urlencoding::encode(dataset);
        self.client
            .get(&format!("/api/v2.0/zfs/snapshot?dataset={}", encoded))
            .await
    }

    // === iSCSI Targets ===

    pub async fn list_iscsi_targets(&self) -> Result<Vec<IscsiTarget>> {
        self.client.get("/api/v2.0/iscsi/target").await
    }

    pub async fn create_iscsi_target(&self, name: &str) -> Result<IscsiTarget> {
        #[derive(Serialize)]
        struct CreateIscsiRequest {
            name: String,
        }
        self.client
            .post(
                "/api/v2.0/iscsi/target",
                &CreateIscsiRequest {
                    name: name.to_string(),
                },
            )
            .await
    }

    pub async fn delete_iscsi_target(&self, target_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/iscsi/target/{}", target_id))
            .await
    }

    // === System Information ===

    pub async fn get_system_info(&self) -> Result<SystemInfo> {
        self.client.get("/api/v2.0/system/info").await
    }

    // === Apps (Jails/Containers) ===
    #[cfg(feature = "scale")]
    /// List all applications/jails on TrueNAS
    pub async fn list_apps(&self) -> Result<Vec<AppInfo>> {
        // For TrueNAS SCALE with apps (Kubernetes/Helm charts)
        #[derive(Deserialize)]
        struct ScaleAppResponse {
            #[serde(default)]
            name: String,
            #[serde(default)]
            version: Option<String>,
            #[serde(default)]
            state: Option<String>,
            #[serde(default)]
            description: Option<String>,
        }

        #[derive(Deserialize)]
        struct ScaleAppsList {
            #[serde(default)]
            apps: Vec<ScaleAppResponse>,
        }

        // Try SCALE apps endpoint first
        let scale_result: Option<ScaleAppsList> = self.client.get("/api/v2.0/app").await.ok();
        if let Some(response) = scale_result {
            return Ok(response
                .apps
                .into_iter()
                .map(|app| AppInfo {
                    name: app.name,
                    version: app.version,
                    state: app.state,
                    description: app.description,
                    port: None,
                    image: None,
                })
                .collect());
        }

        // Fall back to CORE jail endpoint
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct JailResponse {
            #[serde(default)]
            id: i32,
            #[serde(default)]
            name: String,
            #[serde(default)]
            state: String,
        }

        #[derive(Deserialize)]
        struct JailsList {
            #[serde(default)]
            #[serde(rename = "jails")]
            jails_list: Vec<JailResponse>,
        }

        let jails: JailsList = self
            .client
            .get("/api/v2.0/jail")
            .await
            .unwrap_or(JailsList { jails_list: vec![] });

        Ok(jails
            .jails_list
            .into_iter()
            .map(|jail| AppInfo {
                name: jail.name,
                version: None,
                state: Some(jail.state),
                description: None,
                port: None,
                image: None,
            })
            .collect())
    }

    /// Get details of a specific application
    pub async fn get_app(&self, app_name: &str) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);

        // Try SCALE app endpoint first
        #[derive(Deserialize)]
        struct ScaleAppDetail {
            #[serde(default)]
            name: String,
            #[serde(default)]
            version: Option<String>,
            #[serde(default)]
            state: Option<String>,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            port: Option<u16>,
            #[serde(default)]
            image: Option<String>,
        }

        let scale_result: Option<ScaleAppDetail> = self
            .client
            .get(&format!("/api/v2.0/app/{}", encoded))
            .await
            .ok();
        if let Some(app) = scale_result {
            return Ok(AppInfo {
                name: app.name,
                version: app.version,
                state: app.state,
                description: app.description,
                port: app.port,
                image: app.image,
            });
        }

        // Fall back to CORE jail endpoint
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct JailDetail {
            #[serde(default)]
            id: i32,
            #[serde(default)]
            name: String,
            #[serde(default)]
            state: String,
        }

        let jail: JailDetail = self
            .client
            .get(&format!("/api/v2.0/jail/{}", encoded))
            .await?;

        Ok(AppInfo {
            name: jail.name,
            version: None,
            state: Some(jail.state),
            description: None,
            port: None,
            image: None,
        })
    }

    /// Start an application
    pub async fn start_app(
        &self,
        app_name: &str,
        options: Option<serde_json::Value>,
    ) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);

        #[derive(Serialize)]
        struct StartRequest {
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<serde_json::Value>,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct StartResponse {
            #[serde(default)]
            name: String,
            #[serde(default)]
            state: Option<String>,
        }

        // Try SCALE endpoint
        let _response: StartResponse = self
            .client
            .post(
                &format!("/api/v2.0/app/{}/start", encoded),
                &StartRequest { options },
            )
            .await?;

        self.get_app(app_name).await
    }

    /// Stop an application
    pub async fn stop_app(&self, app_name: &str, force: bool) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);

        #[derive(Serialize)]
        struct StopRequest {
            force: bool,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct StopResponse {
            #[serde(default)]
            name: String,
            #[serde(default)]
            state: Option<String>,
        }

        // Try SCALE endpoint
        let _response: StopResponse = self
            .client
            .post(
                &format!("/api/v2.0/app/{}/stop", encoded),
                &StopRequest { force },
            )
            .await?;

        self.get_app(app_name).await
    }

    /// Restart an application
    pub async fn restart_app(&self, app_name: &str) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct RestartResponse {
            #[serde(default)]
            name: String,
            #[serde(default)]
            state: Option<String>,
        }

        // Try SCALE endpoint
        let _response: RestartResponse = self
            .client
            .post(&format!("/api/v2.0/app/{}/restart", encoded), &())
            .await?;

        self.get_app(app_name).await
    }

    // === App Management (SCALE-specific) ===
    #[allow(dead_code)]
    pub async fn create_app(
        &self,
        catalog: &str,
        item: &str,
        name: &str,
        values: serde_json::Value,
        version: Option<&str>,
    ) -> Result<AppInfo> {
        #[derive(Serialize)]
        struct CreateAppRequest {
            catalog: String,
            item: String,
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<String>,
            values: serde_json::Value,
        }
        self.client
            .post(
                "/api/v2.0/app",
                &CreateAppRequest {
                    catalog: catalog.to_string(),
                    item: item.to_string(),
                    name: name.to_string(),
                    version: version.map(|v| v.to_string()),
                    values,
                },
            )
            .await
    }

    /// Update an existing application
    #[allow(dead_code)]
    pub async fn update_app(&self, app_name: &str, values: serde_json::Value) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);
        self.client
            .put(&format!("/api/v2.0/app/{}", encoded), &values)
            .await
    }

    /// Delete an application
    #[allow(dead_code)]
    pub async fn delete_app(&self, app_name: &str, force: bool) -> Result<()> {
        let encoded = urlencoding::encode(app_name);
        #[derive(Serialize)]
        struct DeleteRequest {
            force: bool,
        }
        self.client
            .delete_with_body(
                &format!("/api/v2.0/app/{}", encoded),
                &DeleteRequest { force },
            )
            .await
    }

    /// Rollback an application to a previous version
    #[allow(dead_code)]
    pub async fn rollback_app(
        &self,
        app_name: &str,
        rollback_version: Option<&str>,
        snap_name: Option<&str>,
        force: bool,
    ) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);
        #[derive(Serialize)]
        struct RollbackRequest {
            #[serde(skip_serializing_if = "Option::is_none")]
            rollback_version: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            snap_name: Option<String>,
            force: bool,
        }
        self.client
            .post(
                &format!("/api/v2.0/app/{}/rollback", encoded),
                &RollbackRequest {
                    rollback_version: rollback_version.map(|v| v.to_string()),
                    snap_name: snap_name.map(|v| v.to_string()),
                    force,
                },
            )
            .await
    }

    /// Get configuration of an application
    #[allow(dead_code)]
    pub async fn get_app_config(&self, app_name: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(app_name);
        self.client
            .get(&format!("/api/v2.0/app/{}/config", encoded))
            .await
    }

    /// Get upgrade options for an application
    #[allow(dead_code)]
    pub async fn get_app_upgrade_options(&self, app_name: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(app_name);
        self.client
            .get(&format!("/api/v2.0/app/{}/upgrade_options", encoded))
            .await
    }

    /// Upgrade an application
    #[allow(dead_code)]
    pub async fn upgrade_app(&self, app_name: &str, options: serde_json::Value) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);
        self.client
            .post(&format!("/api/v2.0/app/{}/upgrade", encoded), &options)
            .await
    }

    /// List available catalog items
    #[allow(dead_code)]
    pub async fn list_catalog_items(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/catalog").await
    }

    /// Get details of a specific catalog
    #[allow(dead_code)]
    pub async fn get_catalog(&self, catalog_id: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(catalog_id);
        self.client
            .get(&format!("/api/v2.0/catalog/{}", encoded))
            .await
    }

    /// Get all available train versions from a catalog
    #[allow(dead_code)]
    pub async fn get_catalog_trains(&self, catalog_id: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(catalog_id);
        self.client
            .get(&format!("/api/v2.0/catalog/{}/trains", encoded))
            .await
    }

    /// Get item details from a catalog
    #[allow(dead_code)]
    pub async fn get_catalog_item(
        &self,
        catalog_id: &str,
        item: &str,
        train: &str,
    ) -> Result<serde_json::Value> {
        let encoded_catalog = urlencoding::encode(catalog_id);
        let encoded_item = urlencoding::encode(item);
        self.client
            .get(&format!(
                "/api/v2.0/catalog/{}/{}/{}",
                encoded_catalog, encoded_item, train
            ))
            .await
    }

    /// List chart releases (deployed apps)
    #[allow(dead_code)]
    pub async fn list_chart_releases(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/chart/release").await
    }

    /// Get chart release details
    #[allow(dead_code)]
    pub async fn get_chart_release(&self, release_name: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(release_name);
        self.client
            .get(&format!("/api/v2.0/chart/release/{}", encoded))
            .await
    }

    /// Get chart release resources
    #[allow(dead_code)]
    pub async fn get_chart_release_resources(
        &self,
        release_name: &str,
    ) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(release_name);
        self.client
            .get(&format!("/api/v2.0/chart/release/{}/resources", encoded))
            .await
    }

    /// Scale an app replica set
    #[allow(dead_code)]
    pub async fn scale_app(&self, app_name: &str, replica: i32) -> Result<AppInfo> {
        let encoded = urlencoding::encode(app_name);
        #[derive(Serialize)]
        struct ScaleRequest {
            replica: i32,
        }
        self.client
            .post(
                &format!("/api/v2.0/app/{}/scale", encoded),
                &ScaleRequest { replica },
            )
            .await
    }

    // === User Management (Extended) ===

    /// Create a new user
    #[allow(dead_code)]
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        uid: Option<i32>,
        group_ids: Option<Vec<i32>>,
    ) -> Result<User> {
        #[derive(Serialize)]
        struct CreateUserRequest {
            username: String,
            password: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            uid: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            group_ids: Option<Vec<i32>>,
        }
        self.client
            .post(
                "/api/v2.0/user",
                &CreateUserRequest {
                    username: username.to_string(),
                    password: password.to_string(),
                    uid,
                    group_ids,
                },
            )
            .await
    }

    /// Update a user
    #[allow(dead_code)]
    pub async fn update_user(&self, user_id: i32, updates: serde_json::Value) -> Result<User> {
        self.client
            .put(&format!("/api/v2.0/user/{}", user_id), &updates)
            .await
    }

    /// Delete a user
    #[allow(dead_code)]
    pub async fn delete_user(&self, user_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/user/{}", user_id))
            .await
    }

    // === Group Management ===

    /// List all groups
    #[allow(dead_code)]
    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        self.client.get("/api/v2.0/group").await
    }

    /// Get group by ID
    #[allow(dead_code)]
    pub async fn get_group(&self, group_id: i32) -> Result<Group> {
        self.client
            .get(&format!("/api/v2.0/group/{}", group_id))
            .await
    }

    /// Get group by name
    #[allow(dead_code)]
    pub async fn get_group_by_name(&self, name: &str) -> Result<Group> {
        let groups: Vec<Group> = self.client.get("/api/v2.0/group").await?;
        groups.into_iter().find(|g| g.name == name).ok_or_else(|| {
            crate::error::TrueNasError::NotFound(format!("Group '{}' not found", name))
        })
    }

    /// Create a new group
    #[allow(dead_code)]
    pub async fn create_group(
        &self,
        name: &str,
        gid: Option<i32>,
        users: Option<Vec<i32>>,
    ) -> Result<Group> {
        #[derive(Serialize)]
        struct CreateGroupRequest {
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            gid: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            users: Option<Vec<i32>>,
        }
        self.client
            .post(
                "/api/v2.0/group",
                &CreateGroupRequest {
                    name: name.to_string(),
                    gid,
                    users,
                },
            )
            .await
    }

    /// Update a group
    #[allow(dead_code)]
    pub async fn update_group(&self, group_id: i32, updates: serde_json::Value) -> Result<Group> {
        self.client
            .put(&format!("/api/v2.0/group/{}", group_id), &updates)
            .await
    }

    /// Delete a group
    #[allow(dead_code)]
    pub async fn delete_group(&self, group_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/group/{}", group_id))
            .await
    }

    // === VM Management ===

    /// List all VMs
    #[allow(dead_code)]
    pub async fn list_vms(&self) -> Result<Vec<Vm>> {
        self.client.get("/api/v2.0/vm").await
    }

    /// Get VM by ID
    #[allow(dead_code)]
    pub async fn get_vm(&self, vm_id: i32) -> Result<Vm> {
        self.client.get(&format!("/api/v2.0/vm/{}", vm_id)).await
    }

    /// Create a new VM
    #[allow(dead_code)]
    pub async fn create_vm(
        &self,
        name: &str,
        vcpus: i32,
        memory: u64,
        disk_size: Option<u64>,
        iso: Option<&str>,
    ) -> Result<Vm> {
        #[derive(Serialize)]
        struct CreateVmRequest {
            name: String,
            vcpus: i32,
            memory: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            disk_size: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            iso: Option<String>,
        }
        self.client
            .post(
                "/api/v2.0/vm",
                &CreateVmRequest {
                    name: name.to_string(),
                    vcpus,
                    memory,
                    disk_size,
                    iso: iso.map(|s| s.to_string()),
                },
            )
            .await
    }

    /// Update a VM
    #[allow(dead_code)]
    pub async fn update_vm(&self, vm_id: i32, updates: serde_json::Value) -> Result<Vm> {
        self.client
            .put(&format!("/api/v2.0/vm/{}", vm_id), &updates)
            .await
    }

    /// Delete a VM
    #[allow(dead_code)]
    pub async fn delete_vm(&self, vm_id: i32, force: bool) -> Result<()> {
        self.client
            .delete_with_body(&format!("/api/v2.0/vm/{}", vm_id), &force)
            .await
    }

    /// Start a VM
    #[allow(dead_code)]
    pub async fn start_vm(&self, vm_id: i32) -> Result<Vm> {
        self.client
            .post(&format!("/api/v2.0/vm/{}/start", vm_id), &())
            .await
    }

    /// Stop a VM
    #[allow(dead_code)]
    pub async fn stop_vm(&self, vm_id: i32, force: bool) -> Result<Vm> {
        #[derive(Serialize)]
        struct StopRequest {
            force: bool,
        }
        self.client
            .post(
                &format!("/api/v2.0/vm/{}/stop", vm_id),
                &StopRequest { force },
            )
            .await
    }

    /// Restart a VM
    #[allow(dead_code)]
    pub async fn restart_vm(&self, vm_id: i32) -> Result<Vm> {
        self.client
            .post(&format!("/api/v2.0/vm/{}/restart", vm_id), &())
            .await
    }

    /// Power cycle a VM
    #[allow(dead_code)]
    pub async fn powercycle_vm(&self, vm_id: i32) -> Result<Vm> {
        self.client
            .post(&format!("/api/v2.0/vm/{}/powercycle", vm_id), &())
            .await
    }

    /// Clone a VM
    #[allow(dead_code)]
    pub async fn clone_vm(&self, vm_id: i32, name: &str) -> Result<Vm> {
        #[derive(Serialize)]
        struct CloneRequest {
            name: String,
        }
        self.client
            .post(
                &format!("/api/v2.0/vm/{}/clone", vm_id),
                &CloneRequest {
                    name: name.to_string(),
                },
            )
            .await
    }

    // === Network Management ===

    /// List network interfaces
    #[allow(dead_code)]
    pub async fn list_interfaces(&self) -> Result<Vec<NetworkInterface>> {
        self.client.get("/api/v2.0/network/interface").await
    }

    /// Get network interface
    #[allow(dead_code)]
    pub async fn get_interface(&self, interface_id: &str) -> Result<NetworkInterface> {
        let encoded = urlencoding::encode(interface_id);
        self.client
            .get(&format!("/api/v2.0/network/interface/{}", encoded))
            .await
    }

    /// List network routes
    #[allow(dead_code)]
    pub async fn list_routes(&self) -> Result<Vec<NetworkRoute>> {
        self.client.get("/api/v2.0/network/route").await
    }

    /// Get DNS configuration
    #[allow(dead_code)]
    pub async fn get_dns(&self) -> Result<DnsConfig> {
        self.client.get("/api/v2.0/network/dns").await
    }

    /// Update DNS configuration
    #[allow(dead_code)]
    pub async fn update_dns(
        &self,
        nameservers: Vec<String>,
        domains: Vec<String>,
    ) -> Result<DnsConfig> {
        #[derive(Serialize)]
        struct DnsUpdateRequest {
            nameservers: Vec<String>,
            domains: Vec<String>,
        }
        self.client
            .put(
                "/api/v2.0/network/dns",
                &DnsUpdateRequest {
                    nameservers,
                    domains,
                },
            )
            .await
    }

    // === Replication Tasks ===

    /// List replication tasks
    #[allow(dead_code)]
    pub async fn list_replication_tasks(&self) -> Result<Vec<ReplicationTask>> {
        self.client.get("/api/v2.0/replication").await
    }

    /// Get replication task
    #[allow(dead_code)]
    pub async fn get_replication_task(&self, task_id: i32) -> Result<ReplicationTask> {
        self.client
            .get(&format!("/api/v2.0/replication/{}", task_id))
            .await
    }

    /// Create replication task
    #[allow(dead_code)]
    pub async fn create_replication_task(
        &self,
        name: &str,
        source: &str,
        target: &str,
        recursive: bool,
    ) -> Result<ReplicationTask> {
        #[derive(Serialize)]
        struct CreateReplicationRequest {
            name: String,
            source: String,
            target: String,
            recursive: bool,
        }
        self.client
            .post(
                "/api/v2.0/replication",
                &CreateReplicationRequest {
                    name: name.to_string(),
                    source: source.to_string(),
                    target: target.to_string(),
                    recursive,
                },
            )
            .await
    }

    /// Create replication task from JSON
    #[allow(dead_code)]
    pub async fn create_replication_task_json(
        &self,
        task: serde_json::Value,
    ) -> Result<ReplicationTask> {
        self.client.post("/api/v2.0/replication", &task).await
    }

    /// Delete replication task
    #[allow(dead_code)]
    pub async fn delete_replication_task(&self, task_id: i32, force: bool) -> Result<()> {
        #[derive(Serialize)]
        struct DeleteRequest {
            force: bool,
        }
        self.client
            .delete_with_body(
                &format!("/api/v2.0/replication/{}", task_id),
                &DeleteRequest { force },
            )
            .await
    }

    /// Run replication task
    #[allow(dead_code)]
    pub async fn run_replication_task(&self, task_id: i32) -> Result<ReplicationTask> {
        self.client
            .post(&format!("/api/v2.0/replication/{}/run", task_id), &())
            .await
    }

    // === Cloud Sync ===

    /// List cloud sync tasks
    #[allow(dead_code)]
    pub async fn list_cloudsync_tasks(&self) -> Result<Vec<CloudSyncTask>> {
        self.client.get("/api/v2.0/cloudsync").await
    }

    /// Get cloud sync task
    #[allow(dead_code)]
    pub async fn get_cloudsync_task(&self, task_id: i32) -> Result<CloudSyncTask> {
        self.client
            .get(&format!("/api/v2.0/cloudsync/{}", task_id))
            .await
    }

    /// Create cloud sync task
    #[allow(dead_code)]
    pub async fn create_cloudsync_task(
        &self,
        description: &str,
        direction: &str,
        path: &str,
        remote: &str,
    ) -> Result<CloudSyncTask> {
        #[derive(Serialize)]
        struct CreateCloudSyncRequest {
            description: String,
            direction: String,
            path: String,
            remote: String,
        }
        self.client
            .post(
                "/api/v2.0/cloudsync",
                &CreateCloudSyncRequest {
                    description: description.to_string(),
                    direction: direction.to_string(),
                    path: path.to_string(),
                    remote: remote.to_string(),
                },
            )
            .await
    }

    /// Create cloud sync task from JSON
    #[allow(dead_code)]
    pub async fn create_cloudsync_task_json(
        &self,
        task: serde_json::Value,
    ) -> Result<CloudSyncTask> {
        self.client.post("/api/v2.0/cloudsync", &task).await
    }

    /// Delete cloud sync task
    #[allow(dead_code)]
    pub async fn delete_cloudsync_task(&self, task_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/cloudsync/{}", task_id))
            .await
    }

    /// Run cloud sync task
    #[allow(dead_code)]
    pub async fn run_cloudsync_task(&self, task_id: i32) -> Result<CloudSyncTask> {
        self.client
            .post(&format!("/api/v2.0/cloudsync/{}/run", task_id), &())
            .await
    }

    /// List cloud credentials
    #[allow(dead_code)]
    pub async fn list_cloud_credentials(&self) -> Result<Vec<CloudCredential>> {
        self.client.get("/api/v2.0/cloudsync/credentials").await
    }

    // === Services Management ===

    /// List all services
    #[allow(dead_code)]
    pub async fn list_services(&self) -> Result<Vec<Service>> {
        self.client.get("/api/v2.0/service").await
    }

    /// Get service status
    #[allow(dead_code)]
    pub async fn get_service(&self, service_id: i32) -> Result<Service> {
        self.client
            .get(&format!("/api/v2.0/service/{}", service_id))
            .await
    }

    /// Start service
    #[allow(dead_code)]
    pub async fn start_service(&self, service_id: i32) -> Result<Service> {
        self.client
            .post(&format!("/api/v2.0/service/{}/start", service_id), &())
            .await
    }

    /// Stop service
    #[allow(dead_code)]
    pub async fn stop_service(&self, service_id: i32) -> Result<Service> {
        self.client
            .post(&format!("/api/v2.0/service/{}/stop", service_id), &())
            .await
    }

    /// Restart service
    #[allow(dead_code)]
    pub async fn restart_service(&self, service_id: i32) -> Result<Service> {
        self.client
            .post(&format!("/api/v2.0/service/{}/restart", service_id), &())
            .await
    }

    /// Check if service is started
    #[allow(dead_code)]
    pub async fn service_started(&self, service_id: i32) -> Result<bool> {
        self.client
            .get(&format!("/api/v2.0/service/{}/started", service_id))
            .await
    }

    // === System Management ===

    /// Get system alerts
    #[allow(dead_code)]
    pub async fn get_alerts(&self) -> Result<Vec<Alert>> {
        self.client.get("/api/v2.0/system/alert").await
    }

    /// Clear alerts
    #[allow(dead_code)]
    pub async fn clear_alerts(&self) -> Result<()> {
        self.client.delete("/api/v2.0/system/alert").await
    }

    /// Reboot system
    ///
    /// # Safety
    /// This will immediately reboot the TrueNAS system.
    /// Use `confirm = true` to explicitly confirm this destructive operation.
    #[allow(dead_code)]
    pub async fn reboot_system(&self, confirm: bool, delay_seconds: Option<u32>) -> Result<()> {
        if !confirm {
            return Err(TrueNasError::ValidationError(
                "Reboot requires confirmation. Set confirm=true to proceed.".to_string(),
            ));
        }
        #[derive(Serialize)]
        struct RebootRequest {
            delay: u32,
        }
        let delay = delay_seconds.unwrap_or(10);
        self.client
            .post("/api/v2.0/system/reboot", &RebootRequest { delay })
            .await
    }

    /// Shutdown system
    ///
    /// # Safety
    /// This will immediately shut down the TrueNAS system.
    /// Use `confirm = true` to explicitly confirm this destructive operation.
    #[allow(dead_code)]
    pub async fn shutdown_system(&self, confirm: bool, delay_seconds: Option<u32>) -> Result<()> {
        if !confirm {
            return Err(TrueNasError::ValidationError(
                "Shutdown requires confirmation. Set confirm=true to proceed.".to_string(),
            ));
        }
        #[derive(Serialize)]
        struct ShutdownRequest {
            delay: u32,
        }
        let delay = delay_seconds.unwrap_or(10);
        self.client
            .post("/api/v2.0/system/shutdown", &ShutdownRequest { delay })
            .await
    }

    /// Check for updates
    #[allow(dead_code)]
    pub async fn check_for_updates(&self) -> Result<UpdateCheck> {
        self.client.get("/api/v2.0/update/check").await
    }

    /// Update system
    #[allow(dead_code)]
    pub async fn update_system(
        &self,
        options: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        self.client
            .post(
                "/api/v2.0/update",
                &options.unwrap_or(serde_json::json!({})),
            )
            .await
    }

    /// Get system general config
    #[allow(dead_code)]
    pub async fn get_general_config(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/general").await
    }

    /// Update system general config
    #[allow(dead_code)]
    pub async fn update_general_config(
        &self,
        updates: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.client.put("/api/v2.0/system/general", &updates).await
    }

    /// Get system advanced config
    #[allow(dead_code)]
    pub async fn get_advanced_config(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/advanced").await
    }

    /// Update system advanced config
    #[allow(dead_code)]
    pub async fn update_advanced_config(
        &self,
        updates: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.client.put("/api/v2.0/system/advanced", &updates).await
    }

    /// Get boot configuration
    #[allow(dead_code)]
    pub async fn get_boot_config(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/boot").await
    }

    // === Certificate Management ===

    /// List certificates
    #[allow(dead_code)]
    pub async fn list_certificates(&self) -> Result<Vec<Certificate>> {
        self.client.get("/api/v2.0/certificate").await
    }

    /// Get certificate
    #[allow(dead_code)]
    pub async fn get_certificate(&self, cert_id: i32) -> Result<Certificate> {
        self.client
            .get(&format!("/api/v2.0/certificate/{}", cert_id))
            .await
    }

    /// Create certificate
    #[allow(dead_code)]
    pub async fn create_certificate(
        &self,
        name: &str,
        cert_type: &str,
        cert: &str,
        key: &str,
    ) -> Result<Certificate> {
        #[derive(Serialize)]
        struct CreateCertRequest {
            name: String,
            cert_type: String,
            cert: String,
            key: String,
        }
        self.client
            .post(
                "/api/v2.0/certificate",
                &CreateCertRequest {
                    name: name.to_string(),
                    cert_type: cert_type.to_string(),
                    cert: cert.to_string(),
                    key: key.to_string(),
                },
            )
            .await
    }

    /// Delete certificate
    #[allow(dead_code)]
    pub async fn delete_certificate(&self, cert_id: i32, force: bool) -> Result<()> {
        #[derive(Serialize)]
        struct DeleteRequest {
            force: bool,
        }
        self.client
            .delete_with_body(
                &format!("/api/v2.0/certificate/{}", cert_id),
                &DeleteRequest { force },
            )
            .await
    }

    // === Kubernetes (SCALE) ===
    #[cfg(feature = "scale")]
    /// Get Kubernetes status
    #[allow(dead_code)]
    pub async fn get_kubernetes_status(&self) -> Result<KubernetesStatus> {
        self.client.get("/api/v2.0/kubernetes").await
    }

    /// Configure Kubernetes
    #[allow(dead_code)]
    pub async fn configure_kubernetes(
        &self,
        node_ip: &str,
        cluster_cidr: &str,
        service_cidr: &str,
    ) -> Result<KubernetesStatus> {
        #[derive(Serialize)]
        struct K8sConfig {
            node_ip: String,
            cluster_cidr: String,
            service_cidr: String,
        }
        self.client
            .post(
                "/api/v2.0/kubernetes",
                &K8sConfig {
                    node_ip: node_ip.to_string(),
                    cluster_cidr: cluster_cidr.to_string(),
                    service_cidr: service_cidr.to_string(),
                },
            )
            .await
    }

    /// List Kubernetes backups
    #[allow(dead_code)]
    pub async fn list_kubernetes_backups(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/kubernetes/backups").await
    }

    /// Create Kubernetes backup
    #[allow(dead_code)]
    pub async fn create_kubernetes_backup(&self, name: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct BackupRequest {
            name: String,
        }
        self.client
            .post(
                "/api/v2.0/kubernetes/backups",
                &BackupRequest {
                    name: name.to_string(),
                },
            )
            .await
    }

    /// Restore Kubernetes backup
    #[allow(dead_code)]
    pub async fn restore_kubernetes_backup(&self, backup_name: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(backup_name);
        self.client
            .post(
                &format!("/api/v2.0/kubernetes/backups/{}/restore", encoded),
                &(),
            )
            .await
    }

    // === Jails (CORE) ===
    #[cfg(feature = "core")]
    /// List jails
    #[allow(dead_code)]
    pub async fn list_jails(&self) -> Result<Vec<Jail>> {
        self.client.get("/api/v2.0/jail").await
    }

    /// Get jail by ID
    #[allow(dead_code)]
    pub async fn get_jail(&self, jail_id: i32) -> Result<Jail> {
        self.client
            .get(&format!("/api/v2.0/jail/{}", jail_id))
            .await
    }

    /// Get jail by name
    #[allow(dead_code)]
    pub async fn get_jail_by_name(&self, name: &str) -> Result<Jail> {
        let encoded = urlencoding::encode(name);
        self.client
            .get(&format!("/api/v2.0/jail/{}", encoded))
            .await
    }

    /// Create jail
    #[allow(dead_code)]
    pub async fn create_jail(
        &self,
        name: &str,
        jail_base: &str,
        ip4_addr: Option<&str>,
    ) -> Result<Jail> {
        #[derive(Serialize)]
        struct CreateJailRequest {
            name: String,
            jail_base: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            ip4_addr: Option<String>,
        }
        self.client
            .post(
                "/api/v2.0/jail",
                &CreateJailRequest {
                    name: name.to_string(),
                    jail_base: jail_base.to_string(),
                    ip4_addr: ip4_addr.map(|s| s.to_string()),
                },
            )
            .await
    }

    /// Update jail
    #[allow(dead_code)]
    pub async fn update_jail(&self, jail_id: i32, updates: serde_json::Value) -> Result<Jail> {
        self.client
            .put(&format!("/api/v2.0/jail/{}", jail_id), &updates)
            .await
    }

    /// Delete jail
    #[allow(dead_code)]
    pub async fn delete_jail(&self, jail_id: i32, force: bool) -> Result<()> {
        #[derive(Serialize)]
        struct DeleteRequest {
            force: bool,
        }
        self.client
            .delete_with_body(
                &format!("/api/v2.0/jail/{}", jail_id),
                &DeleteRequest { force },
            )
            .await
    }

    /// Start jail
    #[allow(dead_code)]
    pub async fn start_jail(&self, jail_id: i32) -> Result<Jail> {
        self.client
            .post(&format!("/api/v2.0/jail/{}/start", jail_id), &())
            .await
    }

    /// Stop jail
    #[allow(dead_code)]
    pub async fn stop_jail(&self, jail_id: i32) -> Result<Jail> {
        self.client
            .post(&format!("/api/v2.0/jail/{}/stop", jail_id), &())
            .await
    }

    /// Restart jail
    #[allow(dead_code)]
    pub async fn restart_jail(&self, jail_id: i32) -> Result<Jail> {
        self.client
            .post(&format!("/api/v2.0/jail/{}/restart", jail_id), &())
            .await
    }

    /// Clone jail
    #[allow(dead_code)]
    pub async fn clone_jail(&self, jail_id: i32, name: &str) -> Result<Jail> {
        #[derive(Serialize)]
        struct CloneRequest {
            name: String,
        }
        self.client
            .post(
                &format!("/api/v2.0/jail/{}/clone", jail_id),
                &CloneRequest {
                    name: name.to_string(),
                },
            )
            .await
    }

    /// List jail fstab entries
    #[allow(dead_code)]
    pub async fn list_jail_fstabs(&self, jail_id: i32) -> Result<serde_json::Value> {
        self.client
            .get(&format!("/api/v2.0/jail/{}/fstab", jail_id))
            .await
    }

    // === Enclosure (Hardware) ===

    /// Get enclosure info
    #[allow(dead_code)]
    pub async fn get_enclosure(&self) -> Result<Vec<EnclosureInfo>> {
        self.client.get("/api/v2.0/enclosure").await
    }

    /// Get enclosure status
    #[allow(dead_code)]
    pub async fn get_enclosure_status(&self, enclosure_id: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(enclosure_id);
        self.client
            .get(&format!("/api/v2.0/enclosure/{}/status", encoded))
            .await
    }

    // === Support ===

    /// Get support information
    #[allow(dead_code)]
    pub async fn get_support(&self) -> Result<SupportInfo> {
        self.client.get("/api/v2.0/system/support").await
    }

    // === Alert Categories ===

    /// Alert categories
    #[allow(dead_code)]
    pub async fn get_alert_categories(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/alert/categories").await
    }

    // === Disk Management ===

    /// List all disks
    #[allow(dead_code)]
    pub async fn list_disks(&self) -> Result<Vec<Disk>> {
        self.client.get("/api/v2.0/disk").await
    }

    /// Get disk details
    #[allow(dead_code)]
    pub async fn get_disk(&self, disk_name: &str) -> Result<Disk> {
        let encoded = urlencoding::encode(disk_name);
        self.client
            .get(&format!("/api/v2.0/disk/{}", encoded))
            .await
    }

    /// Wipe a disk
    #[allow(dead_code)]
    pub async fn wipe_disk(&self, disk_name: &str, method: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct WipeRequest {
            method: String,
        }
        let encoded = urlencoding::encode(disk_name);
        self.client
            .post(
                &format!("/api/v2.0/disk/{}/wipe", encoded),
                &WipeRequest {
                    method: method.to_string(),
                },
            )
            .await
    }

    // === Pool Extended Operations ===

    /// Attach a vdev to pool
    #[allow(dead_code)]
    pub async fn pool_attach(&self, pool_name: &str, vdev: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct AttachRequest {
            vdev: String,
        }
        self.client
            .post(
                &format!("/api/v2.0/pool/{}/attach", pool_name),
                &AttachRequest {
                    vdev: vdev.to_string(),
                },
            )
            .await
    }

    /// Detach a vdev from pool
    #[allow(dead_code)]
    pub async fn pool_detach(&self, pool_name: &str, vdev: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct DetachRequest {
            vdev: String,
        }
        self.client
            .post(
                &format!("/api/v2.0/pool/{}/detach", pool_name),
                &DetachRequest {
                    vdev: vdev.to_string(),
                },
            )
            .await
    }

    /// Expand pool
    #[allow(dead_code)]
    pub async fn pool_expand(&self, pool_name: &str) -> Result<serde_json::Value> {
        self.client
            .post(&format!("/api/v2.0/pool/{}/expand", pool_name), &())
            .await
    }

    /// Upgrade pool
    #[allow(dead_code)]
    pub async fn pool_upgrade(&self, pool_name: &str) -> Result<serde_json::Value> {
        self.client
            .post(&format!("/api/v2.0/pool/{}/upgrade", pool_name), &())
            .await
    }

    // === Dataset Quota ===

    /// Get dataset quota
    #[allow(dead_code)]
    pub async fn get_dataset_quota(
        &self,
        dataset_path: &str,
        quota_type: &str,
    ) -> Result<Vec<DatasetQuota>> {
        let encoded = urlencoding::encode(dataset_path);
        self.client
            .get(&format!(
                "/api/v2.0/pool/dataset/{}/quota/{}",
                encoded, quota_type
            ))
            .await
    }

    /// Set dataset quota
    #[allow(dead_code)]
    pub async fn set_dataset_quota(
        &self,
        dataset_path: &str,
        quota_type: &str,
        quotas: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(dataset_path);
        self.client
            .post(
                &format!("/api/v2.0/pool/dataset/{}/quota/{}", encoded, quota_type),
                &quotas,
            )
            .await
    }

    // === Network Extended ===

    /// Get network global config
    #[allow(dead_code)]
    pub async fn get_network_global(&self) -> Result<NetworkGlobalConfig> {
        self.client.get("/api/v2.0/network/global").await
    }

    /// Update network global config
    #[allow(dead_code)]
    pub async fn update_network_global(
        &self,
        updates: serde_json::Value,
    ) -> Result<NetworkGlobalConfig> {
        self.client.put("/api/v2.0/network/global", &updates).await
    }

    /// Get hostname
    #[allow(dead_code)]
    pub async fn get_hostname(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/hostname").await
    }

    /// Set hostname
    #[allow(dead_code)]
    pub async fn set_hostname(&self, hostname: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct HostnameRequest {
            hostname: String,
        }
        self.client
            .put(
                "/api/v2.0/system/hostname",
                &HostnameRequest {
                    hostname: hostname.to_string(),
                },
            )
            .await
    }

    /// Create static route
    #[allow(dead_code)]
    pub async fn create_static_route(
        &self,
        destination: &str,
        gateway: &str,
        description: &str,
    ) -> Result<StaticRoute> {
        #[derive(Serialize)]
        struct RouteRequest {
            destination: String,
            gateway: String,
            description: String,
        }
        self.client
            .post(
                "/api/v2.0/network/staticroute",
                &RouteRequest {
                    destination: destination.to_string(),
                    gateway: gateway.to_string(),
                    description: description.to_string(),
                },
            )
            .await
    }

    /// Delete static route
    #[allow(dead_code)]
    pub async fn delete_static_route(&self, route_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/network/staticroute/{}", route_id))
            .await
    }

    // === System Extended ===

    /// List tunables
    #[allow(dead_code)]
    pub async fn list_tunables(&self) -> Result<Vec<Tunable>> {
        self.client.get("/api/v2.0/system/tunable").await
    }

    /// Create tunable
    #[allow(dead_code)]
    pub async fn create_tunable(
        &self,
        var: &str,
        value: &str,
        tunable_type: &str,
        comment: &str,
    ) -> Result<Tunable> {
        #[derive(Serialize)]
        struct TunableRequest {
            var: String,
            value: String,
            #[serde(rename = "type")]
            type_field: String,
            comment: String,
        }
        self.client
            .post(
                "/api/v2.0/system/tunable",
                &TunableRequest {
                    var: var.to_string(),
                    value: value.to_string(),
                    type_field: tunable_type.to_string(),
                    comment: comment.to_string(),
                },
            )
            .await
    }

    /// Delete tunable
    #[allow(dead_code)]
    pub async fn delete_tunable(&self, tunable_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/system/tunable/{}", tunable_id))
            .await
    }

    /// List NTP servers
    #[allow(dead_code)]
    pub async fn list_ntp_servers(&self) -> Result<Vec<NtpServer>> {
        self.client.get("/api/v2.0/system/ntpserver").await
    }

    /// Create NTP server
    #[allow(dead_code)]
    pub async fn create_ntp_server(
        &self,
        address: &str,
        burst: bool,
        iburst: bool,
        prefer: bool,
        minpoll: i32,
        maxpoll: i32,
    ) -> Result<NtpServer> {
        #[derive(Serialize)]
        struct NtpRequest {
            address: String,
            burst: bool,
            iburst: bool,
            prefer: bool,
            minpoll: i32,
            maxpoll: i32,
        }
        self.client
            .post(
                "/api/v2.0/system/ntpserver",
                &NtpRequest {
                    address: address.to_string(),
                    burst,
                    iburst,
                    prefer,
                    minpoll,
                    maxpoll,
                },
            )
            .await
    }

    /// Delete NTP server
    #[allow(dead_code)]
    pub async fn delete_ntp_server(&self, ntp_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/system/ntpserver/{}", ntp_id))
            .await
    }

    /// List alert filters
    #[allow(dead_code)]
    pub async fn list_alert_filters(&self) -> Result<Vec<AlertFilter>> {
        self.client.get("/api/v2.0/system/alert/filter").await
    }

    /// Create alert filter
    #[allow(dead_code)]
    pub async fn create_alert_filter(
        &self,
        name: &str,
        program: &str,
        level: &str,
        message: &str,
        enabled: bool,
    ) -> Result<AlertFilter> {
        #[derive(Serialize)]
        struct AlertFilterRequest {
            name: String,
            program: String,
            level: String,
            message: String,
            enabled: bool,
        }
        self.client
            .post(
                "/api/v2.0/system/alert/filter",
                &AlertFilterRequest {
                    name: name.to_string(),
                    program: program.to_string(),
                    level: level.to_string(),
                    message: message.to_string(),
                    enabled,
                },
            )
            .await
    }

    /// Delete alert filter
    #[allow(dead_code)]
    pub async fn delete_alert_filter(&self, filter_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/system/alert/filter/{}", filter_id))
            .await
    }

    /// List alert services
    #[allow(dead_code)]
    pub async fn list_alert_services(&self) -> Result<Vec<AlertService>> {
        self.client.get("/api/v2.0/system/alert/service").await
    }

    // === Catalog (SCALE) ===

    /// List catalogs
    #[allow(dead_code)]
    pub async fn list_catalogs(&self) -> Result<Vec<Catalog>> {
        self.client.get("/api/v2.0/catalog").await
    }

    /// Sync catalog
    #[allow(dead_code)]
    pub async fn sync_catalog(&self, catalog_id: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(catalog_id);
        self.client
            .post(&format!("/api/v2.0/catalog/{}", encoded), &())
            .await
    }

    /// Refresh all catalogs
    #[allow(dead_code)]
    pub async fn refresh_catalogs(&self) -> Result<serde_json::Value> {
        self.client.post("/api/v2.0/catalog", &()).await
    }

    /// Delete catalog
    #[allow(dead_code)]
    pub async fn delete_catalog(&self, catalog_id: &str) -> Result<()> {
        let encoded = urlencoding::encode(catalog_id);
        self.client
            .delete(&format!("/api/v2.0/catalog/{}", encoded))
            .await
    }

    // === Reporting ===

    /// Get reporting
    #[allow(dead_code)]
    pub async fn get_reporting(&self) -> Result<Reporting> {
        self.client.get("/api/v2.0/reporting").await
    }

    /// Get disk temperatures
    #[allow(dead_code)]
    pub async fn get_disk_temperatures(&self) -> Result<serde_json::Value> {
        self.client
            .get("/api/v2.0/reporting/disk/temperatures")
            .await
    }

    // === SSH ===

    /// Get SSH config
    #[allow(dead_code)]
    pub async fn get_ssh_config(&self) -> Result<SshConfig> {
        self.client.get("/api/v2.0/ssh").await
    }

    /// Update SSH config
    #[allow(dead_code)]
    pub async fn update_ssh_config(&self, updates: serde_json::Value) -> Result<SshConfig> {
        self.client.put("/api/v2.0/ssh", &updates).await
    }

    /// List SSH keys for a user
    #[allow(dead_code)]
    pub async fn list_ssh_keys(&self, user_id: i32) -> Result<Vec<SshKey>> {
        self.client
            .get(&format!("/api/v2.0/user/{}/ssh_key", user_id))
            .await
    }

    /// Add SSH key
    #[allow(dead_code)]
    pub async fn add_ssh_key(&self, user_id: i32, name: &str, key: &str) -> Result<SshKey> {
        #[derive(Serialize)]
        struct SshKeyRequest {
            name: String,
            key: String,
        }
        self.client
            .post(
                &format!("/api/v2.0/user/{}/ssh_key", user_id),
                &SshKeyRequest {
                    name: name.to_string(),
                    key: key.to_string(),
                },
            )
            .await
    }

    /// Delete SSH key
    #[allow(dead_code)]
    pub async fn delete_ssh_key(&self, user_id: i32, key_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/user/{}/ssh_key/{}", user_id, key_id))
            .await
    }

    // === rsync ===

    /// List rsync tasks
    #[allow(dead_code)]
    pub async fn list_rsync_tasks(&self) -> Result<Vec<RsyncTask>> {
        self.client.get("/api/v2.0/rsync/tasks").await
    }

    /// Get rsync task
    #[allow(dead_code)]
    pub async fn get_rsync_task(&self, task_id: i32) -> Result<RsyncTask> {
        self.client
            .get(&format!("/api/v2.0/rsync/tasks/{}", task_id))
            .await
    }

    /// Create rsync task
    #[allow(dead_code)]
    pub async fn create_rsync_task(&self, task: serde_json::Value) -> Result<RsyncTask> {
        self.client.post("/api/v2.0/rsync/tasks", &task).await
    }

    /// Delete rsync task
    #[allow(dead_code)]
    pub async fn delete_rsync_task(&self, task_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/rsync/tasks/{}", task_id))
            .await
    }

    /// Run rsync task
    #[allow(dead_code)]
    pub async fn run_rsync_task(&self, task_id: i32) -> Result<serde_json::Value> {
        self.client
            .post(&format!("/api/v2.0/rsync/tasks/{}/run", task_id), &())
            .await
    }

    /// List rsync modules
    #[allow(dead_code)]
    pub async fn list_rsync_modules(&self) -> Result<Vec<RsyncModule>> {
        self.client.get("/api/v2.0/rsync/modules").await
    }

    /// Get rsync module
    #[allow(dead_code)]
    pub async fn get_rsync_module(&self, module_id: i32) -> Result<RsyncModule> {
        self.client
            .get(&format!("/api/v2.0/rsync/modules/{}", module_id))
            .await
    }

    /// Create rsync module
    #[allow(dead_code)]
    pub async fn create_rsync_module(&self, module: serde_json::Value) -> Result<RsyncModule> {
        self.client.post("/api/v2.0/rsync/modules", &module).await
    }

    /// Delete rsync module
    #[allow(dead_code)]
    pub async fn delete_rsync_module(&self, module_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/rsync/modules/{}", module_id))
            .await
    }

    // === SMART ===

    /// List SMART tests
    #[allow(dead_code)]
    pub async fn list_smart_tests(&self) -> Result<Vec<SmartTest>> {
        self.client.get("/api/v2.0/smart/test").await
    }

    /// Get SMART test
    #[allow(dead_code)]
    pub async fn get_smart_test(&self, test_id: i32) -> Result<SmartTest> {
        self.client
            .get(&format!("/api/v2.0/smart/test/{}", test_id))
            .await
    }

    /// Create SMART test
    #[allow(dead_code)]
    pub async fn create_smart_test(&self, test: serde_json::Value) -> Result<SmartTest> {
        self.client.post("/api/v2.0/smart/test", &test).await
    }

    /// Delete SMART test
    #[allow(dead_code)]
    pub async fn delete_smart_test(&self, test_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/smart/test/{}", test_id))
            .await
    }

    /// Get SMART config
    #[allow(dead_code)]
    pub async fn get_smart_config(&self) -> Result<SmartConfig> {
        self.client.get("/api/v2.0/smart/config").await
    }

    /// Get SMART status for a disk
    #[allow(dead_code)]
    pub async fn get_smart_status(&self, disk_name: &str) -> Result<serde_json::Value> {
        let encoded = urlencoding::encode(disk_name);
        self.client
            .get(&format!("/api/v2.0/disk/{}/smart", encoded))
            .await
    }

    /// Update SMART config
    #[allow(dead_code)]
    pub async fn update_smart_config(&self, config: serde_json::Value) -> Result<SmartConfig> {
        self.client.put("/api/v2.0/smart/config", &config).await
    }

    // === FTP ===

    /// Get FTP config
    #[allow(dead_code)]
    pub async fn get_ftp_config(&self) -> Result<FtpConfig> {
        self.client.get("/api/v2.0/ftp").await
    }

    /// Update FTP config
    #[allow(dead_code)]
    pub async fn update_ftp_config(&self, config: serde_json::Value) -> Result<FtpConfig> {
        self.client.put("/api/v2.0/ftp", &config).await
    }

    // === SNMP ===

    /// Get SNMP config
    #[allow(dead_code)]
    pub async fn get_snmp_config(&self) -> Result<SnmpConfig> {
        self.client.get("/api/v2.0/snmp").await
    }

    /// Update SNMP config
    #[allow(dead_code)]
    pub async fn update_snmp_config(&self, config: serde_json::Value) -> Result<SnmpConfig> {
        self.client.put("/api/v2.0/snmp", &config).await
    }

    // === Active Directory ===

    /// Get AD config
    #[allow(dead_code)]
    pub async fn get_ad_config(&self) -> Result<AdConfig> {
        self.client
            .get("/api/v2.0/directoryservice/activedirectory")
            .await
    }

    /// Update AD config
    #[allow(dead_code)]
    pub async fn update_ad_config(&self, config: serde_json::Value) -> Result<AdConfig> {
        self.client
            .put("/api/v2.0/directoryservice/activedirectory", &config)
            .await
    }

    /// Join AD
    #[allow(dead_code)]
    pub async fn join_ad(
        &self,
        domain: &str,
        username: &str,
        password: &str,
    ) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct JoinRequest {
            domain: String,
            username: String,
            password: String,
        }
        self.client
            .post(
                "/api/v2.0/directoryservice/activedirectory/join",
                &JoinRequest {
                    domain: domain.to_string(),
                    username: username.to_string(),
                    password: password.to_string(),
                },
            )
            .await
    }

    /// Leave AD
    #[allow(dead_code)]
    pub async fn leave_ad(&self, username: &str, password: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct LeaveRequest {
            username: String,
            password: String,
        }
        self.client
            .post(
                "/api/v2.0/directoryservice/activedirectory/leave",
                &LeaveRequest {
                    username: username.to_string(),
                    password: password.to_string(),
                },
            )
            .await
    }

    // === LDAP ===

    /// Get LDAP config
    #[allow(dead_code)]
    pub async fn get_ldap_config(&self) -> Result<LdapConfig> {
        self.client.get("/api/v2.0/directoryservice/ldap").await
    }

    /// Update LDAP config
    #[allow(dead_code)]
    pub async fn update_ldap_config(&self, config: serde_json::Value) -> Result<LdapConfig> {
        self.client
            .put("/api/v2.0/directoryservice/ldap", &config)
            .await
    }

    /// Test LDAP
    #[allow(dead_code)]
    pub async fn test_ldap(&self) -> Result<serde_json::Value> {
        self.client
            .get("/api/v2.0/directoryservice/ldap/test")
            .await
    }

    // === Interface IPs ===

    /// Create interface IP
    #[allow(dead_code)]
    pub async fn create_interface_ip(
        &self,
        interface: &str,
        ipaddr: &str,
        netmask: u32,
    ) -> Result<InterfaceIp> {
        #[derive(Serialize)]
        struct IpRequest {
            interface: String,
            ipaddr: String,
            netmask: u32,
        }
        self.client
            .post(
                "/api/v2.0/network/interface/ip",
                &IpRequest {
                    interface: interface.to_string(),
                    ipaddr: ipaddr.to_string(),
                    netmask,
                },
            )
            .await
    }

    /// Delete interface IP
    #[allow(dead_code)]
    pub async fn delete_interface_ip(&self, ip_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/network/interface/ip/{}", ip_id))
            .await
    }

    // === Tasks ===

    /// List all tasks
    #[allow(dead_code)]
    pub async fn list_tasks(&self) -> Result<Vec<Task>> {
        self.client.get("/api/v2.0/core/get_tasks").await
    }

    /// Get task status
    #[allow(dead_code)]
    pub async fn get_task_status(&self, task_id: i32) -> Result<Task> {
        self.client
            .get(&format!("/api/v2.0/core/get_tasks/{}", task_id))
            .await
    }

    /// Abort a task
    #[allow(dead_code)]
    pub async fn abort_task(&self, task_id: i32) -> Result<()> {
        self.client
            .post(&format!("/api/v2.0/core/abort_task/{}", task_id), &())
            .await
    }

    // === Kubernetes ===

    /// Get Kubernetes nodes
    #[allow(dead_code)]
    pub async fn get_kubernetes_nodes(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/kubernetes/node").await
    }

    /// Get Kubernetes pods
    #[allow(dead_code)]
    pub async fn get_kubernetes_pods(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/kubernetes/pod").await
    }

    /// Get Kubernetes services
    #[allow(dead_code)]
    pub async fn get_kubernetes_services(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/kubernetes/service").await
    }

    // === Docker ===

    /// List Docker images
    #[allow(dead_code)]
    pub async fn list_docker_images(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/docker/images").await
    }

    /// Pull Docker image
    #[allow(dead_code)]
    pub async fn pull_docker_image(&self, image: &str, tag: &str) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct PullRequest {
            from_image: String,
            tag: String,
        }
        self.client
            .post(
                "/api/v2.0/docker/images/pull",
                &PullRequest {
                    from_image: image.to_string(),
                    tag: tag.to_string(),
                },
            )
            .await
    }

    // === Cloud Credentials ===

    /// Create cloud credential
    #[allow(dead_code)]
    pub async fn create_cloud_credential(
        &self,
        name: &str,
        provider: &str,
        attributes: serde_json::Value,
    ) -> Result<CloudCredential> {
        #[derive(Serialize)]
        struct CreateRequest {
            name: String,
            provider: String,
            attributes: serde_json::Value,
        }
        self.client
            .post(
                "/api/v2.0/cloudsync/credentials",
                &CreateRequest {
                    name: name.to_string(),
                    provider: provider.to_string(),
                    attributes,
                },
            )
            .await
    }

    /// Delete cloud credential
    #[allow(dead_code)]
    pub async fn delete_cloud_credential(&self, cred_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/cloudsync/credentials/{}", cred_id))
            .await
    }

    // === SSH Connections ===

    /// List SSH connections
    #[allow(dead_code)]
    pub async fn list_ssh_connections(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/ssh/connection").await
    }

    /// Create SSH connection
    #[allow(dead_code)]
    pub async fn create_ssh_connection(
        &self,
        connection: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.client
            .post("/api/v2.0/ssh/connection", &connection)
            .await
    }

    /// Delete SSH connection
    #[allow(dead_code)]
    pub async fn delete_ssh_connection(&self, connection_id: i32) -> Result<()> {
        self.client
            .delete(&format!("/api/v2.0/ssh/connection/{}", connection_id))
            .await
    }

    // === TFTP ===

    /// List TFTP services
    #[allow(dead_code)]
    pub async fn list_tftp_services(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/tftp").await
    }

    // === Alerts & Events ===

    /// Get alert classes/categories
    #[allow(dead_code)]
    pub async fn get_alert_classes(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/alert/classes").await
    }

    /// Dismiss an alert
    #[allow(dead_code)]
    pub async fn dismiss_alert(&self, alert_id: &str) -> Result<()> {
        let encoded = urlencoding::encode(alert_id);
        self.client
            .delete(&format!("/api/v2.0/system/alert/{}", encoded))
            .await
    }

    /// Clear all alerts
    #[allow(dead_code)]
    pub async fn clear_all_alerts(&self) -> Result<serde_json::Value> {
        self.client.delete("/api/v2.0/system/alert").await
    }

    /// Get alert destinations (email, SNMP, etc.)
    #[allow(dead_code)]
    pub async fn get_alert_destinations(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/system/alert/destination").await
    }

    /// Create alert destination
    #[allow(dead_code)]
    pub async fn create_alert_destination(
        &self,
        destination: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.client
            .post("/api/v2.0/system/alert/destination", &destination)
            .await
    }

    // === System Events (for AI notifications) ===

    /// Get recent system events/logs
    #[allow(dead_code)]
    pub async fn get_system_events(&self, limit: Option<u32>) -> Result<serde_json::Value> {
        let limit = limit.unwrap_or(50);
        self.client
            .get(&format!("/api/v2.0/system/log/{}?limit={}", limit, limit))
            .await
    }

    /// Get disk health/smart status
    #[allow(dead_code)]
    pub async fn get_disk_health(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/disk").await
    }

    /// Get pool expansion status
    #[allow(dead_code)]
    pub async fn get_pool_expansion_status(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/pool/expansion").await
    }

    /// Get async task queue status
    #[allow(dead_code)]
    pub async fn get_task_queue_status(&self) -> Result<serde_json::Value> {
        self.client.get("/api/v2.0/core/get_jobs").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === Path Validation Tests ===

    #[test]
    fn test_validate_path_rejects_path_traversal() {
        let result = validate_path("../etc/passwd", "path");
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("path traversal"));
        }
    }

    #[test]
    fn test_validate_path_rejects_absolute_path() {
        let result = validate_path("/absolute/path", "path");
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("relative path"));
        }
    }

    #[test]
    fn test_validate_path_rejects_null_bytes() {
        let result = validate_path("path\0with\null", "path");
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("null bytes"));
        }
    }

    #[test]
    fn test_validate_path_accepts_relative_path() {
        let result = validate_path("tank/data", "path");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_accepts_simple_name() {
        let result = validate_path("mydataset", "path");
        assert!(result.is_ok());
    }

    // === FilterCondition Tests ===

    #[test]
    fn test_filter_condition_eq() {
        let cond = FilterCondition {
            field: "status".to_string(),
            operator: "eq".to_string(),
            value: json!("RUNNING"),
        };
        let item = json!({"status": "RUNNING"});
        assert!(cond.matches(&item));

        let item2 = json!({"status": "STOPPED"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_ne() {
        let cond = FilterCondition {
            field: "status".to_string(),
            operator: "ne".to_string(),
            value: json!("RUNNING"),
        };
        let item = json!({"status": "STOPPED"});
        assert!(cond.matches(&item));

        let item2 = json!({"status": "RUNNING"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_gt() {
        let cond = FilterCondition {
            field: "count".to_string(),
            operator: "gt".to_string(),
            value: json!(10),
        };
        let item = json!({"count": 15});
        assert!(cond.matches(&item));

        let item2 = json!({"count": 5});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_contains() {
        let cond = FilterCondition {
            field: "name".to_string(),
            operator: "contains".to_string(),
            value: json!("test"),
        };
        let item = json!({"name": "mytestfile"});
        assert!(cond.matches(&item));

        let item2 = json!({"name": "otherfile"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_startswith() {
        let cond = FilterCondition {
            field: "name".to_string(),
            operator: "startswith".to_string(),
            value: json!("test"),
        };
        let item = json!({"name": "test123"});
        assert!(cond.matches(&item));

        let item2 = json!({"name": "123test"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_endswith() {
        let cond = FilterCondition {
            field: "name".to_string(),
            operator: "endswith".to_string(),
            value: json!(".log"),
        };
        let item = json!({"name": "app.log"});
        assert!(cond.matches(&item));

        let item2 = json!({"name": "log.txt"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_in() {
        let cond = FilterCondition {
            field: "status".to_string(),
            operator: "in".to_string(),
            value: json!(["RUNNING", "STARTING"]),
        };
        let item = json!({"status": "RUNNING"});
        assert!(cond.matches(&item));

        let item2 = json!({"status": "STOPPED"});
        assert!(!cond.matches(&item2));
    }

    #[test]
    fn test_filter_condition_missing_field() {
        let cond = FilterCondition {
            field: "missing".to_string(),
            operator: "eq".to_string(),
            value: json!("value"),
        };
        let item = json!({"other": "value"});
        assert!(!cond.matches(&item));
    }

    // === FilterParams Tests ===

    #[test]
    fn test_filter_params_and_conditions() {
        let params = FilterParams {
            and_conditions: vec![
                FilterCondition {
                    field: "status".to_string(),
                    operator: "eq".to_string(),
                    value: json!("RUNNING"),
                },
                FilterCondition {
                    field: "name".to_string(),
                    operator: "contains".to_string(),
                    value: json!("test"),
                },
            ],
            or_conditions: vec![],
        };

        let item = json!({"status": "RUNNING", "name": "testfile"});
        assert!(params.matches(&item));

        let item2 = json!({"status": "STOPPED", "name": "testfile"});
        assert!(!params.matches(&item2));
    }

    #[test]
    fn test_filter_params_or_conditions() {
        let params = FilterParams {
            and_conditions: vec![],
            or_conditions: vec![
                FilterCondition {
                    field: "status".to_string(),
                    operator: "eq".to_string(),
                    value: json!("RUNNING"),
                },
                FilterCondition {
                    field: "status".to_string(),
                    operator: "eq".to_string(),
                    value: json!("STARTING"),
                },
            ],
        };

        let item = json!({"status": "RUNNING"});
        assert!(params.matches(&item));

        let item2 = json!({"status": "STARTING"});
        assert!(params.matches(&item2));

        let item3 = json!({"status": "STOPPED"});
        assert!(!params.matches(&item3));
    }

    #[test]
    fn test_filter_params_empty() {
        let params = FilterParams::default();
        let item = json!({"any": "value"});
        assert!(params.matches(&item));
    }

    // === PaginationParams Tests ===

    #[test]
    fn test_pagination_apply_pagination_no_limit() {
        let params = PaginationParams {
            offset: Some(0),
            limit: Some(0),
            order_by: None,
        };

        let items: Vec<i32> = (1..=100).collect();
        let (result, offset, limit) = params.apply_pagination(&items);
        assert_eq!(result.len(), 100);
        assert_eq!(offset, 0);
        assert_eq!(limit, 0);
    }

    #[test]
    fn test_pagination_apply_pagination_with_limit() {
        let params = PaginationParams {
            offset: Some(10),
            limit: Some(5),
            order_by: None,
        };

        let items: Vec<i32> = (1..=100).collect();
        let (result, offset, limit) = params.apply_pagination(&items);
        assert_eq!(result.len(), 5);
        assert_eq!(result, vec![11, 12, 13, 14, 15]);
        assert_eq!(offset, 10);
        assert_eq!(limit, 5);
    }

    #[test]
    fn test_pagination_offset_beyond_length() {
        let params = PaginationParams {
            offset: Some(50),
            limit: Some(10),
            order_by: None,
        };

        let items: Vec<i32> = (1..=30).collect();
        let (result, offset, limit) = params.apply_pagination(&items);
        assert!(result.is_empty());
        assert_eq!(offset, 50);
        assert_eq!(limit, 10);
    }

    #[test]
    fn test_pagination_order_by_ascending() {
        let params = PaginationParams {
            offset: None,
            limit: None,
            order_by: Some("name".to_string()),
        };

        let mut items = vec![
            json!({"name": "c", "value": 3}),
            json!({"name": "a", "value": 1}),
            json!({"name": "b", "value": 2}),
        ];

        params.apply_ordering(&mut items);
        assert_eq!(items[0]["name"], "a");
        assert_eq!(items[1]["name"], "b");
        assert_eq!(items[2]["name"], "c");
    }

    #[test]
    fn test_pagination_order_by_descending() {
        let params = PaginationParams {
            offset: None,
            limit: None,
            order_by: Some("-name".to_string()),
        };

        let mut items = vec![
            json!({"name": "a", "value": 1}),
            json!({"name": "c", "value": 3}),
            json!({"name": "b", "value": 2}),
        ];

        params.apply_ordering(&mut items);
        assert_eq!(items[0]["name"], "c");
        assert_eq!(items[1]["name"], "b");
        assert_eq!(items[2]["name"], "a");
    }

    #[test]
    fn test_paginated_response_new() {
        let items = vec!["a", "b", "c"];
        let response = PaginatedResponse::new(items.clone(), 10, 0, 3);
        assert_eq!(response.items, items);
        assert_eq!(response.total, 10);
        assert_eq!(response.offset, 0);
        assert_eq!(response.limit, 3);
        assert!(response.has_more);
    }

    #[test]
    fn test_paginated_response_no_more() {
        let items = vec!["a", "b", "c"];
        let response = PaginatedResponse::new(items.clone(), 3, 0, 3);
        assert!(!response.has_more);
    }
}
