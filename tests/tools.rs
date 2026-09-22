#![allow(clippy::unwrap_used)]

use serde_json::json;
use truenas_master_mcp::tools::{
    Alert, AppInfo, Certificate, CloudCredential, CloudSyncTask, Dataset, DnsConfig, EnclosureInfo,
    Group, IscsiTarget, Jail, KubernetesStatus, NetworkInterface, NetworkRoute, NfsExport, Pool,
    ReplicationTask, Service, SmbShare, Snapshot, SystemInfo, UpdateCheck, User, Vm,
};

mod serialization_tests {
    use super::*;

    #[test]
    fn test_user_serialization_roundtrip() {
        let user = User {
            id: 1000,
            username: "testuser".to_string(),
            uid: 1000,
            home: Some("/home/testuser".to_string()),
            email: Some("test@example.com".to_string()),
            full_name: Some("Test User".to_string()),
        };

        let json = serde_json::to_string(&user).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert_eq!(user.id, decoded.id);
        assert_eq!(user.username, decoded.username);
    }

    #[test]
    fn test_user_with_optional_fields() {
        // User with all optional fields
        let user = User {
            id: 1000,
            username: "testuser".to_string(),
            uid: 1000,
            home: Some("/home/testuser".to_string()),
            email: Some("test@example.com".to_string()),
            full_name: Some("Test User".to_string()),
        };
        let json = serde_json::to_string(&user).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert!(decoded.home.is_some());
        assert!(decoded.email.is_some());

        // User without optional fields
        let user_minimal = User {
            id: 1001,
            username: "minimal".to_string(),
            uid: 1001,
            home: None,
            email: None,
            full_name: None,
        };
        let json = serde_json::to_string(&user_minimal).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert!(decoded.home.is_none());
        assert!(decoded.email.is_none());
    }

    #[test]
    fn test_pool_serialization() {
        let pool = Pool {
            name: "tank".to_string(),
            guid: "1234567890".to_string(),
            status: "ONLINE".to_string(),
            size: 10_000_000_000,
            free: 5_000_000_000,
            description: Some("Main storage".to_string()),
        };

        let json = serde_json::to_string(&pool).unwrap();
        let decoded: Pool = serde_json::from_str(&json).unwrap();
        assert_eq!(pool.name, decoded.name);
        assert_eq!(pool.status, decoded.status);
    }

    #[test]
    fn test_dataset_serialization() {
        let dataset = Dataset {
            name: "tank/data".to_string(),
            pool: "tank".to_string(),
            mountpoint: Some("/mnt/tank/data".to_string()),
            comments: Some("My data".to_string()),
        };

        let json = serde_json::to_string(&dataset).unwrap();
        let decoded: Dataset = serde_json::from_str(&json).unwrap();
        assert_eq!(dataset.name, decoded.name);
    }

    #[test]
    fn test_smb_share_serialization() {
        let share = SmbShare {
            id: 1,
            name: "myshare".to_string(),
            path: "/mnt/tank/shares".to_string(),
            comment: Some("Shared folder".to_string()),
        };

        let json = serde_json::to_string(&share).unwrap();
        let decoded: SmbShare = serde_json::from_str(&json).unwrap();
        assert_eq!(share.id, decoded.id);
        assert_eq!(share.name, decoded.name);
    }

    #[test]
    fn test_nfs_export_serialization() {
        let export = NfsExport {
            id: 1,
            paths: vec!["/mnt/data1".to_string(), "/mnt/data2".to_string()],
            comment: "NFS exports".to_string(),
        };

        let json = serde_json::to_string(&export).unwrap();
        let decoded: NfsExport = serde_json::from_str(&json).unwrap();
        assert_eq!(export.id, decoded.id);
        assert_eq!(export.paths.len(), decoded.paths.len());
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = Snapshot {
            name: "tank/data@snapshot1".to_string(),
            pool: "tank".to_string(),
            dataset: "tank/data".to_string(),
            creation: 1_700_000_000,
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot.name, decoded.name);
    }

    #[test]
    fn test_iscsi_target_serialization() {
        let target = IscsiTarget {
            id: 1,
            name: "iqn.2024-01.com.example:target".to_string(),
            status: "ONLINE".to_string(),
        };

        let json = serde_json::to_string(&target).unwrap();
        let decoded: IscsiTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(target.id, decoded.id);
    }

    #[test]
    fn test_system_info_serialization() {
        let info = SystemInfo {
            version: "TrueNAS-SCALE-24.10.0".to_string(),
            hostname: "truenas.local".to_string(),
            cpu_model: Some("Intel Xeon".to_string()),
            uptime_seconds: Some(3600.0),
            extra: Default::default(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let decoded: SystemInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.version, decoded.version);
        assert!(decoded.cpu_model.is_some());
    }

    #[test]
    fn test_app_info_serialization() {
        let app = AppInfo {
            name: "plex".to_string(),
            version: Some("1.40.0".to_string()),
            state: Some("RUNNING".to_string()),
            description: Some("Media server".to_string()),
            port: Some(32400),
            image: Some("plexinc/pms".to_string()),
        };

        let json = serde_json::to_string(&app).unwrap();
        let decoded: AppInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(app.name, decoded.name);
        assert_eq!(app.port, decoded.port);
    }

    #[test]
    fn test_group_serialization() {
        let group = Group {
            id: 1000,
            gid: 1000,
            name: "testgroup".to_string(),
            users: Some(vec![1000, 1001]),
        };

        let json = serde_json::to_string(&group).unwrap();
        let decoded: Group = serde_json::from_str(&json).unwrap();
        assert_eq!(group.name, decoded.name);
        assert!(decoded.users.is_some());
    }

    #[test]
    fn test_vm_serialization() {
        let vm = Vm {
            id: 1,
            name: "windows-vm".to_string(),
            vcpus: 4,
            memory: 8_000_000_000,
            status: "RUNNING".to_string(),
            description: Some("Windows VM".to_string()),
        };

        let json = serde_json::to_string(&vm).unwrap();
        let decoded: Vm = serde_json::from_str(&json).unwrap();
        assert_eq!(vm.vcpus, decoded.vcpus);
        assert_eq!(vm.memory, decoded.memory);
    }

    #[test]
    fn test_network_interface_serialization() {
        let iface = NetworkInterface {
            id: "eth0".to_string(),
            name: "eth0".to_string(),
            state: "UP".to_string(),
            ipaddr: Some("192.168.1.100".to_string()),
            netmask: Some("255.255.255.0".to_string()),
        };

        let json = serde_json::to_string(&iface).unwrap();
        let decoded: NetworkInterface = serde_json::from_str(&json).unwrap();
        assert_eq!(iface.name, decoded.name);
    }

    #[test]
    fn test_network_route_serialization() {
        let route = NetworkRoute {
            destination: "0.0.0.0/0".to_string(),
            gateway: "192.168.1.1".to_string(),
            interface: "eth0".to_string(),
        };

        let json = serde_json::to_string(&route).unwrap();
        let decoded: NetworkRoute = serde_json::from_str(&json).unwrap();
        assert_eq!(route.destination, decoded.destination);
    }

    #[test]
    fn test_dns_config_serialization() {
        let dns = DnsConfig {
            nameservers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            domains: vec!["example.com".to_string()],
        };

        let json = serde_json::to_string(&dns).unwrap();
        let decoded: DnsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(dns.nameservers.len(), decoded.nameservers.len());
    }

    #[test]
    fn test_replication_task_serialization() {
        let task = ReplicationTask {
            id: 1,
            name: "backup-pool".to_string(),
            source: "tank".to_string(),
            target: "backup-tank".to_string(),
            direction: "PUSH".to_string(),
            state: "FINISHED".to_string(),
        };

        let json = serde_json::to_string(&task).unwrap();
        let decoded: ReplicationTask = serde_json::from_str(&json).unwrap();
        assert_eq!(task.name, decoded.name);
    }

    #[test]
    fn test_cloud_sync_task_serialization() {
        let task = CloudSyncTask {
            id: 1,
            description: "Sync to S3".to_string(),
            direction: "PUSH".to_string(),
            transport: "S3".to_string(),
            state: "IDLE".to_string(),
        };

        let json = serde_json::to_string(&task).unwrap();
        let decoded: CloudSyncTask = serde_json::from_str(&json).unwrap();
        assert_eq!(task.description, decoded.description);
    }

    #[test]
    fn test_cloud_credential_serialization() {
        let cred = CloudCredential {
            id: 1,
            name: "my-s3".to_string(),
            provider: "S3".to_string(),
        };

        let json = serde_json::to_string(&cred).unwrap();
        let decoded: CloudCredential = serde_json::from_str(&json).unwrap();
        assert_eq!(cred.name, decoded.name);
    }

    #[test]
    fn test_service_serialization() {
        let service = Service {
            id: 1,
            service: "smb".to_string(),
            state: "RUNNING".to_string(),
            enable: true,
        };

        let json = serde_json::to_string(&service).unwrap();
        let decoded: Service = serde_json::from_str(&json).unwrap();
        assert_eq!(service.service, decoded.service);
        assert_eq!(service.enable, decoded.enable);
    }

    #[test]
    fn test_alert_serialization() {
        let alert = Alert {
            id: "alert-123".to_string(),
            level: "WARNING".to_string(),
            message: "Pool capacity at 80%".to_string(),
            timestamp: 1_700_000_000,
        };

        let json = serde_json::to_string(&alert).unwrap();
        let decoded: Alert = serde_json::from_str(&json).unwrap();
        assert_eq!(alert.level, decoded.level);
    }

    #[test]
    fn test_update_check_serialization() {
        let update = UpdateCheck {
            status: "AVAILABLE".to_string(),
            version: Some("24.10.1".to_string()),
            description: Some("Security update".to_string()),
        };

        let json = serde_json::to_string(&update).unwrap();
        let decoded: UpdateCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(update.status, decoded.status);
    }

    #[test]
    fn test_certificate_serialization() {
        let cert = Certificate {
            id: 1,
            name: "my-cert".to_string(),
            cert_type: "HTTPS".to_string(),
            state: "ACTIVE".to_string(),
            issuer: Some("Let's Encrypt".to_string()),
            from: Some(1_700_000_000),
            until: Some(1_733_000_000),
        };

        let json = serde_json::to_string(&cert).unwrap();
        let decoded: Certificate = serde_json::from_str(&json).unwrap();
        assert_eq!(cert.name, decoded.name);
    }

    #[test]
    fn test_kubernetes_status_serialization() {
        let k8s = KubernetesStatus {
            node_ip: "192.168.1.50".to_string(),
            cluster_ip: "10.96.0.1".to_string(),
            cluster_cidr: "10.244.0.0/16".to_string(),
            service_cidr: "10.96.0.0/12".to_string(),
            status: "HEALTHY".to_string(),
        };

        let json = serde_json::to_string(&k8s).unwrap();
        let decoded: KubernetesStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(k8s.status, decoded.status);
    }

    #[test]
    fn test_jail_serialization() {
        let jail = Jail {
            id: 1,
            name: "my-jail".to_string(),
            state: "RUNNING".to_string(),
            ip4_addr: Some("192.168.1.100".to_string()),
            ip6_addr: None,
        };

        let json = serde_json::to_string(&jail).unwrap();
        let decoded: Jail = serde_json::from_str(&json).unwrap();
        assert_eq!(jail.name, decoded.name);
        assert!(decoded.ip6_addr.is_none());
    }

    #[test]
    fn test_enclosure_info_serialization() {
        let enclosure = EnclosureInfo {
            id: "enc-123".to_string(),
            name: "TrueNAS-F60".to_string(),
            model: "F60".to_string(),
            status: "ONLINE".to_string(),
        };

        let json = serde_json::to_string(&enclosure).unwrap();
        let decoded: EnclosureInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(enclosure.name, decoded.name);
    }
}

mod deserialization_tests {
    use super::*;

    #[test]
    fn test_user_from_json() {
        let json = json!({
            "id": 1000,
            "username": "testuser",
            "uid": 1000,
            "home": "/home/testuser",
            "email": "test@example.com",
            "full_name": "Test User"
        });
        let user: User = serde_json::from_value(json).unwrap();
        assert_eq!(user.id, 1000);
        assert_eq!(user.username, "testuser");
    }

    #[test]
    fn test_user_minimal_from_json() {
        let json = json!({
            "id": 1001,
            "username": "minimal",
            "uid": 1001
        });
        let user: User = serde_json::from_value(json).unwrap();
        assert_eq!(user.id, 1001);
        assert!(user.home.is_none());
        assert!(user.email.is_none());
    }

    #[test]
    fn test_pool_from_json() {
        let json = json!({
            "name": "tank",
            "guid": "1234567890",
            "status": "ONLINE",
            "size": 10000000000i64,
            "free": 5000000000i64,
            "description": "Main pool"
        });
        let pool: Pool = serde_json::from_value(json).unwrap();
        assert_eq!(pool.name, "tank");
        assert_eq!(pool.size, 10000000000);
    }

    #[test]
    fn test_dataset_from_json() {
        let json = json!({
            "name": "tank/data",
            "pool": "tank",
            "mountpoint": "/mnt/tank/data"
        });
        let dataset: Dataset = serde_json::from_value(json).unwrap();
        assert_eq!(dataset.name, "tank/data");
        assert!(dataset.comments.is_none());
    }

    #[test]
    fn test_system_info_from_json() {
        let json = json!({
            "version": "TrueNAS-SCALE-24.10.0",
            "hostname": "truenas",
            "cpu_model": "Intel Xeon",
            "uptime_seconds": 3600
        });
        let info: SystemInfo = serde_json::from_value(json).unwrap();
        assert_eq!(info.version, "TrueNAS-SCALE-24.10.0");
        assert!(info.cpu_model.is_some());
    }

    #[test]
    fn test_app_info_from_json() {
        let json = json!({
            "name": "plex",
            "version": "1.40.0",
            "state": "RUNNING"
        });
        let app: AppInfo = serde_json::from_value(json).unwrap();
        assert_eq!(app.name, "plex");
        assert!(app.port.is_none());
    }

    #[test]
    fn test_snapshot_from_json() {
        let json = json!({
            "name": "tank/data@snap1",
            "pool": "tank",
            "dataset": "tank/data",
            "creation": 1700000000
        });
        let snapshot: Snapshot = serde_json::from_value(json).unwrap();
        assert!(snapshot.name.contains("@"));
    }

    #[test]
    fn test_nfs_export_from_json() {
        let json = json!({
            "id": 1,
            "paths": ["/mnt/data1", "/mnt/data2"],
            "comment": "NFS share"
        });
        let export: NfsExport = serde_json::from_value(json).unwrap();
        assert_eq!(export.paths.len(), 2);
    }

    #[test]
    fn test_network_route_from_json() {
        let json = json!({
            "destination": "0.0.0.0/0",
            "gateway": "192.168.1.1",
            "interface": "eth0"
        });
        let route: NetworkRoute = serde_json::from_value(json).unwrap();
        assert_eq!(route.destination, "0.0.0.0/0");
    }

    #[test]
    fn test_dns_config_from_json() {
        let json = json!({
            "nameservers": ["8.8.8.8", "8.8.4.4"],
            "domains": ["example.com"]
        });
        let dns: DnsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(dns.nameservers.len(), 2);
    }
}

mod serialization_edge_cases {
    use super::*;

    #[test]
    fn test_empty_strings() {
        let user = User {
            id: 0,
            username: "".to_string(),
            uid: 0,
            home: Some("".to_string()),
            email: None,
            full_name: None,
        };
        let json = serde_json::to_string(&user).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.username, "");
    }

    #[test]
    fn test_special_characters_in_names() {
        let share = SmbShare {
            id: 1,
            name: "share with spaces".to_string(),
            path: "/mnt/tank/folder with spaces".to_string(),
            comment: Some("Special chars: !@#$%".to_string()),
        };
        let json = serde_json::to_string(&share).unwrap();
        let decoded: SmbShare = serde_json::from_str(&json).unwrap();
        assert_eq!(share.name, decoded.name);
    }

    #[test]
    fn test_unicode_in_names() {
        let user = User {
            id: 1,
            username: "用户".to_string(),
            uid: 1000,
            home: None,
            email: Some("用户@example.com".to_string()),
            full_name: Some("中文姓名".to_string()),
        };
        let json = serde_json::to_string(&user).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert_eq!(user.username, decoded.username);
    }

    #[test]
    fn test_large_numbers() {
        let pool = Pool {
            name: "large-pool".to_string(),
            guid: "large-guid".to_string(),
            status: "ONLINE".to_string(),
            size: 100_000_000_000_000,
            free: 50_000_000_000_000,
            description: None,
        };
        let json = serde_json::to_string(&pool).unwrap();
        let decoded: Pool = serde_json::from_str(&json).unwrap();
        assert_eq!(pool.size, decoded.size);
    }

    #[test]
    fn test_negative_ids() {
        let user = User {
            id: -1,
            username: "nobody".to_string(),
            uid: 65534,
            home: None,
            email: None,
            full_name: None,
        };
        let json = serde_json::to_string(&user).unwrap();
        let decoded: User = serde_json::from_str(&json).unwrap();
        assert_eq!(user.id, decoded.id);
    }
}

mod debug_format_tests {
    use super::*;

    #[test]
    fn test_user_debug_format() {
        let user = User {
            id: 1000,
            username: "testuser".to_string(),
            uid: 1000,
            home: Some("/home/testuser".to_string()),
            email: Some("test@example.com".to_string()),
            full_name: Some("Test User".to_string()),
        };
        let debug = format!("{:?}", user);
        assert!(debug.contains("testuser"));
        assert!(debug.contains("1000"));
    }

    #[test]
    fn test_pool_debug_format() {
        let pool = Pool {
            name: "tank".to_string(),
            guid: "guid".to_string(),
            status: "ONLINE".to_string(),
            size: 1000,
            free: 500,
            description: None,
        };
        let debug = format!("{:?}", pool);
        assert!(debug.contains("tank"));
        assert!(debug.contains("ONLINE"));
    }

    #[test]
    fn test_vm_debug_format() {
        let vm = Vm {
            id: 1,
            name: "test-vm".to_string(),
            vcpus: 4,
            memory: 8_000_000_000,
            status: "RUNNING".to_string(),
            description: None,
        };
        let debug = format!("{:?}", vm);
        assert!(debug.contains("test-vm"));
        assert!(debug.contains("4"));
        assert!(debug.contains("8000000000"));
    }
}
