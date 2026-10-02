//! Port of src/features/workspace/model/tabGroups.ts: title-bar tab groups
//! and the per-project appearance overrides (color, label, logo, mascot).
//!
//! The TypeScript read and wrote localStorage directly and kept migration
//! state in module globals. Here the storage comes in as an
//! `AppearanceStore` (the settings crate's `Kv` can implement it with the
//! same keys), `TabGroupAppearance` holds the module state, and the window
//! events the TypeScript dispatched queue up as `AppearanceEvent`s for the
//! caller to drain.

use serde::Serialize;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};

use crate::ids::random_uuid;
use crate::layout::WorkspaceTab;
use crate::paths::{project_key, project_name};

pub use crate::workspace_tab_groups::replace_group_in_tab_order;

/// `TAB_GROUP_COLORS`: a Chrome-like palette, saturated enough to read on
/// dark glass. Index 0 is the neutral gray the hash never picks.
pub const TAB_GROUP_COLORS: [&str; 9] = [
    "hsl(210 8% 58%)",
    "hsl(211 92% 62%)",
    "hsl(12 80% 58%)",
    "hsl(45 90% 55%)",
    "hsl(142 55% 50%)",
    "hsl(330 70% 62%)",
    "hsl(280 55% 62%)",
    "hsl(175 55% 48%)",
    "hsl(25 85% 58%)",
];

/// `tabGroupColor`: a stable palette color for a key, never the gray.
pub fn tab_group_color(project: &str) -> &'static str {
    let mut hash: u64 = 0;
    for unit in project.encode_utf16() {
        hash = (hash * 31 + u64::from(unit)) % (1 << 32);
    }
    let len = TAB_GROUP_COLORS.len() as u64;
    TAB_GROUP_COLORS[((hash % (len - 1)) + 1) as usize]
}

/// `COLOR_KEY`: palette indexes per project, stored as strings.
pub const COLOR_KEY: &str = "monocode:tab-group:colors";
/// `CUSTOM_COLOR_KEY`: `#rrggbb` colors per project.
pub const CUSTOM_COLOR_KEY: &str = "monocode:tab-group:custom-colors";
/// `LABEL_KEY`.
pub const LABEL_KEY: &str = "monocode:tab-group:labels";
/// `LOGO_KEY`.
pub const LOGO_KEY: &str = "monocode:tab-group:logos";
/// `MASCOT_KEY`.
pub const MASCOT_KEY: &str = "monocode:tab-group:mascots";
/// `KEY_VERSION_KEY`: set to `KEY_VERSION` once the folder-name migration finished.
pub const KEY_VERSION_KEY: &str = "monocode:tab-group:key-version";
const KEY_VERSION: &str = "2";
/// `COLLAPSED_KEY`.
pub const COLLAPSED_KEY: &str = "monocode:tab-groups:collapsed";

/// `APPEARANCE_KEYS`.
const APPEARANCE_KEYS: [&str; 5] = [COLOR_KEY, CUSTOM_COLOR_KEY, LABEL_KEY, LOGO_KEY, MASCOT_KEY];

/// `HEX_COLOR_RE`: `/^#[0-9a-fA-F]{6}$/`.
fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

/// A JavaScript object used as a string-keyed record. Iteration follows
/// `Object.entries`: array-index keys first in ascending order, then the
/// other keys in insertion order. The folder-name migration depends on that
/// order when two entries land on the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsRecord<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for JsRecord<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

/// A canonical array index: `"0"` to `"4294967294"` without leading zeros.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

impl<V> JsRecord<V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `record[key]`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    /// `key in record`.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `record[key] = value`: an existing key keeps its place.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        if let Some(entry) = self.entries.iter_mut().find(|(entry, _)| *entry == key) {
            entry.1 = value;
            return;
        }
        match array_index(&key) {
            Some(index) => {
                let at = self
                    .entries
                    .iter()
                    .position(|(entry, _)| array_index(entry).is_none_or(|other| other > index))
                    .unwrap_or(self.entries.len());
                self.entries.insert(at, (key, value));
            }
            None => self.entries.push((key, value)),
        }
    }

    /// `delete record[key]`.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        Some(self.entries.remove(index).1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `Object.entries(record)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }
}

impl<V: Serialize> Serialize for JsRecord<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in &self.entries {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// `JSON.parse(raw)` filtered to string values, keeping document order.
/// Arrays count as objects keyed by index, as `Object.entries` sees them.
struct StringRecord(JsRecord<String>);

impl<'de> Deserialize<'de> for StringRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;

        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = StringRecord;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<StringRecord, A::Error> {
                let mut record = JsRecord::new();
                while let Some((key, value)) = access.next_entry::<String, serde_json::Value>()? {
                    // Every key becomes a property; only string values survive the filter.
                    match value {
                        serde_json::Value::String(value) => record.insert(key, value),
                        _ => {
                            record.remove(&key);
                        }
                    }
                }
                Ok(StringRecord(record))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<StringRecord, A::Error> {
                let mut record = JsRecord::new();
                let mut index = 0usize;
                while let Some(value) = access.next_element::<serde_json::Value>()? {
                    if let serde_json::Value::String(value) = value {
                        record.insert(index.to_string(), value);
                    }
                    index += 1;
                }
                Ok(StringRecord(record))
            }

            fn visit_unit<E>(self) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }

            fn visit_bool<E>(self, _: bool) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }

            fn visit_i64<E>(self, _: i64) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }

            fn visit_u64<E>(self, _: u64) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }

            fn visit_f64<E>(self, _: f64) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }

            fn visit_str<E>(self, _: &str) -> Result<StringRecord, E> {
                Ok(StringRecord(JsRecord::new()))
            }
        }

        deserializer.deserialize_any(RecordVisitor)
    }
}

/// The localStorage the appearance overrides live in.
pub trait AppearanceStore {
    /// `localStorage.getItem`.
    fn get_item(&self, key: &str) -> Option<String>;
    /// `localStorage.setItem`. Returns `false` when the write failed, where
    /// the TypeScript caught a quota error.
    fn set_item(&mut self, key: &str, value: &str) -> bool;
    /// `knownProjectPaths` from src/features/projects/model/recents.ts: every
    /// project path still remembered (rail, pins, saved order, archive).
    fn known_project_paths(&self) -> Vec<String>;
}

/// An in-memory `AppearanceStore`, for tests and headless use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryStore {
    pub items: std::collections::BTreeMap<String, String>,
    pub known_paths: Vec<String>,
}

impl AppearanceStore for MemoryStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.items.get(key).cloned()
    }

    fn set_item(&mut self, key: &str, value: &str) -> bool {
        self.items.insert(key.to_string(), value.to_string());
        true
    }

    fn known_project_paths(&self) -> Vec<String> {
        self.known_paths.clone()
    }
}

/// The window events the TypeScript dispatched after a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppearanceEvent {
    /// `monocode:tab-group-labels-changed`.
    LabelsChanged,
    /// `monocode:tab-group-logos-changed`.
    LogosChanged,
    /// `notifyProjectPathsChanged` in recents.ts.
    ProjectPathsChanged,
}

/// The module state of tabGroups.ts. Keep one per store: the migration is
/// attempted once per storage instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabGroupAppearance {
    logo_display_revision: u64,
    /// Guards the reads the migration itself makes.
    migrating: bool,
    /// One attempt per storage instance, so an unfinished pass is not re-run hot.
    attempted: bool,
    events: Vec<AppearanceEvent>,
}

/// Folder names carry no separator; every path key does, drive roots aside.
fn looks_like_folder_name_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    let drive = bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    !key.contains('/') && key != "~" && !drive
}

impl TabGroupAppearance {
    pub fn new() -> Self {
        Self::default()
    }

    /// `tabGroupLogoDisplayRevision`: bumps whenever logos change on disk.
    pub fn tab_group_logo_display_revision(&self) -> u64 {
        self.logo_display_revision
    }

    /// The events raised since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<AppearanceEvent> {
        std::mem::take(&mut self.events)
    }

    /// `readRecord`.
    fn read_record(&mut self, store: &mut impl AppearanceStore, key: &str) -> JsRecord<String> {
        self.migrate_project_appearance_keys(store);
        let Some(raw) = store.get_item(key).filter(|raw| !raw.is_empty()) else {
            return JsRecord::new();
        };
        serde_json::from_str::<StringRecord>(&raw)
            .map(|record| record.0)
            .unwrap_or_default()
    }

    /// `writeRecord`.
    fn write_record<V: Serialize>(
        store: &mut impl AppearanceStore,
        key: &str,
        value: &JsRecord<V>,
    ) -> bool {
        match serde_json::to_string(value) {
            Ok(json) => store.set_item(key, &json),
            Err(_) => false,
        }
    }

    /// `migrateProjectAppearanceKeys`. Appearance used to be filed under the
    /// project's folder name, so checkouts that share a name
    /// (`cortex/agentbase` and `cortex-finance/agentbase`) overwrote each
    /// other's label, color, logo, and mascot. Keys are full paths now; each
    /// old entry moves to every remembered project that carries its name,
    /// which keeps both rails looking the same until one of them is changed.
    ///
    /// A project we do not remember yet (evicted from recents, never pinned)
    /// cannot be matched, so its entry stays under the old name and the pass
    /// is left unfinished. Later launches retry until every name is claimed,
    /// which is what stops a reopened project from losing its label for good.
    pub fn migrate_project_appearance_keys(&mut self, store: &mut impl AppearanceStore) {
        if self.migrating {
            return;
        }
        if store.get_item(KEY_VERSION_KEY).as_deref() == Some(KEY_VERSION) {
            return;
        }
        if self.attempted {
            return;
        }
        self.attempted = true;
        self.migrating = true;
        if self.migrate_now(store) {
            // A failed write leaves the pass unfinished; it retries next launch.
            store.set_item(KEY_VERSION_KEY, KEY_VERSION);
        }
        self.migrating = false;
    }

    /// `migrateNow`: `true` once nothing folder-name-shaped is left to claim.
    fn migrate_now(&mut self, store: &mut impl AppearanceStore) -> bool {
        let mut by_name: JsRecord<Vec<String>> = JsRecord::new();
        for path in store.known_project_paths() {
            let name = project_name(&path);
            let key = project_key(&path);
            match by_name.entries.iter_mut().find(|(entry, _)| *entry == name) {
                Some((_, keys)) => keys.push(key),
                None => by_name.insert(name, vec![key]),
            }
        }

        let mut done = true;
        for store_key in APPEARANCE_KEYS {
            let record = self.read_record(store, store_key);
            let mut next: JsRecord<String> = JsRecord::new();
            let mut changed = false;
            for (name, value) in record.iter() {
                if let Some(keys) = by_name.get(name) {
                    for key in keys {
                        next.insert(key.clone(), value.clone());
                    }
                    changed = true;
                    continue;
                }
                // A name nothing claims stays put, and keeps the pass unfinished.
                next.insert(name, value.clone());
                done &= !looks_like_folder_name_key(name);
            }
            if changed && !Self::write_record(store, store_key, &next) {
                done = false;
            }
        }
        done
    }

    /// `loadTabGroupColors`: palette indexes per project.
    pub fn load_tab_group_colors(&mut self, store: &mut impl AppearanceStore) -> JsRecord<usize> {
        let raw = self.read_record(store, COLOR_KEY);
        let mut out = JsRecord::new();
        for (project, value) in raw.iter() {
            if let Some(index) = crate::js::parse_int(value)
                && index >= 0.0
                && index < TAB_GROUP_COLORS.len() as f64
            {
                out.insert(project, index as usize);
            }
        }
        out
    }

    /// `loadTabGroupCustomColors`: lowercase `#rrggbb` colors per project.
    pub fn load_tab_group_custom_colors(
        &mut self,
        store: &mut impl AppearanceStore,
    ) -> JsRecord<String> {
        let raw = self.read_record(store, CUSTOM_COLOR_KEY);
        let mut out = JsRecord::new();
        for (project, value) in raw.iter() {
            if is_hex_color(value) {
                out.insert(project, value.to_lowercase());
            }
        }
        out
    }

    /// `writeTabGroupColorIndices`.
    fn write_tab_group_color_indices(store: &mut impl AppearanceStore, next: &JsRecord<usize>) {
        let mut record = JsRecord::new();
        for (key, value) in next.iter() {
            record.insert(key, value.to_string());
        }
        Self::write_record(store, COLOR_KEY, &record);
    }

    /// `clearTabGroupColorIndex`.
    fn clear_tab_group_color_index(&mut self, store: &mut impl AppearanceStore, project: &str) {
        let mut next = self.load_tab_group_colors(store);
        if next.remove(project).is_none() {
            return;
        }
        Self::write_tab_group_color_indices(store, &next);
    }

    /// `clearTabGroupCustomColor`.
    fn clear_tab_group_custom_color(&mut self, store: &mut impl AppearanceStore, project: &str) {
        let mut next = self.load_tab_group_custom_colors(store);
        if next.remove(project).is_none() {
            return;
        }
        Self::write_record(store, CUSTOM_COLOR_KEY, &next);
    }

    /// `saveTabGroupColor`: a palette index, or `None` to clear it.
    pub fn save_tab_group_color(
        &mut self,
        store: &mut impl AppearanceStore,
        project: &str,
        index: Option<usize>,
    ) {
        self.clear_tab_group_custom_color(store, project);
        let mut next = self.load_tab_group_colors(store);
        match index {
            None => {
                next.remove(project);
            }
            Some(index) => next.insert(project, index),
        }
        Self::write_tab_group_color_indices(store, &next);
        self.events.push(AppearanceEvent::ProjectPathsChanged);
    }

    /// `saveTabGroupCustomColor`: a `#rrggbb` color, or `None` to clear it.
    pub fn save_tab_group_custom_color(
        &mut self,
        store: &mut impl AppearanceStore,
        project: &str,
        color: Option<&str>,
    ) {
        self.clear_tab_group_color_index(store, project);
        let mut next = self.load_tab_group_custom_colors(store);
        match color.filter(|color| is_hex_color(color)) {
            None => {
                next.remove(project);
            }
            Some(color) => next.insert(project, color.to_lowercase()),
        }
        Self::write_record(store, CUSTOM_COLOR_KEY, &next);
        self.events.push(AppearanceEvent::ProjectPathsChanged);
    }

    /// `loadTabGroupLabels`.
    pub fn load_tab_group_labels(&mut self, store: &mut impl AppearanceStore) -> JsRecord<String> {
        self.read_record(store, LABEL_KEY)
    }

    /// `saveTabGroupLabel`: a blank label clears the override.
    pub fn save_tab_group_label(
        &mut self,
        store: &mut impl AppearanceStore,
        project: &str,
        label: &str,
    ) {
        let trimmed = monocode_core::js::trim(label);
        let mut next = self.load_tab_group_labels(store);
        if trimmed.is_empty() {
            next.remove(project);
        } else {
            next.insert(project, trimmed.to_string());
        }
        if !Self::write_record(store, LABEL_KEY, &next) {
            return;
        }
        self.events.push(AppearanceEvent::LabelsChanged);
        self.events.push(AppearanceEvent::ProjectPathsChanged);
    }

    /// `loadTabGroupLogos`.
    pub fn load_tab_group_logos(&mut self, store: &mut impl AppearanceStore) -> JsRecord<String> {
        self.read_record(store, LOGO_KEY)
    }

    /// `saveTabGroupLogo`: an image path, or `None` or empty to clear it.
    pub fn save_tab_group_logo(
        &mut self,
        store: &mut impl AppearanceStore,
        project: &str,
        path: Option<&str>,
    ) {
        let mut next = self.load_tab_group_logos(store);
        match path.filter(|path| !path.is_empty()) {
            None => {
                next.remove(project);
            }
            Some(path) => next.insert(project, path.to_string()),
        }
        Self::write_record(store, LOGO_KEY, &next);
    }

    /// `loadTabGroupMascots`.
    pub fn load_tab_group_mascots(&mut self, store: &mut impl AppearanceStore) -> JsRecord<String> {
        self.read_record(store, MASCOT_KEY)
    }

    /// `saveTabGroupMascot`: `None` restores the mascot picked from the
    /// project's name.
    pub fn save_tab_group_mascot(
        &mut self,
        store: &mut impl AppearanceStore,
        project: &str,
        name: Option<&str>,
    ) {
        let mut next = self.load_tab_group_mascots(store);
        match name.filter(|name| !name.is_empty()) {
            None => {
                next.remove(project);
            }
            Some(name) => next.insert(project, name.to_string()),
        }
        Self::write_record(store, MASCOT_KEY, &next);
        self.events.push(AppearanceEvent::ProjectPathsChanged);
    }

    /// `clearTabGroupSettings`: drop every saved appearance override for a project.
    pub fn clear_tab_group_settings(&mut self, store: &mut impl AppearanceStore, project: &str) {
        for key in APPEARANCE_KEYS {
            let mut next = self.read_record(store, key);
            if next.remove(project).is_none() {
                continue;
            }
            if Self::write_record(store, key, &next) && key == LABEL_KEY {
                self.events.push(AppearanceEvent::LabelsChanged);
            }
        }
    }

    /// `rebaseProjectTabGroupSettings`: move a project's appearance overrides
    /// to the key derived from its new path.
    pub fn rebase_project_tab_group_settings(
        &mut self,
        store: &mut impl AppearanceStore,
        from: &str,
        to: &str,
    ) {
        let old_key = project_key(from);
        let new_key = project_key(to);
        if old_key == new_key {
            return;
        }
        let mut labels_changed = false;
        let mut logos_changed = false;
        for key in APPEARANCE_KEYS {
            let mut next = self.read_record(store, key);
            let Some(value) = next.get(&old_key).cloned() else {
                continue;
            };
            if !next.contains_key(&new_key) {
                next.insert(new_key.clone(), value);
            }
            next.remove(&old_key);
            if !Self::write_record(store, key, &next) {
                continue;
            }
            labels_changed |= key == LABEL_KEY;
            logos_changed |= key == LOGO_KEY;
        }
        if labels_changed {
            self.events.push(AppearanceEvent::LabelsChanged);
        }
        if logos_changed {
            self.notify_tab_group_logos_changed();
        }
    }

    /// `notifyTabGroupLogosChanged`.
    pub fn notify_tab_group_logos_changed(&mut self) {
        self.logo_display_revision += 1;
        self.events.push(AppearanceEvent::LogosChanged);
    }
}

/// `resolveTabGroupMascot`.
pub fn resolve_tab_group_mascot(
    project: &str,
    overrides: Option<&JsRecord<String>>,
) -> Option<String> {
    overrides
        .and_then(|overrides| overrides.get(project))
        .cloned()
}

/// `resolveTabGroupLogo`.
pub fn resolve_tab_group_logo(
    project: &str,
    overrides: Option<&JsRecord<String>>,
) -> Option<String> {
    overrides
        .and_then(|overrides| overrides.get(project))
        .cloned()
}

/// `resolveTabGroupColor`: the custom color, else the palette override,
/// else the hash of `fallback_key` (or the project when it is empty).
pub fn resolve_tab_group_color(
    project: &str,
    overrides: Option<&JsRecord<usize>>,
    custom_overrides: Option<&JsRecord<String>>,
    fallback_key: Option<&str>,
) -> String {
    if let Some(custom) = custom_overrides.and_then(|custom| custom.get(project))
        && is_hex_color(custom)
    {
        return custom.clone();
    }
    if let Some(index) = overrides.and_then(|overrides| overrides.get(project))
        && *index < TAB_GROUP_COLORS.len()
    {
        return TAB_GROUP_COLORS[*index].to_string();
    }
    tab_group_color(
        fallback_key
            .filter(|key| !key.is_empty())
            .unwrap_or(project),
    )
    .to_string()
}

/// `resolveTabGroupCustomColor`.
pub fn resolve_tab_group_custom_color(
    project: &str,
    overrides: Option<&JsRecord<String>>,
) -> Option<String> {
    overrides
        .and_then(|overrides| overrides.get(project))
        .filter(|custom| is_hex_color(custom))
        .cloned()
}

/// `resolveTabGroupLabel`. The TypeScript default for `fallback` was "Group".
pub fn resolve_tab_group_label(
    project: &str,
    overrides: Option<&JsRecord<String>>,
    fallback: &str,
) -> String {
    overrides
        .and_then(|overrides| overrides.get(project))
        .map(|label| monocode_core::js::trim(label))
        .filter(|label| !label.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// `resolveTabGroupColorIndex`: the palette override, unless a custom
/// color wins.
pub fn resolve_tab_group_color_index(
    project: &str,
    overrides: Option<&JsRecord<usize>>,
    custom_overrides: Option<&JsRecord<String>>,
) -> Option<usize> {
    if resolve_tab_group_custom_color(project, custom_overrides).is_some() {
        return None;
    }
    overrides
        .and_then(|overrides| overrides.get(project))
        .copied()
        .filter(|index| *index < TAB_GROUP_COLORS.len())
}

/// `GroupedTab`: a tab with an id and an optional group.
pub trait GroupedTab: Clone {
    fn id(&self) -> &str;
    fn group_id(&self) -> Option<&str>;
    fn set_group_id(&mut self, group_id: Option<String>);
}

impl GroupedTab for WorkspaceTab {
    fn id(&self) -> &str {
        &self.id
    }

    fn group_id(&self) -> Option<&str> {
        self.group_id.as_deref()
    }

    fn set_group_id(&mut self, group_id: Option<String>) {
        self.group_id = group_id;
    }
}

/// A title-bar tab that knows its project folder name (`Tab.project`).
pub trait ProjectTab {
    fn project(&self) -> &str;
}

/// `TabProjectLookup`: the project each tab belongs to. Groups never span
/// projects, so every membership change is checked against it. Callers
/// supply the lookup because the project is derived from the tab's session,
/// not stored on the tab.
pub type TabProjectLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The group id when it is truthy (present and not empty).
fn truthy_group<T: GroupedTab>(tab: &T) -> Option<&str> {
    tab.group_id().filter(|group| !group.is_empty())
}

/// `groupScopeKey`: an unknown project is a wildcard, nothing to scope against.
fn group_scope_key(project: Option<String>) -> String {
    project
        .map(|project| monocode_core::js::trim(&project).to_string())
        .unwrap_or_default()
}

/// `sameProject`.
fn same_project(a: &str, b: &str) -> bool {
    a.is_empty() || b.is_empty() || a == b
}

/// `tabGroupProject`: the project a group belongs to, or `None` when it has
/// none in common.
pub fn tab_group_project<T: GroupedTab>(
    tabs: &[T],
    group_id: &str,
    project_of: TabProjectLookup<'_>,
) -> Option<String> {
    let mut project: Option<String> = None;
    for tab in tabs {
        if tab.group_id() != Some(group_id) {
            continue;
        }
        let key = group_scope_key(project_of(tab.id()));
        match &project {
            Some(current) if !current.is_empty() => {
                if !key.is_empty() && *current != key {
                    return None;
                }
            }
            _ => project = Some(key),
        }
    }
    project
}

/// `canJoinTabGroup`: whether a tab may join an existing group.
pub fn can_join_tab_group<T: GroupedTab>(
    tabs: &[T],
    tab_id: &str,
    group_id: &str,
    project_of: Option<TabProjectLookup<'_>>,
) -> bool {
    let Some(project_of) = project_of else {
        return true;
    };
    let Some(project) = tab_group_project(tabs, group_id, project_of) else {
        return true;
    };
    same_project(&project, &group_scope_key(project_of(tab_id)))
}

/// `canJoinTabOnto`: whether dropping one tab onto another may form or
/// extend a group.
pub fn can_join_tab_onto<T: GroupedTab>(
    tabs: &[T],
    dragged_id: &str,
    target_id: &str,
    project_of: Option<TabProjectLookup<'_>>,
) -> bool {
    if dragged_id == target_id {
        return false;
    }
    let Some(lookup) = project_of else {
        return true;
    };
    let (Some(dragged), Some(target)) = (
        tabs.iter().find(|tab| tab.id() == dragged_id),
        tabs.iter().find(|tab| tab.id() == target_id),
    ) else {
        return false;
    };
    if let Some(group) = truthy_group(target) {
        return can_join_tab_group(tabs, dragged_id, group, project_of);
    }
    if let Some(group) = truthy_group(dragged) {
        return can_join_tab_group(tabs, target_id, group, project_of);
    }
    same_project(
        &group_scope_key(lookup(dragged_id)),
        &group_scope_key(lookup(target_id)),
    )
}

/// `newTabGroupId`.
pub fn new_tab_group_id() -> String {
    random_uuid()
}

/// `sharedGroupProject`: the project every tab shares, or `None` for mixed
/// or home projects.
pub fn shared_group_project<T: ProjectTab>(tabs: &[T]) -> Option<String> {
    let first = monocode_core::js::trim(tabs.first()?.project());
    if first.is_empty() || first == "~" {
        return None;
    }
    tabs.iter()
        .all(|tab| monocode_core::js::trim(tab.project()) == first)
        .then(|| first.to_string())
}

/// `withGroup`.
fn with_group<T: GroupedTab>(tab: &T, group_id: Option<&str>) -> T {
    let mut next = tab.clone();
    match group_id.filter(|group| !group.is_empty()) {
        None => {
            if truthy_group(tab).is_some() {
                next.set_group_id(None);
            }
        }
        Some(group) => {
            if tab.group_id() != Some(group) {
                next.set_group_id(Some(group.to_string()));
            }
        }
    }
    next
}

/// `TabGroupSegment`: one tab, or a contiguous run of tabs sharing a group.
#[derive(Debug, PartialEq)]
pub enum TabGroupSegment<'a, T> {
    Single {
        tab: &'a T,
        index: usize,
    },
    Group {
        project: String,
        tabs: &'a [T],
        start_index: usize,
        color: &'static str,
        key: String,
    },
}

/// `segmentTabs`: split the tab strip into singles and group runs.
pub fn segment_tabs<T: GroupedTab + ProjectTab>(tabs: &[T]) -> Vec<TabGroupSegment<'_, T>> {
    let mut segments = Vec::new();
    let mut i = 0;
    while i < tabs.len() {
        let tab = &tabs[i];
        let group_id = tab
            .group_id()
            .map(monocode_core::js::trim)
            .filter(|group| !group.is_empty());
        let Some(group_id) = group_id else {
            segments.push(TabGroupSegment::Single { tab, index: i });
            i += 1;
            continue;
        };

        let mut j = i + 1;
        while j < tabs.len() && tabs[j].group_id() == Some(group_id) {
            j += 1;
        }
        let run = &tabs[i..j];
        segments.push(TabGroupSegment::Group {
            project: shared_group_project(run).unwrap_or_default(),
            tabs: run,
            start_index: i,
            color: tab_group_color(group_id),
            key: group_id.to_string(),
        });
        i = j;
    }
    segments
}

/// `reorderTabSegments`: move a whole segment (group or single tab) to a
/// new position. Returns the new tab id order, or `None` for a bad index.
pub fn reorder_tab_segments<T: GroupedTab + ProjectTab>(
    tabs: &[T],
    from_segment_index: usize,
    to_segment_index: usize,
) -> Option<Vec<String>> {
    let segments = segment_tabs(tabs);
    if from_segment_index >= segments.len() || to_segment_index >= segments.len() {
        return None;
    }
    if from_segment_index == to_segment_index {
        return Some(tabs.iter().map(|tab| tab.id().to_string()).collect());
    }

    let mut next = segments;
    let moved = next.remove(from_segment_index);
    next.insert(to_segment_index, moved);
    Some(
        next.iter()
            .flat_map(|segment| match segment {
                TabGroupSegment::Group { tabs, .. } => {
                    tabs.iter().map(|tab| tab.id().to_string()).collect()
                }
                TabGroupSegment::Single { tab, .. } => vec![tab.id().to_string()],
            })
            .collect(),
    )
}

/// `permutationOf`.
fn permutation_of<T: GroupedTab>(tabs: &[T], ordered_ids: &[String]) -> Option<Vec<T>> {
    if ordered_ids.len() != tabs.len() {
        return None;
    }
    let by_id = |id: &str| tabs.iter().rev().find(|tab| tab.id() == id);
    ordered_ids.iter().map(|id| by_id(id).cloned()).collect()
}

/// `slideOutOfGroup`: push a tab that landed inside a group it cannot join
/// back out of the run.
fn slide_out_of_group<T: GroupedTab>(
    tabs: &[T],
    ordered: &[T],
    moved_index: usize,
    group_id: &str,
) -> Vec<T> {
    let mut start = moved_index;
    while start > 0 && ordered[start - 1].group_id() == Some(group_id) {
        start -= 1;
    }
    let mut end = moved_index;
    while end + 1 < ordered.len() && ordered[end + 1].group_id() == Some(group_id) {
        end += 1;
    }

    let moved = &ordered[moved_index];
    let position = |id: &str| tabs.iter().position(|tab| tab.id() == id);
    let from_left = position(moved.id()) < position(ordered[start].id());

    let mut next = ordered.to_vec();
    let lone = with_group(moved, None);
    next.remove(moved_index);
    let at = if from_left { end } else { start };
    next.insert(at.min(next.len()), lone);
    next
}

/// `applyGroupedReorder`: reorder tabs, joining a drop in the middle of a
/// group and leaving when dragged out.
pub fn apply_grouped_reorder<T: GroupedTab>(
    tabs: &[T],
    ordered_ids: &[String],
    moved_id: &str,
    project_of: Option<TabProjectLookup<'_>>,
) -> Option<Vec<T>> {
    let ordered = permutation_of(tabs, ordered_ids)?;

    let Some(moved_index) = ordered.iter().position(|tab| tab.id() == moved_id) else {
        return Some(ordered);
    };

    let moved = &ordered[moved_index];
    let prev = moved_index
        .checked_sub(1)
        .and_then(|index| ordered.get(index));
    let next = ordered.get(moved_index + 1);
    let prev_group = prev.and_then(GroupedTab::group_id);
    let next_group = next.and_then(GroupedTab::group_id);

    let mut group_id = moved.group_id().map(str::to_string);
    if let Some(prev_group) =
        prev_group.filter(|group| !group.is_empty() && Some(*group) == next_group)
    {
        // A drop inside a run joins it, unless the group belongs to another
        // project; then the tab slides past the group instead of splitting it.
        if !can_join_tab_group(tabs, moved_id, prev_group, project_of) {
            return Some(slide_out_of_group(tabs, &ordered, moved_index, prev_group));
        }
        group_id = Some(prev_group.to_string());
    } else if let Some(own) = truthy_group(moved)
        && (prev_group == Some(own) || next_group == Some(own))
    {
        // Keep membership when sliding along the edge of the same group.
    } else {
        group_id = None;
    }

    Some(
        ordered
            .iter()
            .map(|tab| {
                if tab.id() == moved_id {
                    with_group(tab, group_id.as_deref())
                } else {
                    tab.clone()
                }
            })
            .collect(),
    )
}

/// `addTabToGroup`: move a tab to the end of a group. Returns the tabs
/// unchanged when the tab is missing or may not join.
pub fn add_tab_to_group<T: GroupedTab>(
    tabs: &[T],
    tab_id: &str,
    group_id: &str,
    project_of: Option<TabProjectLookup<'_>>,
) -> Vec<T> {
    let Some(tab) = tabs.iter().find(|entry| entry.id() == tab_id) else {
        return tabs.to_vec();
    };
    if !can_join_tab_group(tabs, tab_id, group_id, project_of) {
        return tabs.to_vec();
    }
    let mut next: Vec<T> = tabs
        .iter()
        .filter(|entry| entry.id() != tab_id)
        .cloned()
        .collect();
    let last_in_group = next
        .iter()
        .rposition(|entry| entry.group_id() == Some(group_id));
    let insert_at = last_in_group.map(|last| last + 1).unwrap_or(next.len());
    next.insert(insert_at, with_group(tab, Some(group_id)));
    next
}

/// `addTabsToNewGroup`: gather tabs into a new group at the first one's place.
pub fn add_tabs_to_new_group<T: GroupedTab>(
    tabs: &[T],
    tab_ids: &[String],
    group_id: &str,
) -> Vec<T> {
    let selected = |tab: &T| tab_ids.iter().any(|id| id == tab.id());
    let members: Vec<T> = tabs
        .iter()
        .filter(|tab| selected(tab))
        .map(|tab| with_group(tab, Some(group_id)))
        .collect();
    if members.is_empty() {
        return tabs.to_vec();
    }
    let first_index = tabs.iter().position(&selected).unwrap_or(0);
    let mut rest: Vec<T> = tabs.iter().filter(|tab| !selected(tab)).cloned().collect();
    let at = first_index.min(rest.len());
    rest.splice(at..at, members);
    rest
}

/// The result of `joinTabOnto`.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinedTabs<T> {
    pub tabs: Vec<T>,
    pub group_id: String,
    pub created: bool,
}

/// `joinTabOnto`: drop one tab onto another to form or extend a group. The
/// dragged tab lands after the target's run. `create_group_id` defaults to
/// `new_tab_group_id` when `None`.
pub fn join_tab_onto<T: GroupedTab>(
    tabs: &[T],
    dragged_id: &str,
    target_id: &str,
    create_group_id: Option<&dyn Fn() -> String>,
    project_of: Option<TabProjectLookup<'_>>,
) -> Option<JoinedTabs<T>> {
    if dragged_id == target_id {
        return None;
    }
    let dragged = tabs.iter().find(|tab| tab.id() == dragged_id)?;
    let target = tabs.iter().find(|tab| tab.id() == target_id)?;
    if !can_join_tab_onto(tabs, dragged_id, target_id, project_of) {
        return None;
    }

    let created = truthy_group(target).is_none() && truthy_group(dragged).is_none();
    let group_id = target
        .group_id()
        .or(dragged.group_id())
        .map(str::to_string)
        .unwrap_or_else(|| match create_group_id {
            Some(create) => create(),
            None => new_tab_group_id(),
        });
    let target_grouped = truthy_group(target).is_some();

    let mut next: Vec<T> = tabs
        .iter()
        .map(|tab| {
            if tab.id() == dragged_id {
                return with_group(tab, Some(&group_id));
            }
            if !target_grouped && tab.id() == target_id {
                return with_group(tab, Some(&group_id));
            }
            tab.clone()
        })
        .collect();

    let mut dest = next.iter().position(|tab| tab.id() == target_id)?;
    while dest + 1 < next.len()
        && next[dest + 1].group_id() == Some(group_id.as_str())
        && next[dest + 1].id() != dragged_id
    {
        dest += 1;
    }

    let from = next.iter().position(|tab| tab.id() == dragged_id)?;
    let item = next.remove(from);
    if from < dest {
        dest -= 1;
    }
    next.insert(dest + 1, item);
    Some(JoinedTabs {
        tabs: next,
        group_id,
        created,
    })
}

/// `ungroupTabs`: clear one group's membership.
pub fn ungroup_tabs<T: GroupedTab>(tabs: &[T], group_id: &str) -> Vec<T> {
    tabs.iter()
        .map(|tab| {
            if tab.group_id() == Some(group_id) {
                with_group(tab, None)
            } else {
                tab.clone()
            }
        })
        .collect()
}

/// `removeTabFromGroup`.
pub fn remove_tab_from_group<T: GroupedTab>(tabs: &[T], tab_id: &str) -> Vec<T> {
    tabs.iter()
        .map(|tab| {
            if tab.id() == tab_id {
                with_group(tab, None)
            } else {
                tab.clone()
            }
        })
        .collect()
}

/// `insertTabBesideActive`: a new tab inherits the active tab's group and
/// sits beside it.
pub fn insert_tab_beside_active<T: GroupedTab>(
    tabs: &[T],
    tab: T,
    active_id: Option<&str>,
    project_of: Option<TabProjectLookup<'_>>,
) -> Vec<T> {
    let active_index = active_id
        .filter(|id| !id.is_empty())
        .and_then(|active_id| tabs.iter().position(|entry| entry.id() == active_id));
    let Some(active_index) = active_index else {
        let mut next = tabs.to_vec();
        next.push(tab);
        return next;
    };
    let active = &tabs[active_index];
    let inherits = truthy_group(active).is_some_and(|group| {
        let mut with_new = tabs.to_vec();
        with_new.push(tab.clone());
        can_join_tab_group(&with_new, tab.id(), group, project_of)
    });
    let incoming = if inherits {
        with_group(&tab, active.group_id())
    } else {
        tab
    };
    let mut next = tabs.to_vec();
    next.insert(active_index + 1, incoming);
    next
}

/// `insertTabInGroup`: add a tab after a group's last member.
pub fn insert_tab_in_group<T: GroupedTab>(tabs: &[T], tab: T, group_id: &str) -> Vec<T> {
    let tab_id = tab.id().to_string();
    let mut all: Vec<T> = tabs
        .iter()
        .filter(|entry| entry.id() != tab_id)
        .cloned()
        .collect();
    all.push(tab);
    add_tab_to_group(&all, &tab_id, group_id, None)
}

/// `loadCollapsedTabGroups`: collapsed group ids, without duplicates.
pub fn load_collapsed_tab_groups(store: &impl AppearanceStore) -> Vec<String> {
    let Some(raw) = store.get_item(COLLAPSED_KEY).filter(|raw| !raw.is_empty()) else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(&raw) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for entry in entries {
        if let serde_json::Value::String(id) = entry
            && !out.contains(&id)
        {
            out.push(id);
        }
    }
    out
}

/// `saveCollapsedTabGroups`.
pub fn save_collapsed_tab_groups(store: &mut impl AppearanceStore, collapsed: &[String]) {
    if let Ok(json) = serde_json::to_string(collapsed) {
        store.set_item(COLLAPSED_KEY, &json);
    }
}

#[cfg(test)]
#[path = "tab_groups_tests.rs"]
mod tests;
