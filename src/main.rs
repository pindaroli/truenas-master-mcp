#![recursion_limit = "512"]

// Use library modules
use truenas_master_mcp::service::{TrueNasServerImpl, run_http, run_sse, run_stdio};

// Re-export ToolCategory and ToolConfig from library
pub use truenas_master_mcp::server::ToolCategory;
pub use truenas_master_mcp::server::ToolConfig;

use anyhow::Context;
use clap::Parser;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use truenas_master_mcp::config::TrueNasConfig;

/// TrueNAS MCP Server
#[derive(Parser, Debug)]
#[command(name = "truenas-master-mcp")]
#[command(version)]
#[command(about = "Official MCP server for TrueNAS API access", long_about = None)]
struct Args {
    /// Transport type to use: stdio, http, or sse
    #[arg(short, long, default_value = "stdio")]
    transport: String,

    /// Host to bind to (for http/sse transports)
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
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
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false),
        )
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
