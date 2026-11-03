//! One-time copies of saved desktop conversations into the native store.
use super::Store;
use crate::{
    agent::Settings,
    provider::Kind,
    tool::{ToolCall, ToolState},
    transcript::Transcript,
};
use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Tauri's application-data location, keyed by its bundle identifier.
pub fn monocode_database() -> PathBuf {
    #[cfg(target_os = "macos")]
    let base = crate::accounts::home()
        .unwrap_or_default()
        .join("Library/Application Support");
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default());
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::accounts::home()
                .unwrap_or_default()
                .join(".local/share")
        });
    base.join("com.monocode.desktop/monocode.db")
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub imported: usize,
    pub existing: usize,
    pub skipped: usize,
    pub latest_project: Option<String>,
}

impl Store {
    /// Import each source session once. Existing native conversations, including
    /// previously imported copies, are never overwritten by a later launch.
    pub fn import_monocode(&mut self, path: &Path) -> Result<ImportReport, String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut source = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        source
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        // A read transaction includes committed WAL records and holds a consistent
        // snapshot even when the desktop app is still saving conversations.
        let source = source.transaction().map_err(|e| e.to_string())?;
        let result = import_sessions(self, &source, &path);
        result.map_err(|e| format!("Cannot import {}: {e}", path.display()))
    }
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(error.into())
}

fn import_sessions(
    store: &mut Store,
    source: &Connection,
    path: &Path,
) -> rusqlite::Result<ImportReport> {
    let columns = source
        .prepare("PRAGMA table_info(sessions)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    for required in [
        "id",
        "cwd",
        "harness",
        "model",
        "model_settings",
        "runtime_mode",
        "title",
        "provider_session_id",
        "blocks_json",
        "updated_at",
    ] {
        if !columns.contains(required) {
            return Err(invalid(format!(
                "Not a Monocode session database (missing {required})"
            )));
        }
    }
    let mut fields = "id,cwd,harness,model,model_settings,runtime_mode,title,provider_session_id,blocks_json,updated_at".to_owned();
    for optional in [
        "archived",
        "pinned",
        "worktree_cwd",
        "worktree_removed",
        "context_used",
        "context_window",
        "sidebar_hidden",
        "inbox_ask",
        "linked_work_item_json",
    ] {
        fields.push(',');
        fields.push_str(if columns.contains(optional) {
            optional
        } else {
            "NULL"
        });
    }
    // Sort only small metadata rows. Sorting full transcripts can spill hundreds
    // of megabytes to disk, and repeat launches shouldn't load imported bodies.
    let summaries = source
        .prepare("SELECT id,cwd FROM sessions ORDER BY updated_at DESC,id")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut sessions = source.prepare(&format!("SELECT {fields} FROM sessions WHERE id=?1"))?;
    let tx = store.conn.transaction()?;
    let mut report = ImportReport::default();
    let source_key = path.to_string_lossy();
    for (source_id, cwd) in summaries {
        let key = format!("import:monocode:{}", json!([source_key, source_id]));
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM native_settings WHERE key=?1)",
            [&key],
            |r| r.get(0),
        )?;
        if exists {
            report.existing += 1;
            report.latest_project.get_or_insert(cwd);
            continue;
        }
        let mut rows = sessions.query([&source_id])?;
        let session = rows.next()?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let provider: String = session.get(2)?;
        // Internal workers and remote connections need their own migration;
        // don't expose them as ordinary local chats.
        if session.get::<_, Option<bool>>(16)?.unwrap_or(false)
            || session.get::<_, Option<String>>(17)?.is_some()
            || !Path::new(&cwd).is_absolute()
            || Kind::parse(&provider).is_none()
        {
            report.skipped += 1;
            continue;
        }
        let raw: String = session.get(8)?;
        let blocks: Vec<Value> = serde_json::from_str(&raw).map_err(invalid)?;
        // Empty/Mono-only histories and unsent drafts aren't completed chats.
        if !blocks
            .iter()
            .any(|b| b["role"] == "user" && b["draft"] != true && b["internal"] != true)
        {
            report.skipped += 1;
            continue;
        }
        let model_settings: String = session.get(4)?;
        let model_settings: HashMap<String, String> =
            serde_json::from_str(&model_settings).map_err(invalid)?;
        let mut settings = Settings {
            model: native_model(&session.get::<_, String>(3)?, &provider),
            effort: ["effort", "reasoningEffort", "reasoning_effort", "variant"]
                .into_iter()
                .find_map(|key| model_settings.get(key).cloned()),
            access: crate::agent::AccessMode::from_id(&session.get::<_, String>(5)?),
            fast: model_settings.get("fast").is_some_and(|v| v == "true"),
            service_tier: model_settings.get("serviceTier").cloned(),
            thinking: model_settings.get("thinking").and_then(|v| v.parse().ok()),
            model_settings,
            worktree_cwd: session.get(12)?,
            worktree_removed: session.get::<_, Option<bool>>(13)?.unwrap_or(false),
            ..Settings::default()
        };
        let transcript = transcript(&blocks)?;
        let mut offset = 0;
        for block in &blocks {
            let rows = self::transcript(std::slice::from_ref(block))?.rows;
            if let Some(prompt) = rows.iter().rposition(crate::transcript::Row::is_prompt) {
                if let Some(info) = super::shared::project_turns(std::slice::from_ref(block))
                    .map_err(invalid)?
                    .into_values()
                    .next()
                {
                    settings.turns.insert(offset + prompt + 1, info);
                }
            }
            offset += rows.len();
        }
        tx.execute("INSERT INTO native_sessions(title,provider,cwd,provider_id,updated_at,archived,pinned,work_item) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![session.get::<_, String>(6)?, provider, cwd, session.get::<_, Option<String>>(7)?,
                session.get::<_, i64>(9)?, session.get::<_, Option<bool>>(10)?.unwrap_or(false),
                session.get::<_, Option<bool>>(11)?.unwrap_or(false), session.get::<_, Option<String>>(18)?])?;
        let id = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare("INSERT INTO native_rows(session_id,row_ix,role,body,first,sent_at) VALUES(?1,?2,?3,?4,?5,?6)")?;
            for (ix, row) in transcript.rows.iter().enumerate() {
                insert.execute(params![
                    id,
                    ix as i64,
                    row.role,
                    row.text,
                    row.first,
                    row.sent_at.map(|at| at.min(i64::MAX as u64) as i64)
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO native_settings(key,value) VALUES(?1,?2)",
            params![
                format!("agent:{id}"),
                serde_json::to_string(&settings).map_err(invalid)?
            ],
        )?;
        if let Some(used) = session.get::<_, Option<i64>>(14)?.filter(|used| *used >= 0) {
            tx.execute(
                "INSERT INTO native_settings(key,value) VALUES(?1,?2)",
                params![
                    format!("context:{id}"),
                    json!({"used":used,"window":session.get::<_, Option<i64>>(15)?.filter(|window| *window > 0)}).to_string()
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO native_settings(key,value) VALUES(?1,?2)",
            params![
                key,
                json!({"session_id":id,"source_id":source_id,"source_path":source_key}).to_string()
            ],
        )?;
        report.imported += 1;
        report.latest_project.get_or_insert(cwd);
    }
    tx.commit()?;
    Ok(report)
}

pub fn native_model(model: &str, provider: &str) -> Option<String> {
    let model = model.trim();
    let model = model.strip_prefix(&format!("{provider}:")).unwrap_or(model);
    let model = model.split('[').next().unwrap_or(model);
    if model.is_empty() || model == "default" {
        return None;
    }
    let model = if provider == "claude"
        && !model.starts_with("claude-")
        && model.chars().any(|c| c.is_ascii_digit())
    {
        format!("claude-{}", model.replace('.', "-"))
    } else {
        model.into()
    };
    Some(model)
}

fn transcript(blocks: &[Value]) -> rusqlite::Result<Transcript> {
    let mut transcript = Transcript::default();
    for block in blocks {
        if block["draft"] == true || block["internal"] == true {
            continue;
        }
        let role = block["role"].as_str().unwrap_or("system");
        let text = block["text"].as_str().unwrap_or("");
        if block["interjection"].is_object() {
            let note = crate::transcript::Interjection {
                text: text.into(),
                custom_type: block["interjection"]["customType"]
                    .as_str()
                    .unwrap_or("custom")
                    .into(),
                severity: block["interjection"]["severity"]
                    .as_str()
                    .map(str::to_owned),
            };
            transcript.begin("interjection");
            transcript.append(&serde_json::to_string(&note).map_err(invalid)?);
        } else if role == "tool" {
            let tool = &block["tool"];
            let preview = &tool["preview"];
            let name = match tool["kind"].as_str().unwrap_or("") {
                "read" => "Read",
                "write" => "Write",
                "edit" => "Edit",
                "search" => "Grep",
                "execute" => "Bash",
                _ => tool["title"].as_str().unwrap_or(text),
            };
            let mut call = ToolCall {
                id: tool["callId"].as_str().or(block["id"].as_str()).unwrap_or("imported-tool").into(),
                name: name.into(),
                input: json!({"command":preview["title"].as_str().or(tool["title"].as_str()).unwrap_or(text),
                    "path":preview["path"],"query":preview["query"]}).to_string(),
                output: tool["detail"].as_str().filter(|s| !s.is_empty()).or(preview["output"].as_str()).unwrap_or("").into(),
                panel: crate::tool::tasks::from_desktop(block),
                display: None,
                // Imported activity is historical, never an active native job.
                state: match tool["status"].as_str() {
                    Some("failed" | "error") => ToolState::Failed,
                    Some("cancelled" | "canceled" | "aborted") => ToolState::Cancelled,
                    _ => ToolState::Completed,
                },
            };
            call.bound_output();
            transcript.begin("tool");
            transcript.rows.last_mut().unwrap().text =
                serde_json::to_string(&call).map_err(invalid)?;
        } else if role == "tasks" && crate::tool::tasks::from_desktop(block).is_some() {
            let mut call = ToolCall::new(block["id"].as_str().unwrap_or(""), "update_plan");
            call.input = block["taskList"].to_string();
            call.panel = crate::tool::tasks::from_desktop(block);
            call.state = ToolState::Completed;
            transcript.begin("tool");
            transcript.rows.last_mut().unwrap().text =
                serde_json::to_string(&call).map_err(invalid)?;
        } else {
            if text.is_empty() {
                continue;
            }
            transcript.begin(match role {
                "user" | "assistant" | "reasoning" => role,
                "plan" | "tasks" => "assistant",
                _ => "system",
            });
            transcript.append(text);
        }
        // Imported history must not acquire today's send time.
        if let Some(row) = transcript.rows.iter_mut().rev().find(|r| r.first) {
            row.sent_at = block["sentAt"].as_u64().or(block["startedAt"].as_u64());
        }
    }
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(path: &Path) -> Connection {
        let source = Connection::open(path).unwrap();
        source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            CREATE TABLE sessions(id TEXT PRIMARY KEY,cwd TEXT NOT NULL,harness TEXT NOT NULL,
                model TEXT NOT NULL,model_settings TEXT NOT NULL,runtime_mode TEXT NOT NULL,
                title TEXT NOT NULL,provider_session_id TEXT,blocks_json TEXT NOT NULL,updated_at INTEGER NOT NULL);")
            .unwrap();
        source
    }

    fn add(source: &Connection, id: &str, cwd: &Path, blocks: Value, updated: i64) {
        source.execute(r#"INSERT INTO sessions(id,cwd,harness,model,model_settings,runtime_mode,title,provider_session_id,blocks_json,updated_at)
            VALUES(?1,?2,'codex','codex:test-model','{"reasoningEffort":"high","fast":"true","serviceTier":"fast"}','auto-accept-edits','Saved conversation','original-thread',?3,?4)"#,
            params![id,cwd.to_string_lossy(),blocks.to_string(),updated]).unwrap();
    }

    #[test]
    fn copies_committed_wal_history_and_settings_without_changing_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let source = source(&path);
        source
            .execute_batch(
                "ALTER TABLE sessions ADD COLUMN archived INTEGER DEFAULT 1;
            ALTER TABLE sessions ADD COLUMN pinned INTEGER DEFAULT 1;
            ALTER TABLE sessions ADD COLUMN worktree_cwd TEXT;
            ALTER TABLE sessions ADD COLUMN worktree_removed INTEGER DEFAULT 0;
            ALTER TABLE sessions ADD COLUMN context_used INTEGER DEFAULT 42;
            ALTER TABLE sessions ADD COLUMN context_window INTEGER DEFAULT 1000;",
            )
            .unwrap();
        let text = "🙂漢字 café\n\n".repeat(3000);
        add(
            &source,
            "original",
            dir.path(),
            json!([
                {"role":"user","text":"Saved prompt","startedAt":100},
                {"role":"reasoning","text":"Visible summary"},
                {"role":"tool","id":"tool-1","text":"Run tests","tool":{
                    "kind":"execute","title":"cargo test","status":"error","detail":"🙂".repeat(20000)}},
                {"role":"assistant","text":text},
                {"role":"user","text":"Unsent draft","draft":true},
                {"role":"user","text":"Internal prompt","internal":true}
            ]),
            200,
        );
        source
            .execute(
                "UPDATE sessions SET worktree_cwd=?1",
                [dir.path().join("worktree").to_string_lossy()],
            )
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut store = Store::open(&dir.path().join("native.sqlite3")).unwrap();
        let existing = store
            .create("claude", &dir.path().to_string_lossy())
            .unwrap();
        store
            .title(existing.id, "Existing native conversation")
            .unwrap();
        let report = store.import_monocode(&path).unwrap();
        assert_eq!(
            (report.imported, report.existing, report.skipped),
            (1, 0, 0)
        );
        assert_eq!(report.latest_project.as_deref(), dir.path().to_str());
        let sessions = store.sessions().unwrap();
        let imported = sessions.iter().find(|s| s.id != existing.id).unwrap();
        assert_eq!(imported.provider_id.as_deref(), Some("original-thread"));
        assert_eq!(imported.updated_at, 200);
        assert!(imported.archived && imported.pinned && imported.has_messages);
        let settings = store.agent_settings(imported.id).unwrap().unwrap();
        assert_eq!(settings.model.as_deref(), Some("test-model"));
        assert_eq!(settings.effort.as_deref(), Some("high"));
        assert!(settings.fast);
        assert_eq!(settings.service_tier.as_deref(), Some("fast"));
        assert_eq!(settings.access, crate::agent::AccessMode::AutoAcceptEdits);
        assert_eq!(
            settings.worktree_cwd.as_deref(),
            dir.path().join("worktree").to_str()
        );
        assert_eq!(store.context_usage(imported.id).unwrap().unwrap().used, 42);
        let history = store.load(imported.id).unwrap();
        assert_eq!(history.rows[0].text, "Saved prompt");
        assert_eq!(history.rows[0].sent_at, Some(100));
        assert_eq!(history.rows[1].sent_at, None);
        let tool: ToolCall = serde_json::from_str(&history.rows[2].text).unwrap();
        assert_eq!(tool.state, ToolState::Failed);
        assert!(tool.output.starts_with("… earlier output omitted …\n"));
        let answer: Vec<_> = history
            .rows
            .iter()
            .filter(|r| r.role == "assistant")
            .collect();
        assert_eq!(
            answer.iter().map(|r| r.text.as_str()).collect::<String>(),
            text
        );
        assert!(answer
            .iter()
            .all(|r| r.text.len() <= 2048 && r.text.matches('\n').count() <= 12));
        assert!(!history
            .rows
            .iter()
            .any(|r| r.text.contains("Unsent draft") || r.text.contains("Internal prompt")));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            source
                .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        // Reruns must preserve native edits and discover only newly saved chats.
        store.title(imported.id, "Edited in GPUI").unwrap();
        source
            .execute("UPDATE sessions SET title='Changed at source'", [])
            .unwrap();
        add(
            &source,
            "new",
            dir.path(),
            json!([{"role":"user","text":"New conversation"}]),
            300,
        );
        let report = store.import_monocode(&path).unwrap();
        assert_eq!((report.imported, report.existing), (1, 1));
        let sessions = store.sessions().unwrap();
        assert_eq!(
            sessions.iter().find(|s| s.id == imported.id).unwrap().title,
            "Edited in GPUI"
        );
        assert_eq!(
            sessions.iter().find(|s| s.id == existing.id).unwrap().title,
            "Existing native conversation"
        );
    }

    #[test]
    fn older_schema_works_and_hidden_remote_and_empty_histories_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let source = source(&path);
        let prompt = json!([{"role":"user","text":"Hello"}]);
        add(&source, "local", dir.path(), prompt.clone(), 100);
        add(
            &source,
            "remote",
            Path::new("ssh://host/project"),
            prompt.clone(),
            110,
        );
        add(
            &source,
            "draft",
            dir.path(),
            json!([{"role":"user","text":"Draft","draft":true}]),
            120,
        );
        add(&source, "unknown", dir.path(), prompt.clone(), 130);
        source
            .execute(
                "UPDATE sessions SET harness='future-provider' WHERE id='unknown'",
                [],
            )
            .unwrap();
        let mut store = Store::open(&dir.path().join("native.sqlite3")).unwrap();
        let report = store.import_monocode(&path).unwrap();
        assert_eq!((report.imported, report.skipped), (1, 3));
        assert!(!store.sessions().unwrap()[0].archived);
        source
            .execute_batch(
                "ALTER TABLE sessions ADD COLUMN sidebar_hidden INTEGER DEFAULT 0;
            ALTER TABLE sessions ADD COLUMN inbox_ask TEXT;",
            )
            .unwrap();
        add(&source, "hidden", dir.path(), prompt.clone(), 140);
        source
            .execute("UPDATE sessions SET sidebar_hidden=1 WHERE id='hidden'", [])
            .unwrap();
        add(&source, "inbox", dir.path(), prompt, 150);
        source
            .execute("UPDATE sessions SET inbox_ask='{}' WHERE id='inbox'", [])
            .unwrap();
        let report = store.import_monocode(&path).unwrap();
        assert_eq!(
            (report.imported, report.existing, report.skipped),
            (0, 1, 5)
        );
    }

    #[test]
    fn invalid_history_rolls_back_the_entire_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let source = source(&path);
        add(
            &source,
            "valid",
            dir.path(),
            json!([{"role":"user","text":"Hello"}]),
            200,
        );
        add(&source, "invalid", dir.path(), json!([]), 100);
        source
            .execute(
                "UPDATE sessions SET blocks_json='broken json' WHERE id='invalid'",
                [],
            )
            .unwrap();
        let mut store = Store::open(&dir.path().join("native.sqlite3")).unwrap();
        assert!(store.import_monocode(&path).is_err());
        assert!(store.sessions().unwrap().is_empty());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM native_settings", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        source
            .execute("DELETE FROM sessions WHERE id='invalid'", [])
            .unwrap();
        assert_eq!(store.import_monocode(&path).unwrap().imported, 1);
    }

    #[test]
    fn missing_or_wrong_source_is_rejected_without_creating_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.db");
        let mut store = Store::open(&dir.path().join("native.sqlite3")).unwrap();
        assert!(store.import_monocode(&path).is_err());
        assert!(!path.exists());
        let error = store
            .import_monocode(&dir.path().join("native.sqlite3"))
            .unwrap_err();
        assert!(error.contains("Not a Monocode session database"));
    }

    #[test]
    fn model_picker_keys_are_translated_to_provider_model_ids() {
        assert_eq!(
            native_model("codex:gpt-6.1-sol", "codex").as_deref(),
            Some("gpt-6.1-sol")
        );
        assert_eq!(
            native_model("claude:opus-4.8[effort=high]", "claude").as_deref(),
            Some("claude-opus-4-8")
        );
        assert_eq!(
            native_model("claude:sonnet", "claude").as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            native_model("opencode:provider/model", "opencode").as_deref(),
            Some("provider/model")
        );
        assert_eq!(native_model("codex:default", "codex"), None);
    }
}
