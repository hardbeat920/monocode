//! A view adapter over the desktop schema: no imported sessions, replacement IDs,
//! persistent native_* tables, or history conversion on open.
mod accounts;
pub(super) mod blocks;
mod edit;
mod settings;
#[cfg(test)]
mod tests;
mod window;
pub use window::{HistoryPage, PAGE_TURNS};

use super::{Reminder, Session, Store};
use crate::{
    agent::Settings,
    context::Usage,
    transcript::{Row, Transcript},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) struct Shared {
    columns: HashSet<String>,
    ids: RefCell<HashMap<i64, String>>,
    extras: RefCell<BTreeMap<String, String>>,
    extras_db: Option<Connection>,
    snapshots: RefCell<HashMap<i64, HistoryRevision>>,
    /// The first block sequence of each indexed Mono's row numbering.
    windows: RefCell<HashMap<i64, i64>>,
    queues: RefCell<HashMap<i64, String>>,
    account_source: RefCell<Option<crate::accounts::Book>>,
    preferences: std::sync::Arc<crate::desktop_preferences::DesktopPreferences>,
    read_only: bool,
}

/// A revision guard, rather than another owned copy of every rich source block.
/// Keep this small guard even when a view releases its history: pending writes
/// must still refuse a source edit made by another window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HistoryRevision([u8; 32]);
impl HistoryRevision {
    fn of(blocks: &[Value]) -> Self {
        let mut hash = Self::start(blocks.len());
        for block in blocks {
            Self::push(&mut hash, block);
        }
        Self(hash.finalize().into())
    }
    fn start(len: usize) -> Sha256 {
        let mut hash = Sha256::new();
        hash.update((len as u64).to_le_bytes());
        hash
    }
    fn push(hash: &mut Sha256, block: &Value) {
        fn size(hash: &mut Sha256, len: usize) {
            hash.update((len as u64).to_le_bytes());
        }
        fn value(hash: &mut Sha256, item: &Value) {
            match item {
                Value::Null => hash.update([0]),
                Value::Bool(boolean) => hash.update([1, u8::from(*boolean)]),
                Value::Number(number) => {
                    hash.update([2]);
                    // serde_json treats floating negative zero as equal to
                    // positive zero, while retaining its distinction from an integer.
                    let number = if number.is_f64() && number.as_f64() == Some(0.0) {
                        "0.0".into()
                    } else {
                        number.to_string()
                    };
                    size(hash, number.len());
                    hash.update(number.as_bytes());
                }
                Value::String(text) => {
                    hash.update([3]);
                    size(hash, text.len());
                    hash.update(text.as_bytes());
                }
                Value::Array(values) => {
                    hash.update([4]);
                    size(hash, values.len());
                    for item in values {
                        value(hash, item);
                    }
                }
                Value::Object(fields) => {
                    hash.update([5]);
                    size(hash, fields.len());
                    // Be canonical even if another crate enables preserve_order.
                    let mut fields: Vec<_> = fields.iter().collect();
                    fields.sort_unstable_by(|a, b| a.0.cmp(b.0));
                    for (key, item) in fields {
                        size(hash, key.len());
                        hash.update(key.as_bytes());
                        value(hash, item);
                    }
                }
            }
        }
        value(hash, block);
    }
    fn matches(&self, blocks: &[Value]) -> bool {
        *self == Self::of(blocks)
    }
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(|e| e.to_string())
}
/// Desktop app CLI requests name children `app-<caller>-<request>`.
/// Both parts can contain hyphens, so the longest existing caller wins.
fn started_caller<'a>(id: &'a str, ids: &HashSet<String>) -> Option<&'a str> {
    let rest = id.strip_prefix("app-")?;
    rest.match_indices('-')
        .rev()
        .map(|(index, _)| &rest[..index])
        .find(|candidate| ids.contains(*candidate))
}
pub(super) fn encode<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}
pub(crate) fn handle(id: &str) -> i64 {
    let bytes = Sha256::digest(id.as_bytes());
    (i64::from_le_bytes(bytes[..8].try_into().unwrap()) & ((1_i64 << 53) - 1)).max(1)
}
pub(super) fn has_table(conn: &Connection, table: &str) -> Result<bool, String> {
    sql(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    ))
}
/// Turn keys are row offsets, including invisible document/card metadata rows.
/// Never substitute the session's current model for missing historical provenance.
pub(super) fn project_turns(
    blocks: &[Value],
) -> Result<HashMap<usize, crate::agent::TurnInfo>, String> {
    let mut turns = HashMap::new();
    let mut offset = 0;
    let mut root_turn = None;
    for block in blocks {
        let rows = blocks::project(std::slice::from_ref(block))?;
        if let Some(prompt) = rows.iter().rposition(Row::is_prompt) {
            let model = &block["turnModel"];
            let duration = block["durationMs"].as_u64();
            let started_at = block["startedAt"].as_u64();
            let turn = offset + prompt + 1;
            if started_at.is_some() {
                root_turn = Some(turn);
            }
            let joined_turn =
                (block["role"] == "user" && block["sentAt"].is_number() && started_at.is_none())
                    .then_some(root_turn)
                    .flatten();
            let metrics = block
                .get("turnMetrics")
                .filter(|value| value.is_object())
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            if model.is_object()
                || duration.is_some()
                || started_at.is_some()
                || metrics.is_some()
                || block["providerTurnId"].is_string()
                || joined_turn.is_some()
            {
                turns.insert(
                    offset + prompt + 1,
                    crate::agent::TurnInfo {
                        joined_turn,
                        provider_turn_id: block["providerTurnId"].as_str().map(str::to_owned),
                        provider: model["harness"].as_str().unwrap_or("").into(),
                        model: model["name"].as_str().unwrap_or("").into(),
                        model_id: model["id"].as_str().map(str::to_owned),
                        duration_ms: duration.unwrap_or(0),
                        duration_known: duration.is_some(),
                        started_at,
                        completed_at: started_at
                            .and_then(|at| at.checked_add(duration.unwrap_or(0))),
                        metrics,
                    },
                );
            }
        }
        offset += rows.len();
    }
    Ok(turns)
}

impl Store {
    /// Open the same sessions, notes and reminders as the desktop app. Only a
    /// brand-new file gets a schema; existing schema/data is not migrated.
    pub fn open_monocode(path: &Path) -> Result<Self, String> {
        Self::open_shared(path, false, None)
    }
    pub fn read_monocode(path: &Path) -> Result<Self, String> {
        Self::open_shared(path, true, None)
    }
    #[cfg(test)]
    pub(crate) fn read_monocode_with_preferences_for_test(
        path: &Path,
        preferences: std::sync::Arc<crate::desktop_preferences::DesktopPreferences>,
    ) -> Result<Self, String> {
        Self::open_shared(path, true, Some(preferences))
    }
    #[cfg(test)]
    pub(crate) fn open_monocode_with_preferences_for_test(
        path: &Path,
        preferences: std::sync::Arc<crate::desktop_preferences::DesktopPreferences>,
    ) -> Result<Self, String> {
        Self::open_shared(path, false, Some(preferences))
    }
    fn open_shared(
        path: &Path,
        read_only: bool,
        preferences: Option<std::sync::Arc<crate::desktop_preferences::DesktopPreferences>>,
    ) -> Result<Self, String> {
        let memory = path == Path::new(":memory:");
        let exists = !memory && path.exists();
        if !read_only && !exists {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        };
        let conn = sql(Connection::open_with_flags(path, flags))?;
        sql(conn.busy_timeout(std::time::Duration::from_secs(5)))?;
        sql(conn.execute_batch("PRAGMA foreign_keys=ON;"))?;
        if !exists && !read_only {
            sql(conn.execute_batch("PRAGMA journal_mode=WAL;"))?;
            let tx = sql(conn.unchecked_transaction())?;
            sql(tx.execute_batch(include_str!("shared_schema.sql")))?;
            sql(tx.commit())?;
        }
        let columns = sql(sql(conn.prepare("PRAGMA table_info(sessions)"))?
            .query_map([], |r| r.get::<_, String>(1)))?
        .collect::<rusqlite::Result<HashSet<_>>>()
        .map_err(|e| e.to_string())?;
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
            "created_at",
            "updated_at",
        ] {
            if !columns.contains(required) {
                return Err(format!(
                    "Unsupported Monocode database: missing sessions.{required}. No schema changes were made."
                ));
            }
        }
        let (extras, extras_db) = settings::open(path, read_only)?;
        let store = Self {
            conn,
            path: path.into(),
            shared: Some(Shared {
                columns,
                ids: RefCell::new(HashMap::new()),
                extras: RefCell::new(extras),
                extras_db,
                snapshots: RefCell::new(HashMap::new()),
                windows: RefCell::new(HashMap::new()),
                queues: RefCell::new(HashMap::new()),
                account_source: RefCell::new(None),
                preferences: preferences.unwrap_or_else(|| {
                    std::sync::Arc::new(
                        path.parent()
                            .filter(|_| !memory)
                            .map(crate::desktop_preferences::DesktopPreferences::load)
                            .unwrap_or_default(),
                    )
                }),
                read_only,
            }),
        };
        // Register only identities here. No blocks_json is read at startup.
        let mut query = sql(store.conn.prepare("SELECT id FROM sessions"))?;
        for id in sql(query.query_map([], |r| r.get::<_, String>(0)))? {
            store.shared_register(&sql(id)?)?;
        }
        drop(query);
        Ok(store)
    }
    pub fn uses_monocode_schema(&self) -> bool {
        self.shared.is_some()
    }
    /// A read-only snapshot of desktop preferences, shared by this store's
    /// consumers. Reopening the store refreshes it without per-message WebKit I/O.
    pub fn desktop_preferences(&self) -> Option<&crate::desktop_preferences::DesktopPreferences> {
        self.shared
            .as_ref()
            .map(|shared| shared.preferences.as_ref())
    }

    pub(super) fn shared_search_reader(&self) -> Result<Self, String> {
        Self::open_shared(
            &self.path,
            true,
            self.shared
                .as_ref()
                .map(|shared| shared.preferences.clone()),
        )
    }

    /// Resolve only a persisted identity; unlike CLI handle parsing this never
    /// treats a stale numeric-looking desktop ID as an existing session.
    pub fn resolve_existing_session_id(&self, id: &str) -> Option<i64> {
        if self.shared.is_some() {
            let exists = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0),
                )
                .ok()?;
            return exists.then(|| self.shared_register(id).ok()).flatten();
        }
        let id = id.parse::<i64>().ok()?;
        self.sessions()
            .ok()?
            .iter()
            .any(|s| s.id == id)
            .then_some(id)
    }
    /// UI handles are stable hashes; the database always retains its original IDs.
    pub fn persisted_session_id(&self, id: i64) -> Result<String, String> {
        if id < 0 {
            if let Some(remote) = self.remote_session(id)? {
                return Ok(format!(
                    "remote:{}:{}",
                    remote.project.environment_id,
                    remote.session_id.unwrap_or_else(|| id.to_string())
                ));
            }
        }
        if let Some(shared) = &self.shared {
            shared
                .ids
                .borrow()
                .get(&id)
                .cloned()
                .ok_or_else(|| "Session no longer exists".into())
        } else {
            Ok(id.to_string())
        }
    }
    pub fn resolve_session_id(&self, id: &str) -> Result<i64, String> {
        if let Some(shared) = &self.shared {
            if let Some((key, _)) = shared
                .ids
                .borrow()
                .iter()
                .find(|(_, value)| value.as_str() == id)
            {
                return Ok(*key);
            }
            if sql(self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            ))? {
                return self.shared_register(id);
            }
        }
        id.parse::<i64>().map_err(|_| "Unknown session ID".into())
    }
    pub(super) fn shared_register(&self, id: &str) -> Result<i64, String> {
        let key = handle(id);
        let mut ids = self.shared.as_ref().unwrap().ids.borrow_mut();
        if ids.get(&key).is_some_and(|existing| existing != id) {
            return Err("Session identity collision; the database has not been modified".into());
        }
        ids.insert(key, id.into());
        Ok(key)
    }
    pub(super) fn shared_writable(&self) -> Result<(), String> {
        if self.shared.as_ref().unwrap().read_only {
            Err("This Monocode store is read-only".into())
        } else {
            Ok(())
        }
    }
    pub(super) fn shared_field(&self, name: &str, fallback: &str) -> String {
        if self.shared.as_ref().unwrap().columns.contains(name) {
            name.into()
        } else {
            fallback.into()
        }
    }
    pub(super) fn shared_patch(&self, id: i64, fields: &[(&str, Value)]) -> Result<(), String> {
        self.shared_writable()?;
        let id = self.persisted_session_id(id)?;
        let fields: Vec<_> = fields
            .iter()
            .filter(|(name, _)| self.shared.as_ref().unwrap().columns.contains(*name))
            .collect();
        if fields.is_empty() {
            return Ok(());
        }
        let updates = fields
            .iter()
            .enumerate()
            .map(|(ix, (name, _))| format!("{name}=?{}", ix + 2))
            .collect::<Vec<_>>()
            .join(",");
        let mut values = vec![rusqlite::types::Value::Text(id)];
        values.extend(fields.iter().map(|(_, v)| match v {
            Value::Null => rusqlite::types::Value::Null,
            Value::Bool(v) => rusqlite::types::Value::Integer(i64::from(*v)),
            Value::Number(v) => rusqlite::types::Value::Integer(v.as_i64().unwrap_or(i64::MAX)),
            Value::String(v) => rusqlite::types::Value::Text(v.clone()),
            v => rusqlite::types::Value::Text(v.to_string()),
        }));
        sql(self.conn.execute(
            &format!("UPDATE sessions SET {updates} WHERE id=?1"),
            rusqlite::params_from_iter(values),
        ))?;
        Ok(())
    }
    pub(super) fn shared_setting(&self, key: &str) -> Result<Option<String>, String> {
        if let Some((id, message)) = key
            .strip_prefix("mono-delivered:")
            .and_then(|v| v.split_once(':'))
        {
            if let Ok(id) = id.parse::<i64>() {
                if self.shared_delivered(id, message)? {
                    return Ok(Some("1".into()));
                }
            }
        }
        Ok(self
            .shared
            .as_ref()
            .unwrap()
            .extras
            .borrow()
            .get(key)
            .cloned())
    }
    pub(super) fn shared_save_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.shared_writable()?;
        let shared = self.shared.as_ref().unwrap();
        if shared
            .extras
            .borrow()
            .get(key)
            .is_some_and(|old| old == value)
        {
            return Ok(());
        }
        if let Some(conn) = &shared.extras_db {
            sql(conn.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            ))?;
        }
        // Publish only committed values. A failed save must remain retryable.
        shared.extras.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    pub(super) fn shared_sessions(&self) -> Result<Vec<Session>, String> {
        let columns = [
            "id",
            "title",
            "harness",
            "cwd",
            "provider_session_id",
            "updated_at",
        ]
        .into_iter()
        .map(str::to_string)
        .chain(
            [
                (
                    "has_user_message",
                    "CASE WHEN blocks_json!='[]' THEN 1 ELSE 0 END",
                ),
                ("archived", "0"),
                ("pinned", "0"),
                ("linked_work_item_json", "NULL"),
                ("sidebar_hidden", "0"),
            ]
            .into_iter()
            .map(|(name, default)| self.shared_field(name, default)),
        )
        .collect::<Vec<_>>()
        .join(",");
        let index = if sql(self.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='sessions_cwd_cover_idx')",[],|r|r.get::<_,bool>(0)))? {
            " INDEXED BY sessions_cwd_cover_idx"
        } else { "" };
        let mut filters = vec!["1=1".to_string()];
        if self.shared.as_ref().unwrap().columns.contains("inbox_ask") {
            filters.push("id NOT IN (SELECT id FROM sessions WHERE inbox_ask IS NOT NULL)".into());
        }
        if has_table(&self.conn, "orchestration_workers")? {
            filters.push("id NOT IN (SELECT session_id FROM orchestration_workers)".into());
        }
        let mut query = sql(self.conn.prepare(&format!(
            "SELECT {columns} FROM sessions{index} WHERE {} ORDER BY updated_at DESC,id",
            filters.join(" AND ")
        )))?;
        let mut rows = sql(query.query([]))?;
        let mut result = Vec::new();
        let mut hidden = HashSet::new();
        let mut originals = HashMap::new();
        while let Some(row) = sql(rows.next())? {
            let original: String = sql(row.get(0))?;
            let id = self.shared_register(&original)?;
            if sql(row.get::<_, bool>(10))? {
                hidden.insert(id);
            }
            originals.insert(id, original);
            result.push(Session {
                id,
                title: sql(row.get(1))?,
                provider: sql(row.get(2))?,
                cwd: sql(row.get(3))?,
                provider_id: sql(row.get(4))?,
                updated_at: sql(row.get::<_, i64>(5))?.max(0) as u64,
                has_messages: sql(row.get(6))?,
                archived: sql(row.get(7))?,
                pinned: sql(row.get(8))?,
                work_item: sql(row.get::<_, Option<String>>(9))?.and_then(|raw| parse(&raw).ok()),
            });
        }
        // Keep the source sidebar's compatibility exception for hidden
        // /operator launches. A Mono's own hidden conversation cannot be a
        // listed caller. The UI also checks project/worktree and Mono scope.
        let shown: HashMap<_, _> = result
            .iter()
            .filter(|s| !hidden.contains(&s.id) && s.has_messages)
            .map(|s| (originals[&s.id].clone(), s.cwd.clone()))
            .collect();
        let shown_ids = shown.keys().cloned().collect();
        result.retain(|s| {
            !hidden.contains(&s.id)
                || started_caller(&originals[&s.id], &shown_ids)
                    .is_some_and(|caller| shown[caller] == s.cwd)
        });
        Ok(result)
    }
    pub(super) fn shared_create(&self, provider: &str, cwd: &str) -> Result<Session, String> {
        self.shared_create_named(provider, cwd, &crate::accounts::request_id())
            .map(|(session, _)| session)
    }
    pub(super) fn shared_create_named(
        &self,
        provider: &str,
        cwd: &str,
        original: &str,
    ) -> Result<(Session, bool), String> {
        self.shared_writable()?;
        let label = crate::provider::Kind::parse(provider)
            .ok_or_else(|| format!("Unknown provider: {provider}"))?
            .label();
        let id = self.shared_register(original)?;
        let exists: bool = sql(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
            [original],
            |row| row.get(0),
        ))?;
        if exists {
            let existing = self
                .shared_sessions()?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or("The session created by this request was removed")?;
            if existing.provider != provider
                || crate::files::path_key(&existing.cwd) != crate::files::path_key(cwd)
            {
                return Err("Request ID was already used for another session".into());
            }
            return Ok((existing, false));
        }
        let now = crate::time::now_ms().min(i64::MAX as u64) as i64;
        let title = format!("New {label} session");
        sql(self.conn.execute("INSERT INTO sessions(id,cwd,harness,model,model_settings,runtime_mode,title,blocks_json,created_at,updated_at)
            VALUES(?1,?2,?3,?4,'{}','supervised',?5,'[]',?6,?6)",
            params![original,cwd,provider,format!("{provider}:default"),title,now]))?;
        Ok((
            Session {
                id,
                title,
                provider: provider.into(),
                cwd: cwd.into(),
                provider_id: None,
                has_messages: false,
                updated_at: now as u64,
                archived: false,
                pinned: false,
                work_item: None,
            },
            true,
        ))
    }
    pub(super) fn shared_agent(&self, id: i64) -> Result<Option<Settings>, String> {
        let fields = ["harness", "model", "model_settings", "runtime_mode"]
            .into_iter()
            .map(str::to_owned)
            .chain(
                ["worktree_cwd", "worktree_removed", "provider_account_id"]
                    .into_iter()
                    .map(|name| self.shared_field(name, "NULL")),
            )
            .chain([
                "cwd".to_owned(),
                "provider_session_id".to_owned(),
                self.shared_field(
                    "has_user_message",
                    "CASE WHEN blocks_json!='[]' THEN 1 ELSE 0 END",
                ),
            ])
            .collect::<Vec<_>>()
            .join(",");
        let raw = sql(self
            .conn
            .query_row(
                &format!("SELECT {fields} FROM sessions WHERE id=?1"),
                [self.persisted_session_id(id)?],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<bool>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, bool>(9)?,
                    ))
                },
            )
            .optional())?;
        let Some((
            provider,
            model,
            controls,
            access,
            cwd,
            removed,
            account,
            project,
            provider_id,
            has_messages,
        )) = raw
        else {
            return Ok(None);
        };
        let mut controls: HashMap<String, String> = parse(&controls)?;
        let mut settings: Settings = self
            .shared_setting(&format!("agent:{id}"))?
            .map(|raw| parse(&raw))
            .transpose()?
            .unwrap_or_default();
        settings.model = super::monocode_import::native_model(&model, &provider);
        // A prior native launch may have created an untouched default blank.
        // Seed only that blank, never a saved or resumed conversation's model.
        let default_controls = controls.iter().all(|(key, value)| {
            (key == "fast" && value == "false") || (key == "serviceTier" && value == "default")
        });
        if settings.model.is_none()
            && default_controls
            && settings.revision == 0
            && !has_messages
            && provider_id.is_none()
        {
            if let Some(defaults) = self
                .desktop_preferences()
                .and_then(|preferences| preferences.session_defaults(&provider, &project))
            {
                settings.model = defaults.model;
                controls = defaults.model_settings;
            }
        }
        settings.access = crate::agent::AccessMode::from_id(&access);
        settings.effort = [
            "effort",
            "reasoning",
            "reasoningEffort",
            "reasoning_effort",
            "variant",
        ]
        .into_iter()
        .find_map(|key| controls.get(key).cloned());
        settings.fast = controls.get("fast").is_some_and(|v| v == "true");
        settings.service_tier = controls.get("serviceTier").cloned();
        settings.thinking = controls.get("thinking").and_then(|s| s.parse().ok());
        settings.model_settings = controls;
        settings.worktree_cwd = cwd;
        settings.worktree_removed = removed.unwrap_or(false);
        settings.account.id = account.unwrap_or_else(|| crate::accounts::DEFAULT.into());
        // Desktop blocks are authoritative. Native-only legacy metadata remains
        // available, but cannot override a saved desktop turn's identity/time.
        let blocks = self.shared_blocks(id)?;
        settings.operator_enabled |= blocks.iter().any(|block| {
            block["role"] == "user"
                && block["draft"] != true
                && block["internal"] != true
                && (block["monocode"] == true
                    || ["mono", "monocode"].into_iter().any(|command| {
                        crate::composer::consume_command(
                            block["text"].as_str().unwrap_or_default(),
                            command,
                        )
                        .is_some()
                    }))
        });
        for (turn, source) in project_turns(&blocks)? {
            let info = settings.turns.entry(turn).or_default();
            if !source.provider.is_empty() {
                info.provider = source.provider;
            }
            if !source.model.is_empty() {
                info.model = source.model;
            }
            if source.model_id.is_some() {
                info.model_id = source.model_id;
            }
            if source.provider_turn_id.is_some() {
                info.provider_turn_id = source.provider_turn_id;
            }
            if source.joined_turn.is_some() {
                info.joined_turn = source.joined_turn;
            }
            if source.duration_known {
                info.duration_ms = source.duration_ms;
                info.duration_known = true;
            }
            if source.started_at.is_some() {
                info.started_at = source.started_at;
                info.completed_at = source.completed_at;
            }
            if source.metrics.is_some() {
                info.metrics = source.metrics;
            }
        }
        Ok(Some(settings))
    }
    pub(super) fn shared_save_agent(&self, id: i64, settings: &Settings) -> Result<(), String> {
        self.shared_save_turns(id, &settings.turns)?;
        self.shared_save_agent_preferences(id, settings)
    }
    /// Preference-only updates for validated operations which already preserve
    /// their source transcript inside a surrounding transaction (rewind and
    /// worktree cleanup). These must not re-save or re-index turn receipts.
    fn shared_save_agent_preferences(&self, id: i64, settings: &Settings) -> Result<(), String> {
        let current = self.shared_agent(id)?.unwrap_or_default();
        let mut controls = current.model_settings;
        controls.extend(settings.model_settings.clone());
        let effort_key = [
            "effort",
            "reasoning",
            "reasoningEffort",
            "reasoning_effort",
            "variant",
        ]
        .into_iter()
        .find(|key| controls.contains_key(*key))
        .unwrap_or("effort");
        if let Some(effort) = &settings.effort {
            controls.insert(effort_key.into(), effort.clone());
        } else {
            controls.remove(effort_key);
        }
        controls.insert("fast".into(), settings.fast.to_string());
        if let Some(tier) = &settings.service_tier {
            controls.insert("serviceTier".into(), tier.clone());
        }
        if let Some(thinking) = settings.thinking {
            controls.insert("thinking".into(), thinking.to_string());
        }
        let mut fields = vec![
            ("model_settings", json!(encode(&controls)?)),
            ("runtime_mode", json!(settings.access.id())),
            ("worktree_cwd", json!(settings.worktree_cwd)),
            ("worktree_removed", json!(settings.worktree_removed)),
            ("provider_account_id", json!(settings.account.id)),
        ];
        let (provider, saved_model): (String, String) = sql(self.conn.query_row(
            "SELECT harness,model FROM sessions WHERE id=?1",
            [self.persisted_session_id(id)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        if settings.model != super::monocode_import::native_model(&saved_model, &provider) {
            let model = settings.model.as_deref().unwrap_or("default");
            let model = if provider == "claude" {
                model.strip_prefix("claude-").unwrap_or(model)
            } else {
                model
            };
            fields.push(("model", json!(format!("{provider}:{model}"))));
        }
        self.shared_patch(id, &fields)?;
        self.shared_save_setting(&format!("agent:{id}"), &encode(settings)?)
    }
    /// Add native turn receipts to their original desktop prompt objects.
    /// Unknown rich fields and existing provenance are retained verbatim.
    fn shared_save_turns(
        &self,
        id: i64,
        turns: &HashMap<usize, crate::agent::TurnInfo>,
    ) -> Result<(), String> {
        if turns.is_empty() {
            return Ok(());
        }
        self.shared_writable()?;
        let tx = sql(self.conn.unchecked_transaction())?;
        let mut blocks = self.shared_blocks(id)?;
        if self
            .shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow()
            .get(&id)
            .is_some_and(|revision| !revision.matches(&blocks))
        {
            return Err("This conversation changed in another window. Reopen it before saving; its data was preserved.".into());
        }
        let (session_provider, session_model): (String, String) = sql(self.conn.query_row(
            "SELECT harness,model FROM sessions WHERE id=?1",
            [self.persisted_session_id(id)?],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        let mut offset = 0;
        let mut changed = false;
        for block in &mut blocks {
            let rows = blocks::project(std::slice::from_ref(block))?;
            let turn = rows
                .iter()
                .rposition(Row::is_prompt)
                .map(|prompt| offset + prompt + 1);
            offset += rows.len();
            let Some(info) = turn.and_then(|turn| turns.get(&turn)) else {
                continue;
            };
            let mut fields = Vec::new();
            if !info.provider.is_empty() && (!info.model.is_empty() || info.model_id.is_some()) {
                let mut model = json!({"harness":info.provider});
                if !info.model.is_empty() {
                    model["name"] = json!(info.model);
                }
                if let Some(id) = &info.model_id {
                    model["id"] = json!(if info.provider == session_provider
                        && super::monocode_import::native_model(&session_model, &session_provider)
                            .as_ref()
                            == Some(id)
                    {
                        session_model.clone()
                    } else if id.starts_with(&format!("{}:", info.provider)) {
                        id.clone()
                    } else {
                        format!("{}:{id}", info.provider)
                    });
                }
                fields.push(("turnModel", model));
            }
            if info.has_duration() {
                fields.push(("durationMs", json!(info.duration_ms)));
            }
            if let Some(id) = &info.provider_turn_id {
                fields.push(("providerTurnId", json!(id)));
            }
            if let Some(at) = info.started_at {
                // A native root prompt is written with its send time before
                // its turn receipt arrives; the desktop schema calls it startedAt.
                if block["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("gpui-"))
                    && block["startedAt"].is_null()
                {
                    block["startedAt"] = json!(at);
                    if block["sentAt"].as_u64() == Some(at) {
                        block.as_object_mut().unwrap().remove("sentAt");
                    }
                    changed = true;
                }
            }
            if let Some(metrics) = info.metrics.filter(|metrics| metrics.reported()) {
                fields.push(("turnMetrics", json!({"inputTokens":metrics.input_tokens,
                    "outputTokens":metrics.output_tokens,"cacheReadTokens":metrics.cache_read_tokens,
                    "cacheWriteTokens":metrics.cache_write_tokens,"cacheHitPercent":metrics.cache_hit_percent})));
            }
            for (key, value) in fields {
                if block[key].is_null() {
                    block[key] = value;
                    changed = true;
                } else if key == "turnModel" {
                    if let (Some(saved), Some(incoming)) =
                        (block[key].as_object_mut(), value.as_object())
                    {
                        for (name, value) in incoming {
                            if !saved.contains_key(name) {
                                saved.insert(name.clone(), value.clone());
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if changed {
            self.shared_store_blocks(id, &blocks)?;
        }
        sql(tx.commit())?;
        if changed {
            self.shared
                .as_ref()
                .unwrap()
                .snapshots
                .borrow_mut()
                .insert(id, HistoryRevision::of(&blocks));
        }
        Ok(())
    }
    pub(super) fn shared_context(&self, id: i64) -> Result<Option<Usage>, String> {
        let fields = format!(
            "{},{}",
            self.shared_field("context_used", "NULL"),
            self.shared_field("context_window", "NULL")
        );
        let (used, window) = sql(self.conn.query_row(
            &format!("SELECT {fields} FROM sessions WHERE id=?1"),
            [self.persisted_session_id(id)?],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
        ))?;
        Ok(used.filter(|u| *u >= 0).map(|used| Usage {
            used: used as u64,
            window: window.filter(|w| *w > 0).map(|w| w as u64),
        }))
    }
    /// The blocks whose rows this store numbers: an indexed Mono's window, or
    /// the whole history of one stored as a blob.
    pub(super) fn shared_blocks(&self, id: i64) -> Result<Vec<Value>, String> {
        let original = self.persisted_session_id(id)?;
        if let Some(from) = self.shared_window(id, &original)? {
            self.shared_indexed_blocks(&original, from, i64::MAX)
        } else {
            let raw: String = sql(self.conn.query_row(
                "SELECT blocks_json FROM sessions WHERE id=?1",
                [original],
                |r| r.get(0),
            ))?;
            parse(&raw)
        }
    }
    pub(super) fn shared_load(&self, id: i64) -> Result<Transcript, String> {
        let original = self.persisted_session_id(id)?;
        if let Some(from) = self.shared_window(id, &original)? {
            // Project one source block at a time. A long Mono must not retain
            // a second complete JSON history alongside its display rows.
            // The scalar count and the rows share one SQLite read snapshot.
            let mut query = sql(self.conn.prepare(
                "SELECT block_json,(SELECT count(*) FROM mono_blocks WHERE session_id=?1 AND seq>=?2)
                 FROM mono_blocks WHERE session_id=?1 AND seq>=?2 ORDER BY seq",
            ))?;
            let mut records = sql(query.query(params![original, from]))?;
            let mut rows = Vec::new();
            let mut digest = HistoryRevision::start(0);
            let mut first = true;
            while let Some(record) = sql(records.next())? {
                if first {
                    digest = HistoryRevision::start(sql(record.get::<_, i64>(1))? as usize);
                    first = false;
                }
                let raw: String = sql(record.get(0))?;
                let block: Value = parse(&raw)?;
                HistoryRevision::push(&mut digest, &block);
                rows.extend(blocks::project(std::slice::from_ref(&block))?);
            }
            self.shared
                .as_ref()
                .unwrap()
                .snapshots
                .borrow_mut()
                .insert(id, HistoryRevision(digest.finalize().into()));
            return Ok(Transcript { rows });
        }
        let blocks = self.shared_blocks(id)?;
        let rows = blocks::project(&blocks)?;
        self.shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow_mut()
            .insert(id, HistoryRevision::of(&blocks));
        Ok(Transcript { rows })
    }
    pub(super) fn shared_store_blocks(&self, id: i64, blocks: &[Value]) -> Result<(), String> {
        let original = self.persisted_session_id(id)?;
        let (has_user, draft) = if let Some(from) = self.shared_window(id, &original)? {
            self.shared_store_indexed_blocks(&original, from, blocks)?;
            // Blocks before the window count too, and were never read.
            sql(self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM mono_blocks WHERE session_id=?1 AND is_user=1),
                 EXISTS(SELECT 1 FROM mono_blocks WHERE session_id=?1 AND is_user=1 AND is_draft=1)",
                [&original],
                |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
            ))?
        } else {
            self.shared_patch(id, &[("blocks_json", json!(encode(&blocks)?))])?;
            (
                blocks.iter().any(|b| b["role"] == "user"),
                blocks
                    .iter()
                    .any(|b| b["role"] == "user" && b["draft"] == true),
            )
        };
        self.shared_patch(
            id,
            &[
                ("has_user_message", json!(has_user)),
                ("is_draft", json!(draft)),
            ],
        )
    }
    /// Replace the window's blocks, from sequence `from` on. Earlier blocks are
    /// outside every row index this store handed out, so they stay untouched.
    fn shared_store_indexed_blocks(
        &self,
        original: &str,
        from: i64,
        blocks: &[Value],
    ) -> Result<(), String> {
        let mut query = sql(self.conn.prepare(
            "SELECT seq,block_id,block_json FROM mono_blocks WHERE session_id=?1 AND seq>=?2 ORDER BY seq",
        ))?;
        let mut old = HashMap::new();
        // New blocks always land inside the window, after everything stored.
        let first = from.max(1);
        let mut max_seq = sql(self.conn.query_row(
            "SELECT coalesce(max(seq),0) FROM mono_blocks WHERE session_id=?1",
            [original],
            |row| row.get::<_, i64>(0),
        ))?
        .max(first - 1);
        for row in sql(query.query_map(params![original, from], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }))? {
            let (seq, id, raw) = sql(row)?;
            old.insert(id, (seq, parse::<Value>(&raw)?));
        }
        let retained: HashSet<_> = blocks.iter().filter_map(|b| b["id"].as_str()).collect();
        if retained.len() != blocks.len() {
            return Err("Indexed conversation contains missing or duplicate block IDs".into());
        }
        let mut previous = 0;
        let mut appended = false;
        let mut resequence = false;
        for block in blocks {
            if let Some((seq, _)) = old.get(block["id"].as_str().unwrap()) {
                if appended || *seq <= previous {
                    resequence = true;
                }
                previous = *seq;
            } else {
                appended = true;
            }
        }
        if resequence {
            // An explicit insertion/reorder changes turn positions. Payloads
            // and block identities survive, in one enclosing transaction.
            sql(self.conn.execute(
                "DELETE FROM mono_turns WHERE session_id=?1 AND seq>=?2",
                params![original, from],
            ))?;
            sql(self.conn.execute(
                "DELETE FROM mono_blocks WHERE session_id=?1 AND seq>=?2",
                params![original, from],
            ))?;
            old.clear();
            max_seq = first - 1;
        } else {
            for (id, (seq, _)) in &old {
                if !retained.contains(id.as_str()) {
                    sql(self.conn.execute(
                        "DELETE FROM mono_turns WHERE session_id=?1 AND seq=?2",
                        params![original, seq],
                    ))?;
                    sql(self.conn.execute(
                        "DELETE FROM mono_blocks WHERE session_id=?1 AND block_id=?2",
                        params![original, id],
                    ))?;
                }
            }
        }
        let mut after_handoff = false;
        for (ix, block) in blocks.iter().enumerate() {
            let id = block["id"].as_str().unwrap();
            let (seq, changed) = match old.get(id) {
                Some((seq, old)) => (*seq, old != block),
                None => {
                    max_seq += 1;
                    (max_seq, true)
                }
            };
            if changed {
                let search = [
                    block["text"].as_str(),
                    block["tool"]["title"].as_str(),
                    block["tool"]["detail"].as_str(),
                    block["image"]["name"].as_str(),
                    block["image"]["alt"].as_str(),
                    block["tool"]["preview"]["path"].as_str(),
                    block["tool"]["preview"]["query"].as_str(),
                    block["tool"]["preview"]["output"].as_str(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n")
                .to_lowercase();
                sql(self.conn.execute("INSERT INTO mono_blocks(session_id,seq,block_id,block_json,search_text,is_user,is_draft)
                    VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(session_id,seq) DO UPDATE SET block_json=excluded.block_json,
                    search_text=excluded.search_text,is_user=excluded.is_user,is_draft=excluded.is_draft",
                    params![original,seq,id,encode(block)?,search,block["role"]=="user",block["draft"]==true]))?;
            }
            if ix == 0
                || after_handoff
                || (block["role"] == "user" && block["internal"] != true)
                || block.get("monoHabit").is_some()
            {
                sql(self.conn.execute(
                    "INSERT OR IGNORE INTO mono_turns(session_id,seq) VALUES(?1,?2)",
                    params![original, seq],
                ))?;
            }
            after_handoff = block["role"] == "handoff";
        }
        Ok(())
    }
    pub(super) fn shared_write_rows(
        &self,
        id: i64,
        start: usize,
        rows: &[Row],
        message_id: Option<&str>,
    ) -> Result<(), String> {
        self.shared_writable()?;
        let tx = sql(self.conn.unchecked_transaction())?;
        let current = self.shared_blocks(id)?;
        if self
            .shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow()
            .get(&id)
            .is_some_and(|old| !old.matches(&current))
        {
            return Err("This conversation changed in another window. Reopen it before saving; its data was preserved.".into());
        }
        let mut next = blocks::merge(&current, start, rows)?;
        if let Some(message) = message_id {
            if let Some(prompt) = next
                .iter_mut()
                .rev()
                .find(|b| b["role"] == "user" && b["draft"] != true)
            {
                prompt["appRequestId"] = json!(message);
            }
            next.retain(|b| !(b["draft"] == true && b["id"].as_str() == Some(message)));
        }
        self.shared_store_blocks(id, &next)?;
        let mut next_queue = None;
        if let Some(message) = message_id {
            let field = self.shared_field("queued_messages_json", "'[]'");
            let raw: String = sql(self.conn.query_row(
                &format!("SELECT {field} FROM sessions WHERE id=?1"),
                [self.persisted_session_id(id)?],
                |r| r.get(0),
            ))?;
            let mut queued: Vec<Value> = parse(&raw)?;
            queued.retain(|m| m["id"].as_str() != Some(message));
            let encoded = encode(&queued)?;
            self.shared_patch(id, &[("queued_messages_json", json!(encoded))])?;
            next_queue = Some(encoded);
        }

        if !rows.is_empty() {
            self.shared_patch(id, &[("updated_at", json!(crate::time::now_ms()))])?;
        }
        sql(tx.commit())?;
        if let Some(queue) = next_queue {
            self.shared
                .as_ref()
                .unwrap()
                .queues
                .borrow_mut()
                .insert(id, queue);
        }
        self.shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow_mut()
            .insert(id, HistoryRevision::of(&next));
        // The source appRequestId is the durable, transactional receipt.
        Ok(())
    }
    pub(super) fn shared_reset_provider_id_if(
        &self,
        id: i64,
        expected: Option<&str>,
    ) -> Result<(), String> {
        self.shared_writable()?;
        sql(self.conn.execute("UPDATE sessions SET provider_session_id=NULL WHERE id=?1 AND provider_session_id IS ?2",
            params![self.persisted_session_id(id)?,expected]))?;
        Ok(())
    }
    pub(super) fn shared_change_provider(&self, id: i64, provider: &str) -> Result<(), String> {
        self.shared_writable()?;
        sql(self.conn.execute(
            "UPDATE sessions SET harness=?2,provider_session_id=NULL WHERE id=?1 AND harness<>?2",
            params![self.persisted_session_id(id)?, provider],
        ))?;
        Ok(())
    }
    pub(super) fn shared_replace_title(
        &self,
        id: i64,
        expected: &str,
        title: &str,
    ) -> Result<bool, String> {
        self.shared_writable()?;
        sql(self.conn.execute(
            "UPDATE sessions SET title=?3 WHERE id=?1 AND title=?2",
            params![self.persisted_session_id(id)?, expected, title],
        ))
        .map(|n| n == 1)
    }
    pub(super) fn shared_worktrees(&self) -> Result<HashMap<i64, Option<String>>, String> {
        let mut query = sql(self.conn.prepare(&format!(
            "SELECT id,{} FROM sessions",
            self.shared_field("worktree_cwd", "NULL")
        )))?;
        let mut out = HashMap::new();
        for row in sql(query.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))? {
            let (id, path) = sql(row)?;
            out.insert(self.shared_register(&id)?, path);
        }
        Ok(out)
    }
    pub(super) fn shared_sidebar_hidden(&self) -> Result<HashSet<i64>, String> {
        let mut query = sql(self.conn.prepare(&format!(
            "SELECT id FROM sessions WHERE {}!=0",
            self.shared_field("sidebar_hidden", "0")
        )))?;
        let mut out = HashSet::new();
        for id in sql(query.query_map([], |r| r.get::<_, String>(0)))? {
            out.insert(self.shared_register(&sql(id)?)?);
        }
        Ok(out)
    }
    pub(super) fn shared_started_links(&self) -> Result<HashMap<i64, i64>, String> {
        let mut query = sql(self.conn.prepare(&format!(
            "SELECT id,{} FROM sessions",
            self.shared_field("sidebar_hidden", "0")
        )))?;
        let rows: Vec<(String, bool)> = sql(query.query_map([], |r| Ok((r.get(0)?, r.get(1)?))))?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        let ids = rows.iter().map(|(id, _)| id.clone()).collect();
        let shown = rows
            .iter()
            .filter(|(_, hidden)| !hidden)
            .map(|(id, _)| id.clone())
            .collect();
        rows.iter()
            .filter_map(|(id, hidden)| {
                started_caller(id, if *hidden { &shown } else { &ids }).map(|caller| (id, caller))
            })
            .map(|(id, caller)| Ok((self.shared_register(id)?, self.shared_register(caller)?)))
            .collect()
    }
    pub(super) fn shared_pins(&self, ids: &[i64], pinned: bool) -> Result<(), String> {
        let tx = sql(self.conn.unchecked_transaction())?;
        for id in ids {
            self.shared_patch(*id, &[("pinned", json!(pinned))])?;
        }
        sql(tx.commit())
    }
    pub(super) fn shared_settings_with_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(i64, String)>, String> {
        Ok(self
            .shared
            .as_ref()
            .unwrap()
            .extras
            .borrow()
            .iter()
            .filter_map(|(key, value)| {
                Some((key.strip_prefix(prefix)?.parse().ok()?, value.clone()))
            })
            .collect())
    }
    pub(super) fn shared_drafts(&self) -> Result<HashSet<i64>, String> {
        let mut query = sql(self.conn.prepare(&format!(
            "SELECT id FROM sessions WHERE {}!=0",
            self.shared_field("is_draft", "0")
        )))?;
        let mut out = HashSet::new();
        for id in sql(query.query_map([], |r| r.get::<_, String>(0)))? {
            out.insert(self.shared_register(&sql(id)?)?);
        }
        Ok(out)
    }
    pub(super) fn shared_groups(&self, cwd: &str) -> Result<crate::session_groups::Groups, String> {
        if let Some(raw) = self.shared_setting(&format!("groups:{cwd}"))? {
            return parse(&raw);
        }
        Ok(self
            .desktop_preferences()
            .and_then(|preferences| {
                preferences.groups(cwd, |id| self.resolve_existing_session_id(id))
            })
            .unwrap_or_default())
    }
    pub(super) fn shared_ui_state(&self) -> Result<Option<String>, String> {
        let saved = self.shared_setting("workspace")?;
        let mut workspace = saved.as_deref().map(parse::<Value>).transpose()?;
        if has_table(&self.conn, "workspace_snapshot")? {
            let raw: Option<String> = sql(self
                .conn
                .query_row(
                    "SELECT snapshot_json FROM workspace_snapshot WHERE id=1",
                    [],
                    |r| r.get(0),
                )
                .optional())?;
            if let Some(raw) = raw {
                let snapshot: Value = parse(&raw)?;
                if workspace.is_none() {
                    workspace = Some(
                        crate::desktop_preferences::DesktopPreferences::workspace_snapshot(
                            &snapshot,
                            |id| self.resolve_existing_session_id(id),
                        ),
                    );
                } else if let Some(saved) = workspace.as_mut().and_then(Value::as_object_mut) {
                    // Preserve the source snapshot alongside native layouts;
                    // unsupported panes can be recovered without rewriting it.
                    saved
                        .entry("desktop_workspace_snapshot")
                        .or_insert(snapshot);
                }
            }
        }
        let mut merged = workspace.clone().unwrap_or_else(|| json!({}));
        if let Some(preferences) = self.desktop_preferences() {
            preferences.apply_workspace(&mut merged);
        }
        if merged.as_object().is_some_and(|object| !object.is_empty()) {
            Ok(Some(merged.to_string()))
        } else {
            Ok(workspace.map(|value| value.to_string()))
        }
    }
    pub(super) fn shared_reminders(&self) -> Result<Vec<Reminder>, String> {
        if !has_table(&self.conn, "session_reminders")? {
            return Ok(Vec::new());
        }
        let mut query = sql(self.conn.prepare(
            "SELECT session_id,due_at,fired_at FROM session_reminders ORDER BY due_at,session_id",
        ))?;
        let mut out = Vec::new();
        for row in sql(query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        }))? {
            let (id, due, fired) = sql(row)?;
            out.push(Reminder {
                session_id: self.shared_register(&id)?,
                due_at: due.max(0) as u64,
                fired_at: fired.map(|at| at.max(0) as u64),
            });
        }
        Ok(out)
    }
    pub(super) fn shared_set_reminders(&self, ids: &[i64], due: u64) -> Result<(), String> {
        self.shared_writable()?;
        let tx = sql(self.conn.unchecked_transaction())?;
        for id in ids {
            sql(self.conn.execute(
                "INSERT INTO session_reminders(session_id,due_at,fired_at) VALUES(?1,?2,NULL)
                ON CONFLICT(session_id) DO UPDATE SET due_at=excluded.due_at,fired_at=NULL",
                params![
                    self.persisted_session_id(*id)?,
                    due.min(i64::MAX as u64) as i64
                ],
            ))?;
        }
        sql(tx.commit())
    }
    pub(super) fn shared_snooze_reminder_if(
        &self,
        id: i64,
        expected_due: u64,
        new_due: u64,
    ) -> Result<bool, String> {
        self.shared_writable()?;
        sql(self.conn.execute("UPDATE session_reminders SET due_at=?3,fired_at=NULL WHERE session_id=?1 AND due_at=?2",
            params![self.persisted_session_id(id)?, expected_due.min(i64::MAX as u64) as i64, new_due.min(i64::MAX as u64) as i64])).map(|changed|changed==1)
    }
    pub(super) fn shared_clear_reminders(
        &self,
        ids: &[i64],
        expected: Option<u64>,
    ) -> Result<(), String> {
        self.shared_writable()?;
        for id in ids {
            sql(self.conn.execute(
                "DELETE FROM session_reminders WHERE session_id=?1 AND (?2 IS NULL OR due_at=?2)",
                params![
                    self.persisted_session_id(*id)?,
                    expected.map(|at| at.min(i64::MAX as u64) as i64)
                ],
            ))?;
        }
        Ok(())
    }
    pub(super) fn shared_fire_reminders(
        &self,
        due: &[Reminder],
        now: u64,
    ) -> Result<Vec<Reminder>, String> {
        self.shared_writable()?;
        let tx = sql(self.conn.unchecked_transaction())?;
        let mut fired = Vec::new();
        for reminder in due {
            let changed=sql(self.conn.execute("UPDATE session_reminders SET fired_at=?2 WHERE session_id=?1 AND fired_at IS NULL AND due_at=?3",
                params![self.persisted_session_id(reminder.session_id)?, now.min(i64::MAX as u64) as i64, reminder.due_at.min(i64::MAX as u64) as i64]))?;
            if changed == 1 {
                fired.push(reminder.clone());
            }
        }
        sql(tx.commit())?;
        Ok(fired)
    }
    pub(super) fn shared_delete(&self, id: i64) -> Result<(), String> {
        self.shared_writable()?;
        let original = self.persisted_session_id(id)?;
        let tx = sql(self.conn.unchecked_transaction())?;
        for (table, column) in [
            ("session_reminders", "session_id"),
            ("mono_blocks", "session_id"),
            ("mono_turns", "session_id"),
            ("mono_transcripts", "session_id"),
            ("in_flight_sessions", "session_id"),
            ("orchestration_workers", "session_id"),
            ("orchestration_sidebar", "lead_id"),
            ("orchestration_runs", "lead_id"),
        ] {
            if has_table(&self.conn, table)? {
                sql(self.conn.execute(
                    &format!("DELETE FROM {table} WHERE {column}=?1"),
                    [&original],
                ))?;
            }
        }
        sql(self
            .conn
            .execute("DELETE FROM sessions WHERE id=?1", [original]))?;
        sql(tx.commit())?;
        self.shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow_mut()
            .remove(&id);
        Ok(())
    }
    pub(super) fn shared_detach_worktree(
        &self,
        path: &str,
        main: &str,
    ) -> Result<Vec<(Session, Settings)>, String> {
        let tx = sql(self.conn.unchecked_transaction())?;
        let mut out = Vec::new();
        for mut session in self.shared_sessions()? {
            let mut settings = self.shared_agent(session.id)?.unwrap_or_default();
            let execution = settings.worktree_cwd.as_deref().unwrap_or(&session.cwd);
            if !crate::files::is_equal_or_inside(execution, path) {
                continue;
            }
            settings.worktree_cwd = Some(execution.into());
            settings.worktree_removed = true;
            settings.revision += 1;
            settings.new_worktree = false;
            settings.worktree_base = None;
            if crate::files::is_equal_or_inside(&session.cwd, path) {
                session.cwd = main.into();
            }
            session.provider_id = None;
            self.shared_patch(
                session.id,
                &[
                    ("cwd", json!(session.cwd)),
                    ("provider_session_id", Value::Null),
                ],
            )?;
            self.shared_save_agent_preferences(session.id, &settings)?;
            out.push((session, settings));
        }
        sql(tx.commit())?;
        Ok(out)
    }
    pub(super) fn shared_search(
        &self,
        query: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(Vec<crate::search::Hit>, bool), String> {
        use std::sync::atomic::Ordering;
        if query.trim().is_empty() || cancel.load(Ordering::Acquire) {
            return Ok((Vec::new(), false));
        }
        let flag = cancel.clone();
        sql(self
            .conn
            .progress_handler(2000, Some(move || flag.load(Ordering::Acquire))))?;
        let result = (|| {
            let summaries = self.shared_sessions()?;
            let mut hits = Vec::new();
            let mut truncated = false;
            'sessions: for summary in summaries.iter().filter(|s| !s.archived && s.has_messages) {
                if cancel.load(Ordering::Acquire) {
                    return Ok((Vec::new(), false));
                }
                let rows = blocks::project(&self.shared_all_blocks(summary.id)?)?;
                let mut ix = 0;
                while ix < rows.len() {
                    if cancel.load(Ordering::Acquire) {
                        return Ok((Vec::new(), false));
                    }
                    let from = ix;
                    let mut text = String::new();
                    loop {
                        let body = &rows[ix].text;
                        let mut end = body
                            .len()
                            .min((2 * 1024 * 1024usize).saturating_sub(text.len()));
                        while !body.is_char_boundary(end) {
                            end -= 1;
                        }
                        text.push_str(&body[..end]);
                        truncated |= end < body.len();
                        ix += 1;
                        if ix == rows.len() || rows[ix].first {
                            break;
                        }
                        if cancel.load(Ordering::Acquire) {
                            return Ok((Vec::new(), false));
                        }
                    }
                    if let Some(hit) = crate::search::message_hit(
                        summary,
                        from,
                        &rows[from].role,
                        &text,
                        query,
                        false,
                    ) {
                        hits.push(hit);
                    }
                    if hits.len() >= 400 {
                        truncated = true;
                        break 'sessions;
                    }
                }
            }
            hits.extend(crate::search::titles(&summaries, query));
            Ok((hits, truncated))
        })();
        sql(self.conn.progress_handler(0, None::<fn() -> bool>))?;
        if cancel.load(Ordering::Acquire) {
            Ok((Vec::new(), false))
        } else {
            result
        }
    }
    pub(crate) fn shared_follow_ups(&self, id: i64) -> Result<crate::follow_up::State, String> {
        let fields = format!(
            "{},{}",
            self.shared_field("queued_messages_json", "'[]'"),
            self.shared_field("queue_status", "NULL")
        );
        let (raw, status) = sql(self.conn.query_row(
            &format!("SELECT {fields} FROM sessions WHERE id=?1"),
            [self.persisted_session_id(id)?],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        ))?;
        let mut state: crate::follow_up::State = self
            .shared_setting(&format!("follow-up:{id}"))?
            .map(|raw| parse(&raw))
            .transpose()?
            .unwrap_or_default();
        self.shared
            .as_ref()
            .unwrap()
            .queues
            .borrow_mut()
            .insert(id, raw.clone());
        state.messages = parse::<Vec<Value>>(&raw)?.iter().map(follow_up).collect();
        let blocks = self.shared_blocks(id)?;
        state.draft = None;
        if let Some((at, block)) = blocks.iter().enumerate().find(|(_, b)| b["draft"] == true) {
            let mut draft = follow_up(block);
            draft.at = blocks::project(&blocks[..at])?.len();
            state.draft = Some(draft);
        }
        state.paused = status.as_deref() == Some("paused") || !state.messages.is_empty();
        Ok(state)
    }
    pub(crate) fn shared_save_follow_ups(
        &self,
        id: i64,
        state: &crate::follow_up::State,
    ) -> Result<(), String> {
        self.shared_writable()?;
        let previous: crate::follow_up::State = self
            .shared_setting(&format!("follow-up:{id}"))?
            .map(|raw| parse(&raw))
            .transpose()?
            .unwrap_or_default();
        if previous.revision > state.revision {
            return Ok(());
        }
        let tx = sql(self.conn.unchecked_transaction())?;
        let fields = self.shared_field("queued_messages_json", "'[]'");
        let raw: String = sql(self.conn.query_row(
            &format!("SELECT {fields} FROM sessions WHERE id=?1"),
            [self.persisted_session_id(id)?],
            |r| r.get(0),
        ))?;
        if self
            .shared
            .as_ref()
            .unwrap()
            .queues
            .borrow()
            .get(&id)
            .is_some_and(|expected| expected != &raw)
        {
            return Err("The message queue changed in another window. Reopen it before saving; its data was preserved.".into());
        }
        let old: Vec<Value> = parse(&raw)?;
        let messages = state
            .messages
            .iter()
            .map(|m| {
                let mut value = old
                    .iter()
                    .find(|v| v["id"].as_str() == Some(&m.id))
                    .cloned()
                    .unwrap_or(json!({}));
                update_follow_up(&mut value, m);
                value
            })
            .collect::<Vec<_>>();
        self.shared_patch(
            id,
            &[
                ("queued_messages_json", json!(encode(&messages)?)),
                (
                    "queue_status",
                    json!(if state.paused { "paused" } else { "active" }),
                ),
            ],
        )?;
        let mut blocks = self.shared_blocks(id)?;
        if self
            .shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow()
            .get(&id)
            .is_some_and(|old| !old.matches(&blocks))
        {
            return Err("This conversation changed in another window. Reopen it before saving; its data was preserved.".into());
        }

        let old_draft = blocks.iter().find(|b| b["draft"] == true).cloned();
        blocks.retain(|b| b["draft"] != true);
        if let Some(draft) = &state.draft {
            let mut value = old_draft
                .filter(|v| v["id"].as_str() == Some(&draft.id))
                .unwrap_or(json!({"id":draft.id,"role":"user","draft":true}));
            update_follow_up(&mut value, draft);
            let mut at = blocks.len();
            let mut row = 0;
            for (ix, block) in blocks.iter().enumerate() {
                if row >= draft.at {
                    at = ix;
                    break;
                }
                row += blocks::project(std::slice::from_ref(block))?.len();
            }
            blocks.insert(at, value);
        }
        self.shared_store_blocks(id, &blocks)?;
        sql(tx.commit())?;
        self.shared
            .as_ref()
            .unwrap()
            .snapshots
            .borrow_mut()
            .insert(id, HistoryRevision::of(&blocks));
        self.shared
            .as_ref()
            .unwrap()
            .queues
            .borrow_mut()
            .insert(id, encode(&messages)?);
        self.shared_save_setting(&format!("follow-up:{id}"), &encode(state)?)
    }
}

fn follow_up(value: &Value) -> crate::follow_up::Message {
    crate::follow_up::Message {
        id: value["id"].as_str().unwrap_or("").into(),
        text: value["text"].as_str().unwrap_or("").into(),
        prepared_prompt: value["preparedPrompt"].as_str().map(str::to_owned),
        attachments: value["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["path"].as_str())
            .map(PathBuf::from)
            .collect(),
        mode: value["intent"].as_str().map(str::to_owned),
        error: value["error"].as_str().map(str::to_owned),
        ..Default::default()
    }
}
fn update_follow_up(value: &mut Value, message: &crate::follow_up::Message) {
    value["id"] = json!(message.id);
    value["text"] = json!(message.text);
    value["preparedPrompt"] = json!(message.prepared_prompt);
    if let Some(mode) = &message.mode {
        value["intent"] = json!(mode);
    }
    if let Some(error) = &message.error {
        value["error"] = json!(error);
    }
    if value["attachments"].is_null() {
        value["attachments"]=json!(message.attachments.iter().map(|p|json!({"id":p.to_string_lossy(),"path":p,"name":p.file_name().unwrap_or_default().to_string_lossy(),
            "mimeType":"application/octet-stream","kind":"file","size":std::fs::metadata(p).map(|m|m.len()).unwrap_or(0)})).collect::<Vec<_>>());
    }
}
