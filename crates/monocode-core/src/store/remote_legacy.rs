//! Read-only bindings for the original desktop's remote shell tabs. Original
//! session IDs, metadata, block history, and WebKit preferences stay intact.
use super::{desktop_session_handle, RemoteSession, Session, Store};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

const HIDDEN: &str = "remote-hidden-legacy.v1";
const OWNERS: &str = "remote-legacy-command-owners.v1";

impl Store {
    pub(super) fn legacy_remote_sessions(&self) -> Result<BTreeMap<i64, RemoteSession>, String> {
        let sql = if self.shared.is_some() {
            "SELECT id FROM sessions WHERE cwd >= 'remote://' AND cwd < 'remote:/0'"
        } else {
            "SELECT CAST(id AS TEXT) FROM native_sessions WHERE cwd >= 'remote://' AND cwd < 'remote:/0'"
        };
        let mut query = self.conn.prepare(sql).map_err(|error| error.to_string())?;
        let originals: Vec<String> = query
            .query_map([], |row| row.get(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let mut result = BTreeMap::new();
        for original in originals {
            let id = if self.shared.is_some() {
                desktop_session_handle(&original)
            } else {
                original.parse::<i64>().map_err(|error| error.to_string())?
            };
            if let Some(remote) = self.legacy_remote_session(id)? {
                result.insert(id, remote);
            }
        }
        Ok(result)
    }

    pub(super) fn legacy_remote_session(&self, id: i64) -> Result<Option<RemoteSession>, String> {
        if id < 0 {
            return Ok(None);
        }
        let Some(original) = self.legacy_remote_original_id(id) else {
            return Ok(None);
        };
        let Some(summary) = self.legacy_source_summary(id, &original)? else {
            return Ok(None);
        };
        let projects = self.remote_projects()?;
        let preferences = self.desktop_preferences();
        let tabs = preferences
            .and_then(|prefs| prefs.json("monocode.remote-tabs.v2"))
            .unwrap_or(json!({}));
        let Some(project) = projects
            .values()
            .find(|project| {
                crate::files::path_key(&project.key) == crate::files::path_key(&summary.cwd)
            })
            .cloned()
        else {
            return Ok(None);
        };
        let session_id = tabs[&original]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let cached = session_id.as_ref().and_then(|id| {
            preferences?
                .json(&format!("monocode.remote-history.v2:{}", project.key))?
                .as_array()?
                .iter()
                .find(|item| item["id"].as_str() == Some(id))
                .cloned()
        });
        let mut remote = RemoteSession {
            summary,
            project,
            session_id,
            outbox: vec![],
            uploads: vec![],
            rejected: None,
            running: false,
            needs_input: false,
            configuration: None,
        };
        if let Some(cached) = cached {
            apply_cached(&mut remote, &cached);
        }
        if let Some(preferences) = preferences {
            let owners = self.legacy_command_owners()?;
            let mut ambiguous = Vec::new();
            let prefix = format!(
                "monocode.remote-command.v1:{}:",
                json!([remote.project.key, remote.project.environment_id])
            );
            let candidate_creates = preferences
                .entries()
                .filter(|(key, _)| key.starts_with(&prefix))
                .filter_map(|(key, raw)| Some((key, serde_json::from_str::<Value>(raw).ok()?)))
                .filter(|(key, entry)| {
                    let command = entry.get("command").unwrap_or(entry);
                    command["type"] == "create"
                        && command["commandId"].is_string()
                        && entry["shellId"]
                            .as_str()
                            .is_none_or(|shell| shell == original)
                        && owners.get(*key).is_none_or(|owner| *owner == id)
                })
                .count();
            for (key, raw) in preferences
                .entries()
                .filter(|(key, _)| key.starts_with(&prefix))
            {
                let Ok(entry) = serde_json::from_str::<Value>(raw) else {
                    continue;
                };
                let command = entry.get("command").unwrap_or(&entry);
                let matches = if command["type"] == "create" {
                    if remote.session_id.is_some() {
                        false
                    } else if let Some(shell) = entry["shellId"].as_str() {
                        shell == original
                    } else if let Some(owner) = owners.get(key) {
                        *owner == id
                    } else {
                        let candidates = self.legacy_unbound_tabs(&remote.project, &tabs)?;
                        let unique = candidate_creates == 1
                            && candidates.len() == 1
                            && candidates[0] == id
                            && command["projectId"].as_str()
                                == Some(remote.project.project_id.as_str());
                        if !unique && candidates.contains(&id) {
                            ambiguous.push(command.clone());
                            if let Some(followup) = entry.get("followup") {
                                ambiguous.push(followup.clone());
                            }
                        }
                        unique
                    }
                } else {
                    command["sessionId"].as_str() == remote.session_id.as_deref()
                        && remote.session_id.is_some()
                };
                if matches && command["commandId"].is_string() {
                    remote.outbox.push(command.clone());
                    if let Some(mut followup) = entry
                        .get("followup")
                        .filter(|value| value.is_object())
                        .cloned()
                    {
                        if followup["sessionId"] == "" {
                            followup["sessionId"] = Value::Null;
                        }
                        remote.outbox.push(followup);
                    }
                }
            }
            if !ambiguous.is_empty() {
                remote.rejected = Some(
                    json!({"legacyRecovery":true,"error":"This project has older pending requests that cannot be linked to one tab. Open the original desktop app to recover them. New requests and Retry are blocked here to avoid sending them twice.","commands":ambiguous}),
                );
            }
        }
        Ok(Some(remote))
    }

    fn legacy_remote_original_id(&self, id: i64) -> Option<String> {
        (id >= 0)
            .then(|| self.persisted_session_id(id).ok())
            .flatten()
    }

    fn legacy_command_owners(&self) -> Result<BTreeMap<String, i64>, String> {
        self.setting(OWNERS)?
            .map(|raw| serde_json::from_str(&raw).map_err(|error| error.to_string()))
            .unwrap_or_else(|| Ok(BTreeMap::new()))
    }

    fn legacy_unbound_tabs(
        &self,
        project: &super::RemoteProject,
        tabs: &Value,
    ) -> Result<Vec<i64>, String> {
        let sql = if self.shared.is_some() {
            "SELECT id,cwd FROM sessions WHERE cwd >= 'remote://' AND cwd < 'remote:/0'"
        } else {
            "SELECT CAST(id AS TEXT),cwd FROM native_sessions WHERE cwd >= 'remote://' AND cwd < 'remote:/0'"
        };
        let mut query = self.conn.prepare(sql).map_err(|error| error.to_string())?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?;
        let stored: BTreeMap<i64, RemoteSession> = self
            .setting("remote-sessions.v1")?
            .map(|raw| serde_json::from_str(&raw).map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_default();
        let hidden = self.hidden_legacy_remote_sessions()?;
        let mut result = Vec::new();
        for row in rows {
            let (original, cwd) = row.map_err(|error| error.to_string())?;
            let id = if self.shared.is_some() {
                desktop_session_handle(&original)
            } else {
                original
                    .parse()
                    .map_err(|error: std::num::ParseIntError| error.to_string())?
            };
            if crate::files::path_key(&cwd) == crate::files::path_key(&project.key)
                && tabs[&original].as_str().is_none_or(str::is_empty)
                && !hidden.contains(&id)
                && stored
                    .get(&id)
                    .is_none_or(|remote| remote.session_id.is_none())
            {
                result.push(id);
            }
        }
        Ok(result)
    }

    /// Record a deterministic legacy claim with the journal, so immutable old
    /// WebKit entries cannot later be adopted by another newly blank tab.
    pub(super) fn claim_legacy_remote_commands(
        &self,
        remote: &RemoteSession,
    ) -> Result<(), String> {
        let Some(preferences) = self.desktop_preferences() else {
            return Ok(());
        };
        if remote.outbox.is_empty() {
            return Ok(());
        }
        let prefix = format!(
            "monocode.remote-command.v1:{}:",
            json!([remote.project.key, remote.project.environment_id])
        );
        let mut owners = self.legacy_command_owners()?;
        let mut changed = false;
        for (key, raw) in preferences
            .entries()
            .filter(|(key, _)| key.starts_with(&prefix))
        {
            let Ok(entry) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            let command = entry.get("command").unwrap_or(&entry);
            if command["type"] == "create"
                && entry["shellId"].as_str().is_none()
                && remote
                    .outbox
                    .iter()
                    .any(|queued| queued["commandId"] == command["commandId"])
            {
                if owners
                    .get(key)
                    .is_some_and(|owner| *owner != remote.summary.id)
                {
                    return Err("This legacy request is already assigned to another tab".into());
                }
                changed |= owners.insert(key.into(), remote.summary.id) != Some(remote.summary.id);
            }
        }
        if changed {
            self.save_setting(
                OWNERS,
                &serde_json::to_string(&owners).map_err(|error| error.to_string())?,
            )?;
        }
        Ok(())
    }

    fn legacy_source_summary(&self, id: i64, original: &str) -> Result<Option<Session>, String> {
        let sql = if self.shared.is_some() {
            format!("SELECT title,harness,cwd,provider_session_id,updated_at,{},{},{},{} FROM sessions WHERE id=?1 AND cwd >= 'remote://' AND cwd < 'remote:/0'",self.shared_field("has_user_message","0"),self.shared_field("archived","0"),self.shared_field("pinned","0"),self.shared_field("linked_work_item_json","NULL"))
        } else {
            "SELECT title,provider,cwd,provider_id,updated_at,0,archived,pinned,work_item FROM native_sessions WHERE id=?1 AND cwd >= 'remote://' AND cwd < 'remote:/0'".into()
        };
        self.conn
            .query_row(&sql, [original], |row| {
                Ok(Session {
                    id,
                    title: row.get(0)?,
                    provider: row.get(1)?,
                    cwd: row.get(2)?,
                    provider_id: row.get(3)?,
                    updated_at: row.get::<_, i64>(4)?.max(0) as u64,
                    has_messages: row.get(5)?,
                    archived: row.get(6)?,
                    pinned: row.get(7)?,
                    work_item: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|raw| serde_json::from_str(&raw).ok()),
                })
            })
            .optional()
            .map_err(|error| error.to_string())
    }

    /// Execution directory for a pre-existing remote tab, qualified for the
    /// host filesystem even when its old pending-worktree preference was raw.
    pub(crate) fn legacy_remote_worktree(&self, id: i64) -> Result<Option<String>, String> {
        let Some(remote) = self.legacy_remote_session(id)? else {
            return Ok(None);
        };
        let Some(preferences) = self.desktop_preferences() else {
            return Ok(None);
        };
        let Some(original) = self.legacy_remote_original_id(id) else {
            return Ok(None);
        };
        let cached = remote.session_id.as_ref().and_then(|host| {
            preferences
                .json(&format!(
                    "monocode.remote-history.v2:{}",
                    remote.project.key
                ))?
                .as_array()?
                .iter()
                .find(|item| item["id"].as_str() == Some(host))
                .cloned()
        });
        let pending = preferences
            .json("monocode.remote-pending-worktrees.v1")
            .and_then(|all| all[&original].as_str().map(str::to_owned));
        let cwd = cached
            .as_ref()
            .and_then(|value| value["cwd"].as_str())
            .map(str::to_owned)
            .or(pending);
        Ok(cwd
            .filter(|cwd| cwd != &remote.project.cwd)
            .and_then(|cwd| {
                if let Some(path) = crate::remote::parse_remote_path(&cwd) {
                    (path.environment_id == remote.project.environment_id).then_some(cwd)
                } else {
                    Some(crate::remote::remote_path(
                        &remote.project.environment_id,
                        &cwd,
                    ))
                }
            }))
    }

    pub(super) fn hidden_legacy_remote_sessions(&self) -> Result<HashSet<i64>, String> {
        self.setting(HIDDEN)?
            .map(|raw| serde_json::from_str(&raw).map_err(|error| error.to_string()))
            .unwrap_or_else(|| Ok(HashSet::new()))
    }
    pub(super) fn hide_legacy_remote_sessions(
        &self,
        ids: impl IntoIterator<Item = i64>,
    ) -> Result<(), String> {
        let mut hidden = self.hidden_legacy_remote_sessions()?;
        hidden.extend(ids.into_iter().filter(|id| *id >= 0));
        self.save_setting(
            HIDDEN,
            &serde_json::to_string(&hidden).map_err(|error| error.to_string())?,
        )
    }
}

fn apply_cached(remote: &mut RemoteSession, cached: &Value) {
    let summary = &mut remote.summary;
    if let Some(title) = cached["title"].as_str() {
        summary.title = title.into();
    }
    if let Some(provider) = cached["harness"].as_str() {
        summary.provider = provider.into();
    }
    summary.provider_id = cached["providerSessionId"]
        .as_str()
        .map(str::to_owned)
        .or(summary.provider_id.clone());
    summary.has_messages = true;
    summary.updated_at = cached["updatedAt"].as_u64().unwrap_or(summary.updated_at);
    summary.archived = cached["archived"].as_bool().unwrap_or(summary.archived);
    summary.pinned = cached["pinned"].as_bool().unwrap_or(summary.pinned);
    remote.running = cached["status"] == "running";
    remote.needs_input = cached["needsInput"] == true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::Settings, desktop_preferences::DesktopPreferences};
    use std::{path::Path, sync::Arc};

    const PROJECT: &str = "remote://legacy/home/repo";

    fn fixture(path: &Path) -> (i64, String, i64, String) {
        let store = Store::open_monocode(path).unwrap();
        let bound = store.local_create("codex", PROJECT).unwrap();
        let blank = store.local_create("claude", PROJECT).unwrap();
        let bound_original = store.persisted_session_id(bound.id).unwrap();
        let blank_original = store.persisted_session_id(blank.id).unwrap();
        let settings = Settings {
            model: Some("gpt-saved".into()),
            ..Default::default()
        };
        store.save_agent_settings(bound.id, &settings).unwrap();
        (bound.id, bound_original, blank.id, blank_original)
    }

    fn preferences(bound: &str, blank: &str) -> DesktopPreferences {
        let command_key = format!(
            "monocode.remote-command.v1:{}:pending",
            json!([PROJECT, "legacy"])
        );
        DesktopPreferences::from_values(&[
            (
                "monocode.remote-projects.v2",
                json!({PROJECT:{"key":PROJECT,"environmentId":"legacy","projectId":"host-project","cwd":"/home/repo"}}),
            ),
            ("monocode.remote-tabs.v2", json!({bound:"host-session"})),
            (
                "monocode.remote-pending-worktrees.v1",
                json!({bound:"/home/repo-worktrees/old",blank:"/home/repo-worktrees/pending"}),
            ),
            (
                &format!("monocode.remote-history.v2:{PROJECT}"),
                json!([{"id":"host-session","title":"Host title","harness":"codex","cwd":"/home/repo-worktrees/current","updatedAt":123,"pinned":true}]),
            ),
            (
                &command_key,
                json!({"shellId":blank,"command":{"type":"create","commandId":"original-create","projectId":"host-project","harness":"claude","model":"claude:opus"},"followup":{"type":"send","commandId":"original-send","sessionId":"","text":"Keep this exact pending text"}}),
            ),
        ])
    }

    fn bare_preferences(bound: &str, blank: &str, ambiguous: bool) -> DesktopPreferences {
        let source = preferences(bound, blank);
        let entries: Vec<(String, Value)> = source
            .entries()
            .map(|(key, raw)| {
                let mut value: Value = serde_json::from_str(raw).unwrap();
                if key.starts_with("monocode.remote-command.v1:") {
                    value = value["command"].clone();
                }
                if ambiguous && key == "monocode.remote-tabs.v2" {
                    value = json!({});
                }
                (key.into(), value)
            })
            .collect();
        DesktopPreferences::from_values(
            &entries
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone()))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn bare_create_uses_the_unique_unbound_tab_and_keeps_its_claim_after_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let (_, original, blank, blank_original) = fixture(&path);
        let prefs = Arc::new(bare_preferences(&original, &blank_original, false));
        let store = Store::open_monocode_with_preferences_for_test(&path, prefs).unwrap();
        let mut remote = store.remote_session(blank).unwrap().unwrap();
        assert_eq!(remote.outbox.len(), 1);
        assert_eq!(remote.outbox[0]["commandId"], "original-create");
        store.save_remote_session(&remote).unwrap();
        remote.session_id = Some("accepted-host-session".into());
        remote.outbox.clear();
        store.save_remote_session(&remote).unwrap();
        let later = store.local_create("codex", PROJECT).unwrap();
        let later = store.remote_session(later.id).unwrap().unwrap();
        assert!(later.outbox.is_empty());
        assert!(later.rejected.is_none());
    }

    #[test]
    fn ambiguous_bare_create_is_visible_recovery_state_without_a_replayable_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let (bound, original, blank, blank_original) = fixture(&path);
        let before = std::fs::read(&path).unwrap();
        let prefs = Arc::new(bare_preferences(&original, &blank_original, true));
        let store = Store::read_monocode_with_preferences_for_test(&path, prefs).unwrap();
        for id in [bound, blank] {
            let remote = store.remote_session(id).unwrap().unwrap();
            assert!(remote.outbox.is_empty());
            let recovery = remote.rejected.unwrap();
            assert_eq!(recovery["legacyRecovery"], true);
            assert_eq!(recovery["commands"][0]["commandId"], "original-create");
            assert!(recovery["error"]
                .as_str()
                .unwrap()
                .contains("cannot be linked"));
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn multiple_bare_creates_cannot_be_replayed_into_one_blank_tab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let (_, original, blank, blank_original) = fixture(&path);
        let source = bare_preferences(&original, &blank_original, false);
        let mut entries: Vec<(String, Value)> = source
            .entries()
            .map(|(key, raw)| (key.into(), serde_json::from_str(raw).unwrap()))
            .collect();
        entries.push((format!("monocode.remote-command.v1:{}:second",json!([PROJECT,"legacy"])),json!({"type":"create","commandId":"second-create","projectId":"host-project","harness":"claude","model":"claude:opus"})));
        let prefs = DesktopPreferences::from_values(
            &entries
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone()))
                .collect::<Vec<_>>(),
        );
        let store = Store::read_monocode_with_preferences_for_test(&path, Arc::new(prefs)).unwrap();
        let remote = store.remote_session(blank).unwrap().unwrap();
        assert!(remote.outbox.is_empty());
        let recovery = remote.rejected.unwrap();
        assert_eq!(recovery["legacyRecovery"], true);
        assert_eq!(recovery["commands"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn legacy_remote_tabs_restore_positive_handles_settings_and_outbox_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let (bound, original, blank, blank_original) = fixture(&path);
        let before = std::fs::read(&path).unwrap();
        let prefs = Arc::new(preferences(&original, &blank_original));
        let store = Store::read_monocode_with_preferences_for_test(&path, prefs.clone()).unwrap();
        let rows = store.sessions().unwrap();
        assert_eq!(rows.len(), 2);
        let remote = store.remote_session(bound).unwrap().unwrap();
        assert!(remote.summary.id > 0);
        assert_eq!(remote.summary.id, bound);
        assert_eq!(remote.session_id.as_deref(), Some("host-session"));
        assert_eq!(remote.summary.title, "Host title");
        assert_eq!(store.persisted_session_id(bound).unwrap(), original);
        assert_eq!(
            store
                .agent_settings(bound)
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            Some("gpt-saved")
        );
        assert_eq!(
            store.legacy_remote_worktree(bound).unwrap().as_deref(),
            Some("remote://legacy/home/repo-worktrees/current")
        );
        assert_eq!(
            store.legacy_remote_worktree(blank).unwrap().as_deref(),
            Some("remote://legacy/home/repo-worktrees/pending")
        );
        let draft = store.remote_session(blank).unwrap().unwrap();
        assert!(draft.session_id.is_none());
        assert_eq!(draft.outbox.len(), 2);
        assert_eq!(draft.outbox[0]["commandId"], "original-create");
        assert_eq!(draft.outbox[1]["commandId"], "original-send");
        assert!(draft.outbox[1]["sessionId"].is_null());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            prefs.json("monocode.remote-tabs.v2").unwrap()[original],
            "host-session"
        );
    }

    #[test]
    fn native_host_refresh_reuses_legacy_handle_and_deletion_cannot_resurrect_shell_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop.db");
        let (bound, original, _, blank_original) = fixture(&path);
        let prefs = Arc::new(preferences(&original, &blank_original));
        let store = Store::open_monocode_with_preferences_for_test(&path, prefs).unwrap();
        let project = store.remote_projects().unwrap()[PROJECT].clone();
        let refreshed=store.register_remote_summary(&project,&json!({"id":"host-session","title":"Fresh host title","harness":"codex","updatedAt":456})).unwrap();
        assert_eq!(refreshed.id, bound);
        assert_eq!(
            store
                .sessions()
                .unwrap()
                .iter()
                .filter(|row| row.id == bound)
                .count(),
            1
        );
        store
            .prune_remote_sessions(&project, &HashSet::new())
            .unwrap();
        assert!(store.remote_session(bound).unwrap().is_none());
        assert!(!store.sessions().unwrap().iter().any(|row| row.id == bound));
        let source_id: String = store
            .conn
            .query_row("SELECT id FROM sessions WHERE id=?1", [&original], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(source_id, original);
    }
}
