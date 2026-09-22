//! Translate historical TrueNAS REST paths into JSON-RPC 2.0 calls.
//!
//! Tool names stay on the REST-shaped helpers in `tools.rs`. Nothing in this
//! module opens a socket or sends HTTP.

use crate::error::{Result, TrueNasError};
use serde_json::{Value, json};
use std::collections::HashMap;

/// One JSON-RPC call produced from a REST-shaped helper.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub method: String,
    pub params: Vec<Value>,
    /// True only when the method returns a job id, not a resource id.
    pub job: bool,
    /// True when a query filter stands in for REST's single-object GET.
    pub unwrap_one: bool,
}

/// Map an HTTP method plus a `/api/v2.0/...` endpoint onto a JSON-RPC call.
pub fn translate(http_method: &str, endpoint: &str, body: Option<&Value>) -> Result<RpcCall> {
    let http = http_method.to_ascii_uppercase();
    let (segments, query) = parse_endpoint(endpoint)?;
    if segments.is_empty() {
        return Err(missing(endpoint));
    }
    let path = segments.join("/");
    match route(http.as_str(), &path, &query, body) {
        Some(call) => call,
        None => Err(missing(endpoint)),
    }
}

fn route(
    http: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: Option<&Value>,
) -> Option<Result<RpcCall>> {
    if let Some(reason) = removed(path) {
        return Some(Err(TrueNasError::ConfigError(reason)));
    }
    if http == "GET"
        && path == "zfs/snapshot"
        && let Some(dataset) = query.get("dataset")
    {
        return Some(Ok(filter_query("pool.snapshot.query", "dataset", dataset)));
    }
    if http == "DELETE" && path == "system/alert" {
        return Some(Err(TrueNasError::ConfigError(
            "TrueNAS 26 has no alert.clear; dismiss one alert with DELETE /api/v2.0/system/alert/{id}"
                .to_string(),
        )));
    }

    if let Some(call) = exact(http, path, body) {
        return Some(Ok(call));
    }

    if let Some(call) = prefixed(http, path, body) {
        return Some(call);
    }

    None
}

fn removed(path: &str) -> Option<String> {
    let reason = if path == "jail" || path.starts_with("jail/") {
        "jails were removed in TrueNAS 26"
    } else if path == "kubernetes" || path.starts_with("kubernetes/") {
        "the TrueNAS Kubernetes service was removed in TrueNAS 26"
    } else if path.starts_with("app/") && path.ends_with("/scale") {
        "app.scale was the Kubernetes replica knob and is not in TrueNAS 25 or 26"
    } else if path == "chart/release" || path.starts_with("chart/release/") || path == "chart" {
        "chart releases were removed in TrueNAS 26"
    } else if path == "tftp" || path.starts_with("tftp/") {
        "TFTP has no JSON-RPC method in TrueNAS 26"
    } else if path == "smart/test"
        || path.starts_with("smart/test/")
        || path == "smart/config"
        || path.starts_with("smart/")
    {
        "SMART test schedules have no JSON-RPC method in TrueNAS 26; disk health is disk.query / disk.details"
    } else if path == "rsync/modules" || path.starts_with("rsync/modules/") {
        "rsync modules have no JSON-RPC method in TrueNAS 26"
    } else if path == "pool/expansion" {
        "pool expansion has no JSON-RPC method in TrueNAS 26"
    } else if path == "system/log" || path.starts_with("system/log/") {
        "system/log has no JSON-RPC method in TrueNAS 26"
    } else if path == "system/alert/filter" || path.starts_with("system/alert/filter/") {
        if path == "system/alert/filter" {
            return None;
        }
        "TrueNAS 26 has alert.list_policies, not REST alert filters"
    } else {
        return None;
    };
    Some(format!("{path}: {reason}"))
}

fn exact(http: &str, path: &str, body: Option<&Value>) -> Option<RpcCall> {
    let call = match (http, path) {
        ("GET", "pool") => plain("pool.query", false),
        ("POST", "pool") => with_body("pool.create", body, true),
        ("GET", "pool/dataset") => plain("pool.dataset.query", false),
        ("POST", "pool/dataset") => with_body("pool.dataset.create", body, false),
        ("POST", "pool/scrub") => scrub(body),
        ("GET", "user") => plain("user.query", false),
        ("POST", "user") => with_body("user.create", body, false),
        ("GET", "group") => plain("group.query", false),
        ("POST", "group") => with_body("group.create", body, false),
        ("GET", "sharing/smb") => plain("sharing.smb.query", false),
        ("POST", "sharing/smb") => with_body("sharing.smb.create", body, false),
        ("GET", "sharing/nfs") => plain("sharing.nfs.query", false),
        ("POST", "sharing/nfs") => with_body("sharing.nfs.create", body, false),
        ("GET", "zfs/snapshot") => plain("pool.snapshot.query", false),
        ("POST", "zfs/snapshot") => with_body("pool.snapshot.create", body, false),
        ("POST", "zfs/snapshot/rollback") => with_body("pool.snapshot.rollback", body, true),
        ("POST", "zfs/snapshot/clone") => with_body("pool.snapshot.clone", body, true),
        ("GET", "iscsi/target") => plain("iscsi.target.query", false),
        ("POST", "iscsi/target") => with_body("iscsi.target.create", body, false),
        ("GET", "system/info") => plain("system.info", false),
        ("GET", "system/alert") => plain("alert.list", false),
        ("GET", "system/alert/categories") => plain("alert.list_categories", false),
        ("GET", "system/alert/classes") => plain("alertclasses.config", false),
        ("PUT", "system/alert/classes") => with_body("alertclasses.update", body, false),
        ("GET", "system/alert/filter") => plain("alert.list_policies", false),
        ("GET", "system/alert/service") => plain("alertservice.query", false),
        ("POST", "system/alert/service") => with_body("alertservice.create", body, false),
        ("GET", "system/alert/destination") => plain("alertservice.query", false),
        ("POST", "system/alert/destination") => with_body("alertservice.create", body, false),
        ("GET", "system/general") => plain("system.general.config", false),
        ("PUT", "system/general") => with_body("system.general.update", body, false),
        ("GET", "system/advanced") => plain("system.advanced.config", false),
        ("PUT", "system/advanced") => with_body("system.advanced.update", body, false),
        ("GET", "system/support") => plain("support.config", false),
        ("PUT", "system/support") => with_body("support.update", body, false),
        ("GET", "system/hostname") => plain("system.general.config", false),
        ("PUT", "system/hostname") => with_body("system.general.update", body, false),
        ("POST", "system/reboot") => plain("system.reboot", true),
        ("POST", "system/shutdown") => plain("system.shutdown", true),
        ("GET", "system/ntpserver") => plain("system.ntpserver.query", false),
        ("POST", "system/ntpserver") => with_body("system.ntpserver.create", body, false),
        ("GET", "system/tunable") => plain("tunable.query", false),
        ("POST", "system/tunable") => with_body("tunable.create", body, false),
        ("GET", "network/dns") | ("GET", "network/global") => {
            plain("network.configuration.config", false)
        }
        ("PUT", "network/dns") | ("PUT", "network/global") => {
            with_body("network.configuration.update", body, false)
        }
        ("GET", "network/interface") => plain("interface.query", false),
        ("POST", "network/interface") => with_body("interface.create", body, false),
        ("GET", "network/interface/ip") => plain("interface.ip_in_use", false),
        ("GET", "network/route") => plain("route.system_routes", false),
        ("GET", "network/staticroute") => plain("staticroute.query", false),
        ("POST", "network/staticroute") => with_body("staticroute.create", body, false),
        ("GET", "core/get_jobs") | ("GET", "core/get_tasks") => plain("core.get_jobs", false),
        ("GET", "update") => plain("update.status", false),
        ("POST", "update/check") => plain("update.available_versions", false),
        ("GET", "ftp") => plain("ftp.config", false),
        ("PUT", "ftp") => with_body("ftp.update", body, false),
        ("GET", "snmp") => plain("snmp.config", false),
        ("PUT", "snmp") => with_body("snmp.update", body, false),
        ("GET", "ssh") => plain("ssh.config", false),
        ("PUT", "ssh") => with_body("ssh.update", body, false),
        ("GET", "ssh/connection") => plain("keychaincredential.query", false),
        ("POST", "ssh/connection") => with_body("keychaincredential.create", body, false),
        ("GET", "boot") => plain("boot.get_state", false),
        ("GET", "disk") => plain("disk.query", false),
        ("GET", "service") => plain("service.query", false),
        ("GET", "vm") => plain("vm.query", false),
        ("POST", "vm") => with_body("vm.create", body, true),
        ("GET", "app") => plain("app.query", false),
        ("POST", "app") => with_body("app.create", body, true),
        ("GET", "replication") => plain("replication.query", false),
        ("POST", "replication") => with_body("replication.create", body, false),
        ("GET", "certificate") => plain("certificate.query", false),
        ("POST", "certificate") => with_body("certificate.create", body, false),
        ("GET", "catalog") => plain("catalog.trains", false),
        ("GET", "docker/images") => plain("app.image.query", false),
        ("POST", "docker/images/pull") => with_body("app.image.pull", body, true),
        ("GET", "cloudsync") => plain("cloudsync.query", false),
        ("POST", "cloudsync") => with_body("cloudsync.create", body, false),
        ("GET", "cloudsync/credentials") => plain("cloudsync.credentials.query", false),
        ("POST", "cloudsync/credentials") => with_body("cloudsync.credentials.create", body, false),
        ("GET", "rsync/tasks") => plain("rsynctask.query", false),
        ("POST", "rsync/tasks") => with_body("rsynctask.create", body, false),
        ("GET", "enclosure") => plain("enclosure2.query", false),
        ("GET", "reporting") => plain("reporting.config", false),
        ("GET", "reporting/disk/temperatures") => plain("disk.temperatures", false),
        ("GET", "directoryservice/activedirectory") | ("GET", "directoryservice/ldap") => {
            plain("directoryservices.config", false)
        }
        ("PUT", "directoryservice/activedirectory") | ("PUT", "directoryservice/ldap") => {
            with_body("directoryservices.update", body, false)
        }
        ("POST", "directoryservice/activedirectory/join") => {
            with_body("directoryservices.update", body, true)
        }
        ("POST", "directoryservice/activedirectory/leave") => {
            with_body("directoryservices.leave", body, true)
        }
        ("GET", "directoryservice/ldap/test") => plain("directoryservices.status", false),
        _ => return None,
    };
    Some(call)
}

fn prefixed(http: &str, path: &str, body: Option<&Value>) -> Option<Result<RpcCall>> {
    if let Some(rest) = path.strip_prefix("pool/dataset/") {
        return Some(Ok(dataset_call(http, rest, body)));
    }
    if let Some(rest) = path.strip_prefix("zfs/snapshot/") {
        if rest == "rollback" || rest == "clone" {
            return None;
        }
        return Some(Ok(id_method(
            http,
            "pool.snapshot",
            rest,
            body,
            false,
            false,
        )));
    }
    if let Some(rest) = path.strip_prefix("pool/") {
        if rest == "dataset" || rest == "scrub" || rest == "expansion" || rest.contains('/') {
            if let Some((id, action)) = rest.split_once('/')
                && !matches!(id, "dataset" | "scrub" | "expansion")
            {
                return Some(Ok(pool_action(http, id, action, body)));
            }
            return None;
        }
        return Some(Ok(pool_by_name(http, rest, body)));
    }
    Some(match_one_id(http, path, body))
}

fn dataset_call(http: &str, rest: &str, body: Option<&Value>) -> RpcCall {
    if let Some((id, quota)) = rest.split_once("/quota/") {
        return match http {
            "PUT" | "POST" | "PATCH" => RpcCall {
                method: "pool.dataset.set_quota".to_string(),
                params: vec![
                    json!(id),
                    json!(quota),
                    body.cloned().unwrap_or(Value::Null),
                ],
                job: false,
                unwrap_one: false,
            },
            _ => RpcCall {
                method: "pool.dataset.get_quota".to_string(),
                params: vec![json!(id), json!(quota)],
                job: false,
                unwrap_one: false,
            },
        };
    }
    id_method(http, "pool.dataset", rest, body, false, false)
}

fn pool_by_name(http: &str, name: &str, body: Option<&Value>) -> RpcCall {
    match http {
        "GET" => RpcCall {
            method: "pool.query".to_string(),
            params: vec![json!([["name", "=", name]])],
            job: false,
            unwrap_one: true,
        },
        "DELETE" => RpcCall {
            method: "pool.export".to_string(),
            params: vec![json!(name)],
            job: true,
            unwrap_one: false,
        },
        _ => with_id_body("pool.update", name, body, false),
    }
}

fn pool_action(http: &str, id: &str, action: &str, body: Option<&Value>) -> RpcCall {
    let (method, job) = match action {
        "attach" => ("pool.attach", true),
        "detach" => ("pool.detach", true),
        "expand" => ("pool.expand", true),
        "upgrade" => ("pool.upgrade", true),
        _ => return with_id_body(&format!("pool.{action}"), id, body, true),
    };
    let _ = http;
    with_id_body(method, id, body, job)
}

fn match_one_id(http: &str, path: &str, body: Option<&Value>) -> Result<RpcCall> {
    let mapped = one_id(path);
    if mapped.is_none() {
        return Err(missing(path));
    }
    let (prefix, id, action) = mapped.unwrap();
    if let Some(action) = action {
        return Ok(action_call(http, prefix, id, action, body));
    }
    let (method_prefix, job_create) = prefix;
    Ok(id_method(http, method_prefix, id, body, job_create, false))
}

fn one_id(path: &str) -> Option<((&'static str, bool), &str, Option<&str>)> {
    const PREFIXES: &[(&str, &str, bool)] = &[
        ("user/", "user", false),
        ("group/", "group", false),
        ("sharing/smb/", "sharing.smb", false),
        ("sharing/nfs/", "sharing.nfs", false),
        ("iscsi/target/", "iscsi.target", false),
        ("vm/", "vm", true),
        ("app/", "app", true),
        ("replication/", "replication", false),
        ("certificate/", "certificate", false),
        ("service/", "service", false),
        ("disk/", "disk", false),
        ("cloudsync/credentials/", "cloudsync.credentials", false),
        ("cloudsync/", "cloudsync", false),
        ("rsync/tasks/", "rsynctask", false),
        ("ssh/connection/", "keychaincredential", false),
        ("network/interface/", "interface", false),
        ("network/staticroute/", "staticroute", false),
        ("system/ntpserver/", "system.ntpserver", false),
        ("system/tunable/", "tunable", false),
        ("system/alert/destination/", "alertservice", false),
        ("system/alert/service/", "alertservice", false),
        ("system/alert/", "alert", false),
        ("catalog/", "catalog", false),
        ("enclosure/", "enclosure2", false),
        ("core/abort_task/", "core.job_abort", false),
        ("core/get_tasks/", "core.get_jobs", false),
    ];

    for (prefix, method, job_create) in PREFIXES {
        if let Some(rest) = path.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with("credentials/") {
                continue;
            }
            if *prefix == "user/" && rest.contains("/ssh_key/") {
                return Some(((*method, *job_create), rest, Some("ssh_key")));
            }
            if let Some((id, action)) = split_action(rest) {
                return Some(((*method, *job_create), id, Some(action)));
            }
            return Some(((*method, *job_create), rest, None));
        }
    }
    None
}

fn split_action(rest: &str) -> Option<(&str, &str)> {
    const ACTIONS: &[&str] = &[
        "start",
        "stop",
        "restart",
        "upgrade",
        "rollback",
        "clone",
        "run",
        "wipe",
        "attach",
        "detach",
        "expand",
        "pull",
        "restore",
        "scale",
        "powercycle",
        "fstab",
        "upgrade_options",
        "config",
        "resources",
        "trains",
        "status",
        "started",
        "ssh_key",
    ];
    let (id, action) = rest.rsplit_once('/')?;
    if ACTIONS.contains(&action) && !id.is_empty() {
        Some((id, action))
    } else {
        None
    }
}

fn action_call(
    http: &str,
    prefix: (&'static str, bool),
    id: &str,
    action: &str,
    body: Option<&Value>,
) -> RpcCall {
    let (namespace, _) = prefix;
    let (method, job) = match (namespace, action) {
        ("app", "start") => ("app.start", true),
        ("app", "stop") => ("app.stop", true),
        ("app", "restart") => ("app.redeploy", true),
        ("app", "upgrade") => ("app.upgrade", true),
        ("app", "rollback") => ("app.rollback", true),
        ("app", "config") => ("app.config", false),
        ("app", "upgrade_options") => ("app.upgrade_summary", false),
        ("vm", "start") => ("vm.start", true),
        ("vm", "stop") => ("vm.stop", true),
        ("vm", "restart") | ("vm", "powercycle") => ("vm.restart", true),
        ("vm", "clone") => ("vm.clone", true),
        ("service", "start") => ("service.start", true),
        ("service", "stop") => ("service.stop", true),
        ("service", "restart") => ("service.restart", true),
        ("service", "started") => ("service.started", false),
        ("replication", "run") => ("replication.run", true),
        ("cloudsync", "run") => ("cloudsync.sync", true),
        ("rsynctask", "run") => ("rsynctask.run", true),
        ("disk", "wipe") => ("disk.wipe", true),
        ("disk", "smart") => ("disk.details", false),
        ("alert", _) if http == "DELETE" => ("alert.dismiss", false),
        ("catalog", "trains") => ("catalog.trains", false),
        ("enclosure2", "status") => ("enclosure2.query", false),
        _ if action == "ssh_key" => {
            return ssh_key_call(http, id, body);
        }
        _ => {
            let method = format!("{namespace}.{action}");
            return with_id_body(&method, id, body, false);
        }
    };
    if method == "enclosure2.query"
        || method == "catalog.trains"
        || method == "app.config"
        || method == "app.upgrade_summary"
        || method == "service.started"
        || method == "disk.details"
        || method == "alert.dismiss"
    {
        return RpcCall {
            method: method.to_string(),
            params: if method == "enclosure2.query" {
                Vec::new()
            } else {
                vec![id_value(id)]
            },
            job,
            unwrap_one: false,
        };
    }
    with_id_body(method, id, body, job)
}

fn ssh_key_call(http: &str, id: &str, body: Option<&Value>) -> RpcCall {
    if let Some((_, key_id)) = id.split_once("/ssh_key/") {
        return match http {
            "DELETE" => RpcCall {
                method: "keychaincredential.delete".to_string(),
                params: vec![id_value(key_id)],
                job: false,
                unwrap_one: false,
            },
            _ => RpcCall {
                method: "keychaincredential.get_instance".to_string(),
                params: vec![id_value(key_id)],
                job: false,
                unwrap_one: false,
            },
        };
    }
    if let Some((user_id, key_id)) = id.split_once('/') {
        let _ = user_id;
        return match http {
            "DELETE" => RpcCall {
                method: "keychaincredential.delete".to_string(),
                params: vec![id_value(key_id)],
                job: false,
                unwrap_one: false,
            },
            _ => RpcCall {
                method: "keychaincredential.get_instance".to_string(),
                params: vec![id_value(key_id)],
                job: false,
                unwrap_one: false,
            },
        };
    }
    match http {
        "POST" => with_body("keychaincredential.create", body, false),
        "DELETE" => with_id_body("keychaincredential.delete", id, body, false),
        _ => plain("keychaincredential.query", false),
    }
}

fn id_method(
    http: &str,
    prefix: &str,
    id: &str,
    body: Option<&Value>,
    job_create: bool,
    job_delete: bool,
) -> RpcCall {
    if prefix == "core.job_abort" {
        return RpcCall {
            method: "core.job_abort".to_string(),
            params: vec![id_value(id)],
            job: false,
            unwrap_one: false,
        };
    }
    if prefix == "core.get_jobs" {
        return RpcCall {
            method: "core.get_jobs".to_string(),
            params: vec![json!([["id", "=", id_value(id)]])],
            job: false,
            unwrap_one: false,
        };
    }
    if prefix == "alert" && http == "DELETE" {
        return RpcCall {
            method: "alert.dismiss".to_string(),
            params: vec![id_value(id)],
            job: false,
            unwrap_one: false,
        };
    }
    if prefix == "disk" && http == "GET" {
        return RpcCall {
            method: "disk.query".to_string(),
            params: vec![json!([["name", "=", id]])],
            job: false,
            unwrap_one: true,
        };
    }
    let (method, params, job) = match http {
        "GET" => (format!("{prefix}.get_instance"), vec![id_value(id)], false),
        "DELETE" => (format!("{prefix}.delete"), vec![id_value(id)], job_delete),
        "POST" => (format!("{prefix}.create"), params_body(body), job_create),
        _ => (format!("{prefix}.update"), params_id_body(id, body), false),
    };
    RpcCall {
        method,
        params,
        job,
        unwrap_one: false,
    }
}

fn scrub(body: Option<&Value>) -> RpcCall {
    let params = body
        .and_then(|value| value.get("name"))
        .cloned()
        .map(|name| vec![name])
        .unwrap_or_else(|| params_body(body));
    RpcCall {
        method: "pool.scrub.run".to_string(),
        params,
        job: true,
        unwrap_one: false,
    }
}

fn filter_query(method: &str, field: &str, value: &str) -> RpcCall {
    RpcCall {
        method: method.to_string(),
        params: vec![json!([[field, "=", value]])],
        job: false,
        unwrap_one: false,
    }
}

fn plain(method: &str, job: bool) -> RpcCall {
    RpcCall {
        method: method.to_string(),
        params: Vec::new(),
        job,
        unwrap_one: false,
    }
}

fn with_body(method: &str, body: Option<&Value>, job: bool) -> RpcCall {
    RpcCall {
        method: method.to_string(),
        params: params_body(body),
        job,
        unwrap_one: false,
    }
}

fn with_id_body(method: &str, id: &str, body: Option<&Value>, job: bool) -> RpcCall {
    RpcCall {
        method: method.to_string(),
        params: params_id_body(id, body),
        job,
        unwrap_one: false,
    }
}

fn params_body(body: Option<&Value>) -> Vec<Value> {
    match body {
        Some(value) if !value.is_null() => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn params_id_body(id: &str, body: Option<&Value>) -> Vec<Value> {
    let mut params = vec![id_value(id)];
    if let Some(value) = body
        && !value.is_null()
    {
        params.push(value.clone());
    }
    params
}

fn id_value(id: &str) -> Value {
    match id.parse::<i64>() {
        Ok(number) if number.to_string() == id => json!(number),
        _ => json!(id),
    }
}

fn missing(endpoint: &str) -> TrueNasError {
    TrueNasError::ConfigError(format!(
        "no TrueNAS 26 JSON-RPC method for REST endpoint {endpoint}"
    ))
}

fn parse_endpoint(endpoint: &str) -> Result<(Vec<String>, HashMap<String, String>)> {
    let endpoint = endpoint.trim();
    let (path, query_str) = endpoint.split_once('?').unwrap_or((endpoint, ""));
    let path = path.trim_start_matches('/');
    let path = path
        .strip_prefix("api/v2.0/")
        .or_else(|| path.strip_prefix("api/v2.0"))
        .ok_or_else(|| missing(endpoint))?;
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect();
    let mut query = HashMap::new();
    if !query_str.is_empty() {
        for pair in query_str.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            query.insert(percent_decode(key), percent_decode(value));
        }
    }
    Ok((segments, query))
}

fn percent_decode(value: &str) -> String {
    urlencoding::decode(value)
        .map(|decoded| decoded.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(http: &str, endpoint: &str) -> String {
        translate(http, endpoint, None).unwrap().method
    }

    #[test]
    fn lists_become_queries() {
        assert_eq!(method("GET", "/api/v2.0/pool"), "pool.query");
        assert_eq!(method("GET", "/api/v2.0/user"), "user.query");
        assert_eq!(method("GET", "/api/v2.0/sharing/nfs"), "sharing.nfs.query");
        assert_eq!(method("GET", "/api/v2.0/vm"), "vm.query");
        assert_eq!(method("GET", "/api/v2.0/app"), "app.query");
        assert_eq!(method("GET", "/api/v2.0/disk"), "disk.query");
        assert_eq!(
            method("GET", "/api/v2.0/zfs/snapshot"),
            "pool.snapshot.query"
        );
    }

    #[test]
    fn system_info_and_alerts_are_not_rest() {
        assert_eq!(method("GET", "/api/v2.0/system/info"), "system.info");
        assert_eq!(method("GET", "/api/v2.0/system/alert"), "alert.list");
        assert_eq!(
            method("GET", "/api/v2.0/system/alert/categories"),
            "alert.list_categories"
        );
        assert_eq!(
            method("GET", "/api/v2.0/system/general"),
            "system.general.config"
        );
        let scrub = translate(
            "POST",
            "/api/v2.0/pool/scrub",
            Some(&json!({"name": "stripe"})),
        )
        .unwrap();
        assert_eq!(scrub.method, "pool.scrub.run");
        assert_eq!(scrub.params, vec![json!("stripe")]);
        assert!(scrub.job);
    }

    #[test]
    fn dataset_id_is_decoded_and_jobs_are_marked() {
        let call = translate("GET", "/api/v2.0/pool/dataset/tank%2Fdocs", None).unwrap();
        assert_eq!(call.method, "pool.dataset.get_instance");
        assert_eq!(call.params, vec![json!("tank/docs")]);
        assert!(!call.job);

        let snap = translate("GET", "/api/v2.0/zfs/snapshot?dataset=tank%2Fdocs", None).unwrap();
        assert_eq!(snap.method, "pool.snapshot.query");
        assert_eq!(snap.params, vec![json!([["dataset", "=", "tank/docs"]])]);

        let start = translate("POST", "/api/v2.0/service/nfs/restart", None).unwrap();
        assert_eq!(start.method, "service.restart");
        assert_eq!(start.params, vec![json!("nfs")]);
        assert!(start.job);
    }

    #[test]
    fn removed_namespaces_do_not_invent_a_call() {
        let err = translate("GET", "/api/v2.0/jail", None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("removed"));
        assert!(translate("GET", "/api/v2.0/kubernetes", None).is_err());
        assert!(translate("GET", "/api/v2.0/chart/release", None).is_err());
        let scale = translate("POST", "/api/v2.0/app/plex/scale", None).unwrap_err();
        assert!(scale.to_string().contains("app.scale"));
    }

    #[test]
    fn pool_get_by_name_unwraps_a_query() {
        let call = translate("GET", "/api/v2.0/pool/stripe", None).unwrap();
        assert_eq!(call.method, "pool.query");
        assert!(call.unwrap_one);
        assert_eq!(call.params, vec![json!([["name", "=", "stripe"]])]);
    }
}
