//! Port of src/features/inbox/model/inboxSeen.ts: which inbox revisions the
//! user has read, stored under `monocode.inboxSeen`, and the in-memory list
//! of fetched items that the Inbox view, the background poll, and the rail
//! menu share.
//!
//! localStorage becomes `Kv` with the same key and JSON. The listeners and
//! `useInboxSeenTick` become `InboxSignal::Seen` on the `InboxClient`.

use monocode_settings::Kv;
use serde_json::{Number, Value};

use super::time::date_parse;
use crate::runtime::util::project_path::same_project_path;

/// The localStorage key.
pub const INBOX_SEEN_KEY: &str = "monocode.inboxSeen";
/// The key before the `{ seeded, items }` shape.
pub const INBOX_SEEN_LEGACY_KEY: &str = "monocode.inboxSeenAt";

/// `InboxSeenEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxSeenEntry {
    pub key: String,
    pub updated_at: String,
}

impl InboxSeenEntry {
    pub fn new(key: impl Into<String>, updated_at: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            updated_at: updated_at.into(),
        }
    }
}

/// `SeenStore`. Values are epoch milliseconds, kept in insertion order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeenStore {
    pub seeded: bool,
    pub items: Vec<(String, f64)>,
}

impl SeenStore {
    fn get(&self, key: &str) -> Option<f64> {
        self.items
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| *value)
    }

    fn set(&mut self, key: &str, value: f64) {
        match self.items.iter_mut().find(|(existing, _)| existing == key) {
            Some(entry) => entry.1 = value,
            None => self.items.push((key.to_string(), value)),
        }
    }
}

/// `inboxUpdatedAt`: the parsed timestamp, or 0.
pub fn inbox_updated_at(updated_at: &str) -> i64 {
    date_parse(updated_at).unwrap_or(0)
}

fn seen_map(value: &Value) -> Option<Vec<(String, f64)>> {
    let object = value.as_object()?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| (key.clone(), value))
        })
        .collect()
}

/// `loadInboxSeenStore`.
pub fn load_inbox_seen_store(kv: &Kv) -> SeenStore {
    let Some(raw) = kv.get_item(INBOX_SEEN_KEY).filter(|raw| !raw.is_empty()) else {
        return SeenStore::default();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return SeenStore::default();
    };
    let Some(object) = parsed.as_object() else {
        return SeenStore::default();
    };
    if let (Some(seeded), Some(items)) = (
        object.get("seeded").and_then(Value::as_bool),
        object.get("items").and_then(seen_map),
    ) {
        return SeenStore { seeded, items };
    }
    if let Some(items) = seen_map(&parsed) {
        return SeenStore {
            seeded: true,
            items,
        };
    }
    SeenStore::default()
}

fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        Value::Number(Number::from(value as i64))
    } else {
        Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

/// `saveInboxSeenStore`. The JSON keeps the TypeScript key order. A `Kv`
/// write cannot fail, so this always returns true; the caller notifies
/// listeners.
pub fn save_inbox_seen_store(kv: &Kv, store: &SeenStore) -> bool {
    let items: Vec<String> = store
        .items
        .iter()
        .map(|(key, value)| format!("{}:{}", Value::String(key.clone()), number(*value)))
        .collect();
    let json = format!(
        r#"{{"seeded":{},"items":{{{}}}}}"#,
        store.seeded,
        items.join(",")
    );
    kv.set_item(INBOX_SEEN_KEY, &json);
    kv.remove_item(INBOX_SEEN_LEGACY_KEY);
    true
}

fn merge_seen(items: &mut SeenStore, entry: &InboxSeenEntry) {
    if entry.key.is_empty() {
        return;
    }
    let next = items
        .get(&entry.key)
        .unwrap_or(0.0)
        .max(inbox_updated_at(&entry.updated_at) as f64);
    items.set(&entry.key, next);
}

fn entry_is_unseen(entry: &InboxSeenEntry, store: &SeenStore) -> bool {
    if entry.key.is_empty() {
        return false;
    }
    match store.get(&entry.key) {
        None => true,
        Some(was) => inbox_updated_at(&entry.updated_at) as f64 > was,
    }
}

/// `inboxSeenIsSeeded`.
pub fn inbox_seen_is_seeded(kv: &Kv) -> bool {
    load_inbox_seen_store(kv).seeded
}

/// `isInboxEntryUnseen`.
pub fn is_inbox_entry_unseen(kv: &Kv, entry: &InboxSeenEntry) -> bool {
    let store = load_inbox_seen_store(kv);
    if !store.seeded {
        return false;
    }
    entry_is_unseen(entry, &store)
}

/// `markInboxItemSeen`.
pub fn mark_inbox_item_seen(kv: &Kv, entry: &InboxSeenEntry) -> bool {
    let mut store = load_inbox_seen_store(kv);
    merge_seen(&mut store, entry);
    save_inbox_seen_store(kv, &store)
}

/// `markInboxItemsSeen`.
pub fn mark_inbox_items_seen(kv: &Kv, entries: &[InboxSeenEntry]) -> bool {
    let mut store = load_inbox_seen_store(kv);
    for entry in entries {
        merge_seen(&mut store, entry);
    }
    save_inbox_seen_store(kv, &store)
}

/// `seedInboxSeenIfNeeded`: the first snapshot of the list is remembered so
/// existing items do not badge. Returns whether it saved.
pub fn seed_inbox_seen_if_needed(kv: &Kv, items: &[InboxSeenEntry]) -> bool {
    let mut store = load_inbox_seen_store(kv);
    if store.seeded || items.is_empty() {
        return false;
    }
    store.seeded = true;
    // `{ ...store.items, ...mapFrom(items) }`: the snapshot overwrites.
    for item in items {
        if item.key.is_empty() {
            continue;
        }
        store.set(&item.key, inbox_updated_at(&item.updated_at) as f64);
    }
    save_inbox_seen_store(kv, &store)
}

/// `inboxHasUnseenItems`.
pub fn inbox_has_unseen_items(kv: &Kv, items: &[InboxSeenEntry]) -> bool {
    let store = load_inbox_seen_store(kv);
    if !store.seeded {
        return false;
    }
    items.iter().any(|item| entry_is_unseen(item, &store))
}

/// A fetched item and the local checkouts it was listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownInboxItem {
    pub key: String,
    pub updated_at: String,
    pub project_paths: Vec<String>,
}

/// One `rememberInboxItems` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedInboxItem {
    pub key: String,
    pub updated_at: String,
    pub project_path: String,
}

/// The module-level `knownItems` map, in insertion order.
#[derive(Debug, Default)]
pub struct KnownInboxItems {
    items: Vec<KnownInboxItem>,
}

impl KnownInboxItems {
    /// `rememberInboxItems` without the listener call. Returns whether
    /// anything changed.
    pub fn remember(&mut self, entries: &[RememberedInboxItem]) -> bool {
        let mut changed = false;
        for entry in entries {
            let index = self.items.iter().position(|known| known.key == entry.key);
            let previous = index.map(|index| &self.items[index]);
            let project_paths = previous
                .map(|previous| previous.project_paths.clone())
                .unwrap_or_default();
            let known_path = project_paths
                .iter()
                .any(|path| same_project_path(path, &entry.project_path));
            let newer = previous.is_none_or(|previous| {
                inbox_updated_at(&entry.updated_at) > inbox_updated_at(&previous.updated_at)
            });
            if known_path && !newer {
                continue;
            }
            let updated_at = match previous {
                Some(previous) if !newer => previous.updated_at.clone(),
                _ => entry.updated_at.clone(),
            };
            let mut paths = project_paths;
            if !known_path {
                paths.push(entry.project_path.clone());
            }
            let next = KnownInboxItem {
                key: entry.key.clone(),
                updated_at,
                project_paths: paths,
            };
            match index {
                Some(index) => self.items[index] = next,
                None => self.items.push(next),
            }
            changed = true;
        }
        changed
    }

    /// `knownInboxEntries`: entries listed under any of these checkouts, or
    /// under no checkout.
    pub fn entries(&self, project_paths: &[String]) -> Vec<InboxSeenEntry> {
        self.items
            .iter()
            .filter(|entry| {
                entry.project_paths.iter().any(|known| {
                    known.is_empty()
                        || project_paths
                            .iter()
                            .any(|path| same_project_path(path, known))
                })
            })
            .map(|entry| InboxSeenEntry::new(entry.key.clone(), entry.updated_at.clone()))
            .collect()
    }

    /// `clearKnownInboxItems` without the listener call.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, updated_at: &str) -> InboxSeenEntry {
        InboxSeenEntry::new(key, updated_at)
    }

    #[test]
    fn seeds_current_items_so_a_long_standing_inbox_does_not_badge() {
        let kv = Kv::in_memory();
        let items = [
            entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
            entry("github:acme/web:issue:4", "2026-08-27T11:00:00Z"),
        ];
        seed_inbox_seen_if_needed(&kv, &items);
        assert!(!inbox_has_unseen_items(&kv, &items));
    }

    #[test]
    fn flags_a_newly_appeared_item() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        assert!(inbox_has_unseen_items(
            &kv,
            &[
                entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
                entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
            ]
        ));
    }

    #[test]
    fn flags_an_existing_item_that_got_a_newer_update() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        assert!(inbox_has_unseen_items(
            &kv,
            &[entry("linear:ENG-1", "2026-08-27T11:00:00Z")]
        ));
    }

    #[test]
    fn does_not_flag_the_same_items_again() {
        let kv = Kv::in_memory();
        let items = [entry("linear:ENG-1", "2026-08-27T10:00:00Z")];
        seed_inbox_seen_if_needed(&kv, &items);
        assert!(!inbox_has_unseen_items(&kv, &items));
    }

    #[test]
    fn clears_one_item_when_that_card_is_opened_leaving_other_unseen() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        let next = [
            entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
            entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
        ];
        assert!(inbox_has_unseen_items(&kv, &next));
        mark_inbox_item_seen(&kv, &next[1]);
        assert!(!is_inbox_entry_unseen(&kv, &next[1]));
        assert!(!is_inbox_entry_unseen(&kv, &next[0]));
        assert!(!inbox_has_unseen_items(&kv, &next));
    }

    #[test]
    fn keeps_the_badge_until_every_new_card_has_been_opened() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        let next = [
            entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
            entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
            entry("linear:ENG-3", "2026-08-27T10:02:00Z"),
        ];
        assert!(inbox_has_unseen_items(&kv, &next));
        mark_inbox_item_seen(&kv, &next[1]);
        assert!(inbox_has_unseen_items(&kv, &next));
        mark_inbox_item_seen(&kv, &next[2]);
        assert!(!inbox_has_unseen_items(&kv, &next));
    }

    #[test]
    fn marks_all_supplied_items_as_seen() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        let next = [
            entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
            entry("linear:ENG-3", "2026-08-27T10:02:00Z"),
        ];
        assert!(inbox_has_unseen_items(&kv, &next));
        mark_inbox_items_seen(&kv, &next);
        assert!(!inbox_has_unseen_items(&kv, &next));
    }

    #[test]
    fn marks_an_unchanged_item_read_from_either_local_checkout_after_an_older_snapshot_arrives() {
        let kv = Kv::in_memory();
        let mut known = KnownInboxItems::default();
        let older = entry("github:acme/web:issue:4", "2026-08-27T10:00:00Z");
        let current = entry("github:acme/web:issue:4", "2026-08-27T11:00:00Z");
        let remembered = |entry: &InboxSeenEntry, path: &str| RememberedInboxItem {
            key: entry.key.clone(),
            updated_at: entry.updated_at.clone(),
            project_path: path.into(),
        };
        seed_inbox_seen_if_needed(&kv, std::slice::from_ref(&older));
        known.remember(&[remembered(&current, "/repos/old")]);
        known.remember(&[remembered(&current, "/repos/new")]);
        known.remember(&[remembered(&older, "/repos/old")]);
        for path in ["/repos/old", "/repos/new"] {
            assert_eq!(known.entries(&[path.into()]), vec![current.clone()]);
        }
        assert_eq!(known.entries(&["/repos/unrelated".into()]), vec![]);
        mark_inbox_items_seen(&kv, &known.entries(&["/repos/new".into()]));
        assert!(!is_inbox_entry_unseen(&kv, &current));
        assert!(is_inbox_entry_unseen(
            &kv,
            &entry(&current.key, "2026-08-27T12:00:00Z")
        ));
    }

    #[test]
    fn treats_a_new_item_with_an_unreadable_timestamp_as_unseen() {
        let kv = Kv::in_memory();
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        assert!(inbox_has_unseen_items(
            &kv,
            &[entry("linear:ENG-2", "not-a-date")]
        ));
    }

    #[test]
    fn is_seeded_after_the_first_snapshot() {
        let kv = Kv::in_memory();
        assert!(!inbox_seen_is_seeded(&kv));
        seed_inbox_seen_if_needed(&kv, &[entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
        assert!(inbox_seen_is_seeded(&kv));
    }

    #[test]
    fn does_not_treat_existing_items_as_new_if_a_card_is_opened_before_seed() {
        let kv = Kv::in_memory();
        mark_inbox_item_seen(&kv, &entry("linear:ENG-2", "2026-08-27T10:01:00Z"));
        assert!(!inbox_seen_is_seeded(&kv));
        let items = [
            entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
            entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
        ];
        seed_inbox_seen_if_needed(&kv, &items);
        assert!(!inbox_has_unseen_items(&kv, &items));
    }

    #[test]
    fn reads_the_legacy_flat_map_as_seeded_and_writes_integers() {
        let kv = Kv::in_memory();
        kv.set_item(INBOX_SEEN_KEY, r#"{"a":5}"#);
        kv.set_item(INBOX_SEEN_LEGACY_KEY, "1");
        let store = load_inbox_seen_store(&kv);
        assert!(store.seeded);
        assert_eq!(store.items, vec![("a".to_string(), 5.0)]);
        mark_inbox_item_seen(&kv, &entry("b", "2026-08-27T12:00:00Z"));
        assert_eq!(
            kv.get_item(INBOX_SEEN_KEY).unwrap(),
            r#"{"seeded":true,"items":{"a":5,"b":1787832000000}}"#
        );
        assert_eq!(kv.get_item(INBOX_SEEN_LEGACY_KEY), None);
    }
}
