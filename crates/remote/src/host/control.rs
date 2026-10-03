//! Port of host/control.ts: reaching a running host through its local
//! lifecycle endpoint.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Written by `serve` to `<data dir>/running.json`, readable only by the
/// owner. `secret` authorizes local lifecycle requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningHost {
    pub pid: i64,
    pub port: u16,
    pub secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub enabled: bool,
    pub bind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// Absent for hosts older than 0.5, which report nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_turns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkStatus>,
}

/// A running host's status and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningStatus {
    pub status: HostStatus,
    pub state: RunningHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Status,
    Stop,
    Network,
}

impl LifecycleAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Stop => "stop",
            Self::Network => "network",
        }
    }
}

pub fn read_running(directory: &Path) -> Option<RunningHost> {
    let text = std::fs::read_to_string(directory.join("running.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Asks the running host to report its status, stop, or reload its network
/// settings. Fails when no host answers on the recorded port. Each request
/// opens its own connection: a pooled one may belong to a host that stopped.
pub fn lifecycle(state: &RunningHost, action: LifecycleAction) -> Result<HostStatus, String> {
    let agent = ureq::AgentBuilder::new()
        .max_idle_connections(0)
        .timeout(Duration::from_secs(5))
        .redirects(0)
        .build();
    let response = agent
        .post(&format!("http://127.0.0.1:{}/lifecycle", state.port))
        .set("Authorization", &format!("Bearer {}", state.secret))
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({ "action": action.name() }).to_string());
    let response = match response {
        Ok(response) if response.status() == 200 => response,
        Ok(_) | Err(ureq::Error::Status(..)) => {
            return Err("Could not verify the running host".into());
        }
        Err(ureq::Error::Transport(error)) => {
            return Err(match error.kind() {
                ureq::ErrorKind::Io => "The host did not answer".into(),
                _ => error.to_string(),
            });
        }
    };
    let mut text = String::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

/// The running host's status, or `None` when none answers.
pub fn running_status(directory: &Path) -> Option<RunningStatus> {
    let state = read_running(directory)?;
    let status = lifecycle(&state, LifecycleAction::Status).ok()?;
    Some(RunningStatus { status, state })
}

/// Compares `a.b.c` versions; prerelease suffixes sort before the release.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    let parse = |value: &str| -> (Vec<f64>, String) {
        // `value.split("-", 2)` keeps only the first two pieces.
        let mut pieces = value.split('-');
        let core = pieces.next().unwrap_or("");
        let pre = pieces.next().unwrap_or("").to_string();
        (
            core.split('.')
                .map(super::protocol::js_number_or_zero)
                .collect(),
            pre,
        )
    };
    let (left, left_pre) = parse(a);
    let (right, right_pre) = parse(b);
    for i in 0..3 {
        let delta = left.get(i).copied().unwrap_or(0.0) - right.get(i).copied().unwrap_or(0.0);
        if delta != 0.0 {
            return if delta > 0.0 { 1 } else { -1 };
        }
    }
    if left_pre == right_pre {
        0
    } else if left_pre.is_empty() {
        1
    } else if right_pre.is_empty() || left_pre < right_pre {
        -1
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prereleases_sort_before_their_release() {
        assert_eq!(compare_versions("0.6.0", "0.5.9"), 1);
        assert_eq!(compare_versions("0.6.0-beta.1", "0.6.0"), -1);
        assert_eq!(compare_versions("0.6.0", "0.6.0-beta.1"), 1);
        assert_eq!(compare_versions("0.6.0-a", "0.6.0-b"), -1);
        assert_eq!(compare_versions("1.0", "1.0.0"), 0);
    }

    #[test]
    fn reads_the_state_node_hosts_wrote_and_fails_without_a_host() {
        let directory = crate::host::store::tests::temporary("monocode-control-");
        assert_eq!(read_running(directory.path()), None);
        std::fs::write(
            directory.path().join("running.json"),
            r#"{"pid":42,"port":1,"secret":"s"}"#,
        )
        .unwrap();
        let state = read_running(directory.path()).unwrap();
        assert_eq!(state.pid, 42);
        assert!(lifecycle(&state, LifecycleAction::Status).is_err());
        assert_eq!(running_status(directory.path()), None);
    }
}
