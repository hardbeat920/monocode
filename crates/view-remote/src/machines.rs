//! Labels and checks shared by the Connect views. Ports of the helpers at
//! the top of ConnectionsSettings.tsx and the status check in its effect.

use monocode_remote::host::protocol::{
    HostDescriptor, REMOTE_PROVIDERS, RemoteMachine, host_needs_update, parse_provider,
    provider_name,
};
use serde_json::{Value, json};

/// `String(reason).replace(/^Error: /, "")`: a failure as the views show it.
pub fn strip_error(reason: &str) -> String {
    reason.strip_prefix("Error: ").unwrap_or(reason).to_string()
}

/// `routeLabel`: how the desktop reaches a machine. Its first direct
/// address, the SSH forward, or both.
pub fn route_label(machine: &RemoteMachine) -> String {
    let ssh = machine
        .ssh
        .as_ref()
        .map(|ssh| match ssh.port {
            Some(port) if port != 0 => format!("{} · port {port}", ssh.target),
            _ => ssh.target.clone(),
        })
        .unwrap_or_default();
    let endpoints = machine.endpoints.as_deref().unwrap_or_default();
    let direct = endpoints
        .first()
        .map(|first| first.strip_prefix("https://").unwrap_or(first))
        .filter(|direct| !direct.is_empty());
    if let Some(direct) = direct {
        let more = if endpoints.len() > 1 {
            format!(" (+{})", endpoints.len() - 1)
        } else {
            String::new()
        };
        return if ssh.is_empty() {
            format!("{direct}{more}")
        } else {
            format!("{direct}{more} · SSH fallback {ssh}")
        };
    }
    if ssh.is_empty() {
        machine.endpoint.clone()
    } else {
        format!("SSH · {ssh}")
    }
}

/// `connectedNotice`: what Settings says after a machine pairs.
pub fn connected_notice(machine: &RemoteMachine) -> String {
    format!(
        "{} is connected. To work on it, click + next to Projects in the project rail and choose Open folder on a machine.",
        machine.name
    )
}

/// The `environment.describe` params every check sends: the providers this
/// desktop can run, so the host reports which of them it has.
pub fn describe_params() -> Value {
    let providers: Vec<&str> = REMOTE_PROVIDERS.into_iter().map(provider_name).collect();
    json!({ "supportedProviders": providers })
}

/// Reads a describe answer the way the TypeScript did: without
/// `requireHostDescriptor`, so a missing field reads as empty rather than
/// failing the check.
pub fn descriptor_from_value(value: &Value) -> HostDescriptor {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let strings = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    HostDescriptor {
        protocol_version: value
            .get("protocolVersion")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        environment_id: text("environmentId").unwrap_or_default(),
        name: text("name").unwrap_or_default(),
        providers: strings("providers")
            .iter()
            .filter_map(|name| parse_provider(name))
            .collect(),
        capabilities: strings("capabilities"),
        platform: text("platform"),
        host_version: text("hostVersion"),
        endpoints: value.get("endpoints").map(|_| strings("endpoints")),
    }
}

/// `MachineStatus`: the line under a machine in Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct MachineStatus {
    pub label: String,
    pub host: Option<HostDescriptor>,
    pub offline: bool,
}

impl MachineStatus {
    /// Shown while a check runs.
    pub fn checking() -> Self {
        Self {
            label: "Checking connection…".into(),
            host: None,
            offline: false,
        }
    }

    /// Whether this desktop should offer a host update.
    pub fn needs_update(&self, version: Option<&str>) -> bool {
        self.host
            .as_ref()
            .is_some_and(|host| host_needs_update(host, version))
    }
}

/// The status for one `environment.describe` answer.
pub fn machine_status(
    machine: &RemoteMachine,
    answer: Result<Value, String>,
    version: Option<&str>,
) -> MachineStatus {
    let checked = answer.and_then(|value| {
        let host = descriptor_from_value(&value);
        if host.environment_id != machine.environment_id {
            return Err("Host identity changed".to_string());
        }
        Ok(host)
    });
    match checked {
        Ok(host) => {
            let mut parts = vec![format!(
                "Connected · host {}",
                host.host_version.as_deref().unwrap_or("older than 0.5")
            )];
            if host.providers.is_empty() {
                parts.push("install a supported provider on the host".into());
            }
            if host_needs_update(&host, version) {
                parts.push("update available".into());
            }
            MachineStatus {
                label: parts.join(" · "),
                host: Some(host),
                offline: false,
            }
        }
        Err(reason) => MachineStatus {
            label: format!("Offline · {}", strip_error(&reason)),
            host: None,
            offline: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use monocode_remote::host::protocol::RemoteMachineSsh;
    use serde_json::json;

    use super::*;

    fn ssh_machine() -> RemoteMachine {
        RemoteMachine {
            id: "machine".into(),
            name: "Home Mac".into(),
            endpoint: "ssh://me@home".into(),
            endpoints: None,
            environment_id: "env".into(),
            ssh: Some(RemoteMachineSsh {
                target: "me@home".into(),
                port: None,
                remote_port: 3774,
            }),
        }
    }

    fn direct_machine() -> RemoteMachine {
        RemoteMachine {
            id: "direct".into(),
            name: "Studio".into(),
            endpoint: "10.0.0.2:3774".into(),
            endpoints: Some(vec![
                "https://10.0.0.2:3774".into(),
                "https://100.64.0.9:3774".into(),
            ]),
            environment_id: "env-2".into(),
            ssh: None,
        }
    }

    #[test]
    fn labels_each_route() {
        assert_eq!(route_label(&ssh_machine()), "SSH · me@home");
        assert_eq!(route_label(&direct_machine()), "10.0.0.2:3774 (+1)");
        let mut both = direct_machine();
        both.ssh = Some(RemoteMachineSsh {
            target: "me@studio".into(),
            port: Some(2222),
            remote_port: 3774,
        });
        assert_eq!(
            route_label(&both),
            "10.0.0.2:3774 (+1) · SSH fallback me@studio · port 2222"
        );
        let mut bare = direct_machine();
        bare.endpoints = Some(Vec::new());
        assert_eq!(route_label(&bare), "10.0.0.2:3774");
    }

    #[test]
    fn advertises_every_supported_provider() {
        let params = describe_params();
        let names = params["supportedProviders"].as_array().unwrap();
        assert_eq!(names.len(), REMOTE_PROVIDERS.len());
        assert_eq!(names[0], "codex");
        assert_eq!(names[1], "claude");
    }

    #[test]
    fn an_old_host_offers_an_update() {
        let status = machine_status(
            &ssh_machine(),
            Ok(json!({ "environmentId": "env", "providers": ["codex"] })),
            Some("1.2.3"),
        );
        assert_eq!(
            status.label,
            "Connected · host older than 0.5 · update available"
        );
        assert!(status.needs_update(Some("1.2.3")));
        assert!(!status.offline);
    }

    #[test]
    fn a_current_host_without_providers_asks_for_one() {
        let status = machine_status(
            &ssh_machine(),
            Ok(json!({
                "environmentId": "env",
                "providers": [],
                "hostVersion": "1.2.3",
                "capabilities": ["changes.wait"]
            })),
            Some("1.2.3"),
        );
        assert_eq!(
            status.label,
            "Connected · host 1.2.3 · install a supported provider on the host"
        );
        assert!(!status.needs_update(Some("1.2.3")));
    }

    #[test]
    fn a_failed_or_replaced_host_is_offline() {
        let offline = machine_status(
            &direct_machine(),
            Err("Error: Machine is unreachable. 10.0.0.2:3774 did not answer".into()),
            None,
        );
        assert_eq!(
            offline.label,
            "Offline · Machine is unreachable. 10.0.0.2:3774 did not answer"
        );
        assert!(offline.offline);
        let replaced = machine_status(
            &direct_machine(),
            Ok(json!({ "environmentId": "other", "providers": ["codex"] })),
            None,
        );
        assert_eq!(replaced.label, "Offline · Host identity changed");
    }

    #[test]
    fn strips_only_a_leading_error_prefix() {
        assert_eq!(strip_error("Error: boom"), "boom");
        assert_eq!(strip_error("boom Error: x"), "boom Error: x");
    }
}
