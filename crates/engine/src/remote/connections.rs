//! Port of the storage half of src/features/connections/model/connections.ts:
//! which host session each remote tab shows, the worktree a new remote tab
//! will use, the command outbox, and the cached session lists. The machine
//! list, the change feed, and machine status live in `remote_connections`;
//! session loading lives in `client`.
//!
//! Each function reads and writes `Kv` with the same keys and JSON values
//! localStorage held.

use monocode_remote::host::protocol::{HostCommand, HostSessionSummary};
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `CHANGE`: the machine list changed.
pub const MACHINES_CHANGED: &str = "monocode:remote-machines";
pub const REMOTE_HISTORY_CHANGE: &str = "monocode:remote-history";
pub const REMOTE_HISTORY_UPDATED: &str = "monocode:remote-history-updated";
pub const OPEN_CONNECTIONS_EVENT: &str = "monocode:open-connections";
pub const OPEN_REMOTE_PROJECT_EVENT: &str = "monocode:open-remote-project";
/// Dispatched when a watched machine reports session writes.
pub const REMOTE_CHANGES: &str = "monocode:remote-changes";

pub const TAB_KEY: &str = "monocode.remote-tabs.v2";
pub const WORKTREE_KEY: &str = "monocode.remote-pending-worktrees.v1";

/// `JSON.parse(localStorage.getItem(key) ?? "{}")` as an object. `None` when
/// the stored value does not parse or is not an object, where the
/// TypeScript threw and fell back.
fn read_record(kv: &Kv, key: &str) -> Option<Map<String, Value>> {
    let raw = kv.get_item(key).unwrap_or_else(|| "{}".into());
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(record)) => Some(record),
        _ => None,
    }
}

/// `record[id]` when it is a string.
fn record_string(kv: &Kv, key: &str, id: &str) -> Option<String> {
    read_record(kv, key)?
        .get(id)
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `all[id] = value` or `delete all[id]`, then save.
fn write_record_string(kv: &Kv, key: &str, id: &str, value: Option<&str>) {
    let Some(mut all) = read_record(kv, key) else {
        return;
    };
    match value {
        Some(value) => {
            all.insert(id.to_string(), Value::String(value.to_string()));
        }
        None => {
            all.remove(id);
        }
    }
    kv.set_item(key, &Value::Object(all).to_string());
}

/// `remotePendingWorktree`: the host checkout chosen for a tab whose session
/// does not exist yet.
pub fn remote_pending_worktree(kv: &Kv, shell_id: &str) -> Option<String> {
    record_string(kv, WORKTREE_KEY, shell_id)
}

/// `rememberRemotePendingWorktree`. `None` forgets it.
pub fn remember_remote_pending_worktree(kv: &Kv, shell_id: &str, path: Option<&str>) {
    write_record_string(kv, WORKTREE_KEY, shell_id, path);
}

/// `remoteSessionFor`: the host session a tab in a remote project shows;
/// none for a new session.
pub fn remote_session_for(kv: &Kv, shell_id: &str) -> Option<String> {
    record_string(kv, TAB_KEY, shell_id)
}

/// `rememberRemoteSession`, without the event. Use
/// `RemoteConnections::remember_remote_session`, which also announces
/// `REMOTE_HISTORY_CHANGE`.
pub fn remember_remote_session(kv: &Kv, shell_id: &str, session_id: Option<&str>) {
    write_record_string(kv, TAB_KEY, shell_id, session_id);
}

/// `remoteTabCwd`: the host checkout currently used by a remote tab.
pub fn remote_tab_cwd(kv: &Kv, project: &str, shell_id: Option<&str>) -> Option<String> {
    let shell_id = shell_id?;
    remote_session_for(kv, shell_id)
        .and_then(|session_id| cached_remote_session_summary(kv, project, &session_id))
        .and_then(|summary| summary.cwd)
        .or_else(|| remote_pending_worktree(kv, shell_id))
}

/// `pendingPrefix`.
pub fn pending_prefix(project: &str, environment: &str) -> String {
    let pair = serde_json::to_string(&[project, environment]).unwrap_or_default();
    format!("monocode.remote-command.v1:{pair}:")
}

/// One outbox entry. `followup` is the first message of a session whose
/// `create` has not been confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEntry {
    pub command: HostCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup: Option<HostCommand>,
}

/// `readPendingEntry`: the earlier desktop saved the bare command.
fn read_pending_entry(value: &str) -> Option<PendingEntry> {
    // TODO(port): the TypeScript threw on an entry it could not parse, which
    // failed the whole lookup. This skips the entry instead.
    let parsed: Value = serde_json::from_str(value).ok()?;
    if parsed.get("command").is_some() {
        serde_json::from_value(parsed).ok()
    } else {
        Some(PendingEntry {
            command: serde_json::from_value(parsed).ok()?,
            shell_id: None,
            followup: None,
        })
    }
}

/// `pendingRemoteFollowup`.
pub fn pending_remote_followup(
    kv: &Kv,
    project: &str,
    environment: &str,
    id: &str,
) -> Option<HostCommand> {
    let value = kv.get_item(&format!("{}{id}", pending_prefix(project, environment)))?;
    read_pending_entry(&value)?.followup
}

/// Which unconfirmed command `pending_remote_command` looks for. The
/// TypeScript passed `undefined`, `null`, or a session id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingScope<'a> {
    /// `undefined`: any command for the project.
    Any,
    /// `null`: a `create` this tab started, or one saved without a tab.
    NewSession,
    /// A command for this host session.
    Session(&'a str),
}

impl<'a> PendingScope<'a> {
    /// `sessionId ?? null`.
    pub fn for_session(session_id: Option<&'a str>) -> Self {
        session_id.map_or(Self::NewSession, Self::Session)
    }
}

fn command_session_id(command: &HostCommand) -> Option<&str> {
    match command {
        HostCommand::Create { .. } => None,
        HostCommand::Configure { session_id, .. }
        | HostCommand::Compact { session_id, .. }
        | HostCommand::Send { session_id, .. }
        | HostCommand::Draft { session_id, .. }
        | HostCommand::RemoveDraft { session_id, .. }
        | HostCommand::Cancel { session_id, .. }
        | HostCommand::Approve { session_id, .. }
        | HostCommand::Answer { session_id, .. } => Some(session_id),
    }
}

/// `pendingRemoteCommand`: a command whose result this desktop never saw.
pub fn pending_remote_command(
    kv: &Kv,
    project: &str,
    environment: &str,
    scope: PendingScope<'_>,
    shell_id: Option<&str>,
) -> Option<HostCommand> {
    let prefix = pending_prefix(project, environment);
    for key in kv.keys() {
        if !key.starts_with(&prefix) {
            continue;
        }
        let Some(entry) = kv
            .get_item(&key)
            .and_then(|value| read_pending_entry(&value))
        else {
            continue;
        };
        let matches = match scope {
            PendingScope::Any => true,
            PendingScope::NewSession => {
                matches!(entry.command, HostCommand::Create { .. })
                    && (entry.shell_id.as_deref().is_none_or(str::is_empty)
                        || entry.shell_id.as_deref() == shell_id)
            }
            PendingScope::Session(session_id) => {
                command_session_id(&entry.command) == Some(session_id)
            }
        };
        if matches {
            return Some(entry.command);
        }
    }
    None
}

/// `savePendingRemoteCommand`. Each command owns its storage entry, so a
/// late receipt from another pane never erases this pane's uncertain
/// request. The TypeScript threw when localStorage refused the write; `Kv`
/// writes cannot fail here.
pub fn save_pending_remote_command(
    kv: &Kv,
    project: &str,
    environment: &str,
    command: &HostCommand,
    shell_id: Option<&str>,
    followup: Option<&HostCommand>,
) {
    let id = command_id(command);
    let entry = PendingEntry {
        command: command.clone(),
        shell_id: shell_id.map(str::to_string),
        followup: followup
            .cloned()
            .or_else(|| pending_remote_followup(kv, project, environment, id)),
    };
    if let Ok(value) = serde_json::to_string(&entry) {
        kv.set_item(
            &format!("{}{id}", pending_prefix(project, environment)),
            &value,
        );
    }
}

/// `clearPendingRemoteCommand`.
pub fn clear_pending_remote_command(kv: &Kv, project: &str, environment: &str, command_id: &str) {
    kv.remove_item(&format!(
        "{}{command_id}",
        pending_prefix(project, environment)
    ));
}

/// A command's `commandId`.
pub fn command_id(command: &HostCommand) -> &str {
    match command {
        HostCommand::Create { command_id, .. }
        | HostCommand::Configure { command_id, .. }
        | HostCommand::Compact { command_id, .. }
        | HostCommand::Send { command_id, .. }
        | HostCommand::Draft { command_id, .. }
        | HostCommand::RemoveDraft { command_id, .. }
        | HostCommand::Cancel { command_id, .. }
        | HostCommand::Approve { command_id, .. }
        | HostCommand::Answer { command_id, .. } => command_id,
    }
}

/// `historyKey`.
pub fn history_key(project: &str) -> String {
    format!("monocode.remote-history.v2:{project}")
}

/// `cachedSessions`: the last session list read for a remote project.
pub fn cached_sessions(kv: &Kv, project: &str) -> Vec<HostSessionSummary> {
    let raw = kv
        .get_item(&history_key(project))
        .unwrap_or_else(|| "[]".into());
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Array(entries)) => entries
            .into_iter()
            .filter_map(|entry| serde_json::from_value(entry).ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// Save a session list read from the host.
pub fn cache_sessions(kv: &Kv, project: &str, sessions: &[HostSessionSummary]) {
    if let Ok(value) = serde_json::to_string(sessions) {
        kv.set_item(&history_key(project), &value);
    }
}

/// `cachedRemoteSessionSummary`.
pub fn cached_remote_session_summary(
    kv: &Kv,
    project: &str,
    session_id: &str,
) -> Option<HostSessionSummary> {
    cached_sessions(kv, project)
        .into_iter()
        .find(|session| session.id == session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::RuntimeMode;
    use monocode_remote::host::protocol::RemoteProvider;

    fn create() -> HostCommand {
        HostCommand::Create {
            command_id: "create-1".into(),
            project_id: "p".into(),
            worktree_cwd: None,
            auto_worktree_branch: None,
            harness: HarnessId::Codex as RemoteProvider,
            model: "test".into(),
            model_settings: None,
            runtime_mode: RuntimeMode::Supervised,
        }
    }

    fn followup() -> HostCommand {
        HostCommand::Send {
            command_id: "send-1".into(),
            session_id: String::new(),
            text: "First message".into(),
            attachments: None,
            intent: None,
            draft_block_id: None,
            plan_block_id: None,
        }
    }

    // remoteOutbox.test.ts
    #[test]
    fn isolates_unfinished_creates_by_tab_and_keeps_their_original_first_message_on_retry() {
        let kv = Kv::in_memory();
        save_pending_remote_command(
            &kv,
            "project",
            "env",
            &create(),
            Some("first-tab"),
            Some(&followup()),
        );
        assert_eq!(
            pending_remote_command(
                &kv,
                "project",
                "env",
                PendingScope::NewSession,
                Some("second-tab")
            ),
            None
        );
        assert_eq!(
            pending_remote_command(
                &kv,
                "project",
                "env",
                PendingScope::NewSession,
                Some("first-tab")
            ),
            Some(create())
        );
        save_pending_remote_command(&kv, "project", "env", &create(), Some("first-tab"), None);
        assert_eq!(
            pending_remote_followup(&kv, "project", "env", "create-1"),
            Some(followup())
        );
    }

    // remoteOutbox.test.ts
    #[test]
    fn can_recover_commands_saved_by_the_earlier_desktop() {
        let kv = Kv::in_memory();
        kv.set_item(
            "monocode.remote-command.v1:[\"project\",\"env\"]:create-1",
            &serde_json::to_string(&create()).unwrap(),
        );
        assert_eq!(
            pending_remote_command(
                &kv,
                "project",
                "env",
                PendingScope::NewSession,
                Some("first-tab")
            ),
            Some(create())
        );
    }

    #[test]
    fn finds_commands_by_session_and_clears_them() {
        let kv = Kv::in_memory();
        let send = HostCommand::Send {
            command_id: "send-2".into(),
            session_id: "s1".into(),
            text: "Hi".into(),
            attachments: Some(Vec::new()),
            intent: None,
            draft_block_id: None,
            plan_block_id: None,
        };
        save_pending_remote_command(&kv, "project", "env", &send, Some("tab"), None);
        assert_eq!(
            pending_remote_command(&kv, "project", "env", PendingScope::Session("s1"), None),
            Some(send.clone())
        );
        assert_eq!(
            pending_remote_command(&kv, "project", "env", PendingScope::Session("s2"), None),
            None
        );
        assert_eq!(
            pending_remote_command(&kv, "project", "env", PendingScope::NewSession, Some("tab")),
            None
        );
        assert_eq!(
            pending_remote_command(&kv, "project", "other", PendingScope::Any, None),
            None
        );
        clear_pending_remote_command(&kv, "project", "env", "send-2");
        assert_eq!(
            pending_remote_command(&kv, "project", "env", PendingScope::Any, None),
            None
        );
    }

    #[test]
    fn tab_bindings_and_pending_worktrees_round_trip() {
        let kv = Kv::in_memory();
        remember_remote_session(&kv, "tab", Some("host-1"));
        assert_eq!(remote_session_for(&kv, "tab").as_deref(), Some("host-1"));
        remember_remote_session(&kv, "tab", None);
        assert_eq!(remote_session_for(&kv, "tab"), None);

        remember_remote_pending_worktree(&kv, "tab", Some("/host/tree"));
        assert_eq!(
            remote_tab_cwd(&kv, "remote://env/repo", Some("tab")).as_deref(),
            Some("/host/tree")
        );
        let summary: HostSessionSummary = serde_json::from_value(serde_json::json!({
            "projectId": "p", "revision": 1, "status": "idle", "updatedAt": 0,
            "id": "host-1", "title": "Work", "harness": "codex", "cwd": "/host/repo",
        }))
        .unwrap();
        cache_sessions(&kv, "remote://env/repo", &[summary]);
        remember_remote_session(&kv, "tab", Some("host-1"));
        assert_eq!(
            remote_tab_cwd(&kv, "remote://env/repo", Some("tab")).as_deref(),
            Some("/host/repo")
        );
        assert_eq!(remote_tab_cwd(&kv, "remote://env/repo", None), None);

        // A stored value that is not an object is left alone.
        kv.set_item(TAB_KEY, "null");
        remember_remote_session(&kv, "tab", Some("host-2"));
        assert_eq!(kv.get_item(TAB_KEY).as_deref(), Some("null"));
        assert_eq!(remote_session_for(&kv, "tab"), None);
    }
}
