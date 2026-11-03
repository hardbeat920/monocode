//! Host-owned sessions keep only their desktop identity and command receipts here.
//! Transcripts remain on the host and use the same lossless block projection as
//! desktop conversations; no remote row is inserted into the local session table.
use super::{shared, Session, Store};
use crate::{
    agent::{Settings, TurnInfo},
    remote::{Connections, Machine},
    transcript::Transcript,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

const PROJECTS: &str = "remote-projects.v2";
const SESSIONS: &str = "remote-sessions.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub key: String,
    pub environment_id: String,
    pub project_id: String,
    pub cwd: String,
}

/// Local bytes needed to retry the same journaled attachment references.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteUpload {
    pub command_id: String,
    pub id: String,
    pub path: std::path::PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteSession {
    pub summary: Session,
    pub project: RemoteProject,
    pub session_id: Option<String>,
    /// Recorded before dispatch, and removed only after a definite receipt.
    #[serde(default)]
    pub outbox: Vec<Value>,
    #[serde(default)]
    pub uploads: Vec<RemoteUpload>,
    /// Definite host rejections retain the original text for explicit recovery.
    #[serde(default)]
    pub rejected: Option<Value>,
    /// Last full host snapshot, used to reconcile changes from other desktops.
    #[serde(default)]
    pub configuration: Option<Value>,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub needs_input: bool,
}
impl RemoteSession {
    pub fn machine(&self, client: &Connections) -> Result<Machine, String> {
        client
            .list()?
            .into_iter()
            .find(|machine| machine.environment_id == self.project.environment_id)
            .ok_or_else(|| {
                "Machine is no longer connected. Reconnect in Settings → Connections.".into()
            })
    }
}
fn host_settings(project: &RemoteProject, host: &Value) -> Settings {
    let mut settings = Settings::default();
    settings.model = host["model"].as_str().map(str::to_owned);
    settings.access =
        crate::agent::AccessMode::from_id(host["runtimeMode"].as_str().unwrap_or("supervised"));
    settings.model_settings = host
        .get("modelSettings")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    settings.effort = settings
        .model_settings
        .get("reasoningEffort")
        .or_else(|| settings.model_settings.get("effort"))
        .cloned();
    settings.fast = settings
        .model_settings
        .get("fast")
        .is_some_and(|value| value == "true");
    settings.service_tier = settings.model_settings.get("serviceTier").cloned();
    settings.thinking = settings
        .model_settings
        .get("thinking")
        .and_then(|value| value.parse::<bool>().ok());
    if let Some(cwd) = host["cwd"].as_str().filter(|cwd| *cwd != project.cwd) {
        settings.worktree_cwd = Some(crate::remote::remote_path(&project.environment_id, cwd));
    }
    settings
}

fn adopt_host_configuration(settings: &mut Settings, host: Settings) {
    settings.model = host.model;
    settings.access = host.access;
    settings.model_settings = host.model_settings;
    settings.effort = host.effort;
    settings.fast = host.fast;
    settings.service_tier = host.service_tier;
    settings.thinking = host.thinking;
    settings.worktree_cwd = host.worktree_cwd;
    settings.revision = settings.revision.saturating_add(1);
}

impl Store {
    pub fn remote_projects(&self) -> Result<BTreeMap<String, RemoteProject>, String> {
        let mut projects: BTreeMap<String, RemoteProject> = self
            .desktop_preferences()
            .and_then(|prefs| prefs.get("monocode.remote-projects.v2"))
            .map(|raw| {
                serde_json::from_str(raw)
                    .map_err(|e| format!("Cannot read desktop remote projects: {e}"))
            })
            .unwrap_or_else(|| Ok(BTreeMap::new()))?;
        if let Some(raw) = self.setting(PROJECTS)? {
            projects.extend(
                serde_json::from_str::<BTreeMap<String, RemoteProject>>(&raw)
                    .map_err(|e| format!("Cannot read remote projects: {e}"))?,
            );
        }
        Ok(projects)
    }
    pub fn remember_remote_project(
        &self,
        environment_id: &str,
        project: &Value,
    ) -> Result<RemoteProject, String> {
        let cwd = project["cwd"]
            .as_str()
            .ok_or("Host project has no directory")?;
        let id = project["id"]
            .as_str()
            .ok_or("Host project has no identity")?;
        let remote = RemoteProject {
            key: crate::remote::remote_path(
                environment_id,
                cwd.replace('\\', "/").trim_end_matches('/'),
            ),
            environment_id: environment_id.into(),
            project_id: id.into(),
            cwd: cwd.into(),
        };
        let mut projects = self.remote_projects()?;
        projects.insert(remote.key.clone(), remote.clone());
        self.save_setting(
            PROJECTS,
            &serde_json::to_string(&projects).map_err(|e| e.to_string())?,
        )?;
        Ok(remote)
    }
    fn stored_remote_sessions(&self) -> Result<BTreeMap<i64, RemoteSession>, String> {
        self.setting(SESSIONS)?
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|e| format!("Cannot read remote sessions: {e}"))
            })
            .unwrap_or_else(|| Ok(BTreeMap::new()))
    }
    fn remote_sessions(&self) -> Result<BTreeMap<i64, RemoteSession>, String> {
        let hidden = self.hidden_legacy_remote_sessions()?;
        let mut sessions = self.legacy_remote_sessions()?;
        sessions.retain(|id, _| !hidden.contains(id));
        for (id, mut saved) in self.stored_remote_sessions()? {
            let canonical = saved
                .session_id
                .as_ref()
                .and_then(|host| {
                    sessions.values().find(|legacy| {
                        legacy.summary.id >= 0
                            && legacy.project.environment_id == saved.project.environment_id
                            && legacy.session_id.as_ref() == Some(host)
                    })
                })
                .map(|legacy| legacy.summary.id)
                .unwrap_or(id);
            saved.summary.id = canonical;
            sessions.insert(canonical, saved);
        }
        Ok(sessions)
    }
    pub fn remote_session(&self, id: i64) -> Result<Option<RemoteSession>, String> {
        let mut stored = self.stored_remote_sessions()?;
        if let Some(remote) = stored.remove(&id) {
            if remote
                .rejected
                .as_ref()
                .is_some_and(|recovery| recovery["legacyRecovery"] == true)
            {
                // Re-check immutable source preferences on a subsequent store
                // open after the original app has completed recovery.
                if let Some(legacy) = self.legacy_remote_session(id)? {
                    return Ok(Some(legacy));
                }
            }
            return Ok(Some(remote));
        }
        if id >= 0 && !self.hidden_legacy_remote_sessions()?.contains(&id) {
            if let Some(legacy) = self.legacy_remote_session(id)? {
                if let Some(mut saved) = legacy
                    .session_id
                    .as_ref()
                    .and_then(|host| {
                        stored.values().find(|saved| {
                            saved.project.environment_id == legacy.project.environment_id
                                && saved.session_id.as_ref() == Some(host)
                        })
                    })
                    .cloned()
                {
                    saved.summary.id = id;
                    return Ok(Some(saved));
                }
                return Ok(Some(legacy));
            }
        }
        Ok(None)
    }
    pub fn save_remote_session(&self, session: &RemoteSession) -> Result<(), String> {
        self.claim_legacy_remote_commands(session)?;
        let mut sessions = self.stored_remote_sessions()?;
        if session.summary.id >= 0 && session.session_id.is_some() {
            sessions.retain(|id, existing| {
                *id == session.summary.id
                    || existing.project.environment_id != session.project.environment_id
                    || existing.session_id != session.session_id
            });
        }
        sessions.insert(session.summary.id, session.clone());
        self.save_setting(
            SESSIONS,
            &serde_json::to_string(&sessions).map_err(|e| e.to_string())?,
        )
    }
    pub fn sessions(&self) -> Result<Vec<Session>, String> {
        let hidden = self.hidden_legacy_remote_sessions()?;
        let mut sessions: BTreeMap<_, _> = self
            .local_sessions()?
            .into_iter()
            .filter(|summary| !hidden.contains(&summary.id))
            .map(|summary| (summary.id, summary))
            .collect();
        sessions.extend(
            self.remote_sessions()?
                .into_iter()
                .map(|(id, remote)| (id, remote.summary)),
        );
        let mut sessions: Vec<_> = sessions.into_values().collect();
        sessions.sort_by_key(|s| (std::cmp::Reverse(s.updated_at), std::cmp::Reverse(s.id)));
        Ok(sessions)
    }
    pub fn create(&self, provider: &str, cwd: &str) -> Result<Session, String> {
        if !cwd.starts_with("remote://") {
            return self.local_create(provider, cwd);
        }
        let kind = crate::provider::Kind::parse(provider).ok_or("Unknown remote provider")?;
        let project = self
            .remote_projects()?
            .into_values()
            .find(|project| {
                project.key.trim_end_matches('/') == cwd.trim_end_matches('/')
                    || crate::remote::remote_path(
                        &project.environment_id,
                        project.cwd.replace('\\', "/").trim_end_matches('/'),
                    )
                    .trim_end_matches('/')
                        == cwd.trim_end_matches('/')
            })
            .ok_or("Open this folder on its machine again to restore its project identity")?;
        let id = -shared::handle(&format!("remote:blank:{}", uuid::Uuid::new_v4()));
        let summary = Session {
            id,
            title: format!("New {} session", kind.label()),
            provider: provider.into(),
            cwd: project.key.clone(),
            provider_id: None,
            has_messages: false,
            updated_at: crate::time::now_ms(),
            archived: false,
            pinned: false,
            work_item: None,
        };
        self.save_remote_session(&RemoteSession {
            summary: summary.clone(),
            project,
            session_id: None,
            outbox: vec![],
            uploads: vec![],
            rejected: None,
            configuration: None,
            running: false,
            needs_input: false,
        })?;
        Ok(summary)
    }
    /// Reconcile only after a complete, successful host session listing. Draft
    /// desktop handles and commands awaiting receipts survive external deletion.
    pub fn prune_remote_sessions(
        &self,
        project: &RemoteProject,
        live_host_ids: &std::collections::HashSet<String>,
    ) -> Result<(), String> {
        let mut sessions = self.remote_sessions()?;
        let before: std::collections::HashSet<_> = sessions.keys().copied().collect();
        sessions.retain(|_, remote| {
            remote.project.environment_id != project.environment_id
                || remote.project.project_id != project.project_id
                || remote
                    .session_id
                    .as_ref()
                    .is_none_or(|id| live_host_ids.contains(id))
                || !remote.outbox.is_empty()
                || !remote.uploads.is_empty()
        });
        self.hide_legacy_remote_sessions(
            before.into_iter().filter(|id| !sessions.contains_key(id)),
        )?;
        self.save_setting(
            SESSIONS,
            &serde_json::to_string(&sessions).map_err(|e| e.to_string())?,
        )
    }

    pub fn register_remote_summary(
        &self,
        project: &RemoteProject,
        value: &Value,
    ) -> Result<Session, String> {
        let host = value.get("session").unwrap_or(value);
        let host_id = host["id"]
            .as_str()
            .ok_or("Host session identity is missing")?;
        let mut sessions = self.remote_sessions()?;
        let id = sessions
            .values()
            .find(|r| {
                r.project.environment_id == project.environment_id
                    && r.session_id.as_deref() == Some(host_id)
            })
            .map_or_else(
                || -shared::handle(&format!("remote:{}:{host_id}", project.environment_id)),
                |r| r.summary.id,
            );
        let prior = sessions.remove(&id);
        let summary = Session {
            id,
            title: host["title"].as_str().unwrap_or("Remote session").into(),
            provider: host["harness"].as_str().unwrap_or("codex").into(),
            cwd: project.key.clone(),
            provider_id: host["providerSessionId"].as_str().map(str::to_owned),
            has_messages: host
                .get("blocks")
                .and_then(Value::as_array)
                .is_none_or(|b| !b.is_empty()),
            updated_at: value["updatedAt"].as_u64().unwrap_or(0),
            archived: value["archived"].as_bool().unwrap_or(false),
            pinned: value["pinned"].as_bool().unwrap_or(false),
            work_item: host
                .get("linkedWorkItem")
                .filter(|v| !v.is_null())
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
        };
        let imported = host_settings(project, host);
        let full_snapshot = value.get("session").is_some_and(Value::is_object);
        let configuration = if full_snapshot {
            Some(crate::remote::protocol::settings_configuration(
                &summary.provider,
                &imported,
            ))
        } else {
            prior
                .as_ref()
                .and_then(|remote| remote.configuration.clone())
        };
        let existing = self.agent_settings(id)?;
        let mut settings_to_save = None;
        match existing {
            None => settings_to_save = Some(imported),
            Some(mut settings) if full_snapshot => {
                let pending = prior
                    .as_ref()
                    .is_some_and(|remote| !remote.outbox.is_empty() || !remote.uploads.is_empty());
                let local =
                    crate::remote::protocol::settings_configuration(&summary.provider, &settings);
                let unchanged = prior
                    .as_ref()
                    .and_then(|remote| remote.configuration.as_ref())
                    .map_or(settings.revision == 0, |baseline| baseline == &local);
                if !pending
                    && unchanged
                    && (configuration.as_ref() != Some(&local)
                        || settings.worktree_cwd != imported.worktree_cwd)
                {
                    adopt_host_configuration(&mut settings, imported);
                    settings_to_save = Some(settings);
                }
            }
            _ => {}
        }
        if let Some(settings) = settings_to_save {
            self.save_agent_settings(id, &settings)?;
        }
        let remote = RemoteSession {
            summary: summary.clone(),
            project: project.clone(),
            session_id: Some(host_id.into()),
            outbox: prior.as_ref().map(|r| r.outbox.clone()).unwrap_or_default(),
            uploads: prior
                .as_ref()
                .map(|r| r.uploads.clone())
                .unwrap_or_default(),
            rejected: prior.as_ref().and_then(|remote| remote.rejected.clone()),
            configuration,
            running: value["status"] == "running",
            needs_input: host["needsInput"] == true
                || host["pendingQuestion"].is_object()
                || host["blocks"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|b| {
                        b["approval"].is_object()
                            && b["approval"].get("decided").is_none_or(Value::is_null)
                    })
                }),
        };
        sessions.insert(id, remote);
        self.save_setting(
            SESSIONS,
            &serde_json::to_string(&sessions).map_err(|e| e.to_string())?,
        )?;
        Ok(summary)
    }
    pub fn project_remote_history(
        snapshot: &Value,
    ) -> Result<(Transcript, HashMap<usize, TurnInfo>), String> {
        let blocks = snapshot["session"]["blocks"]
            .as_array()
            .ok_or("Host transcript is missing")?;
        let blocks: Vec<_> = blocks
            .iter()
            .cloned()
            .map(|mut block| {
                if block["role"] == "plan" {
                    block["nativeRemotePlan"] = Value::Bool(true);
                }
                block
            })
            .collect();
        Ok((
            Transcript {
                rows: shared::blocks::project(&blocks)?,
            },
            shared::project_turns(&blocks)?,
        ))
    }
    pub fn patch_remote_session(&self, id: i64, patch: Value) -> Result<(), String> {
        let mut remote = self
            .remote_session(id)?
            .ok_or("Remote session no longer exists")?;
        if let Some(session_id) = &remote.session_id {
            let client = crate::remote::default_connections()?;
            let machine = remote.machine(&client)?;
            let mut params = patch.clone();
            params["sessionId"] = json!(session_id);
            params["projectId"] = json!(remote.project.project_id);
            client.request(&machine.id, "sessions.update", params)?;
        }
        if let Some(title) = patch["title"].as_str() {
            remote.summary.title = title.into();
        }
        if let Some(archived) = patch["archived"].as_bool() {
            remote.summary.archived = archived;
        }
        if let Some(pinned) = patch["pinned"].as_bool() {
            remote.summary.pinned = pinned;
        }
        if let Some(item) = patch.get("linkedWorkItem") {
            remote.summary.work_item = serde_json::from_value(item.clone()).ok();
        }
        self.save_remote_session(&remote)
    }
    pub fn delete_remote_session(&self, id: i64) -> Result<(), String> {
        let remote = self
            .remote_session(id)?
            .ok_or("Remote session no longer exists")?;
        if let Some(session_id) = &remote.session_id {
            let client = crate::remote::default_connections()?;
            let machine = remote.machine(&client)?;
            client.request(
                &machine.id,
                "sessions.delete",
                json!({"sessionId": session_id,"projectId": remote.project.project_id}),
            )?;
        }
        let mut sessions = self.remote_sessions()?;
        sessions.remove(&id);
        self.hide_legacy_remote_sessions([id])?;
        self.save_setting(
            SESSIONS,
            &serde_json::to_string(&sessions).map_err(|e| e.to_string())?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn project(store: &Store, environment: &str) -> RemoteProject {
        store
            .remember_remote_project(environment, &json!({"id":"project","cwd":"/host/repo"}))
            .unwrap()
    }
    fn snapshot(id: &str) -> Value {
        json!({"projectId":"project","revision":4,"status":"idle","updatedAt":20,
            "session":{"id":id,"harness":"codex","title":"Host conversation","model":"codex:live-model",
                "runtimeMode":"auto","modelSettings":{"reasoningEffort":"high"},"cwd":"/host/repo/worktree",
                "blocks":[{"id":"prompt","role":"user","text":"hello"}]}})
    }

    #[test]
    fn remote_metadata_and_original_outbox_survive_restart_without_local_sessions_or_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native.db");
        let store = Store::open(&path).unwrap();
        let project = project(&store, "env");
        let summary = store.create("codex", &project.key).unwrap();
        assert!(summary.id < 0);
        let mut binding = store.remote_session(summary.id).unwrap().unwrap();
        binding.outbox = vec![
            json!({"type":"create","commandId":"original-create"}),
            json!({"type":"send","commandId":"original-send","text":"keep me"}),
        ];
        binding.uploads = vec![RemoteUpload {
            command_id: "original-send".into(),
            id: uuid::Uuid::new_v4().to_string(),
            path: dir.path().join("photo.png"),
            size: 3,
        }];
        store.save_remote_session(&binding).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let restored = store.remote_session(summary.id).unwrap().unwrap();
        assert_eq!(restored.outbox, binding.outbox);
        assert_eq!(restored.uploads[0].id, binding.uploads[0].id);
        assert_eq!(store.sessions().unwrap()[0].id, summary.id);
        for table in ["native_sessions", "native_rows"] {
            let count: i64 = store
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0);
        }
    }

    #[test]
    fn host_refresh_preserves_selected_handle_pending_commands_and_unsent_settings() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store, "env");
        let summary = store.create("codex", &project.key).unwrap();
        let mut binding = store.remote_session(summary.id).unwrap().unwrap();
        binding.session_id = Some("host-session".into());
        binding
            .outbox
            .push(json!({"type":"send","commandId":"pending","text":"unsent"}));
        store.save_remote_session(&binding).unwrap();
        let settings = Settings {
            model: Some("codex:unsent-choice".into()),
            revision: 7,
            ..Default::default()
        };
        store.save_agent_settings(summary.id, &settings).unwrap();
        let refreshed = store
            .register_remote_summary(&project, &snapshot("host-session"))
            .unwrap();
        assert_eq!(refreshed.id, summary.id);
        assert_eq!(
            store.remote_session(summary.id).unwrap().unwrap().outbox,
            binding.outbox
        );
        assert_eq!(
            store.agent_settings(summary.id).unwrap().unwrap().model,
            settings.model
        );
        assert_eq!(store.sessions().unwrap().len(), 1);
    }

    #[test]
    fn imported_host_handles_are_scoped_to_environment_and_use_remote_worktree_paths() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let one = project(&store, "one");
        let two = project(&store, "two");
        let first = store
            .register_remote_summary(&one, &snapshot("same-host-id"))
            .unwrap();
        let second = store
            .register_remote_summary(&two, &snapshot("same-host-id"))
            .unwrap();
        assert_ne!(first.id, second.id);
        let settings = store.agent_settings(first.id).unwrap().unwrap();
        assert_eq!(
            settings.worktree_cwd.as_deref(),
            Some("remote://one/host/repo/worktree")
        );
        assert_eq!(settings.access, crate::agent::AccessMode::Auto);
        assert_eq!(settings.model_settings["reasoningEffort"], "high");
    }

    #[test]
    fn resolved_approval_strings_do_not_keep_needs_input_set() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store, "env");
        for decision in [
            Value::Null,
            json!("allow"),
            json!("deny"),
            json!("cancelled"),
        ] {
            let mut value = snapshot("session");
            value["session"]["blocks"] =
                json!([{"id":"tool","role":"tool","approval":{"requestId":3,"decided":decision}}]);
            let summary = store.register_remote_summary(&project, &value).unwrap();
            assert_eq!(
                store
                    .remote_session(summary.id)
                    .unwrap()
                    .unwrap()
                    .needs_input,
                decision.is_null()
            );
        }
    }

    #[test]
    fn pruning_external_deletions_keeps_pending_and_unbound_sessions() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store, "env");
        let blank = store.create("codex", &project.key).unwrap();
        let gone = store
            .register_remote_summary(&project, &snapshot("gone"))
            .unwrap();
        let pending = store
            .register_remote_summary(&project, &snapshot("pending"))
            .unwrap();
        let mut binding = store.remote_session(pending.id).unwrap().unwrap();
        binding.outbox.push(json!({"commandId":"keep-original"}));
        store.save_remote_session(&binding).unwrap();
        let present = store
            .register_remote_summary(&project, &snapshot("present"))
            .unwrap();
        let other = self::project(&store, "other-env");
        let elsewhere = store
            .register_remote_summary(&other, &snapshot("gone"))
            .unwrap();
        store
            .prune_remote_sessions(
                &project,
                &std::collections::HashSet::from(["present".into()]),
            )
            .unwrap();
        assert!(store.remote_session(gone.id).unwrap().is_none());
        for id in [blank.id, pending.id, present.id, elsewhere.id] {
            assert!(store.remote_session(id).unwrap().is_some());
        }
    }

    #[test]
    fn windows_project_keys_normalize_trailing_separators_and_find_legacy_unc_keys() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = store
            .remember_remote_project("env", &json!({"id":"project","cwd":r"C:\repo\"}))
            .unwrap();
        assert_eq!(project.key, "remote://env/C:/repo");
        let legacy = RemoteProject {
            key: "remote://env/server/share/repo".into(),
            environment_id: "env".into(),
            project_id: "unc".into(),
            cwd: r"\\server\share\repo".into(),
        };
        store
            .save_setting(
                PROJECTS,
                &serde_json::to_string(&BTreeMap::from([(legacy.key.clone(), legacy)])).unwrap(),
            )
            .unwrap();
        assert!(store
            .create("codex", "remote://env//server/share/repo")
            .is_ok());
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use std::path::Path;
    fn host(model: &str, settings: Value) -> Value {
        json!({"projectId":"project","revision":3,"status":"idle","updatedAt":1,"session":{
            "id":"host-session","harness":"claude","title":"Host","model":model,"runtimeMode":"auto",
            "modelSettings":settings,"cwd":"/host/repo","blocks":[]}})
    }
    fn project(store: &Store) -> RemoteProject {
        store
            .remember_remote_project("env", &json!({"id":"project","cwd":"/host/repo"}))
            .unwrap()
    }
    #[test]
    fn imported_common_controls_match_host_instead_of_overwriting_fast_and_effort() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store);
        let value = host(
            "claude:opus",
            json!({"fast":"true","effort":"max","thinking":"true","context":"1m"}),
        );
        let summary = store.register_remote_summary(&project, &value).unwrap();
        let settings = store.agent_settings(summary.id).unwrap().unwrap();
        assert!(settings.fast);
        assert_eq!(settings.effort.as_deref(), Some("max"));
        assert_eq!(settings.thinking, Some(true));
        let wire = crate::remote::protocol::wire_model_settings("claude", &settings);
        assert_eq!(wire["fast"], "true");
        assert_eq!(wire["effort"], "max");
        assert_eq!(wire["context"], "1m");
    }
    #[test]
    fn external_host_configuration_updates_unmodified_native_settings_and_revision() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store);
        let first = host("claude:sonnet", json!({}));
        let summary = store.register_remote_summary(&project, &first).unwrap();
        let revision = store.agent_settings(summary.id).unwrap().unwrap().revision;
        let mut next = host("claude:opus", json!({"fast":"true","effort":"high"}));
        next["session"]["runtimeMode"] = json!("full-access");
        store.register_remote_summary(&project, &next).unwrap();
        let settings = store.agent_settings(summary.id).unwrap().unwrap();
        assert_eq!(settings.model.as_deref(), Some("claude:opus"));
        assert!(settings.fast);
        assert_eq!(settings.access, crate::agent::AccessMode::FullAccess);
        assert!(settings.revision > revision);
        // Summary listings omit modelSettings and must not erase the baseline.
        let before = store
            .remote_session(summary.id)
            .unwrap()
            .unwrap()
            .configuration;
        store.register_remote_summary(&project,&json!({"id":"host-session","harness":"claude","title":"Listed","model":"claude:opus","runtimeMode":"full-access","updatedAt":10})).unwrap();
        assert_eq!(
            store
                .remote_session(summary.id)
                .unwrap()
                .unwrap()
                .configuration,
            before
        );
        assert!(store.agent_settings(summary.id).unwrap().unwrap().fast);
    }
    #[test]
    fn unsent_override_survives_host_changes_then_external_updates_resume_after_acceptance() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let project = project(&store);
        let summary = store
            .register_remote_summary(&project, &host("claude:sonnet", json!({})))
            .unwrap();
        let mut local = store.agent_settings(summary.id).unwrap().unwrap();
        local.model = Some("claude:local-choice".into());
        local.revision += 1;
        store.save_agent_settings(summary.id, &local).unwrap();
        store
            .register_remote_summary(
                &project,
                &host("claude:external-choice", json!({"effort":"high"})),
            )
            .unwrap();
        assert_eq!(
            store.agent_settings(summary.id).unwrap().unwrap().model,
            local.model
        );
        let accepted = crate::remote::protocol::wire_model_settings("claude", &local);
        store
            .register_remote_summary(&project, &host("claude:local-choice", json!(accepted)))
            .unwrap();
        store
            .register_remote_summary(
                &project,
                &host("claude:next-external", json!({"effort":"max"})),
            )
            .unwrap();
        assert_eq!(
            store
                .agent_settings(summary.id)
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            Some("claude:next-external")
        );
    }
}
