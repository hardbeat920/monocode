//! Port of src/features/sessions/model/sessionFolders.ts: sidebar folders,
//! the pinned and reminder groups, and the per-project folder storage.
//!
//! localStorage becomes `Kv` with the same keys and JSON values. The change
//! event the TypeScript dispatched after a save becomes a `Kv` key
//! subscription (`subscribe_session_folders`).
//!
//! The TypeScript returned the same array when nothing changed, and callers
//! compared by identity. These functions return a new `Vec` either way, and
//! callers compare with `==`.

use std::collections::HashSet;

use monocode_core::appearance::is_hex_color;
use monocode_layout::tab_groups::TAB_GROUP_COLORS;
use monocode_settings::{Kv, Subscription};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::runtime::session_history::compare_session_summaries;
use crate::runtime::session_store::SessionSummary;
use crate::runtime::util::project_path::normalize_project_path;
use crate::runtime::util::reorder::{HasId, order_by_ids};

/// `KEY`.
pub const SESSION_FOLDERS_KEY: &str = "monocode.sessionFolders";
/// `PINNED_COLLAPSED_KEY`.
pub const PINNED_COLLAPSED_KEY: &str = "monocode.pinnedSessionsCollapsed";
/// `REMINDERS_COLLAPSED_KEY`.
pub const REMINDERS_COLLAPSED_KEY: &str = "monocode.reminderSessionsCollapsed";
/// The opacity `folderShellFill` mixes the accent at.
pub const FOLDER_SHELL_FILL_ALPHA: f32 = 0.18;

/// `SessionFolder`. Loading rebuilds folders from their known fields, as the
/// TypeScript did, so there is no `extra` map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFolder {
    pub id: String,
    pub name: String,
    pub session_ids: Vec<String>,
    pub collapsed: bool,
    /// Palette index from `TAB_GROUP_COLORS`. Missing or 0 is the default wash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_index: Option<i64>,
    /// Custom hex from the folder color picker. Wins over `color_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_color: Option<String>,
}

impl SessionFolder {
    /// A folder with no color.
    pub fn new(id: impl Into<String>, name: impl Into<String>, session_ids: Vec<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            session_ids,
            collapsed: false,
            color_index: None,
            custom_color: None,
        }
    }
}

impl HasId for SessionFolder {
    fn id(&self) -> &str {
        &self.id
    }
}

/// `SessionFolderTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionFolderTarget {
    Existing { folder_id: String },
    New { name: String },
}

/// `SessionListDropTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionListDropTarget {
    Folder { id: String },
    Session { id: String },
}

/// `SessionListEntry`: one row group of the sidebar session list.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionListEntry {
    Folder {
        folder: SessionFolder,
        sessions: Vec<SessionSummary>,
    },
    Pinned {
        collapsed: bool,
        sessions: Vec<SessionSummary>,
    },
    Reminders {
        collapsed: bool,
        sessions: Vec<SessionSummary>,
    },
    /// Boxed, because a row is much larger than the other variants.
    Session { session: Box<SessionSummary> },
}

/// The reminder group `buildSessionList` puts first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReminderGroup {
    pub session_ids: Vec<String>,
    pub collapsed: bool,
}

/// `folderContaining`.
pub fn folder_containing<'a>(
    folders: &'a [SessionFolder],
    session_id: &str,
) -> Option<&'a SessionFolder> {
    folders
        .iter()
        .find(|folder| folder.session_ids.iter().any(|id| id == session_id))
}

/// `uniqueFolderName` with the default base.
pub fn unique_folder_name(folders: &[SessionFolder]) -> String {
    unique_folder_name_from(folders, "New folder")
}

/// `uniqueFolderName`.
pub fn unique_folder_name_from(folders: &[SessionFolder], base: &str) -> String {
    let names: HashSet<&str> = folders.iter().map(|folder| folder.name.as_str()).collect();
    if !names.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base} {n}");
        if !names.contains(candidate.as_str()) {
            return candidate;
        }
        n += 1;
    }
}

fn grouped_ids(folders: &[SessionFolder]) -> HashSet<&str> {
    folders
        .iter()
        .flat_map(|folder| folder.session_ids.iter().map(String::as_str))
        .collect()
}

/// `ungroupedSessions`.
pub fn ungrouped_sessions(
    sessions: &[SessionSummary],
    folders: &[SessionFolder],
) -> Vec<SessionSummary> {
    let grouped = grouped_ids(folders);
    sessions
        .iter()
        .filter(|session| !grouped.contains(session.id.as_str()))
        .cloned()
        .collect()
}

/// `mergeFolderSessionSummaries`: open tabs that belong to a folder but are
/// not in history yet (blank chats).
pub fn merge_folder_session_summaries(
    visible: &[SessionSummary],
    extras: &[SessionSummary],
    folders: &[SessionFolder],
) -> Vec<SessionSummary> {
    if extras.is_empty() || folders.is_empty() {
        return visible.to_vec();
    }
    let grouped = grouped_ids(folders);
    let have: HashSet<&str> = visible.iter().map(|session| session.id.as_str()).collect();
    let mut merged = visible.to_vec();
    merged.extend(
        extras
            .iter()
            .filter(|session| {
                grouped.contains(session.id.as_str()) && !have.contains(session.id.as_str())
            })
            .cloned(),
    );
    merged
}

/// `buildSessionList`: reminders always lead, followed by folders, pins, and
/// loose sessions. Reminder membership only changes this view, preserving
/// saved folders and pins.
pub fn build_session_list(
    visible: &[SessionSummary],
    folders: &[SessionFolder],
    ungrouped: &[SessionSummary],
    pinned_collapsed: bool,
    reminder_group: Option<&ReminderGroup>,
) -> Vec<SessionListEntry> {
    let by_id = |id: &str| visible.iter().find(|session| session.id == id);
    // A JS Set keeps first-insertion order and drops repeats.
    let mut reminder_ids: Vec<&str> = Vec::new();
    for id in reminder_group
        .map(|group| group.session_ids.as_slice())
        .unwrap_or(&[])
    {
        if !reminder_ids.contains(&id.as_str()) {
            reminder_ids.push(id);
        }
    }
    let mut entries = Vec::new();
    let reminder_sessions: Vec<SessionSummary> = reminder_ids
        .iter()
        .filter_map(|id| by_id(id).cloned())
        .collect();
    if !reminder_sessions.is_empty() {
        entries.push(SessionListEntry::Reminders {
            collapsed: reminder_group.is_some_and(|group| group.collapsed),
            sessions: reminder_sessions,
        });
    }
    for folder in folders {
        let mut members: Vec<SessionSummary> = folder
            .session_ids
            .iter()
            .filter(|id| !reminder_ids.contains(&id.as_str()))
            .filter_map(|id| by_id(id).cloned())
            .collect();
        if members.is_empty() {
            continue;
        }
        members.sort_by(compare_session_summaries);
        entries.push(SessionListEntry::Folder {
            folder: folder.clone(),
            sessions: members,
        });
    }
    let remaining: Vec<&SessionSummary> = ungrouped
        .iter()
        .filter(|session| !reminder_ids.contains(&session.id.as_str()))
        .collect();
    let pinned: Vec<SessionSummary> = remaining
        .iter()
        .filter(|session| session.pinned == Some(true))
        .map(|session| (*session).clone())
        .collect();
    if !pinned.is_empty() {
        entries.push(SessionListEntry::Pinned {
            collapsed: pinned_collapsed,
            sessions: pinned,
        });
    }
    for session in remaining {
        if session.pinned != Some(true) {
            entries.push(SessionListEntry::Session {
                session: Box::new(session.clone()),
            });
        }
    }
    entries
}

/// `sessionListNavigationIds`.
pub fn session_list_navigation_ids(
    entries: &[SessionListEntry],
    expand_collapsed: bool,
) -> Vec<String> {
    let mut ids = Vec::new();
    for entry in entries {
        match entry {
            SessionListEntry::Session { session } => ids.push(session.id.clone()),
            SessionListEntry::Pinned {
                collapsed,
                sessions,
            }
            | SessionListEntry::Reminders {
                collapsed,
                sessions,
            } => {
                if !collapsed || expand_collapsed {
                    ids.extend(sessions.iter().map(|session| session.id.clone()));
                }
            }
            SessionListEntry::Folder { folder, sessions } => {
                if folder.collapsed && !expand_collapsed {
                    continue;
                }
                ids.extend(sessions.iter().map(|session| session.id.clone()));
            }
        }
    }
    ids
}

/// `createFolderWithSessions`: the new folder goes on top. The id is empty
/// when no session was given.
pub fn create_folder_with_sessions(
    folders: &[SessionFolder],
    session_ids: &[String],
    name: Option<&str>,
) -> (Vec<SessionFolder>, String) {
    let ids = unique_ids(session_ids);
    if ids.is_empty() {
        return (folders.to_vec(), String::new());
    }
    let next = remove_sessions(folders, &ids);
    let id = uuid::Uuid::new_v4().to_string();
    let folder = SessionFolder::new(
        id.clone(),
        name.map(str::to_string)
            .unwrap_or_else(|| unique_folder_name(&next)),
        ids,
    );
    let mut out = Vec::with_capacity(next.len() + 1);
    out.push(folder);
    out.extend(next);
    (out, id)
}

/// `addSessionToFolder`.
pub fn add_session_to_folder(
    folders: &[SessionFolder],
    folder_id: &str,
    session_id: &str,
) -> Vec<SessionFolder> {
    if session_id.is_empty() || !folders.iter().any(|folder| folder.id == folder_id) {
        return folders.to_vec();
    }
    if folder_containing(folders, session_id).is_some_and(|current| current.id == folder_id) {
        return folders.to_vec();
    }
    remove_empty(
        folders
            .iter()
            .map(|folder| {
                let mut folder = folder.clone();
                if folder.id == folder_id {
                    folder.session_ids.push(session_id.to_string());
                } else {
                    folder.session_ids.retain(|id| id != session_id);
                }
                folder
            })
            .collect(),
    )
}

/// `placeSessionInFolder`.
pub fn place_session_in_folder(
    folders: &[SessionFolder],
    session_id: &str,
    target: &SessionFolderTarget,
) -> Vec<SessionFolder> {
    match target {
        SessionFolderTarget::Existing { folder_id } => set_folder_collapsed(
            &add_session_to_folder(folders, folder_id, session_id),
            folder_id,
            false,
        ),
        SessionFolderTarget::New { name } => {
            let name = monocode_core::js::trim(name);
            if name.is_empty() {
                return folders.to_vec();
            }
            create_folder_with_sessions(folders, &[session_id.to_string()], Some(name)).0
        }
    }
}

/// `removeSessionFromFolder`.
pub fn remove_session_from_folder(
    folders: &[SessionFolder],
    session_id: &str,
) -> Vec<SessionFolder> {
    if folder_containing(folders, session_id).is_none() {
        return folders.to_vec();
    }
    remove_empty(
        folders
            .iter()
            .map(|folder| {
                let mut folder = folder.clone();
                folder.session_ids.retain(|id| id != session_id);
                folder
            })
            .collect(),
    )
}

/// `dissolveFolder`.
pub fn dissolve_folder(folders: &[SessionFolder], folder_id: &str) -> Vec<SessionFolder> {
    folders
        .iter()
        .filter(|folder| folder.id != folder_id)
        .cloned()
        .collect()
}

/// `renameFolder`: a blank name changes nothing.
pub fn rename_folder(folders: &[SessionFolder], folder_id: &str, name: &str) -> Vec<SessionFolder> {
    let trimmed = monocode_core::js::trim(name);
    if trimmed.is_empty() {
        return folders.to_vec();
    }
    map_folder(folders, folder_id, |folder| {
        (folder.name != trimmed).then(|| SessionFolder {
            name: trimmed.to_string(),
            ..folder.clone()
        })
    })
}

/// `setFolderCollapsed`.
pub fn set_folder_collapsed(
    folders: &[SessionFolder],
    folder_id: &str,
    collapsed: bool,
) -> Vec<SessionFolder> {
    map_folder(folders, folder_id, |folder| {
        (folder.collapsed != collapsed).then(|| SessionFolder {
            collapsed,
            ..folder.clone()
        })
    })
}

/// `setFolderColor`: a palette index, or `None` (and 0) for the default wash.
/// Clears any custom color.
pub fn set_folder_color(
    folders: &[SessionFolder],
    folder_id: &str,
    color_index: Option<i64>,
) -> Vec<SessionFolder> {
    let next_index = sanitize_color_index(color_index);
    map_folder(folders, folder_id, |folder| {
        if folder.color_index == next_index && folder.custom_color.is_none() {
            return None;
        }
        Some(SessionFolder {
            color_index: next_index,
            ..without_colors(folder)
        })
    })
}

/// `setFolderCustomColor`: a `#rrggbb` hex, or `None` to clear. An invalid
/// hex changes nothing.
pub fn set_folder_custom_color(
    folders: &[SessionFolder],
    folder_id: &str,
    color: Option<&str>,
) -> Vec<SessionFolder> {
    map_folder(folders, folder_id, |folder| match color {
        None => folder
            .custom_color
            .is_some()
            .then(|| without_colors(folder)),
        Some(color) => {
            let hex = parse_custom_hex(Some(color))?;
            if folder.custom_color.as_deref() == Some(hex.as_str()) && folder.color_index.is_none()
            {
                return None;
            }
            Some(SessionFolder {
                custom_color: Some(hex),
                ..without_colors(folder)
            })
        }
    })
}

/// `reorderSessionFolders`: reorder only the named folders; anything else
/// keeps its slot.
pub fn reorder_session_folders(folders: &[SessionFolder], ids: &[String]) -> Vec<SessionFolder> {
    let id_set: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let moving: Vec<SessionFolder> = folders
        .iter()
        .filter(|folder| id_set.contains(folder.id.as_str()))
        .cloned()
        .collect();
    let ordered = order_by_ids(&moving, ids);
    if ordered.len() != moving.len()
        || ordered
            .iter()
            .zip(&moving)
            .all(|(folder, previous)| folder.id == previous.id)
    {
        return folders.to_vec();
    }
    let mut next = ordered.into_iter();
    folders
        .iter()
        .map(|folder| {
            if id_set.contains(folder.id.as_str()) {
                next.next().unwrap_or_else(|| folder.clone())
            } else {
                folder.clone()
            }
        })
        .collect()
}

/// `folderAccent`: the custom hex, else the saturated palette color, else
/// `None` for the default wash.
pub fn folder_accent(color_index: Option<i64>, custom_color: Option<&str>) -> Option<String> {
    if let Some(hex) = parse_custom_hex(custom_color) {
        return Some(hex);
    }
    sanitize_color_index(color_index).map(|index| TAB_GROUP_COLORS[index as usize].to_string())
}

/// `folderShellFill`: a quiet CSS fill so a folder tint never reads as a
/// solid chip. Views draw it as the accent at `FOLDER_SHELL_FILL_ALPHA`.
pub fn folder_shell_fill(color_index: Option<i64>, custom_color: Option<&str>) -> Option<String> {
    let accent = folder_accent(color_index, custom_color)?;
    Some(format!("color-mix(in srgb, {accent} 18%, transparent)"))
}

fn sanitize_color_index(color_index: Option<i64>) -> Option<i64> {
    let index = color_index?;
    if index <= 0 || index >= TAB_GROUP_COLORS.len() as i64 {
        return None;
    }
    Some(index)
}

/// `applySessionListDrop`: dropping on a folder (or a session already in one)
/// joins that folder. Dropping on an ungrouped session opens a new folder
/// around both. The second value is the id of a created folder.
pub fn apply_session_list_drop(
    folders: &[SessionFolder],
    dragged_id: &str,
    target: &SessionListDropTarget,
) -> (Vec<SessionFolder>, Option<String>) {
    if dragged_id.is_empty() {
        return (folders.to_vec(), None);
    }
    match target {
        SessionListDropTarget::Session { id } if id == dragged_id => (folders.to_vec(), None),
        SessionListDropTarget::Folder { id } => {
            let next = add_session_to_folder(folders, id, dragged_id);
            (set_folder_collapsed(&next, id, false), None)
        }
        SessionListDropTarget::Session { id } => {
            if let Some(dest) = folder_containing(folders, id) {
                if folder_containing(folders, dragged_id).is_some_and(|from| from.id == dest.id) {
                    return (folders.to_vec(), None);
                }
                let next = add_session_to_folder(folders, &dest.id, dragged_id);
                return (set_folder_collapsed(&next, &dest.id, false), None);
            }
            let (next, created) =
                create_folder_with_sessions(folders, &[dragged_id.to_string(), id.clone()], None);
            if created.is_empty() {
                (folders.to_vec(), None)
            } else {
                (next, Some(created))
            }
        }
    }
}

/// `pruneSessionFolders`: drop unknown members and emptied folders.
pub fn prune_session_folders(
    folders: &[SessionFolder],
    known_ids: &HashSet<String>,
) -> Vec<SessionFolder> {
    folders
        .iter()
        .filter_map(|folder| {
            let session_ids: Vec<String> = folder
                .session_ids
                .iter()
                .filter(|id| known_ids.contains(*id))
                .cloned()
                .collect();
            (!session_ids.is_empty()).then(|| SessionFolder {
                session_ids,
                ..folder.clone()
            })
        })
        .collect()
}

// Storage.

/// `loadSessionFolders`.
pub fn load_session_folders(kv: &Kv, cwd: &str) -> Vec<SessionFolder> {
    let Some(key) = storage_key(cwd) else {
        return Vec::new();
    };
    parse_store(kv).remove(&key).unwrap_or_default()
}

/// `saveSessionFolders`. Saving no folders drops the project's key.
pub fn save_session_folders(kv: &Kv, cwd: &str, folders: &[SessionFolder]) {
    let Some(key) = storage_key(cwd) else {
        return;
    };
    let mut store = parse_store(kv);
    if folders.is_empty() {
        store.remove(&key);
    } else {
        store.insert(key, folders.to_vec());
    }
    if let Ok(raw) = serde_json::to_string(&store) {
        kv.set_item(SESSION_FOLDERS_KEY, &raw);
    }
}

/// `rebaseSessionFolderSettings`: move folder and collapsed-group state when a
/// project path changes.
pub fn rebase_session_folder_settings(kv: &Kv, from: &str, to: &str) {
    let (Some(old_key), Some(new_key)) = (storage_key(from), storage_key(to)) else {
        return;
    };
    if old_key == new_key {
        return;
    }
    let mut folders = parse_store(kv);
    if let Some(moved) = folders.remove(&old_key) {
        folders.entry(new_key.clone()).or_insert(moved);
        if let Ok(raw) = serde_json::to_string(&folders) {
            kv.set_item(SESSION_FOLDERS_KEY, &raw);
        }
    }
    for store_key in [PINNED_COLLAPSED_KEY, REMINDERS_COLLAPSED_KEY] {
        // A stored value that fails to parse ends the TypeScript loop in its
        // catch, so later keys keep their old project.
        let Some(parsed) = read_json(kv, store_key) else {
            return;
        };
        let Value::Object(mut state) = parsed else {
            continue;
        };
        let Some(moved) = state.remove(&old_key) else {
            continue;
        };
        state.entry(new_key.clone()).or_insert(moved);
        kv.set_item(store_key, &Value::Object(state).to_string());
    }
}

/// `loadPinnedSessionsCollapsed`.
pub fn load_pinned_sessions_collapsed(kv: &Kv, cwd: &str) -> bool {
    load_group_collapsed(kv, cwd, PINNED_COLLAPSED_KEY)
}

/// `loadReminderSessionsCollapsed`.
pub fn load_reminder_sessions_collapsed(kv: &Kv, cwd: &str) -> bool {
    load_group_collapsed(kv, cwd, REMINDERS_COLLAPSED_KEY)
}

/// `savePinnedSessionsCollapsed`.
pub fn save_pinned_sessions_collapsed(kv: &Kv, cwd: &str, collapsed: bool) {
    save_group_collapsed(kv, cwd, collapsed, PINNED_COLLAPSED_KEY);
}

/// `saveReminderSessionsCollapsed`.
pub fn save_reminder_sessions_collapsed(kv: &Kv, cwd: &str, collapsed: bool) {
    save_group_collapsed(kv, cwd, collapsed, REMINDERS_COLLAPSED_KEY);
}

fn load_group_collapsed(kv: &Kv, cwd: &str, store_key: &str) -> bool {
    let Some(key) = storage_key(cwd) else {
        return false;
    };
    match read_json(kv, store_key) {
        Some(Value::Object(state)) => state.get(&key) == Some(&Value::Bool(true)),
        _ => false,
    }
}

fn save_group_collapsed(kv: &Kv, cwd: &str, collapsed: bool, store_key: &str) {
    let Some(key) = storage_key(cwd) else {
        return;
    };
    let raw = kv.get_item(store_key);
    let parsed = match raw.as_deref().filter(|raw| !raw.is_empty()) {
        // A parse failure lands in the TypeScript catch and saves nothing.
        Some(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(value) => value,
            Err(_) => return,
        },
        None => Value::Object(Map::new()),
    };
    let mut state = match parsed {
        Value::Object(state) => state,
        _ => Map::new(),
    };
    if collapsed {
        state.insert(key, Value::Bool(true));
    } else {
        state.remove(&key);
    }
    kv.set_item(store_key, &Value::Object(state).to_string());
}

/// `subscribeSessionFolders`: call `on_change` when the saved folders of this
/// project change. The TypeScript heard its own change event, which every
/// save fired; this hears the stored value, so a save that changes nothing
/// stays quiet.
pub fn subscribe_session_folders(
    kv: &Kv,
    cwd: &str,
    on_change: impl Fn() + Send + Sync + 'static,
) -> Option<Subscription> {
    let key = storage_key(cwd)?;
    Some(kv.subscribe_key(SESSION_FOLDERS_KEY, move |change| {
        let before = parse_store_raw(change.old_value.as_deref()).remove(&key);
        let after = parse_store_raw(change.new_value.as_deref()).remove(&key);
        if before != after {
            on_change();
        }
    }))
}

fn storage_key(cwd: &str) -> Option<String> {
    if cwd.is_empty() || cwd == "~" {
        return None;
    }
    Some(normalize_project_path(cwd))
}

fn read_json(kv: &Kv, key: &str) -> Option<Value> {
    match kv.get_item(key) {
        Some(raw) if !raw.is_empty() => serde_json::from_str(&raw).ok(),
        _ => Some(Value::Object(Map::new())),
    }
}

// TODO(port): the TypeScript kept the stored key order; `serde_json` here
// sorts object keys, so a save can reorder projects in the stored JSON.
type FolderStore = std::collections::BTreeMap<String, Vec<SessionFolder>>;

fn parse_store(kv: &Kv) -> FolderStore {
    parse_store_raw(kv.get_item(SESSION_FOLDERS_KEY).as_deref())
}

fn parse_store_raw(raw: Option<&str>) -> FolderStore {
    let mut out = FolderStore::new();
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return out;
    };
    let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(raw) else {
        return out;
    };
    for (cwd, value) in parsed {
        if cwd.is_empty() || cwd == "~" {
            continue;
        }
        let folders = parse_folders(&value);
        if !folders.is_empty() {
            out.insert(normalize_project_path(&cwd), folders);
        }
    }
    out
}

fn parse_folders(value: &Value) -> Vec<SessionFolder> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let Some(folder) = parse_folder(item) else {
            continue;
        };
        if !seen.insert(folder.id.clone()) {
            continue;
        }
        out.push(folder);
    }
    out
}

fn parse_folder(value: &Value) -> Option<SessionFolder> {
    let rec = value.as_object()?;
    let id = rec.get("id")?.as_str().filter(|id| !id.is_empty())?;
    let name = monocode_core::js::trim(rec.get("name")?.as_str()?);
    if name.is_empty() {
        return None;
    }
    let raw_ids: Vec<String> = rec
        .get("sessionIds")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    let session_ids = unique_ids(&raw_ids);
    if session_ids.is_empty() {
        return None;
    }
    let custom_color = parse_custom_hex(rec.get("customColor").and_then(Value::as_str));
    // `Number.isInteger` accepts 2.0, so a whole float counts.
    let color_index = sanitize_color_index(rec.get("colorIndex").and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_f64()
                .filter(|n| n.fract() == 0.0 && n.abs() < i64::MAX as f64)
                .map(|n| n as i64)
        })
    }));
    let (custom_color, color_index) = match custom_color {
        Some(hex) => (Some(hex), None),
        None => (None, color_index),
    };
    Some(SessionFolder {
        id: id.to_string(),
        name: name.to_string(),
        session_ids,
        collapsed: rec.get("collapsed") == Some(&Value::Bool(true)),
        color_index,
        custom_color,
    })
}

fn parse_custom_hex(color: Option<&str>) -> Option<String> {
    color
        .filter(|color| is_hex_color(color))
        .map(str::to_lowercase)
}

fn without_colors(folder: &SessionFolder) -> SessionFolder {
    SessionFolder {
        color_index: None,
        custom_color: None,
        ..folder.clone()
    }
}

fn unique_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.iter()
        .filter(|id| !id.is_empty() && seen.insert(id.as_str()))
        .cloned()
        .collect()
}

fn remove_sessions(folders: &[SessionFolder], session_ids: &[String]) -> Vec<SessionFolder> {
    let drop: HashSet<&str> = session_ids.iter().map(String::as_str).collect();
    remove_empty(
        folders
            .iter()
            .map(|folder| {
                let mut folder = folder.clone();
                folder.session_ids.retain(|id| !drop.contains(id.as_str()));
                folder
            })
            .collect(),
    )
}

fn remove_empty(folders: Vec<SessionFolder>) -> Vec<SessionFolder> {
    folders
        .into_iter()
        .filter(|folder| !folder.session_ids.is_empty())
        .collect()
}

/// Replace one folder through `change`, which returns `None` to keep the list
/// as it is. A missing folder changes nothing.
fn map_folder(
    folders: &[SessionFolder],
    folder_id: &str,
    change: impl FnOnce(&SessionFolder) -> Option<SessionFolder>,
) -> Vec<SessionFolder> {
    let Some(index) = folders.iter().position(|folder| folder.id == folder_id) else {
        return folders.to_vec();
    };
    let mut next = folders.to_vec();
    if let Some(changed) = change(&folders[index]) {
        next[index] = changed;
    }
    next
}

#[cfg(test)]
#[path = "session_folders_tests.rs"]
mod tests;
