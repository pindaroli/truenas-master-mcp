# TrueNAS Master MCP

A Model Context Protocol (MCP) server for managing TrueNAS systems through AI assistants.

## Features

This MCP server provides access to TrueNAS API functionality including:

- **User Management**: List, get, create, update, delete users and groups
- **Pool Management**: List pools, get pool status, scrub pools
- **Dataset Management**: List, create, get, delete datasets
- **SMB Shares**: List, create, delete SMB shares
- **NFS Exports**: List, create, delete NFS exports
- **ZFS Snapshots**: List, create, delete, rollback snapshots
- **iSCSI Targets**: List, create, delete iSCSI targets
- **System Monitoring**: Get alerts, events, disk health, system logs
- **VM Management**: List, start, stop, restart, create, clone VMs
- **Network Management**: List interfaces, routes, DNS configuration
- **Service Management**: List, start, stop, restart services
- **Disk Management**: List disks, get disk details, SMART status
- **Certificate Management**: List and get certificates
- **Replication**: List, create, run replication tasks
- **Cloud Sync**: List, create, run cloud sync tasks
- **Docker**: List and pull images used by SCALE apps
- **Task Management**: List, get status, abort running tasks
- **Apps**: Full Docker app lifecycle on TrueNAS SCALE
- **Caching**: Built-in caching for frequently accessed data

## Installation

### Prerequisites

- Rust 1.85 or later
- TrueNAS SCALE or TrueNAS CORE with API access
- API key or username/password for authentication

### Build from source

```bash
# Clone the repository
git clone https://github.com/hongkongkiwi/truenas-master-mcp.git
cd truenas-master-mcp

# Build the project
cargo build --release

# The binary will be at target/release/truenas-master-mcp
```

## Configuration

The server is configured via environment variables:

| Variable | Required | Description | Default |
|----------|----------|-------------|---------|
| `TRUENAS_SERVER_URL` | Yes | TrueNAS server URL | `http://localhost` |
| `TRUENAS_API_KEY` | No* | API key for authentication | - |
| `TRUENAS_USERNAME` | No* | Username for basic auth | - |
| `TRUENAS_PASSWORD` | No* | Password for basic auth | - |
| `TRUENAS_VERIFY_SSL` | No | Verify SSL certificates | `true` |
| `TRUENAS_TIMEOUT` | No | Request timeout in seconds | `30` |
| `TRUENAS_VERSION` | No | TrueNAS version (scale/core) | `scale` |

*Either `TRUENAS_API_KEY` OR both `TRUENAS_USERNAME` and `TRUENAS_PASSWORD` must be provided.

### CLI Options

The server also supports command-line options:

| Option | Description | Default |
|--------|-------------|---------|
| `-t, --transport` | Transport type: stdio, http, sse | stdio |
| `-h, --host` | Host to bind to (http/sse only) | 127.0.0.1 |
| `-p, --port` | Port to bind to (http/sse only) | 3000 |
| `--truenas-version` | TrueNAS version: scale or core | scale |
| `--readonly` | Enable readonly mode | false |
| `-c, --config-file` | Path to config file (JSON/YAML) | - |
| `-l, --log-level` | Log level: debug, info, warn, error | info |
| `-v, --verbose` | Enable verbose output | false |
| `--insecure-ssl` | Disable SSL certificate verification | false |
| `--timeout` | API request timeout in seconds | 30 |
| `--pretty` | Enable JSON pretty printing | false |

### Example .env file

```bash
# Using API key
TRUENAS_SERVER_URL=https://truenas.local
TRUENAS_API_KEY=your-api-key-here

# OR using username/password
TRUENAS_SERVER_URL=https://truenas.local
TRUENAS_USERNAME=admin
TRUENAS_PASSWORD=your-password

# Optional settings
TRUENAS_VERIFY_SSL=true
TRUENAS_TIMEOUT=30
```

## Usage

### Running the server

```bash
# Load environment variables from .env file
export $(cat .env | xargs)

# Run the server
cargo run
```

The server communicates via stdio, which is the standard for MCP servers.

### Using with Claude Desktop

Add to your Claude Desktop config file:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "truenas": {
      "command": "/path/to/truenas-master-mcp",
      "env": {
        "TRUENAS_SERVER_URL": "https://truenas.local",
        "TRUENAS_API_KEY": "your-api-key-here"
      }
    }
  }
}
```

### Using with other MCP clients

The server implements the standard Model Context Protocol. Refer to your MCP client's documentation for how to connect servers.

### Example Usage

Once connected to an MCP client like Claude Desktop, you can manage your TrueNAS system:

```bash
# List all storage pools
list_pools()

# Get details of a specific dataset
get_dataset(dataset_id_or_path="tank/data/mydata")

# Create a new user
create_user(username="newuser", email="user@example.com", full_name="New User")

# List all VMs and start one
list_vms()
start_vm(vm_id=1)

# Check system alerts
get_alerts()
```

## AI Usage Examples

This section provides examples for AI assistants on how to effectively use the TrueNAS MCP tools.

### Monitoring System Health

```python
# Get comprehensive system overview
system_info = get_system_info()
alerts = get_alerts()
pools = list_pools()
disk_health = get_disk_health()

# Check for critical issues
if alerts and len(alerts) > 0:
    critical_alerts = [a for a in alerts if a.get("severity") == "CRITICAL"]
    if critical_alerts:
        # Take action on critical alerts
        for alert in critical_alerts:
            print(f"Alert: {alert['message']}")
```

### Managing Storage

```python
# Find datasets with low space
datasets = list_datasets()
for ds in datasets:
    if ds.get("available", 0) < ds.get("size", 0) * 0.1:
        print(f"Low space: {ds['name']} ({ds['available']} remaining)")

# Create a new dataset for backups
create_dataset(pool_name="tank", dataset_name="backups")

# Take a snapshot before major changes
create_snapshot(dataset="tank/data", snapshot_name="pre-update-$(date +%Y%m%d)")
```

### Managing Applications (SCALE)

```python
# Check app status and health
apps = list_apps()
for app in apps:
    if app.get("state") != "RUNNING":
        print(f"App not running: {app['name']} - {app.get('error', 'Unknown error')}")

# Upgrade an app with available update
upgrade_app(app_name="my-app", upgrade_version="1.2.3")
```

### Managing Virtual Machines

```python
# Find and stop unused VMs
vms = list_vms()
for vm in vms:
    if vm.get("state") == "RUNNING" and vm.get("uptime", 0) > 86400 * 7:
        # VM has been running for more than 7 days
        print(f"Consider restarting: {vm['name']}")

# Clone a VM before testing
clone_vm(vm_id=1, name="vm-test-clone")
```

### Handling Alerts

```python
# Get and categorize alerts
alerts = get_alerts()
for alert in alerts:
    severity = alert.get("severity", "INFO")
    source = alert.get("source", "unknown")
    message = alert.get("message", "")

    if severity == "CRITICAL":
        # Immediately notify and potentially take action
        print(f"CRITICAL: {message}")
        if "disk" in source.lower():
            # Check disk health
            disk = get_disk(disk_name=extract_disk_name(message))
            print(f"Disk SMART status: {disk.get('smart_status')}")

# Dismiss resolved alerts
dismiss_alert(alert_id="alert-123")

# Clear all old alerts
clear_alerts()
```

### Batch Operations

```python
# Execute multiple operations in sequence
operations = [
    {"name": "list_pools", "arguments": {}},
    {"name": "list_apps", "arguments": {}},
    {"name": "get_alerts", "arguments": {}},
]
results = batch(operations=operations)
```

### HTTP/SSE Mode

For remote access or web-based MCP clients:

```bash
# Run in HTTP/SSE mode on port 8080
TRUENAS_SERVER_URL=https://truenas.local TRUENAS_API_KEY=your-key \
  truenas-master-mcp --transport=sse --port=8080
```

The server will be available at:
- SSE stream: `http://localhost:8080/sse`
- POST messages: `http://localhost:8080/messages`

## Available Tools

### User Management
- `list_users` - List all users
- `get_user` - Get user by ID
- `get_user_by_username` - Get user by username

### Pool Management
- `list_pools` - List all storage pools
- `get_pool_status` - Get status of a specific pool
- `scrub_pool` - Start a scrub on a storage pool

### Dataset Management
- `list_datasets` - List all datasets
- `get_dataset` - Get details of a specific dataset
- `create_dataset` - Create a new dataset
- `delete_dataset` - Delete a dataset

### SMB Shares
- `list_smb_shares` - List all SMB shares
- `create_smb_share` - Create a new SMB share
- `delete_smb_share` - Delete an SMB share

### NFS Exports
- `list_nfs_exports` - List all NFS exports
- `create_nfs_export` - Create a new NFS export
- `delete_nfs_export` - Delete an NFS export

### Snapshots
- `list_snapshots` - List all ZFS snapshots
- `create_snapshot` - Create a new snapshot
- `delete_snapshot` - Delete a snapshot

### iSCSI Targets
- `list_iscsi_targets` - List all iSCSI targets
- `create_iscsi_target` - Create a new iSCSI target
- `delete_iscsi_target` - Delete an iSCSI target

### Application Management (SCALE)
- `list_apps` - List all applications
- `get_app` - Get details of a specific application
- `start_app` - Start an application
- `stop_app` - Stop an application
- `restart_app` - Restart an application
- `create_app` - Create a new application from catalog
- `update_app` - Update an application configuration
- `delete_app` - Delete an application
- `rollback_app` - Rollback an application to previous version
- `get_app_config` - Get application configuration
- `get_app_upgrade_options` - Get available upgrade options
- `upgrade_app` - Upgrade an application

### Catalogs
- `list_catalog_items` - List available catalog items
- `get_catalog` - Get catalog details
- `get_catalog_trains` - Get catalog train versions
- `get_catalog_item` - Get specific catalog item details

### System Monitoring
- `get_system_info` - Get system information
- `get_alerts` - Get system alerts
- `get_alert_classes` - Get alert classes/categories
- `dismiss_alert` - Dismiss a specific alert
- `clear_alerts` - Clear all system alerts
- `get_system_events` - Get recent system events/logs
- `get_disk_health` - Get disk health/SMART status
- `check_for_updates` - Check for system updates
- `reboot_system` - Reboot the system (requires confirm=true)
- `shutdown_system` - Shutdown the system (requires confirm=true)

### Group Management
- `list_groups` - List all groups
- `get_group` - Get group by ID
- `get_group_by_name` - Get group by name
- `create_group` - Create a new group
- `delete_group` - Delete a group

### VM Management
- `list_vms` - List all virtual machines
- `get_vm` - Get VM details by ID
- `create_vm` - Create a new virtual machine
- `update_vm` - Update VM configuration
- `start_vm` - Start a virtual machine
- `stop_vm` - Stop a virtual machine
- `restart_vm` - Restart a virtual machine
- `powercycle_vm` - Power cycle (hard reset) a virtual machine
- `clone_vm` - Clone an existing virtual machine
- `delete_vm` - Delete a virtual machine

### Network Management
- `list_interfaces` - List all network interfaces
- `list_routes` - List all network routes
- `get_dns` - Get DNS configuration

### Service Management
- `list_services` - List all services
- `get_service` - Get service details
- `start_service` - Start a service
- `stop_service` - Stop a service
- `restart_service` - Restart a service

### Disk Management
- `list_disks` - List all disks
- `get_disk` - Get disk details by name

### Certificate Management
- `list_certificates` - List all certificates
- `get_certificate` - Get certificate details by ID

### Replication
- `list_replication_tasks` - List all replication tasks
- `get_replication_task` - Get replication task by ID
- `create_replication_task` - Create a new replication task
- `delete_replication_task` - Delete a replication task
- `run_replication_task` - Run a replication task

### Cloud Sync
- `list_cloudsync_tasks` - List all cloud sync tasks
- `get_cloudsync_task` - Get cloud sync task by ID
- `create_cloudsync_task` - Create a new cloud sync task
- `delete_cloudsync_task` - Delete a cloud sync task
- `run_cloudsync_task` - Run a cloud sync task

### Task Management
- `list_tasks` - List all running and recent tasks
- `get_task_status` - Get status of a specific task
- `abort_task` - Abort a running task

### Docker (SCALE)
- `list_docker_images` - List all Docker images
- `pull_docker_image` - Pull a Docker image

### Batch Operations
- `batch` - Execute multiple operations in a single call

### Other
- `get_enclosure` - Get enclosure information
- `get_support` - Get support information

## Development

### Running tests

```bash
cargo test
```

### Checking code

```bash
cargo fmt
cargo clippy
```

### Building for release

```bash
cargo build --release
```

### Docker

A Dockerfile is provided for containerized deployment:

```bash
# Build the image
docker build -t truenas-master-mcp .

# Run the container
docker run -d \
  -p 3000:3000 \
  -e TRUENAS_SERVER_URL=https://truenas.local \
  -e TRUENAS_API_KEY=your-api-key \
  --name truenas-mcp \
  truenas-master-mcp

# Build for specific platform
docker buildx build --platform linux/amd64,linux/arm64 -t truenas-master-mcp --push .
```

## API Compatibility

This server talks to TrueNAS 26 with JSON-RPC 2.0 over WebSocket (`/api/current`). REST `/api/v2.0` was removed. The last REST image is `0.2.0` (tag `v0.2.0`, branch `tn25`). Set `TRUENAS_VERSION=scale` (default) for TrueNAS SCALE.

The server provides 80+ tools covering:
- User and group management
- Pool, dataset, share, snapshot, and iSCSI management
- VM, network, and service management
- Application management (SCALE only)
- Catalog access
- Replication and cloud sync
- Disk and certificate management

## License

MIT

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.
