#![recursion_limit = "512"]

// Use library modules
use truenas_master_mcp::tools;

// Re-export ToolCategory and ToolConfig from library
pub use truenas_master_mcp::server::ToolCategory;
pub use truenas_master_mcp::server::ToolConfig;

use anyhow::Context;
use axum::http;
use clap::Parser;
use serde_json::{Value, json};
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::info;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use truenas_master_mcp::config::TrueNasConfig;
use truenas_master_mcp::tools::TrueNasTools;

/// TrueNAS MCP Server
#[derive(Parser, Debug)]
#[command(name = "truenas-master-mcp")]
#[command(about = "Official MCP server for TrueNAS API access", long_about = None)]
struct Args {
    /// Transport type to use: stdio, http, or sse
    #[arg(short, long, default_value = "stdio")]
    transport: String,

    /// Host to bind to (for http/sse transports)
    #[arg(short, long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind to (for http/sse transports)
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// TrueNAS version: scale or core (default: scale)
    #[arg(long, default_value = "scale")]
    truenas_version: String,

    /// Enable readonly mode (disables all modification tools)
    #[arg(long)]
    readonly: bool,

    /// Enable specific tool categories (comma-separated: users,pools,datasets,shares,snapshots,iscsi,apps,system)
    #[arg(long)]
    enable_category: Vec<String>,

    /// Disable specific tool categories (comma-separated)
    #[arg(long)]
    disable_category: Vec<String>,

    /// Path to configuration file (JSON or YAML)
    #[arg(short, long)]
    config_file: Option<String>,

    /// Log level: debug, info, warn, error
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable verbose output (equivalent to --log-level debug)
    #[arg(short, long)]
    verbose: bool,

    /// Disable SSL certificate verification
    #[arg(long)]
    insecure_ssl: bool,

    /// API request timeout in seconds
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Enable JSON pretty printing for responses
    #[arg(long)]
    pretty: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Determine log level from args
    let log_level = if args.verbose {
        "debug".to_string()
    } else {
        args.log_level.clone()
    };
    let log_level = log_level
        .parse::<tracing::Level>()
        .unwrap_or(tracing::Level::INFO);

    // Initialize tracing
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(log_level.into()))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_ansi(false))
        .init();

    info!("TrueNAS MCP Server v{}", env!("CARGO_PKG_VERSION"));

    // Build tool config from CLI args and environment
    let env_config = ToolConfig::from_env();
    let cli_enabled: Vec<ToolCategory> = args
        .enable_category
        .iter()
        .filter_map(|c| ToolCategory::from_str(c).ok())
        .collect();
    let cli_disabled: Vec<ToolCategory> = args
        .disable_category
        .iter()
        .filter_map(|c| ToolCategory::from_str(c).ok())
        .collect();

    let tool_config = ToolConfig {
        readonly: args.readonly || env_config.readonly,
        enabled_categories: if cli_enabled.is_empty() {
            env_config.enabled_categories
        } else {
            cli_enabled
        },
        disabled_categories: if cli_disabled.is_empty() {
            env_config.disabled_categories
        } else {
            cli_disabled
        },
    };

    // Parse TrueNAS version
    let truenas_version: truenas_master_mcp::config::TrueNasVersion = args
        .truenas_version
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid TrueNAS version: {}. Use 'scale' or 'core'", e))?;

    info!(
        "Starting TrueNAS MCP Server with {} transport",
        args.transport
    );
    info!("TrueNAS version: {:?}", truenas_version);
    if tool_config.readonly {
        info!("Readonly mode ENABLED - modification tools are disabled");
    }
    info!("Enabled categories: {:?}", tool_config.enabled_categories);
    if !tool_config.disabled_categories.is_empty() {
        info!("Disabled categories: {:?}", tool_config.disabled_categories);
    }

    // Load configuration from file or environment
    let mut config = match &args.config_file {
        Some(path) => {
            let path = std::path::Path::new(path);
            TrueNasConfig::from_file(path)
                .with_context(|| format!("Failed to load configuration from {}", path.display()))?
        }
        None => {
            TrueNasConfig::from_env().context("Failed to load configuration from environment")?
        }
    };
    config.version = truenas_version;

    // Log configuration source
    if let Some(path) = &args.config_file {
        info!("Loaded configuration from file: {}", path);
    } else {
        info!("Loaded configuration from environment variables");
    }

    // Apply CLI overrides
    if args.insecure_ssl {
        config.verify_ssl = false;
        info!("SSL verification DISABLED");
    }
    config.timeout_secs = args.timeout;
    info!("API timeout: {} seconds", config.timeout_secs);

    info!("Connecting to TrueNAS at: {}", config.server_url);

    // Create the TrueNAS server handler with tool config
    let server = Arc::new(TrueNasServerImpl::new(config, tool_config)?);

    match args.transport.as_str() {
        "stdio" => {
            run_stdio(server).await?;
        }
        "http" => {
            run_http(server, &args.host, args.port).await?;
        }
        "sse" => {
            run_sse(server, &args.host, args.port).await?;
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown transport type: {}. Use: stdio, http, or sse",
                args.transport
            ));
        }
    }

    Ok(())
}

/// TrueNAS MCP Server implementation
#[derive(Clone)]
struct TrueNasServerImpl {
    tools: Arc<TrueNasTools>,
    tool_config: ToolConfig,
}

impl TrueNasServerImpl {
    fn new(config: TrueNasConfig, tool_config: ToolConfig) -> anyhow::Result<Self> {
        let client = truenas_master_mcp::client::TrueNasClient::new(config)?;
        let tools = Arc::new(TrueNasTools::new(client));
        Ok(Self { tools, tool_config })
    }

    fn get_server_info(&self) -> Value {
        json!({
            "name": "truenas-master-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Official MCP server for TrueNAS API access",
            "readonly": self.tool_config.readonly,
            "enabled_categories": format!("{:?}", self.tool_config.enabled_categories),
            "instructions": "This server provides access to TrueNAS SCALE/CORE management features including:\n- User management\n- Pool and dataset management\n- SMB and NFS share management\n- Snapshot management\n- iSCSI target management\n- Apps/Jails management\n- System information\n\nUse --readonly flag or TRUENAS_READONLY=true to disable modification tools."
        })
    }

    /// Get tool category and whether it's a modification tool
    fn get_tool_info(name: &str) -> (ToolCategory, bool) {
        match name {
            // Users - read operations
            "list_users" | "get_user" | "get_user_by_username" => (ToolCategory::Users, false),
            // Users - modification operations
            "create_user" | "update_user" | "delete_user" => (ToolCategory::Users, true),
            // Groups - read operations
            "list_groups" | "get_group" | "get_group_by_name" => (ToolCategory::Users, false),
            // Groups - modification operations
            "create_group" | "delete_group" => (ToolCategory::Users, true),
            // Pools
            "list_pools" | "get_pool_status" => (ToolCategory::Pools, false),
            "scrub_pool" => (ToolCategory::Pools, true),
            // Datasets
            "list_datasets" | "get_dataset" => (ToolCategory::Datasets, false),
            "create_dataset" | "delete_dataset" | "update_dataset" => {
                (ToolCategory::Datasets, true)
            }
            // Shares
            "list_smb_shares" | "list_nfs_exports" | "get_smb_share" | "get_nfs_export" => {
                (ToolCategory::Shares, false)
            }
            "create_smb_share" | "delete_smb_share" | "create_nfs_export" | "delete_nfs_export" => {
                (ToolCategory::Shares, true)
            }
            // Snapshots
            "list_snapshots" | "get_snapshot" => (ToolCategory::Snapshots, false),
            "create_snapshot" | "delete_snapshot" | "rollback_snapshot" | "clone_snapshot" => {
                (ToolCategory::Snapshots, true)
            }
            // iSCSI
            "list_iscsi_targets" => (ToolCategory::Iscsi, false),
            "create_iscsi_target" | "delete_iscsi_target" => (ToolCategory::Iscsi, true),
            // Apps (SCALE)
            "list_apps"
            | "get_app"
            | "get_app_config"
            | "get_app_upgrade_options"
            | "list_catalog_items"
            | "get_catalog"
            | "get_catalog_trains"
            | "get_catalog_item"
            | "list_chart_releases"
            | "get_chart_release"
            | "get_chart_release_resources" => (ToolCategory::Apps, false),
            "create_app" | "update_app" | "delete_app" | "start_app" | "stop_app"
            | "restart_app" | "rollback_app" | "upgrade_app" | "scale_app" => {
                (ToolCategory::Apps, true)
            }
            // VMs
            "list_vms" | "get_vm" => (ToolCategory::Apps, false),
            "start_vm" | "stop_vm" | "restart_vm" | "powercycle_vm" | "create_vm" | "update_vm"
            | "delete_vm" | "clone_vm" => (ToolCategory::Apps, true),
            // Network
            "list_interfaces" | "list_routes" | "get_dns" => (ToolCategory::Network, false),
            // Services
            "list_services" | "get_service" => (ToolCategory::Network, false),
            "start_service" | "stop_service" | "restart_service" => (ToolCategory::Network, true),
            // System
            "get_system_info" | "get_alerts" | "check_for_updates" | "get_enclosure"
            | "get_support" => (ToolCategory::System, false),
            "reboot_system" | "shutdown_system" => (ToolCategory::System, true),
            // Disks
            "list_disks" | "get_disk" => (ToolCategory::Pools, false),
            // Certificates
            "list_certificates" | "get_certificate" => (ToolCategory::System, false),
            // Replication
            "list_replication_tasks" => (ToolCategory::System, false),
            "run_replication_task" => (ToolCategory::System, true),
            // Cloud Sync
            "list_cloudsync_tasks" => (ToolCategory::System, false),
            "run_cloudsync_task" => (ToolCategory::System, true),
            // Jails (CORE)
            "list_jails" | "get_jail" | "get_jail_by_name" | "list_jail_fstabs" => {
                (ToolCategory::Apps, false)
            }
            "create_jail" | "update_jail" | "delete_jail" | "start_jail" | "stop_jail"
            | "restart_jail" | "clone_jail" => (ToolCategory::Apps, true),
            // Default
            _ => (ToolCategory::All, false),
        }
    }

    /// Check if a tool can be executed
    fn check_tool_access(&self, name: &str) -> Result<(), String> {
        let (category, is_modification) = Self::get_tool_info(name);
        self.tool_config.can_execute(&category, is_modification)
    }

    /// List available resources
    fn list_resources(&self) -> Value {
        let mut resources = vec![];

        // System info resource
        resources.push(json!({
            "uri": "truenas://system/info",
            "name": "System Information",
            "description": "Get system information from TrueNAS including version, model, serial number, and uptime",
            "mimeType": "application/json"
        }));

        // Pool resources
        resources.push(json!({
            "uri": "truenas://pools/list",
            "name": "Storage Pools",
            "description": "List all storage pools on the TrueNAS system",
            "mimeType": "application/json"
        }));

        // Datasets resource
        resources.push(json!({
            "uri": "truenas://datasets/tree",
            "name": "Dataset Tree",
            "description": "Get the complete dataset hierarchy on the TrueNAS system",
            "mimeType": "application/json"
        }));

        // Apps resource (SCALE)
        resources.push(json!({
            "uri": "truenas://apps/list",
            "name": "Applications",
            "description": "List all applications deployed on TrueNAS SCALE",
            "mimeType": "application/json"
        }));

        // Disks resource
        resources.push(json!({
            "uri": "truenas://disks/list",
            "name": "Physical Disks",
            "description": "List all physical disks installed in the TrueNAS system",
            "mimeType": "application/json"
        }));

        // Network interfaces resource
        resources.push(json!({
            "uri": "truenas://network/interfaces",
            "name": "Network Interfaces",
            "description": "List all network interfaces on the TrueNAS system",
            "mimeType": "application/json"
        }));

        json!({ "resources": resources })
    }

    /// List available prompts
    fn list_prompts(&self) -> Value {
        let prompts = vec![
            // Core system prompts
            json!({
                "name": "system-overview",
                "description": "Generate a system overview summary including pools, datasets, and app status"
            }),
            json!({
                "name": "storage-report",
                "description": "Generate a storage usage report with pool capacity and dataset sizes"
            }),
            json!({
                "name": "user-activity-summary",
                "description": "Summarize recent user activity and authentication events",
                "arguments": [
                    {"name": "days", "description": "Number of days to look back", "required": false}
                ]
            }),
            json!({
                "name": "backup-checklist",
                "description": "Create a checklist for verifying backup integrity across datasets and snapshots"
            }),
            json!({
                "name": "app-maintenance-guide",
                "description": "Provide maintenance guidance for a specific application",
                "arguments": [
                    {"name": "app_name", "description": "The name of the application", "required": true}
                ]
            }),
            // Health & troubleshooting prompts
            json!({
                "name": "health-check",
                "description": "Perform a comprehensive health check with pool status, disk health, alerts, and service status"
            }),
            json!({
                "name": "troubleshoot-issue",
                "description": "Diagnose and troubleshoot a reported issue",
                "arguments": [
                    {"name": "issue_description", "description": "Description of the problem", "required": true}
                ]
            }),
            json!({
                "name": "security-audit",
                "description": "Generate a security audit report including users, services, certificates, and potential vulnerabilities"
            }),
            // Performance prompts
            json!({
                "name": "performance-analysis",
                "description": "Analyze system performance including pool I/O, VM resource usage, and app performance"
            }),
            json!({
                "name": "capacity-planning",
                "description": "Generate capacity planning recommendations based on current usage trends and growth projections"
            }),
            // Disaster recovery prompts
            json!({
                "name": "disaster-recovery-plan",
                "description": "Create a disaster recovery plan including snapshots, replication status, and restore procedures"
            }),
            json!({
                "name": "incident-response",
                "description": "Generate incident response steps for common scenarios (data loss, pool failure, service outage)",
                "arguments": [
                    {"name": "incident_type", "description": "Type of incident: pool_failure, data_corruption, service_outage, security_breach", "required": true}
                ]
            }),
        ];

        json!({ "prompts": prompts })
    }

    /// Get a specific prompt by name
    async fn get_prompt(&self, name: &str, arguments: Option<&Value>) -> Result<Value, String> {
        match name {
            "system-overview" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let datasets = self
                    .tools
                    .list_datasets()
                    .await
                    .map_err(|e| e.to_string())?;
                let apps = self.tools.list_apps().await.map_err(|e| e.to_string())?;
                let system_info = self
                    .tools
                    .get_system_info()
                    .await
                    .map_err(|e| e.to_string())?;

                let uptime_secs = system_info.uptime_seconds.unwrap_or(0);
                let uptime_days = uptime_secs / 86400;
                let uptime_hours = (uptime_secs % 86400) / 3600;
                let uptime_str = format!("{}d {}h", uptime_days, uptime_hours);

                let description = format!(
                    "# TrueNAS System Overview\n\n**Version:** {}\n**Hostname:** {}\n**CPU:** {}\n**Uptime:** {}\n\n## Storage Pools\n{}\n\n## Datasets\nTotal datasets: {}\n\n## Applications\nRunning apps: {}",
                    system_info.version,
                    system_info.hostname,
                    system_info
                        .cpu_model
                        .unwrap_or_else(|| "Unknown".to_string()),
                    uptime_str,
                    serde_json::to_string_pretty(&pools)
                        .unwrap_or_else(|_| "Error serializing pools".to_string()),
                    datasets.len(),
                    apps.len()
                );

                Ok(json!({
                    "description": "Generate a system overview summary",
                    "messages": [{"role": "user", "content": {"type": "text", "text": description}}]
                }))
            }
            "storage-report" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let datasets = self
                    .tools
                    .list_datasets()
                    .await
                    .map_err(|e| e.to_string())?;

                let pool_report: Vec<Value> = pools
                    .iter()
                    .map(|pool| {
                        json!({
                            "name": pool.name,
                            "status": pool.status,
                            "size": pool.size,
                            "free": pool.free,
                            "allocated": pool.size.saturating_sub(pool.free)
                        })
                    })
                    .collect();

                let total_size: u64 = pools.iter().map(|p| p.size).sum();
                let total_free: u64 = pools.iter().map(|p| p.free).sum();

                let description = format!(
                    "# TrueNAS Storage Report\n\n**Total Pool Size:** {} GB\n**Total Free Space:** {} GB\n**Total Used:** {} GB\n\n## Pool Details\n{}\n\n## Dataset Summary\nTotal datasets: {}",
                    total_size / 1_073_741_824,
                    total_free / 1_073_741_824,
                    (total_size.saturating_sub(total_free)) / 1_073_741_824,
                    serde_json::to_string_pretty(&pool_report)
                        .unwrap_or_else(|_| "Error".to_string()),
                    datasets.len()
                );

                Ok(json!({
                    "description": "Generate a storage usage report",
                    "messages": [{"role": "user", "content": {"type": "text", "text": description}}]
                }))
            }
            "backup-checklist" => {
                let snapshots = self
                    .tools
                    .list_snapshots()
                    .await
                    .map_err(|e| e.to_string())?;
                let datasets = self
                    .tools
                    .list_datasets()
                    .await
                    .map_err(|e| e.to_string())?;

                let recent_snapshots: Vec<Value> = snapshots
                    .iter()
                    .filter(|s| {
                        // creation is i64 timestamp
                        s.creation > 0
                    })
                    .take(10)
                    .map(|s| {
                        json!({
                            "dataset": s.dataset,
                            "name": s.name,
                            "creation_timestamp": s.creation
                        })
                    })
                    .collect();

                let checklist = format!(
                    "# Backup Integrity Checklist\n\n## Datasets to Backup\n{}\n\n## Recent Snapshots (Last 10)\n{}\n\n## Verification Steps\n- [ ] Check pool health status\n- [ ] Verify recent snapshots exist\n- [ ] Test restore on non-production dataset\n- [ ] Confirm replication tasks completed successfully\n- [ ] Verify cloud sync uploads completed",
                    datasets
                        .iter()
                        .take(20)
                        .map(|d| format!("- {}", d.name))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    serde_json::to_string_pretty(&recent_snapshots)
                        .unwrap_or_else(|_| "No recent snapshots".to_string())
                );

                Ok(json!({
                    "description": "Create a checklist for verifying backup integrity",
                    "messages": [{"role": "user", "content": {"type": "text", "text": checklist}}]
                }))
            }
            "user-activity-summary" => {
                let users = self.tools.list_users().await.map_err(|e| e.to_string())?;

                let summary = format!(
                    "# User Activity Summary\n\n## User Accounts\nTotal users: {}\n\n## Users List\n{}\n\n## Recommendations\n- Review users with UID 0 (root equivalent)\n- Check for inactive accounts\n- Verify group memberships for sensitive datasets",
                    users.len(),
                    users
                        .iter()
                        .take(20)
                        .map(|u| format!("- {} (UID: {})", u.username, u.uid))
                        .collect::<Vec<_>>()
                        .join("\n")
                );

                Ok(json!({
                    "description": "Summarize user activity",
                    "messages": [{"role": "user", "content": {"type": "text", "text": summary}}]
                }))
            }
            "app-maintenance-guide" => {
                let app_name = arguments
                    .and_then(|args| args.get("app_name"))
                    .and_then(|v| v.as_str())
                    .ok_or("app_name argument is required")?;

                let app = self
                    .tools
                    .get_app(app_name)
                    .await
                    .map_err(|e| e.to_string())?;

                let guide = format!(
                    "# Maintenance Guide for {}\n\n## App Status\n- **Status:** {}\n- **Version:** {}\n\n## Maintenance Tasks\n1. Check app logs for errors\n2. Review resource usage (CPU/Memory)\n3. Verify data persistence\n4. Check for available updates\n5. Review backup status\n\n## Common Operations\n- **Restart:** Use the restart_app tool\n- **Stop:** Use the stop_app tool (use force=true if needed)\n- **Update:** Use the upgrade_app tool when updates are available\n\n## Troubleshooting\n- Check /var/log for application logs\n- Verify PVC (Persistent Volume Claims) are healthy\n- Check Kubernetes pod status",
                    app_name,
                    app.state.unwrap_or_else(|| "unknown".to_string()),
                    app.version.unwrap_or_else(|| "unknown".to_string())
                );

                Ok(json!({
                    "description": "Provide maintenance guidance",
                    "messages": [{"role": "user", "content": {"type": "text", "text": guide}}]
                }))
            }
            // Health check prompt
            "health-check" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let alerts = self.tools.get_alerts().await.map_err(|e| e.to_string())?;
                let disks = self.tools.list_disks().await.map_err(|e| e.to_string())?;
                let services = self
                    .tools
                    .list_services()
                    .await
                    .map_err(|e| e.to_string())?;
                let apps = self.tools.list_apps().await.map_err(|e| e.to_string())?;
                let vms = self.tools.list_vms().await.map_err(|e| e.to_string())?;

                let critical_alerts: Vec<Value> = alerts
                    .iter()
                    .filter(|a| a.level == "CRITICAL" || a.level == "WARNING")
                    .map(|a| json!({"id": a.id, "level": a.level, "message": a.message, "timestamp": a.timestamp}))
                    .collect();

                let unhealthy_disks: Vec<Value> = disks
                    .iter()
                    .filter(|d| d.crit != "OK" && d.crit != "PASSED")
                    .map(|d| json!({"name": d.name, "model": d.model, "smart_status": d.crit}))
                    .collect();

                let stopped_services: Vec<Value> = services
                    .iter()
                    .filter(|s| s.state != "RUNNING")
                    .map(|s| json!({"service": s.service, "state": s.state}))
                    .collect();

                let stopped_vms: Vec<Value> = vms
                    .iter()
                    .filter(|v| v.status != "RUNNING")
                    .map(|v| json!({"name": v.name, "status": v.status}))
                    .collect();

                let unhealthy_apps: Vec<Value> = apps
                    .iter()
                    .filter(|a| a.state.as_ref().map(|s| s != "RUNNING").unwrap_or(true))
                    .map(|a| json!({"name": a.name, "state": a.state, "version": a.version}))
                    .collect();

                let pool_issues: Vec<Value> = pools
                    .iter()
                    .filter(|p| p.status != "ONLINE")
                    .map(|p| json!({"name": p.name, "status": p.status}))
                    .collect();

                let report = format!(
                    "# TrueNAS Health Check Report\n\n## Summary
- **Pools:** {} total, {} with issues
- **Disks:** {} total, {} with SMART issues
- **Services:** {} running, {} stopped
- **Apps:** {} running, {} with issues
- **VMs:** {} running, {} stopped
- **Critical/Warnings:** {}\n\n## Critical Issues\n{}\n\n## Pool Status\n{}\n\n## Disk Health\n{}\n\n## Stopped Services\n{}\n\n## Unhealthy Apps\n{}\n\n## Stopped VMs\n{}\n\n## Recommendations\n{}",
                    pools.len(),
                    pool_issues.len(),
                    disks.len(),
                    unhealthy_disks.len(),
                    services.iter().filter(|s| s.state == "RUNNING").count(),
                    stopped_services.len(),
                    apps.iter().filter(|a| a.state.as_ref().map(|s| s == "RUNNING").unwrap_or(false)).count(),
                    unhealthy_apps.len(),
                    vms.iter().filter(|v| v.status == "RUNNING").count(),
                    stopped_vms.len(),
                    critical_alerts.len(),
                    if critical_alerts.is_empty() { "No critical alerts".to_string() } else { serde_json::to_string_pretty(&critical_alerts).unwrap() },
                    serde_json::to_string_pretty(&pool_issues).unwrap_or_else(|_| "No pool data".to_string()),
                    if unhealthy_disks.is_empty() { "All disks healthy".to_string() } else { serde_json::to_string_pretty(&unhealthy_disks).unwrap() },
                    serde_json::to_string_pretty(&stopped_services).unwrap_or_else(|_| "No stopped services".to_string()),
                    serde_json::to_string_pretty(&unhealthy_apps).unwrap_or_else(|_| "No unhealthy apps".to_string()),
                    serde_json::to_string_pretty(&stopped_vms).unwrap_or_else(|_| "No stopped VMs".to_string()),
                    if pool_issues.is_empty() && unhealthy_disks.is_empty() && critical_alerts.is_empty() {
                        "System appears healthy. Continue regular monitoring.".to_string()
                    } else {
                        "Immediate action recommended: 1) Address pool issues first 2) Check SMART status of failing disks 3) Review critical alerts 4) Restart stopped services as needed".to_string()
                    }
                );

                Ok(json!({
                    "description": "Comprehensive health check report",
                    "messages": [{"role": "user", "content": {"type": "text", "text": report}}]
                }))
            }
            // Security audit prompt
            "security-audit" => {
                let users = self.tools.list_users().await.map_err(|e| e.to_string())?;
                let services = self
                    .tools
                    .list_services()
                    .await
                    .map_err(|e| e.to_string())?;
                let certificates = self
                    .tools
                    .list_certificates()
                    .await
                    .map_err(|e| e.to_string())?;
                let alerts = self.tools.get_alerts().await.map_err(|e| e.to_string())?;
                let ssh_connections = self
                    .tools
                    .list_ssh_connections()
                    .await
                    .map_err(|e| e.to_string())?;

                let root_users: Vec<_> = users.iter().filter(|u| u.uid == 0).collect();
                let users_without_home: Vec<_> =
                    users.iter().filter(|u| u.home.is_none()).collect();
                let disabled_services: Vec<_> = services
                    .iter()
                    .filter(|s| s.state != "RUNNING" && s.state != "DEGRADED")
                    .collect();
                let expiring_certs: Vec<_> = certificates
                    .iter()
                    .filter(|c| {
                        if let Some(until) = c.until {
                            // Check if expiring within 30 days
                            let now = chrono::Utc::now().timestamp();
                            let thirty_days = 30 * 24 * 60 * 60;
                            until < now + thirty_days && until > now
                        } else {
                            false
                        }
                    })
                    .collect();
                let security_alerts: Vec<_> = alerts
                    .iter()
                    .filter(|a| a.level == "CRITICAL" || a.level == "WARNING")
                    .collect();

                let ssh_count = ssh_connections.as_array().map(|v| v.len()).unwrap_or(0);
                let root_users_str = if !root_users.is_empty() {
                    root_users
                        .iter()
                        .map(|u| u.username.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    "No root accounts found".to_string()
                };
                let expiring_certs_str = if !expiring_certs.is_empty() {
                    expiring_certs
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    "No certificates expiring soon".to_string()
                };

                let audit = format!(
                    "# TrueNAS Security Audit Report\n\n## User Security\n- **Total users:** {}\n- **Root-equivalent accounts (UID 0):** {}\n- **Users without home directories:** {}\n\n## Service Status\n- **Total services:** {}\n- **Disabled/stopped services:** {}\n\n## Certificate Status\n- **Total certificates:** {}\n- **Expiring within 30 days:** {}\n\n## SSH Connections\n- **Configured connections:** {}\n\n## Security Alerts\n- **Critical/Warnings:** {}\n\n## Findings\n### High Priority\n{}\n\n### Medium Priority\n{}\n\n## Recommendations\n{}",
                    users.len(),
                    root_users.len(),
                    users_without_home.len(),
                    services.len(),
                    disabled_services.len(),
                    certificates.len(),
                    expiring_certs.len(),
                    ssh_count,
                    security_alerts.len(),
                    root_users_str,
                    expiring_certs_str,
                    "1. Review root-equivalent accounts for necessity\n2. Create home directories for users or disable account\n3. Review stopped services for security implications\n4. Renew expiring certificates before expiration\n5. Address security alerts immediately\n6. Review SSH connection security"
                );

                Ok(json!({
                    "description": "Security audit report",
                    "messages": [{"role": "user", "content": {"type": "text", "text": audit}}]
                }))
            }
            // Performance analysis prompt
            "performance-analysis" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let vms = self.tools.list_vms().await.map_err(|e| e.to_string())?;
                let apps = self.tools.list_apps().await.map_err(|e| e.to_string())?;
                let system_info = self
                    .tools
                    .get_system_info()
                    .await
                    .map_err(|e| e.to_string())?;

                let pool_usage: Vec<_> = pools
                    .iter()
                    .map(|p| {
                        let used_pct = if p.size > 0 {
                            (p.size - p.free) * 100 / p.size
                        } else {
                            0
                        };
                        json!({
                            "name": p.name,
                            "status": p.status,
                            "usage_percent": used_pct,
                            "size_gb": p.size / 1_073_741_824,
                            "free_gb": p.free / 1_073_741_824
                        })
                    })
                    .collect();

                let high_memory_vms: Vec<_> = vms.iter()
                    .filter(|v| v.memory > 8_589_934_592) // > 8GB
                    .map(|v| json!({"name": v.name, "memory_gb": v.memory / 1_073_741_824, "vcpus": v.vcpus}))
                    .collect();

                let resource_heavy_apps: Vec<_> = apps
                    .iter()
                    .filter(|a| {
                        let state = a.state.as_ref().map(|s| s == "RUNNING").unwrap_or(false);
                        state && a.port.unwrap_or(0) > 0 // Apps with ports typically need more resources
                    })
                    .map(|a| json!({"name": a.name, "version": a.version}))
                    .collect();

                let analysis = format!(
                    "# TrueNAS Performance Analysis\n\n## System Information\n- **Version:** {}\n- **Uptime:** {} seconds\n- **CPU:** {}\n\n## Pool Performance\n{}\n\n## VM Resource Usage\n- **Total VMs:** {}\n- **High-memory VMs (>8GB):** {}\n\n## Application Resources\n- **Running apps:** {}\n- **Resource-intensive apps:** {}\n\n## Performance Warnings\n{}\n\n## Recommendations\n{}",
                    system_info.version,
                    system_info.uptime_seconds.unwrap_or(0),
                    system_info
                        .cpu_model
                        .unwrap_or_else(|| "Unknown".to_string()),
                    serde_json::to_string_pretty(&pool_usage)
                        .unwrap_or_else(|_| "No pool data".to_string()),
                    vms.len(),
                    if high_memory_vms.is_empty() {
                        "None".to_string()
                    } else {
                        serde_json::to_string_pretty(&high_memory_vms).unwrap()
                    },
                    apps.iter()
                        .filter(|a| a.state.as_ref().map(|s| s == "RUNNING").unwrap_or(false))
                        .count(),
                    if resource_heavy_apps.is_empty() {
                        "None detected".to_string()
                    } else {
                        serde_json::to_string_pretty(&resource_heavy_apps).unwrap()
                    },
                    pool_usage
                        .iter()
                        .filter(|p| p["usage_percent"].as_u64().unwrap_or(0) > 80)
                        .map(|p| format!("- Pool {} is {}% full", p["name"], p["usage_percent"]))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    "1. Consider pool expansion if usage > 80%\n2. Review high-memory VM configurations\n3. Monitor app resource usage over time\n4. Consider load balancing across pools if available\n5. Archive old data to free pool space"
                );

                Ok(json!({
                    "description": "Performance analysis report",
                    "messages": [{"role": "user", "content": {"type": "text", "text": analysis}}]
                }))
            }
            // Capacity planning prompt
            "capacity-planning" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let datasets = self
                    .tools
                    .list_datasets()
                    .await
                    .map_err(|e| e.to_string())?;

                let total_size: u64 = pools.iter().map(|p| p.size).sum();
                let total_free: u64 = pools.iter().map(|p| p.free).sum();
                let total_used = total_size.saturating_sub(total_free);
                let usage_percent = if total_size > 0 {
                    total_used * 100 / total_size
                } else {
                    0
                };

                let dataset_count = datasets.len();
                let avg_dataset_size = if dataset_count > 0 {
                    total_used / dataset_count as u64
                } else {
                    0
                };

                let growth_rate_estimate = 10; // Assume 10% monthly growth (configurable)
                let months_until_full = if usage_percent > 0 {
                    let remaining_pct = 100 - usage_percent;
                    (remaining_pct / growth_rate_estimate) as i32
                } else {
                    999
                };

                let months_until_80_pct = if usage_percent >= 80 {
                    "ALREADY OVER 80%".to_string()
                } else {
                    format!("{}", (100 - usage_percent - 10) / growth_rate_estimate)
                };
                let months_until_90_pct = if usage_percent >= 90 {
                    "ALREADY OVER 90%".to_string()
                } else {
                    format!("{}", (100 - usage_percent) / growth_rate_estimate)
                };

                let planning = format!(
                    "# TrueNAS Capacity Planning Report\n\n## Current Capacity\n- **Total Pool Size:** {:.1} TB\n- **Used Space:** {:.1} TB\n- **Free Space:** {:.1} TB\n- **Usage:** {}%\n- **Datasets:** {}\n\n## Growth Projections\n- **Estimated monthly growth:** {}% (assumed)\n- **Estimated months until 80% capacity:** {}\n- **Estimated months until 90% capacity:** {}\n- **Estimated months until full:** {}\n\n## Dataset Analysis\n- **Average dataset size:** {:.2} GB\n- **Largest datasets:**\n{}\n\n## Recommendations\n{}\n\n## Action Items\n{}",
                    total_size as f64 / 1_099_511_627_776.0,
                    total_used as f64 / 1_099_511_627_776.0,
                    total_free as f64 / 1_099_511_627_776.0,
                    usage_percent,
                    dataset_count,
                    growth_rate_estimate,
                    months_until_80_pct,
                    months_until_90_pct,
                    months_until_full,
                    avg_dataset_size as f64 / 1_073_741_824.0,
                    datasets
                        .iter()
                        .take(5)
                        .map(|d| format!("- {} (pool: {})", d.name, d.pool))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    match usage_percent {
                        p if p >= 90 => "CRITICAL: Immediate capacity expansion required!",
                        p if p >= 80 => "WARNING: Near capacity, plan expansion soon",
                        p if p >= 70 => "CAUTION: Monitor closely, consider expansion planning",
                        _ => "Healthy: Normal monitoring recommended",
                    },
                    "1. [Immediate] Review pools at >80% capacity\n2. [This Month] Identify archival candidates\n3. [This Quarter] Plan storage expansion\n4. [Ongoing] Monitor usage trends weekly\n5. [Annually] Reassess growth projections"
                );

                Ok(json!({
                    "description": "Capacity planning report",
                    "messages": [{"role": "user", "content": {"type": "text", "text": planning}}]
                }))
            }
            // Disaster recovery plan prompt
            "disaster-recovery-plan" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let snapshots = self
                    .tools
                    .list_snapshots()
                    .await
                    .map_err(|e| e.to_string())?;
                let replication_tasks = self
                    .tools
                    .list_replication_tasks()
                    .await
                    .map_err(|e| e.to_string())?;
                let cloudsync_tasks = self
                    .tools
                    .list_cloudsync_tasks()
                    .await
                    .map_err(|e| e.to_string())?;

                // Get the most recent snapshot for each dataset
                let mut latest_by_dataset: std::collections::HashMap<String, &tools::Snapshot> =
                    std::collections::HashMap::new();
                for s in snapshots.iter().filter(|s| s.creation > 0) {
                    if let Some(existing) = latest_by_dataset.get(&s.dataset) {
                        if s.creation > existing.creation {
                            latest_by_dataset.insert(s.dataset.clone(), s);
                        }
                    } else {
                        latest_by_dataset.insert(s.dataset.clone(), s);
                    }
                }
                let latest_snapshots: Vec<_> = latest_by_dataset.into_values().take(10).collect();

                let healthy_pools = pools.iter().filter(|p| p.status == "ONLINE").count();
                let failed_pools = pools.len() - healthy_pools;

                let replication_count = replication_tasks.len();
                let cloudsync_count = cloudsync_tasks.len();

                let plan = format!(
                    "# TrueNAS Disaster Recovery Plan\n\n## Executive Summary\n- **Total Pools:** {} ({} healthy, {} with issues)\n- **Replication Tasks:** {}\n- **Cloud Sync Tasks:** {}\n- **Recent Snapshots:** {}\n\n## Recovery Procedures\n\n### 1. Pool Failure Recovery\n- **If single pool fails:**\n  1. Check physical disk status\n  2. Review alert logs for root cause\n  3. If pool is degraded, replace failed disk\n  4. Initiate resilver\n  5. Verify data integrity after resilver\n\n- **If complete pool loss:**\n  1. Check if offsite replication exists\n  2. Restore from replicated backup\n  3. Verify restoration integrity\n\n### 2. Data Corruption Recovery\n- **Procedure:**\n  1. Identify affected dataset\n  2. Rollback to last known good snapshot\n  3. If snapshot unavailable, use replication\n  4. Verify data integrity post-recovery\n\n### 3. Application Recovery\n- **For Kubernetes apps:**\n  1. Check chart release status\n  2. Review PVC health\n  3. Rollback to previous version if needed\n  4. Restore application data from snapshots\n\n### 4. VM Recovery\n- **Procedure:**\n  1. Check VM snapshot history\n  2. Clone from last snapshot\n  3. Verify VM functionality\n  4. Update DNS if IP changed\n\n## Current Backup Status\n### Snapshots\n{}\n\n### Replication\n- **Configured tasks:** {}\n- **Status:** Review each task for last run status\n\n### Cloud Sync\n- **Configured tasks:** {}\n- **Status:** Review each task for last run status\n\n## Pre-Recovery Checklist\n- [ ] Notify stakeholders of outage\n- [ ] Document current state\n- [ ] Verify backup integrity\n- [ ] Prepare recovery environment\n- [ ] Test recovery procedure\n\n## Post-Recovery Checklist\n- [ ] Verify all services running\n- [ ] Confirm data integrity\n- [ ] Update documentation\n- [ ] Conduct post-mortem\n- [ ] Implement preventive measures",
                    pools.len(),
                    healthy_pools,
                    failed_pools,
                    replication_count,
                    cloudsync_count,
                    latest_snapshots.len(),
                    if latest_snapshots.is_empty() {
                        "No recent snapshots found - CREATE IMMEDIATELY".to_string()
                    } else {
                        latest_snapshots
                            .iter()
                            .map(|s| format!("- {} (created: {})", s.name, s.creation))
                            .collect::<Vec<_>>()
                            .join("\n")
                    },
                    if replication_count == 0 {
                        "No replication configured - RECOMMEND IMMEDIATELY".to_string()
                    } else {
                        format!("{} tasks configured", replication_count)
                    },
                    if cloudsync_count == 0 {
                        "No cloud sync configured - RECOMMEND for critical data".to_string()
                    } else {
                        format!("{} tasks configured", cloudsync_count)
                    }
                );

                Ok(json!({
                    "description": "Disaster recovery plan",
                    "messages": [{"role": "user", "content": {"type": "text", "text": plan}}]
                }))
            }
            // Incident response prompt
            "incident-response" => {
                let incident_type = arguments
                    .and_then(|args| args.get("incident_type"))
                    .and_then(|v| v.as_str())
                    .ok_or("incident_type argument is required")?;

                let response_steps = match incident_type {
                    "pool_failure" => vec![
                        ("Assess", "Check pool status and identify failed vdevs"),
                        ("Contain", "If degraded, do not autoline - assess first-off"),
                        (
                            "Identify",
                            "Determine root cause: disk failure, controller issue, power loss",
                        ),
                        ("Replace", "If disk failure, replace and initiate resilver"),
                        ("Recover", "Monitor resilver completion"),
                        ("Verify", "Run scrub and verify pool health"),
                        ("Document", "Log incident and root cause analysis"),
                    ],
                    "data_corruption" => vec![
                        (
                            "Assess",
                            "Identify affected datasets and scope of corruption",
                        ),
                        (
                            "Snapshot",
                            "Take emergency snapshot of current state if possible",
                        ),
                        ("Restore", "Rollback to last known good snapshot"),
                        ("Verify", "Check for hidden corruption with scrub"),
                        (
                            "Escalate",
                            "If pool-level corruption, consider professional recovery",
                        ),
                        ("Prevent", "Enable regular scrubs and checksums"),
                    ],
                    "service_outage" => vec![
                        ("Identify", "Determine which service is affected"),
                        ("Check", "Review service logs for errors"),
                        ("Restart", "Attempt service restart if safe"),
                        (
                            "Dependencies",
                            "Check if underlying issues (storage, network)",
                        ),
                        (
                            "Fallback",
                            "Consider stopping app and restarting if persistent",
                        ),
                        ("Escalate", "If system-level issue, may need reboot"),
                    ],
                    "security_breach" => vec![
                        (
                            "Isolate",
                            "Disconnect from network if active intrusion suspected",
                        ),
                        (
                            "Assess",
                            "Identify scope: which users, data, services affected",
                        ),
                        ("Preserve", "Take forensic snapshots of affected systems"),
                        (
                            "Identify",
                            "Find attack vector: SSH, app vulnerability, credentials",
                        ),
                        (
                            "Remediate",
                            "Close attack vector, change credentials, patch",
                        ),
                        ("Recover", "Restore from clean backups if needed"),
                        ("Report", "Document for compliance and security teams"),
                    ],
                    _ => vec![
                        ("Assess", "Gather information about the incident"),
                        ("Identify", "Determine type and scope"),
                        ("Contain", "Limit damage if active"),
                        ("Recover", "Restore normal operation"),
                        ("Document", "Record incident details"),
                    ],
                };

                let steps_markdown: String = response_steps
                    .iter()
                    .map(|(action, desc)| {
                        format!(
                            "### {}. {}\n{}\n",
                            response_steps
                                .iter()
                                .position(|(a, _)| a == action)
                                .unwrap_or(0)
                                + 1,
                            action,
                            desc
                        )
                    })
                    .collect();

                let alerts = self.tools.get_alerts().await.map_err(|e| e.to_string())?;
                let relevant_alerts: Vec<_> = alerts
                    .iter()
                    .filter(|a| {
                        a.message
                            .to_lowercase()
                            .contains(&incident_type.to_lowercase())
                    })
                    .collect();

                let incident_guide = format!(
                    "# Incident Response: {}\n\n## Overview\n**Incident Type:** {}\n**Severity:** {}\n**Timestamp:** {}\n\n## Response Steps\n{}\n\n## Current System State\n### Relevant Alerts\n{}\n\n## Immediate Actions Required\n1. **STOP** - Do not make changes until assessment complete\n2. **ASSESS** - Gather all relevant information\n3. **COMMUNICATE** - Notify stakeholders if needed\n4. **EXECUTE** - Follow steps above\n5. **DOCUMENT** - Log all actions taken\n\n## Escalation Criteria\n- If issue not resolved within 1 hour: Escalate to senior admin\n- If data loss suspected: Escalate immediately\n- If security breach confirmed: Follow security incident response\n\n## Post-Incident\n- [ ] Complete incident report\n- [ ] Identify root cause\n- [ ] Implement preventive measures\n- [ ] Update runbooks\n- [ ] Conduct team debrief",
                    incident_type.to_uppercase(),
                    incident_type,
                    if relevant_alerts.is_empty() {
                        "Assessing"
                    } else {
                        "Active"
                    },
                    chrono::Utc::now().to_rfc3339(),
                    steps_markdown,
                    if relevant_alerts.is_empty() {
                        "No directly relevant alerts found. Check system logs manually.".to_string()
                    } else {
                        serde_json::to_string_pretty(&relevant_alerts)
                            .unwrap_or_else(|_| "Error displaying alerts".to_string())
                    }
                );

                Ok(json!({
                    "description": format!("Incident response guide for {}", incident_type),
                    "messages": [{"role": "user", "content": {"type": "text", "text": incident_guide}}]
                }))
            }
            // Troubleshooting prompt
            "troubleshoot-issue" => {
                let issue = arguments
                    .and_then(|args| args.get("issue_description"))
                    .and_then(|v| v.as_str())
                    .ok_or("issue_description argument is required")?;

                let system_info = self
                    .tools
                    .get_system_info()
                    .await
                    .map_err(|e| e.to_string())?;
                let alerts = self.tools.get_alerts().await.map_err(|e| e.to_string())?;
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                let apps = self.tools.list_apps().await.map_err(|e| e.to_string())?;

                let relevant_alerts: Vec<_> = alerts
                    .iter()
                    .filter(|a| {
                        let message = a.message.to_lowercase();
                        let issue_lower = issue.to_lowercase();
                        message.contains(&issue_lower)
                            || issue_lower.contains("slow")
                                && (message.contains("cpu") || message.contains("performance"))
                            || issue_lower.contains("disk")
                                && (message.contains("pool") || message.contains("disk"))
                            || issue_lower.contains("app")
                                && (message.contains("app") || message.contains("pod"))
                    })
                    .collect();

                let troubleshooting = format!(
                    "# Troubleshooting Guide\n\n## Issue Description\n{}\n\n## System Context\n- **Version:** {}\n- **Hostname:** {}\n- **Uptime:** {} seconds\n\n## Relevant Alerts\n{}\n\n## Pool Status\n{}\n\n## Application Status\n{}\n\n## Diagnostic Steps\n1. **Check Alerts** - Review above for clues\n2. **Verify Resources** - CPU, memory, storage usage\n3. **Review Logs** - Application and system logs\n4. **Check Dependencies** - Services, storage, network\n5. **Test Components** - Isolate the failing part\n\n## Common Solutions by Issue Type\n### Performance Issues\n- Check pool I/O utilization\n- Review VM/app resource limits\n- Consider scaling resources\n\n### Connectivity Issues\n- Verify network configuration\n- Check DNS settings\n- Review firewall rules\n\n### Storage Issues\n- Check pool status and resilver progress\n- Review disk SMART status\n- Verify snapshot space\n\n### Application Issues\n- Check app logs\n- Verify PVC health\n- Review resource quotas\n\n## Next Steps\n1. Review relevant alerts above\n2. Check specific component logs\n3. If unclear, gather more information:\n   - Pool status: `get_pool_status`\n   - Disk health: `get_disk_health`\n   - System events: `get_system_events`\n   - App logs: Check Kubernetes pods",
                    issue,
                    system_info.version,
                    system_info.hostname,
                    system_info.uptime_seconds.unwrap_or(0),
                    if relevant_alerts.is_empty() {
                        "No directly relevant alerts found. Check full alert list.".to_string()
                    } else {
                        serde_json::to_string_pretty(&relevant_alerts)
                            .unwrap_or_else(|_| "Error".to_string())
                    },
                    serde_json::to_string_pretty(&pools)
                        .unwrap_or_else(|_| "No pool data".to_string()),
                    serde_json::to_string_pretty(&apps)
                        .unwrap_or_else(|_| "No app data".to_string())
                );

                Ok(json!({
                    "description": format!("Troubleshooting guide for: {}", issue),
                    "messages": [{"role": "user", "content": {"type": "text", "text": troubleshooting}}]
                }))
            }
            _ => Err(format!("Unknown prompt: {}", name)),
        }
    }

    /// Read a resource by URI
    async fn read_resource(&self, uri: &str) -> Result<Value, String> {
        match uri {
            "truenas://system/info" => {
                let info = self
                    .tools
                    .get_system_info()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&info).map_err(|e| e.to_string())?
                    }]
                }))
            }
            "truenas://pools/list" => {
                let pools = self.tools.list_pools().await.map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&pools).map_err(|e| e.to_string())?
                    }]
                }))
            }
            "truenas://datasets/tree" => {
                let datasets = self
                    .tools
                    .list_datasets()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&datasets).map_err(|e| e.to_string())?
                    }]
                }))
            }
            "truenas://apps/list" => {
                let apps = self.tools.list_apps().await.map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&apps).map_err(|e| e.to_string())?
                    }]
                }))
            }
            "truenas://disks/list" => {
                let disks = self.tools.list_disks().await.map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&disks).map_err(|e| e.to_string())?
                    }]
                }))
            }
            "truenas://network/interfaces" => {
                let interfaces = self
                    .tools
                    .list_interfaces()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string(&interfaces).map_err(|e| e.to_string())?
                    }]
                }))
            }
            _ => Err(format!("Unknown resource URI: {}", uri)),
        }
    }

    fn list_tools(&self) -> Value {
        json!([
            // User Management
            {"name": "list_users", "description": "List all users on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "get_user", "description": "Get details of a specific user by ID", "inputSchema": {"type": "object", "properties": {"user_id": {"type": "integer"}}, "required": ["user_id"]}},
            {"name": "get_user_by_username", "description": "Get details of a specific user by username", "inputSchema": {"type": "object", "properties": {"username": {"type": "string"}}, "required": ["username"]}},
            {"name": "create_user", "description": "Create a new user on TrueNAS", "inputSchema": {"type": "object", "properties": {"username": {"type": "string"}, "password": {"type": "string"}, "uid": {"type": "integer"}, "group_ids": {"type": "array", "items": {"type": "integer"}}}, "required": ["username", "password"]}},
            {"name": "update_user", "description": "Update an existing user on TrueNAS", "inputSchema": {"type": "object", "properties": {"user_id": {"type": "integer"}, "updates": {"type": "object"}}, "required": ["user_id"]}},
            {"name": "delete_user", "description": "Delete a user from TrueNAS", "inputSchema": {"type": "object", "properties": {"user_id": {"type": "integer"}}, "required": ["user_id"]}},
            // Group Management
            {"name": "list_groups", "description": "List all groups on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "get_group", "description": "Get details of a specific group by ID", "inputSchema": {"type": "object", "properties": {"group_id": {"type": "integer"}}, "required": ["group_id"]}},
            {"name": "get_group_by_name", "description": "Get details of a specific group by name", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
            {"name": "create_group", "description": "Create a new group on TrueNAS", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "gid": {"type": "integer"}, "users": {"type": "array", "items": {"type": "integer"}}}, "required": ["name"]}},
            {"name": "delete_group", "description": "Delete a group from TrueNAS", "inputSchema": {"type": "object", "properties": {"group_id": {"type": "integer"}}, "required": ["group_id"]}},
            // Pool Management
            {"name": "list_pools", "description": "List all storage pools on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "get_pool_status", "description": "Get the status of a specific storage pool", "inputSchema": {"type": "object", "properties": {"pool_name": {"type": "string"}}, "required": ["pool_name"]}},
            {"name": "scrub_pool", "description": "Start a scrub on a storage pool", "inputSchema": {"type": "object", "properties": {"pool_name": {"type": "string"}}, "required": ["pool_name"]}},
            // Dataset Management
            {"name": "list_datasets", "description": "List all datasets on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "get_dataset", "description": "Get details of a specific dataset", "inputSchema": {"type": "object", "properties": {"dataset_path": {"type": "string"}}, "required": ["dataset_path"]}},
            {"name": "create_dataset", "description": "Create a new dataset in a pool", "inputSchema": {"type": "object", "properties": {"pool_name": {"type": "string"}, "dataset_name": {"type": "string"}}, "required": ["pool_name", "dataset_name"]}},
            {"name": "delete_dataset", "description": "Delete a dataset", "inputSchema": {"type": "object", "properties": {"dataset_path": {"type": "string"}}, "required": ["dataset_path"]}},
            {"name": "update_dataset", "description": "Update a dataset's properties", "inputSchema": {"type": "object", "properties": {"dataset_path": {"type": "string"}, "updates": {"type": "object"}}, "required": ["dataset_path", "updates"]}},
            // SMB Shares
            {"name": "list_smb_shares", "description": "List all SMB shares on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "create_smb_share", "description": "Create a new SMB share", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "path": {"type": "string"}, "comment": {"type": "string"}}, "required": ["name", "path"]}},
            {"name": "delete_smb_share", "description": "Delete an SMB share", "inputSchema": {"type": "object", "properties": {"share_id": {"type": "integer"}}, "required": ["share_id"]}},
            // NFS Exports
            {"name": "list_nfs_exports", "description": "List all NFS exports on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "create_nfs_export", "description": "Create a new NFS export", "inputSchema": {"type": "object", "properties": {"paths": {"type": "array", "items": {"type": "string"}}, "comment": {"type": "string"}}, "required": ["paths", "comment"]}},
            {"name": "delete_nfs_export", "description": "Delete an NFS export", "inputSchema": {"type": "object", "properties": {"export_id": {"type": "integer"}}, "required": ["export_id"]}},
            // Snapshots
            {"name": "list_snapshots", "description": "List all ZFS snapshots on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "create_snapshot", "description": "Create a new ZFS snapshot", "inputSchema": {"type": "object", "properties": {"dataset": {"type": "string"}, "snapshot_name": {"type": "string"}}, "required": ["dataset", "snapshot_name"]}},
            {"name": "delete_snapshot", "description": "Delete a ZFS snapshot", "inputSchema": {"type": "object", "properties": {"snapshot_id": {"type": "string"}}, "required": ["snapshot_id"]}},
            {"name": "rollback_snapshot", "description": "Rollback a dataset to a specific snapshot", "inputSchema": {"type": "object", "properties": {"dataset": {"type": "string"}, "snapshot_name": {"type": "string"}}, "required": ["dataset", "snapshot_name"]}},
            {"name": "clone_snapshot", "description": "Clone a snapshot to a new dataset", "inputSchema": {"type": "object", "properties": {"snapshot_id": {"type": "string"}, "target_name": {"type": "string"}}, "required": ["snapshot_id", "target_name"]}},
            {"name": "get_dataset_snapshots", "description": "Get all snapshots for a specific dataset", "inputSchema": {"type": "object", "properties": {"dataset": {"type": "string"}}, "required": ["dataset"]}},
            // iSCSI
            {"name": "list_iscsi_targets", "description": "List all iSCSI targets on the TrueNAS system", "inputSchema": {"type": "object"}},
            {"name": "create_iscsi_target", "description": "Create a new iSCSI target", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
            {"name": "delete_iscsi_target", "description": "Delete an iSCSI target", "inputSchema": {"type": "object", "properties": {"target_id": {"type": "integer"}}, "required": ["target_id"]}},
            // System
            {"name": "get_system_info", "description": "Get system information from TrueNAS", "inputSchema": {"type": "object"}},
            // Apps (SCALE)
            {"name": "list_apps", "description": "List all applications on TrueNAS SCALE", "inputSchema": {"type": "object"}},
            {"name": "get_app", "description": "Get details of a specific application", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}}, "required": ["app_name"]}},
            {"name": "start_app", "description": "Start an application on TrueNAS", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "options": {"type": "object"}}, "required": ["app_name"]}},
            {"name": "stop_app", "description": "Stop an application on TrueNAS", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "force": {"type": "boolean"}}, "required": ["app_name"]}},
            {"name": "restart_app", "description": "Restart an application on TrueNAS", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}}, "required": ["app_name"]}},
            {"name": "create_app", "description": "Create a new application from a catalog item", "inputSchema": {"type": "object", "properties": {"catalog": {"type": "string"}, "item": {"type": "string"}, "name": {"type": "string"}, "values": {"type": "object"}, "version": {"type": "string"}}, "required": ["catalog", "item", "name", "values"]}},
            {"name": "update_app", "description": "Update an existing application with new configuration", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "values": {"type": "object"}}, "required": ["app_name", "values"]}},
            {"name": "delete_app", "description": "Delete an application from TrueNAS", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "force": {"type": "boolean"}}, "required": ["app_name"]}},
            {"name": "rollback_app", "description": "Rollback an application to a previous version", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "rollback_version": {"type": "string"}, "snap_name": {"type": "string"}, "force": {"type": "boolean"}}, "required": ["app_name"]}},
            {"name": "get_app_config", "description": "Get the configuration of an application", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}}, "required": ["app_name"]}},
            {"name": "get_app_upgrade_options", "description": "Get available upgrade options for an application", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}}, "required": ["app_name"]}},
            {"name": "upgrade_app", "description": "Upgrade an application to a newer version", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "options": {"type": "object"}}, "required": ["app_name"]}},
            {"name": "scale_app", "description": "Scale an application's replica count", "inputSchema": {"type": "object", "properties": {"app_name": {"type": "string"}, "replica": {"type": "integer"}}, "required": ["app_name", "replica"]}},
            // Catalogs (SCALE)
            {"name": "list_catalog_items", "description": "List all available catalog items from TrueNAS catalog", "inputSchema": {"type": "object"}},
            {"name": "get_catalog", "description": "Get details of a specific catalog", "inputSchema": {"type": "object", "properties": {"catalog_id": {"type": "string"}}, "required": ["catalog_id"]}},
            {"name": "get_catalog_trains", "description": "Get all available train versions from a catalog", "inputSchema": {"type": "object", "properties": {"catalog_id": {"type": "string"}}, "required": ["catalog_id"]}},
            {"name": "get_catalog_item", "description": "Get details of a specific item from a catalog", "inputSchema": {"type": "object", "properties": {"catalog_id": {"type": "string"}, "item": {"type": "string"}, "train": {"type": "string"}}, "required": ["catalog_id", "item", "train"]}},
            // Chart Releases (SCALE)
            {"name": "list_chart_releases", "description": "List all deployed chart releases (apps)", "inputSchema": {"type": "object"}},
            {"name": "get_chart_release", "description": "Get details of a specific chart release", "inputSchema": {"type": "object", "properties": {"release_name": {"type": "string"}}, "required": ["release_name"]}},
            {"name": "get_chart_release_resources", "description": "Get resources for a specific chart release", "inputSchema": {"type": "object", "properties": {"release_name": {"type": "string"}}, "required": ["release_name"]}},
            // VMs
            {"name": "list_vms", "description": "List all virtual machines on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_vm", "description": "Get details of a specific virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}}, "required": ["vm_id"]}},
            {"name": "start_vm", "description": "Start a virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}}, "required": ["vm_id"]}},
            {"name": "stop_vm", "description": "Stop a virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}, "force": {"type": "boolean"}}, "required": ["vm_id"]}},
            {"name": "restart_vm", "description": "Restart a virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}}, "required": ["vm_id"]}},
            {"name": "powercycle_vm", "description": "Power cycle a virtual machine (hard reset)", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}}, "required": ["vm_id"]}},
            {"name": "create_vm", "description": "Create a new virtual machine on TrueNAS", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "vcpus": {"type": "integer"}, "memory": {"type": "integer", "minimum": 1}, "disk_size": {"type": "integer", "description": "Disk size in bytes"}, "iso": {"type": "string", "description": "ISO image path"}}, "required": ["name", "vcpus", "memory"]}},
            {"name": "update_vm", "description": "Update configuration of an existing virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}, "updates": {"type": "object", "description": "JSON object with fields to update"}}, "required": ["vm_id"]}},
            {"name": "delete_vm", "description": "Delete a virtual machine from TrueNAS", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}, "force": {"type": "boolean", "description": "Force deletion (stop VM first)"}}, "required": ["vm_id"]}},
            {"name": "clone_vm", "description": "Clone an existing virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}, "name": {"type": "string", "description": "Name for the cloned VM"}}, "required": ["vm_id", "name"]}},
            // Network
            {"name": "list_interfaces", "description": "List all network interfaces on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "list_routes", "description": "List all network routes on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_dns", "description": "Get DNS configuration for TrueNAS", "inputSchema": {"type": "object"}},
            // Services
            {"name": "list_services", "description": "List all services on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_service", "description": "Get details of a specific service", "inputSchema": {"type": "object", "properties": {"service_id": {"type": "integer"}}, "required": ["service_id"]}},
            {"name": "start_service", "description": "Start a service on TrueNAS", "inputSchema": {"type": "object", "properties": {"service_id": {"type": "integer"}}, "required": ["service_id"]}},
            {"name": "stop_service", "description": "Stop a service on TrueNAS", "inputSchema": {"type": "object", "properties": {"service_id": {"type": "integer"}}, "required": ["service_id"]}},
            {"name": "restart_service", "description": "Restart a service on TrueNAS", "inputSchema": {"type": "object", "properties": {"service_id": {"type": "integer"}}, "required": ["service_id"]}},
            // System
            {"name": "get_alerts", "description": "Get system alerts from TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_alert_classes", "description": "Get alert classes/categories available", "inputSchema": {"type": "object"}},
            {"name": "dismiss_alert", "description": "Dismiss a specific alert by ID", "inputSchema": {"type": "object", "properties": {"alert_id": {"type": "string"}}, "required": ["alert_id"]}},
            {"name": "clear_alerts", "description": "Clear all system alerts", "inputSchema": {"type": "object"}},
            {"name": "get_system_events", "description": "Get recent system events/logs for AI monitoring", "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "description": "Number of events to return (default: 50)"}}}},
            {"name": "get_disk_health", "description": "Get disk health/SMART status for AI monitoring", "inputSchema": {"type": "object"}},
            {"name": "check_for_updates", "description": "Check for system updates", "inputSchema": {"type": "object"}},
            {"name": "reboot_system", "description": "Reboot the TrueNAS system. Requires confirm=true for safety.", "inputSchema": {"type": "object", "properties": {"confirm": {"type": "boolean", "description": "Must be true to confirm this destructive operation"}, "delay_seconds": {"type": "integer", "description": "Delay in seconds before rebooting (default: 10)"}}, "required": ["confirm"]}},
            {"name": "shutdown_system", "description": "Shutdown the TrueNAS system. Requires confirm=true for safety.", "inputSchema": {"type": "object", "properties": {"confirm": {"type": "boolean", "description": "Must be true to confirm this destructive operation"}, "delay_seconds": {"type": "integer", "description": "Delay in seconds before shutting down (default: 10)"}}, "required": ["confirm"]}},
            // Disks
            {"name": "list_disks", "description": "List all disks on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_disk", "description": "Get details of a specific disk", "inputSchema": {"type": "object", "properties": {"disk_name": {"type": "string"}}, "required": ["disk_name"]}},
            {"name": "get_smart_status", "description": "Get SMART status for a specific disk", "inputSchema": {"type": "object", "properties": {"disk_name": {"type": "string"}}, "required": ["disk_name"]}},
            // Certificates
            {"name": "list_certificates", "description": "List all certificates on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "get_certificate", "description": "Get details of a specific certificate", "inputSchema": {"type": "object", "properties": {"cert_id": {"type": "integer"}}, "required": ["cert_id"]}},
            // Replication
            {"name": "list_replication_tasks", "description": "List all replication tasks on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "run_replication_task", "description": "Run a replication task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // Cloud Sync
            {"name": "list_cloudsync_tasks", "description": "List all cloud sync tasks on TrueNAS", "inputSchema": {"type": "object"}},
            {"name": "run_cloudsync_task", "description": "Run a cloud sync task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // Enclosure
            {"name": "get_enclosure", "description": "Get enclosure information", "inputSchema": {"type": "object"}},
            // Support
            {"name": "get_support", "description": "Get support information", "inputSchema": {"type": "object"}},
            // Jails (CORE only)
            {"name": "list_jails", "description": "List all jails on TrueNAS CORE", "inputSchema": {"type": "object"}},
            {"name": "get_jail", "description": "Get details of a specific jail by ID", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}}, "required": ["jail_id"]}},
            {"name": "get_jail_by_name", "description": "Get details of a specific jail by name", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
            {"name": "create_jail", "description": "Create a new jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "jail_base": {"type": "string"}, "ip4_addr": {"type": "string"}}, "required": ["name", "jail_base"]}},
            {"name": "update_jail", "description": "Update an existing jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}, "updates": {"type": "object"}}, "required": ["jail_id"]}},
            {"name": "delete_jail", "description": "Delete a jail from TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}, "force": {"type": "boolean"}}, "required": ["jail_id"]}},
            {"name": "start_jail", "description": "Start a jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}}, "required": ["jail_id"]}},
            {"name": "stop_jail", "description": "Stop a jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}}, "required": ["jail_id"]}},
            {"name": "restart_jail", "description": "Restart a jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}}, "required": ["jail_id"]}},
            {"name": "clone_jail", "description": "Clone a jail on TrueNAS CORE", "inputSchema": {"type": "object", "properties": {"jail_id": {"type": "integer"}, "name": {"type": "string"}}, "required": ["jail_id", "name"]}},
            // Batch operations
            {"name": "batch", "description": "Execute multiple operations in a single call", "inputSchema": {"type": "object", "properties": {"operations": {"type": "array", "items": {"type": "object", "properties": {"name": {"type": "string"}, "arguments": {"type": "object"}}, "required": ["name"]}}}, "required": ["operations"]}},
            // Tasks
            {"name": "list_tasks", "description": "List all running and recent tasks", "inputSchema": {"type": "object"}},
            {"name": "get_task_status", "description": "Get the status of a specific task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "abort_task", "description": "Abort a running task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // Kubernetes (SCALE)
            {"name": "get_kubernetes_status", "description": "Get the status of the Kubernetes cluster", "inputSchema": {"type": "object"}},
            {"name": "get_kubernetes_nodes", "description": "List Kubernetes nodes", "inputSchema": {"type": "object"}},
            {"name": "get_kubernetes_pods", "description": "List Kubernetes pods", "inputSchema": {"type": "object"}},
            {"name": "get_kubernetes_services", "description": "List Kubernetes services", "inputSchema": {"type": "object"}},
            // Docker
            {"name": "list_docker_images", "description": "List all Docker images", "inputSchema": {"type": "object"}},
            {"name": "pull_docker_image", "description": "Pull a Docker image", "inputSchema": {"type": "object", "properties": {"image": {"type": "string"}, "tag": {"type": "string"}}, "required": ["image"]}},
            // Cloud credentials
            {"name": "list_cloud_credentials", "description": "List all cloud credentials", "inputSchema": {"type": "object"}},
            {"name": "create_cloud_credential", "description": "Create a new cloud credential", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "provider": {"type": "string"}, "attributes": {"type": "object"}}, "required": ["name", "provider", "attributes"]}},
            {"name": "delete_cloud_credential", "description": "Delete a cloud credential", "inputSchema": {"type": "object", "properties": {"cred_id": {"type": "integer"}}, "required": ["cred_id"]}},
            // Rsync
            {"name": "list_rsync_tasks", "description": "List all rsync tasks", "inputSchema": {"type": "object"}},
            {"name": "get_rsync_task", "description": "Get an rsync task by ID", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "create_rsync_task", "description": "Create a new rsync task", "inputSchema": {"type": "object", "properties": {"task": {"type": "object"}}, "required": ["task"]}},
            {"name": "delete_rsync_task", "description": "Delete an rsync task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "run_rsync_task", "description": "Run an rsync task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // SSH
            {"name": "list_ssh_connections", "description": "List all SSH connections", "inputSchema": {"type": "object"}},
            {"name": "create_ssh_connection", "description": "Create a new SSH connection", "inputSchema": {"type": "object", "properties": {"connection": {"type": "object"}}, "required": ["connection"]}},
            {"name": "delete_ssh_connection", "description": "Delete an SSH connection", "inputSchema": {"type": "object", "properties": {"connection_id": {"type": "integer"}}, "required": ["connection_id"]}},
            {"name": "list_ssh_keys", "description": "List all SSH keys", "inputSchema": {"type": "object"}},
            {"name": "create_ssh_key", "description": "Create a new SSH key", "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "key": {"type": "string"}}, "required": ["name", "key"]}},
            {"name": "delete_ssh_key", "description": "Delete an SSH key", "inputSchema": {"type": "object", "properties": {"key_id": {"type": "integer"}}, "required": ["key_id"]}},
            // TFTP
            {"name": "list_tftp_services", "description": "List all TFTP services", "inputSchema": {"type": "object"}},
            // SMART
            {"name": "list_smart_tests", "description": "List all SMART tests", "inputSchema": {"type": "object"}},
            {"name": "create_smart_test", "description": "Create a new SMART test", "inputSchema": {"type": "object", "properties": {"test": {"type": "object"}}, "required": ["test"]}},
            {"name": "delete_smart_test", "description": "Delete a SMART test", "inputSchema": {"type": "object", "properties": {"test_id": {"type": "integer"}}, "required": ["test_id"]}},
            {"name": "get_smart_config", "description": "Get SMART configuration", "inputSchema": {"type": "object"}},
            {"name": "update_smart_config", "description": "Update SMART configuration", "inputSchema": {"type": "object", "properties": {"config": {"type": "object"}}, "required": ["config"]}},
            // System
            {"name": "get_general_config", "description": "Get general system configuration", "inputSchema": {"type": "object"}},
            {"name": "update_system", "description": "Update system configuration", "inputSchema": {"type": "object", "properties": {"updates": {"type": "object"}}, "required": ["updates"]}},
            {"name": "get_support", "description": "Get support information", "inputSchema": {"type": "object"}},
            {"name": "get_enclosure", "description": "Get enclosure information", "inputSchema": {"type": "object"}},
            {"name": "update_dns", "description": "Update DNS configuration", "inputSchema": {"type": "object", "properties": {"nameservers": {"type": "array", "items": {"type": "string"}}, "domains": {"type": "array", "items": {"type": "string"}}}, "required": ["nameservers"]}},
            // VMs
            {"name": "powercycle_vm", "description": "Power cycle a virtual machine", "inputSchema": {"type": "object", "properties": {"vm_id": {"type": "integer"}}, "required": ["vm_id"]}},
            // Catalogs
            {"name": "refresh_catalogs", "description": "Refresh the catalog cache", "inputSchema": {"type": "object"}},
            {"name": "delete_catalog", "description": "Delete a catalog", "inputSchema": {"type": "object", "properties": {"catalog_id": {"type": "string"}}, "required": ["catalog_id"]}},
            // Pool
            {"name": "scrub_pool", "description": "Start a scrub on a storage pool", "inputSchema": {"type": "object", "properties": {"pool_name": {"type": "string"}}, "required": ["pool_name"]}},
            // Dataset
            {"name": "get_dataset_by_path", "description": "Get a dataset by its mount path", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
            // Certificate
            {"name": "list_certificates", "description": "List all certificates", "inputSchema": {"type": "object"}},
            {"name": "get_certificate", "description": "Get a certificate by ID", "inputSchema": {"type": "object", "properties": {"cert_id": {"type": "integer"}}, "required": ["cert_id"]}},
            // Replication
            {"name": "get_replication_task", "description": "Get a replication task by ID", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "create_replication_task", "description": "Create a new replication task", "inputSchema": {"type": "object", "properties": {"task": {"type": "object"}}, "required": ["task"]}},
            {"name": "delete_replication_task", "description": "Delete a replication task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}, "force": {"type": "boolean"}}, "required": ["task_id"]}},
            {"name": "run_replication_task", "description": "Run a replication task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // Cloud sync
            {"name": "get_cloudsync_task", "description": "Get a cloud sync task by ID", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "create_cloudsync_task", "description": "Create a new cloud sync task", "inputSchema": {"type": "object", "properties": {"task": {"type": "object"}}, "required": ["task"]}},
            {"name": "delete_cloudsync_task", "description": "Delete a cloud sync task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            {"name": "run_cloudsync_task", "description": "Run a cloud sync task", "inputSchema": {"type": "object", "properties": {"task_id": {"type": "integer"}}, "required": ["task_id"]}},
            // Interface
            {"name": "get_interface", "description": "Get a network interface by ID", "inputSchema": {"type": "object", "properties": {"interface_id": {"type": "string"}}, "required": ["interface_id"]}}
        ])
    }

    fn call_tool(
        &self,
        name: &str,
        arguments: &Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>> {
        let name = name.to_string();
        let arguments = arguments.clone();
        let self_ref = self.clone(); // Clone the Arc
        Box::pin(async move { self_ref.execute_tool(&name, &arguments).await })
    }

    async fn execute_tool(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        // Check tool access permissions
        self.check_tool_access(name)
            .map_err(|e| format!("Access denied: {}", e))?;

        match name {
            "list_users" => match self.tools.list_users().await {
                Ok(users) => Ok(json!(users)),
                Err(e) => Err(e.to_string()),
            },
            "get_user" => {
                let user_id = arguments["user_id"].as_i64().ok_or("Missing user_id")? as i32;
                match self.tools.get_user(user_id).await {
                    Ok(user) => Ok(json!(user)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_user_by_username" => {
                let username = arguments["username"].as_str().ok_or("Missing username")?;
                match self.tools.get_user_by_username(username).await {
                    Ok(user) => Ok(json!(user)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_pools" => match self.tools.list_pools().await {
                Ok(pools) => Ok(json!(pools)),
                Err(e) => Err(e.to_string()),
            },
            "get_pool_status" => {
                let pool_name = arguments["pool_name"].as_str().ok_or("Missing pool_name")?;
                match self.tools.get_pool_status(pool_name).await {
                    Ok(pool) => Ok(json!(pool)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_datasets" => match self.tools.list_datasets().await {
                Ok(datasets) => Ok(json!(datasets)),
                Err(e) => Err(e.to_string()),
            },
            "get_dataset" => {
                let dataset_path = arguments["dataset_path"]
                    .as_str()
                    .ok_or("Missing dataset_path")?;
                match self.tools.get_dataset(dataset_path).await {
                    Ok(dataset) => Ok(json!(dataset)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "create_dataset" => {
                let pool_name = arguments["pool_name"].as_str().ok_or("Missing pool_name")?;
                let dataset_name = arguments["dataset_name"]
                    .as_str()
                    .ok_or("Missing dataset_name")?;
                match self.tools.create_dataset(pool_name, dataset_name).await {
                    Ok(dataset) => Ok(json!(dataset)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_dataset" => {
                let dataset_path = arguments["dataset_path"]
                    .as_str()
                    .ok_or("Missing dataset_path")?;
                match self.tools.delete_dataset(dataset_path).await {
                    Ok(_) => Ok(json!({"status": "deleted", "path": dataset_path})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_smb_shares" => match self.tools.list_smb_shares().await {
                Ok(shares) => Ok(json!(shares)),
                Err(e) => Err(e.to_string()),
            },
            "create_smb_share" => {
                let name = arguments["name"].as_str().ok_or("Missing name")?;
                let path = arguments["path"].as_str().ok_or("Missing path")?;
                let comment = arguments["comment"].as_str();
                match self.tools.create_smb_share(name, path, comment).await {
                    Ok(share) => Ok(json!(share)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_smb_share" => {
                let share_id = arguments["share_id"].as_i64().ok_or("Missing share_id")? as i32;
                match self.tools.delete_smb_share(share_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": share_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_nfs_exports" => match self.tools.list_nfs_exports().await {
                Ok(exports) => Ok(json!(exports)),
                Err(e) => Err(e.to_string()),
            },
            "create_nfs_export" => {
                let paths_arr = arguments["paths"].as_array().ok_or("Missing paths")?;
                let paths: Vec<String> = paths_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                let comment = arguments["comment"].as_str().ok_or("Missing comment")?;
                match self
                    .tools
                    .create_nfs_export(paths, comment.to_string())
                    .await
                {
                    Ok(export) => Ok(json!(export)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_nfs_export" => {
                let export_id = arguments["export_id"].as_i64().ok_or("Missing export_id")? as i32;
                match self.tools.delete_nfs_export(export_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": export_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_snapshots" => match self.tools.list_snapshots().await {
                Ok(snapshots) => Ok(json!(snapshots)),
                Err(e) => Err(e.to_string()),
            },
            "create_snapshot" => {
                let dataset = arguments["dataset"].as_str().ok_or("Missing dataset")?;
                let snapshot_name = arguments["snapshot_name"]
                    .as_str()
                    .ok_or("Missing snapshot_name")?;
                match self.tools.create_snapshot(dataset, snapshot_name).await {
                    Ok(snapshot) => Ok(json!(snapshot)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_snapshot" => {
                let snapshot_id = arguments["snapshot_id"]
                    .as_str()
                    .ok_or("Missing snapshot_id")?;
                match self.tools.delete_snapshot(snapshot_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": snapshot_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_iscsi_targets" => match self.tools.list_iscsi_targets().await {
                Ok(targets) => Ok(json!(targets)),
                Err(e) => Err(e.to_string()),
            },
            "create_iscsi_target" => {
                let name = arguments["name"].as_str().ok_or("Missing name")?;
                match self.tools.create_iscsi_target(name).await {
                    Ok(target) => Ok(json!(target)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_iscsi_target" => {
                let target_id = arguments["target_id"].as_i64().ok_or("Missing target_id")? as i32;
                match self.tools.delete_iscsi_target(target_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": target_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_system_info" => match self.tools.get_system_info().await {
                Ok(info) => Ok(json!(info)),
                Err(e) => Err(e.to_string()),
            },
            "list_apps" => match self.tools.list_apps().await {
                Ok(apps) => Ok(json!(apps)),
                Err(e) => Err(e.to_string()),
            },
            "get_app" => {
                let app_name = arguments["app_name"].as_str().ok_or("Missing app_name")?;
                match self.tools.get_app(app_name).await {
                    Ok(app) => Ok(json!(app)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "start_app" => {
                let app_name = arguments["app_name"].as_str().ok_or("Missing app_name")?;
                let options = arguments.get("options").cloned();
                match self.tools.start_app(app_name, options).await {
                    Ok(app) => Ok(json!(app)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "stop_app" => {
                let app_name = arguments["app_name"].as_str().ok_or("Missing app_name")?;
                let force = arguments["force"].as_bool().unwrap_or(false);
                match self.tools.stop_app(app_name, force).await {
                    Ok(app) => Ok(json!(app)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "restart_app" => {
                let app_name = arguments["app_name"].as_str().ok_or("Missing app_name")?;
                match self.tools.restart_app(app_name).await {
                    Ok(app) => Ok(json!(app)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Snapshots
            "rollback_snapshot" => {
                let dataset = arguments["dataset"].as_str().ok_or("Missing dataset")?;
                let snapshot_name = arguments["snapshot_name"]
                    .as_str()
                    .ok_or("Missing snapshot_name")?;
                match self.tools.rollback_snapshot(dataset, snapshot_name).await {
                    Ok(_) => Ok(
                        json!({"status": "rolled_back", "dataset": dataset, "snapshot": snapshot_name}),
                    ),
                    Err(e) => Err(e.to_string()),
                }
            }
            "clone_snapshot" => {
                let snapshot_id = arguments["snapshot_id"]
                    .as_str()
                    .ok_or("Missing snapshot_id")?;
                let target_name = arguments["target_name"]
                    .as_str()
                    .ok_or("Missing target_name")?;
                match self.tools.clone_snapshot(snapshot_id, target_name).await {
                    Ok(dataset) => Ok(json!(dataset)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_dataset_snapshots" => {
                let dataset = arguments["dataset"].as_str().ok_or("Missing dataset")?;
                match self.tools.get_dataset_snapshots(dataset).await {
                    Ok(snapshots) => Ok(json!(snapshots)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Datasets
            "update_dataset" => {
                let dataset_path = arguments["dataset_path"]
                    .as_str()
                    .ok_or("Missing dataset_path")?;
                let updates = arguments["updates"].clone();
                match self.tools.update_dataset(dataset_path, updates).await {
                    Ok(dataset) => Ok(json!(dataset)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // SMART
            "get_smart_status" => {
                let disk_name = arguments["disk_name"].as_str().ok_or("Missing disk_name")?;
                match self.tools.get_smart_status(disk_name).await {
                    Ok(status) => Ok(json!(status)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Alerts
            "get_alerts" => match self.tools.get_alerts().await {
                Ok(alerts) => Ok(json!(alerts)),
                Err(e) => Err(e.to_string()),
            },
            "get_alert_classes" => match self.tools.get_alert_classes().await {
                Ok(classes) => Ok(json!(classes)),
                Err(e) => Err(e.to_string()),
            },
            "dismiss_alert" => {
                let alert_id = arguments["alert_id"].as_str().ok_or("Missing alert_id")?;
                match self.tools.dismiss_alert(alert_id).await {
                    Ok(_) => Ok(json!({"status": "dismissed", "alert_id": alert_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "clear_alerts" => match self.tools.clear_all_alerts().await {
                Ok(_) => Ok(json!({"status": "cleared"})),
                Err(e) => Err(e.to_string()),
            },
            "get_system_events" => {
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                match self.tools.get_system_events(limit).await {
                    Ok(events) => Ok(json!(events)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_disk_health" => match self.tools.get_disk_health().await {
                Ok(disk) => Ok(json!(disk)),
                Err(e) => Err(e.to_string()),
            },
            // System
            "reboot_system" => {
                let confirm = arguments["confirm"].as_bool().ok_or("Missing confirm")?;
                let delay = arguments
                    .get("delay_seconds")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                match self.tools.reboot_system(confirm, delay).await {
                    Ok(_) => Ok(json!({"status": "rebooting"})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "shutdown_system" => {
                let confirm = arguments["confirm"].as_bool().ok_or("Missing confirm")?;
                let delay = arguments
                    .get("delay_seconds")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                match self.tools.shutdown_system(confirm, delay).await {
                    Ok(_) => Ok(json!({"status": "shutting_down"})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "check_for_updates" => match self.tools.check_for_updates().await {
                Ok(update) => Ok(json!(update)),
                Err(e) => Err(e.to_string()),
            },
            // Services
            "start_service" => {
                let service_id = arguments["service_id"]
                    .as_i64()
                    .ok_or("Missing service_id")? as i32;
                match self.tools.start_service(service_id).await {
                    Ok(service) => Ok(json!(service)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "stop_service" => {
                let service_id = arguments["service_id"]
                    .as_i64()
                    .ok_or("Missing service_id")? as i32;
                match self.tools.stop_service(service_id).await {
                    Ok(service) => Ok(json!(service)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "restart_service" => {
                let service_id = arguments["service_id"]
                    .as_i64()
                    .ok_or("Missing service_id")? as i32;
                match self.tools.restart_service(service_id).await {
                    Ok(service) => Ok(json!(service)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Batch operations
            "batch" => {
                let operations = arguments["operations"]
                    .as_array()
                    .ok_or("Missing operations array")?;
                let empty_args = serde_json::json!({});
                let mut results = Vec::new();
                for op in operations {
                    let op_name = op
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or("Missing operation name")?;
                    let op_args = op.get("arguments").unwrap_or(&empty_args);
                    match self.call_tool(op_name, op_args).await {
                        Ok(result) => results
                            .push(json!({"name": op_name, "success": true, "result": result})),
                        Err(e) => {
                            results.push(json!({"name": op_name, "success": false, "error": e}))
                        }
                    }
                }
                Ok(json!({"results": results}))
            }
            // Task status
            "get_task_status" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.get_task_status(task_id).await {
                    Ok(task) => Ok(json!(task)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "list_tasks" => match self.tools.list_tasks().await {
                Ok(tasks) => Ok(json!(tasks)),
                Err(e) => Err(e.to_string()),
            },
            "abort_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.abort_task(task_id).await {
                    Ok(_) => Ok(json!({"status": "aborted", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Kubernetes
            "get_kubernetes_status" => match self.tools.get_kubernetes_status().await {
                Ok(status) => Ok(json!(status)),
                Err(e) => Err(e.to_string()),
            },
            "get_kubernetes_nodes" => match self.tools.get_kubernetes_nodes().await {
                Ok(nodes) => Ok(json!(nodes)),
                Err(e) => Err(e.to_string()),
            },
            "get_kubernetes_pods" => match self.tools.get_kubernetes_pods().await {
                Ok(pods) => Ok(json!(pods)),
                Err(e) => Err(e.to_string()),
            },
            "get_kubernetes_services" => match self.tools.get_kubernetes_services().await {
                Ok(services) => Ok(json!(services)),
                Err(e) => Err(e.to_string()),
            },
            // Docker images
            "list_docker_images" => match self.tools.list_docker_images().await {
                Ok(images) => Ok(json!(images)),
                Err(e) => Err(e.to_string()),
            },
            "pull_docker_image" => {
                let image = arguments["image"].as_str().ok_or("Missing image")?;
                let tag = arguments
                    .get("tag")
                    .and_then(|t| t.as_str())
                    .unwrap_or("latest");
                match self.tools.pull_docker_image(image, tag).await {
                    Ok(_) => Ok(json!({"status": "pulling", "image": image, "tag": tag})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Cloud credentials
            "create_cloud_credential" => {
                let name = arguments["name"].as_str().ok_or("Missing name")?;
                let provider = arguments["provider"].as_str().ok_or("Missing provider")?;
                let attributes = arguments["attributes"].clone();
                match self
                    .tools
                    .create_cloud_credential(name, provider, attributes)
                    .await
                {
                    Ok(cred) => Ok(json!(cred)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_cloud_credential" => {
                let cred_id = arguments["cred_id"].as_i64().ok_or("Missing cred_id")? as i32;
                match self.tools.delete_cloud_credential(cred_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": cred_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Rsync
            "list_rsync_tasks" => match self.tools.list_rsync_tasks().await {
                Ok(tasks) => Ok(json!(tasks)),
                Err(e) => Err(e.to_string()),
            },
            "create_rsync_task" => {
                let task = arguments["task"].clone();
                match self.tools.create_rsync_task(task).await {
                    Ok(task_result) => Ok(json!(task_result)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "run_rsync_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.run_rsync_task(task_id).await {
                    Ok(_) => Ok(json!({"status": "running", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // SSH
            "list_ssh_connections" => match self.tools.list_ssh_connections().await {
                Ok(connections) => Ok(json!(connections)),
                Err(e) => Err(e.to_string()),
            },
            "create_ssh_connection" => {
                let connection = arguments["connection"].clone();
                match self.tools.create_ssh_connection(connection).await {
                    Ok(conn) => Ok(json!(conn)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_ssh_connection" => {
                let conn_id = arguments["connection_id"]
                    .as_i64()
                    .ok_or("Missing connection_id")? as i32;
                match self.tools.delete_ssh_connection(conn_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": conn_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // TFTP
            "list_tftp_services" => match self.tools.list_tftp_services().await {
                Ok(services) => Ok(json!(services)),
                Err(e) => Err(e.to_string()),
            },
            // SMART tests
            "list_smart_tests" => match self.tools.list_smart_tests().await {
                Ok(tests) => Ok(json!(tests)),
                Err(e) => Err(e.to_string()),
            },
            "create_smart_test" => {
                let test = arguments["test"].clone();
                match self.tools.create_smart_test(test).await {
                    Ok(test_result) => Ok(json!(test_result)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_smart_test" => {
                let test_id = arguments["test_id"].as_i64().ok_or("Missing test_id")? as i32;
                match self.tools.delete_smart_test(test_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "id": test_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_smart_config" => match self.tools.get_smart_config().await {
                Ok(config) => Ok(json!(config)),
                Err(e) => Err(e.to_string()),
            },
            // Update
            "update_system" => {
                let updates = arguments.get("updates").cloned();
                match self.tools.update_system(updates).await {
                    Ok(result) => Ok(json!(result)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "get_general_config" => match self.tools.get_general_config().await {
                Ok(config) => Ok(json!(config)),
                Err(e) => Err(e.to_string()),
            },
            "get_support" => match self.tools.get_support().await {
                Ok(support) => Ok(json!(support)),
                Err(e) => Err(e.to_string()),
            },
            "get_enclosure" => match self.tools.get_enclosure().await {
                Ok(enclosure) => Ok(json!(enclosure)),
                Err(e) => Err(e.to_string()),
            },
            // VM clone
            "clone_vm" => {
                let vm_id = arguments["vm_id"].as_i64().ok_or("Missing vm_id")? as i32;
                let name = arguments["name"].as_str().ok_or("Missing name")?;
                match self.tools.clone_vm(vm_id, name).await {
                    Ok(vm) => Ok(json!(vm)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // VM powercycle
            "powercycle_vm" => {
                let vm_id = arguments["vm_id"].as_i64().ok_or("Missing vm_id")? as i32;
                match self.tools.powercycle_vm(vm_id).await {
                    Ok(vm) => Ok(json!(vm)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Catalog refresh
            "refresh_catalogs" => match self.tools.refresh_catalogs().await {
                Ok(_) => Ok(json!({"status": "refreshing"})),
                Err(e) => Err(e.to_string()),
            },
            // Pool scrub
            "scrub_pool" => {
                let pool_name = arguments["pool_name"].as_str().ok_or("Missing pool_name")?;
                match self.tools.scrub_pool(pool_name).await {
                    Ok(_) => Ok(json!({"status": "scrub_started", "pool": pool_name})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Dataset by path
            "get_dataset_by_path" => {
                let path = arguments["path"].as_str().ok_or("Missing path")?;
                match self.tools.get_dataset_by_path(path).await {
                    Ok(dataset) => Ok(json!(dataset)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Certificate
            "get_certificate" => {
                let cert_id = arguments["cert_id"].as_i64().ok_or("Missing cert_id")? as i32;
                match self.tools.get_certificate(cert_id).await {
                    Ok(cert) => Ok(json!(cert)),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Replication
            "get_replication_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.get_replication_task(task_id).await {
                    Ok(task) => Ok(json!(task)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "create_replication_task" => {
                let task = arguments["task"].clone();
                match self.tools.create_replication_task_json(task).await {
                    Ok(task_result) => Ok(json!(task_result)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_replication_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                let force = arguments
                    .get("force")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(false);
                match self.tools.delete_replication_task(task_id, force).await {
                    Ok(_) => Ok(json!({"status": "deleted", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "run_replication_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.run_replication_task(task_id).await {
                    Ok(_) => Ok(json!({"status": "running", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Cloud sync
            "get_cloudsync_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.get_cloudsync_task(task_id).await {
                    Ok(task) => Ok(json!(task)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "create_cloudsync_task" => {
                let task = arguments["task"].clone();
                match self.tools.create_cloudsync_task_json(task).await {
                    Ok(task_result) => Ok(json!(task_result)),
                    Err(e) => Err(e.to_string()),
                }
            }
            "delete_cloudsync_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.delete_cloudsync_task(task_id).await {
                    Ok(_) => Ok(json!({"status": "deleted", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            "run_cloudsync_task" => {
                let task_id = arguments["task_id"].as_i64().ok_or("Missing task_id")? as i32;
                match self.tools.run_cloudsync_task(task_id).await {
                    Ok(_) => Ok(json!({"status": "running", "task_id": task_id})),
                    Err(e) => Err(e.to_string()),
                }
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}

/// Run server with stdio transport
async fn run_stdio(server: Arc<TrueNasServerImpl>) -> anyhow::Result<()> {
    info!("Starting TrueNAS MCP Server on stdio");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;

        // Check for EOF (read_line returns Ok(0) when EOF is reached)
        if bytes_read == 0 {
            break;
        }

        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // Parse error - send JSON-RPC error response instead of crashing
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                }).to_string();
                let mut response_line = response;
                response_line.push('\n');
                if let Err(write_err) = writer.write_all(response_line.as_bytes()).await {
                    tracing::error!("Failed to write parse error response: {}", write_err);
                }
                let _ = writer.flush().await;
                continue;
            }
        };

        match handle_request(&server, request).await {
            Ok(Some(response)) => {
                let mut response_line = response;
                response_line.push('\n');
                writer.write_all(response_line.as_bytes()).await?;
                writer.flush().await?;
            }
            Ok(None) => {
                // It was a notification, no response required
            }
            Err(e) => {
                tracing::error!("Internal error handling request: {}", e);
            }
        }
    }

    Ok(())
}

/// Handle MCP JSON-RPC request
async fn handle_request(server: &TrueNasServerImpl, request: Value) -> anyhow::Result<Option<String>> {
    let method = match request["method"].as_str() {
        Some(m) => m,
        None => {
            let id = request.get("id").cloned();
            if id.is_some() {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request: Missing method"
                    }
                }).to_string();
                return Ok(Some(err_resp));
            }
            return Ok(None);
        }
    };

    let id = request.get("id").cloned();
    let is_notification = id.is_none();

    let result = match method {
        "initialize" => {
            let capabilities = json!({
                "tools": {},
                "resources": {
                    "list": true,
                    "read": true
                },
                "prompts": {
                    "list": true,
                    "get": true
                }
            });
            Ok(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": server.get_server_info(),
                "capabilities": capabilities
            }))
        }
        "notifications/initialized" => {
            // Standard MCP notification, safely ignore
            return Ok(None);
        }
        "ping" => {
            Ok(json!({}))
        }
        "tools/list" => {
            Ok(json!({
                "tools": server.list_tools()
            }))
        }
        "tools/call" => {
            let get_result = async {
                let params = request.get("params").ok_or_else(|| anyhow::anyhow!("Missing params"))?;
                let name = params["name"].as_str().ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;
                let empty_args = json!({});
                let arguments = params.get("arguments").unwrap_or(&empty_args);
                server
                    .call_tool(name, arguments)
                    .await
                    .map_err(|e| anyhow::anyhow!("Tool error: {}", e))
            }.await;
            get_result
        }
        "resources/list" => {
            Ok(server.list_resources())
        }
        "resources/read" => {
            let get_result = async {
                let params = request.get("params").ok_or_else(|| anyhow::anyhow!("Missing params"))?;
                let uri = params["uri"].as_str().ok_or_else(|| anyhow::anyhow!("Missing resource URI"))?;
                server
                    .read_resource(uri)
                    .await
                    .map_err(|e| anyhow::anyhow!("Resource error: {}", e))
            }.await;
            get_result
        }
        "prompts/list" => {
            Ok(server.list_prompts())
        }
        "prompts/get" => {
            let get_result = async {
                let params = request.get("params").ok_or_else(|| anyhow::anyhow!("Missing params"))?;
                let name = params["name"].as_str().ok_or_else(|| anyhow::anyhow!("Missing prompt name"))?;
                let arguments = params.get("arguments");
                server
                    .get_prompt(name, arguments)
                    .await
                    .map_err(|e| anyhow::anyhow!("Prompt error: {}", e))
            }.await;
            get_result
        }
        _ => {
            if is_notification {
                return Ok(None);
            } else {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                }).to_string();
                return Ok(Some(err_resp));
            }
        }
    };

    match result {
        Ok(res_val) => {
            if is_notification {
                Ok(None)
            } else {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": res_val
                }).to_string();
                Ok(Some(resp))
            }
        }
        Err(e) => {
            if is_notification {
                tracing::warn!("Error processing notification '{}': {}", method, e);
                Ok(None)
            } else {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32603,
                        "message": format!("Internal error: {}", e)
                    }
                }).to_string();
                Ok(Some(err_resp))
            }
        }
    }
}

/// Build CORS layer based on environment configuration
fn build_cors_layer() -> tower_http::cors::CorsLayer {
    let allowed_origins: Vec<String> = std::env::var("TRUENAS_MCP_ALLOWED_ORIGINS")
        .ok()
        .map(|s| s.split(',').map(|strim| strim.trim().to_string()).collect())
        .unwrap_or_default();

    if allowed_origins.is_empty() {
        // Default: permissive for development, but log a warning
        tracing::warn!(
            "CORS is permissive (no TRUENAS_MCP_ALLOWED_ORIGINS set). Consider setting allowed origins for production."
        );
        tower_http::cors::CorsLayer::permissive()
    } else {
        let cors = tower_http::cors::CorsLayer::new()
            .allow_methods([http::Method::GET, http::Method::POST, http::Method::OPTIONS])
            .allow_headers([http::header::CONTENT_TYPE, http::header::AUTHORIZATION]);

        // Configure allowed origins
        use tower_http::cors::AllowOrigin;
        let allow_origin: AllowOrigin = if allowed_origins.contains(&"*".to_string()) {
            AllowOrigin::any()
        } else {
            let origins: Vec<http::HeaderValue> = allowed_origins
                .iter()
                .filter_map(|origin| origin.parse().ok())
                .collect();
            AllowOrigin::list(origins)
        };

        cors.allow_origin(allow_origin)
    }
}

/// Run server with HTTP transport
async fn run_http(server: Arc<TrueNasServerImpl>, host: &str, port: u16) -> anyhow::Result<()> {
    info!("Starting TrueNAS MCP Server on HTTP {}:{}", host, port);

    let server = std::sync::Arc::new(server);

    // Build Axum app with CORS
    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(|| async {
                axum::Json(json!({"status": "TrueNAS MCP Server running"}))
            }),
        )
        .route("/mcp", axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let server = server.clone();
            async move {
                let response = handle_request(&server, request).await
                    .map_err(|e| tracing::error!("Request error: {}", e));
                match response {
                    Ok(Some(r)) => axum::Json(serde_json::from_str::<Value>(&r).unwrap_or_else(|_| json!({"error": "Invalid response"}))),
                    Ok(None) => axum::Json(json!({"jsonrpc": "2.0", "result": null})),
                    Err(_) => axum::Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32603, "message": "Internal error"}})),
                }
            }
        }))
        .layer(build_cors_layer());

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;

    info!("TrueNAS HTTP MCP server listening on {}", addr);

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}

/// Run server with SSE transport
async fn run_sse(server: Arc<TrueNasServerImpl>, host: &str, port: u16) -> anyhow::Result<()> {
    info!("Starting TrueNAS MCP Server with SSE on {}:{}", host, port);

    let server = std::sync::Arc::new(server);

    // Build Axum app with SSE support and CORS
    let app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(|| async {
                axum::Json(json!({"status": "TrueNAS MCP Server running", "endpoints": ["/sse", "/messages"]}))
            }),
        )
        .route("/sse", axum::routing::get(sse_handler))
        .route("/messages", axum::routing::post(move |axum::Json(request): axum::Json<Value>| {
            let server = server.clone();
            async move {
                let response = handle_request(&server, request).await
                    .map_err(|e| tracing::error!("Request error: {}", e));
                match response {
                    Ok(Some(r)) => axum::Json(serde_json::from_str::<Value>(&r).unwrap_or_else(|_| json!({"error": "Invalid response"}))),
                    Ok(None) => axum::Json(json!({"jsonrpc": "2.0", "result": null})),
                    Err(_) => axum::Json(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32603, "message": "Internal error"}})),
                }
            }
        }))
        .layer(build_cors_layer());

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;

    info!("TrueNAS SSE MCP server listening on {}", addr);

    axum::serve(listener, app)
        .await
        .context("SSE server error")?;

    Ok(())
}

/// SSE endpoint handler
async fn sse_handler() -> impl axum::response::IntoResponse {
    use axum::response::sse::{Event, Sse};
    use futures_util::stream;

    let stream = stream::once(async move {
        Ok::<_, std::convert::Infallible>(Event::default().data("MCP SSE connection established"))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}
