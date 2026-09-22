//! # TrueNAS Master MCP Server
//!
//! An MCP server for TrueNAS API access.
//! Calls use JSON-RPC 2.0 over the WebSocket at `/api/current`.
//! Historical `/api/v2.0` paths are translated inside the client so tool names stay.
//!
//! ## Features
//!
//! - **User Management**: List, create, update, and delete users and groups
//! - **Pool Management**: View storage pools and their status
//! - **Dataset Management**: Create, list, and delete ZFS datasets
//! - **Share Management**: Manage SMB and NFS shares
//! - **Snapshot Management**: Create and manage ZFS snapshots
//! - **iSCSI Management**: Configure iSCSI targets
//! - **App Management**: Deploy and manage applications (SCALE) or jails (CORE)
//! - **System Monitoring**: View system info, alerts, and updates
//! - **Network Management**: Configure interfaces, routes, and DNS
//! - **Service Management**: Start, stop, and restart services
//!
//! ## Usage
//!
//! ```bash
//! # Set environment variables
//! export TRUENAS_SERVER_URL="https://truenas.local"
//! export TRUENAS_API_KEY="your-api-key"
//!
//! # Run with stdio transport (default)
//! cargo run
//!
//! # Run with HTTP transport
//! cargo run -- --transport http --port 3000
//!
//! # Run in readonly mode
//! cargo run -- --readonly
//! ```
//!
//! ## Environment Variables
//!
//! | Variable | Required | Description |
//! |----------|----------|-------------|
//! | `TRUENAS_SERVER_URL` | Yes | TrueNAS server URL |
//! | `TRUENAS_API_KEY` | No* | API key for authentication |
//! | `TRUENAS_USERNAME` | No* | Username for basic auth |
//! | `TRUENAS_PASSWORD` | No* | Password for basic auth |
//! | `TRUENAS_VERIFY_SSL` | No | Verify SSL (default: true) |
//! | `TRUENAS_TIMEOUT` | No | Timeout in seconds (default: 30) |
//! | `TRUENAS_VERSION` | No | TrueNAS version (scale/core, default: scale) |
//! | `TRUENAS_READONLY` | No | Enable readonly mode |
//!
//! *Either API key OR username/password required

pub mod api_client;
pub mod cache;
pub mod client;
pub mod config;
pub mod error;
pub mod rest_map;
mod rpc;
pub mod server;
pub mod tools;

// Re-export commonly used types
pub use crate::config::TrueNasConfig;
pub use crate::error::{Result, TrueNasError};
pub use crate::tools::TrueNasTools;
