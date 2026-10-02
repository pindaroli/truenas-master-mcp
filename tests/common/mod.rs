#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

pub mod mcp_client;
pub mod mock_truenas;

use std::sync::Arc;
use truenas_master_mcp::config::TrueNasConfig;
use truenas_master_mcp::server::ToolConfig;
use truenas_master_mcp::service::TrueNasServerImpl;

/// Build a standard test configuration pointing to a mock TrueNAS server.
pub fn config_for_mock(server_url: &str) -> TrueNasConfig {
    TrueNasConfig {
        server_url: server_url.to_string(),
        api_key: Some("test-api-key-12345".to_string()),
        username: None,
        password: None,
        verify_ssl: false,
        timeout_secs: 5,
        version: Default::default(),
    }
}

/// ToolConfig with All categories enabled
pub fn all_enabled_tool_config() -> ToolConfig {
    ToolConfig {
        readonly: false,
        enabled_categories: vec![truenas_master_mcp::server::ToolCategory::All],
        disabled_categories: vec![],
    }
}

/// Create an Arc<TrueNasServerImpl> configured for the given mock and tool config.
pub fn create_test_server(server_url: &str, mut tool_config: ToolConfig) -> Arc<TrueNasServerImpl> {
    if tool_config.enabled_categories.is_empty() && tool_config.disabled_categories.is_empty() {
        tool_config.enabled_categories = vec![truenas_master_mcp::server::ToolCategory::All];
    }
    let cfg = config_for_mock(server_url);
    Arc::new(TrueNasServerImpl::new(cfg, tool_config).unwrap())
}
