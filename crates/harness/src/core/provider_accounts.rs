//! Port of src/features/providers/model/providerAccounts.ts: locally named
//! provider account profiles and the per-project selection.
//!
//! The TypeScript read and wrote `localStorage`; every function here takes
//! the [`LocalStore`] instead. It also fired a window event after each
//! change (`subscribeProviderAccounts`). The functions that change storage
//! return whether they did, and the caller tells its listeners.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use monocode_core::harness::HarnessId;
use monocode_core::js;
use monocode_core::paths::path_key;

use super::local_store::LocalStore;

const ACCOUNTS_KEY: &str = "monocode.providerAccounts.v1";
const SELECTIONS_KEY: &str = "monocode.providerAccountSelections.v1";

/// `DEFAULT_PROVIDER_ACCOUNT_ID`.
pub const DEFAULT_PROVIDER_ACCOUNT_ID: &str = "default";
const DEFAULT_PROVIDER_ACCOUNT_LABEL: &str = "Default account";

/// `PROVIDER_ACCOUNT_PROVIDERS`: providers whose CLIs support isolated,
/// locally named account profiles.
pub const PROVIDER_ACCOUNT_PROVIDERS: [HarnessId; 2] = [HarnessId::Claude, HarnessId::Codex];

/// `sameProviderAccountId`. Legacy sessions predate persisted account ids and
/// belong to the default profile.
pub fn same_provider_account_id(left: Option<&str>, right: Option<&str>) -> bool {
    left.unwrap_or(DEFAULT_PROVIDER_ACCOUNT_ID) == right.unwrap_or(DEFAULT_PROVIDER_ACCOUNT_ID)
}

/// `supportsProviderAccounts`.
pub fn supports_provider_accounts(provider: HarnessId) -> bool {
    PROVIDER_ACCOUNT_PROVIDERS.contains(&provider)
}

/// `ProviderAccount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    pub id: String,
    pub provider: HarnessId,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

impl ProviderAccount {
    pub fn new(id: &str, provider: HarnessId, label: &str) -> Self {
        Self {
            id: id.to_string(),
            provider,
            label: label.to_string(),
            is_default: None,
        }
    }
}

fn read_json(store: &dyn LocalStore, key: &str) -> Value {
    store
        .get_item(key)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// `readRecord`: the stored object, or an empty one.
fn read_record(store: &dyn LocalStore, key: &str) -> Map<String, Value> {
    match read_json(store, key) {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// `writeJson`. A private or full store must not block the provider.
fn write_json(store: &dyn LocalStore, key: &str, value: &Map<String, Value>) {
    if let Ok(raw) = serde_json::to_string(value) {
        let _ = store.set_item(key, &raw);
    }
}

/// `cleanLabel`.
fn clean_label(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => {
            let collapsed = text
                .split(js::is_space)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            js::slice_prefix(js::trim(&collapsed), 48).to_string()
        }
        _ => String::new(),
    }
}

fn clean_label_str(value: &str) -> String {
    clean_label(Some(&Value::String(value.to_string())))
}

/// `validAccountId`.
fn valid_account_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// `providerAccounts`: the default account first, then the named profiles.
pub fn provider_accounts(store: &dyn LocalStore, provider: HarnessId) -> Vec<ProviderAccount> {
    let stored = read_record(store, ACCOUNTS_KEY);
    let mut seen = vec![DEFAULT_PROVIDER_ACCOUNT_ID.to_string()];
    let empty = Vec::new();
    let accounts = match stored.get(provider.as_str()) {
        Some(Value::Array(items)) => items,
        _ => &empty,
    };
    let mut default_label = DEFAULT_PROVIDER_ACCOUNT_LABEL.to_string();
    let mut found_default = false;
    let mut profiles = Vec::new();
    for account in accounts {
        let id = match account.get("id") {
            Some(Value::String(id)) if valid_account_id(id) => id.clone(),
            _ => String::new(),
        };
        let label = clean_label(account.get("label"));
        if id == DEFAULT_PROVIDER_ACCOUNT_ID {
            if !label.is_empty() && !found_default {
                default_label = label;
                found_default = true;
            }
            continue;
        }
        if id.is_empty() || label.is_empty() || seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        profiles.push(ProviderAccount::new(&id, provider, &label));
    }
    let mut all = vec![ProviderAccount {
        id: DEFAULT_PROVIDER_ACCOUNT_ID.into(),
        provider,
        label: default_label,
        is_default: Some(true),
    }];
    all.extend(profiles);
    all
}

/// `newProviderAccount` with the random id chosen by the caller.
pub fn new_provider_account_with_uuid(
    store: &dyn LocalStore,
    provider: HarnessId,
    label: &str,
    uuid: &str,
) -> ProviderAccount {
    let mut cleaned = clean_label_str(label);
    if cleaned.is_empty() {
        cleaned = format!("Account {}", provider_accounts(store, provider).len() + 1);
    }
    ProviderAccount::new(&format!("account-{uuid}"), provider, &cleaned)
}

/// `newProviderAccount`.
pub fn new_provider_account(
    store: &dyn LocalStore,
    provider: HarnessId,
    label: &str,
) -> ProviderAccount {
    new_provider_account_with_uuid(store, provider, label, &uuid::Uuid::new_v4().to_string())
}

/// `serializeProviderAccounts`.
fn serialize_provider_accounts(accounts: &[ProviderAccount]) -> Value {
    let entries = accounts
        .iter()
        .filter_map(|account| {
            let label = clean_label_str(&account.label);
            if label.is_empty() {
                return None;
            }
            if account.id == DEFAULT_PROVIDER_ACCOUNT_ID && label == DEFAULT_PROVIDER_ACCOUNT_LABEL
            {
                return None;
            }
            serde_json::to_value(ProviderAccount::new(&account.id, account.provider, &label)).ok()
        })
        .collect();
    Value::Array(entries)
}

/// `saveProviderAccount`. Returns true when it wrote.
pub fn save_provider_account(store: &dyn LocalStore, account: &ProviderAccount) -> bool {
    if account.id == DEFAULT_PROVIDER_ACCOUNT_ID || !valid_account_id(&account.id) {
        return false;
    }
    let label = clean_label_str(&account.label);
    if label.is_empty() {
        return false;
    }
    let mut stored = read_record(store, ACCOUNTS_KEY);
    let mut next: Vec<ProviderAccount> = provider_accounts(store, account.provider)
        .into_iter()
        .filter(|entry| entry.id != account.id)
        .collect();
    next.push(ProviderAccount::new(&account.id, account.provider, &label));
    stored.insert(
        account.provider.as_str().into(),
        serialize_provider_accounts(&next),
    );
    write_json(store, ACCOUNTS_KEY, &stored);
    true
}

/// `renameProviderAccount`.
pub fn rename_provider_account(
    store: &dyn LocalStore,
    provider: HarnessId,
    account_id: &str,
    label: &str,
) -> Option<ProviderAccount> {
    if !valid_account_id(account_id) {
        return None;
    }
    let next_label = clean_label_str(label);
    if next_label.is_empty() {
        return None;
    }
    let accounts = provider_accounts(store, provider);
    let target = accounts.iter().find(|account| account.id == account_id)?;
    let renamed = ProviderAccount {
        label: next_label,
        ..target.clone()
    };
    let mut stored = read_record(store, ACCOUNTS_KEY);
    let updated: Vec<ProviderAccount> = accounts
        .iter()
        .map(|account| {
            if account.id == account_id {
                renamed.clone()
            } else {
                account.clone()
            }
        })
        .collect();
    stored.insert(
        provider.as_str().into(),
        serialize_provider_accounts(&updated),
    );
    write_json(store, ACCOUNTS_KEY, &stored);
    Some(renamed)
}

/// `removeProviderAccount`: remove account metadata after native credential
/// cleanup has succeeded.
pub fn remove_provider_account(
    store: &dyn LocalStore,
    provider: HarnessId,
    account_id: &str,
) -> bool {
    if account_id == DEFAULT_PROVIDER_ACCOUNT_ID || !valid_account_id(account_id) {
        return false;
    }
    let accounts = provider_accounts(store, provider);
    if !accounts.iter().any(|account| account.id == account_id) {
        return false;
    }
    let mut stored = read_record(store, ACCOUNTS_KEY);
    let kept: Vec<ProviderAccount> = accounts
        .into_iter()
        .filter(|account| account.id != account_id)
        .collect();
    stored.insert(provider.as_str().into(), serialize_provider_accounts(&kept));
    write_json(store, ACCOUNTS_KEY, &stored);

    let mut selections = read_record(store, SELECTIONS_KEY);
    let keys: Vec<String> = selections.keys().cloned().collect();
    for key in keys {
        let Some(Value::Object(selection)) = selections.get(&key) else {
            continue;
        };
        if selection.get(provider.as_str()).and_then(Value::as_str) != Some(account_id) {
            continue;
        }
        let mut next_selection = selection.clone();
        next_selection.remove(provider.as_str());
        if next_selection.is_empty() {
            selections.remove(&key);
        } else {
            selections.insert(key, Value::Object(next_selection));
        }
    }
    write_json(store, SELECTIONS_KEY, &selections);
    true
}

/// `providerAccountExists`.
pub fn provider_account_exists(
    store: &dyn LocalStore,
    provider: HarnessId,
    account_id: Option<&str>,
) -> bool {
    provider_accounts(store, provider)
        .iter()
        .any(|account| Some(account.id.as_str()) == account_id)
}

/// `selectionKey`.
fn selection_key(project: Option<&str>) -> String {
    let project = project.map(js::trim).filter(|project| !project.is_empty());
    path_key(project.unwrap_or("~"))
}

/// `selectedProviderAccountId`.
pub fn selected_provider_account_id(
    store: &dyn LocalStore,
    provider: HarnessId,
    project: Option<&str>,
) -> String {
    let selections = read_record(store, SELECTIONS_KEY);
    let id = selections
        .get(&selection_key(project))
        .and_then(|selection| selection.get(provider.as_str()))
        .and_then(Value::as_str);
    match id {
        Some(id)
            if provider_accounts(store, provider)
                .iter()
                .any(|account| account.id == id) =>
        {
            id.to_string()
        }
        _ => DEFAULT_PROVIDER_ACCOUNT_ID.to_string(),
    }
}

/// `selectProviderAccount`. Returns true when it wrote.
pub fn select_provider_account(
    store: &dyn LocalStore,
    provider: HarnessId,
    project: Option<&str>,
    account_id: &str,
) -> bool {
    if !provider_accounts(store, provider)
        .iter()
        .any(|account| account.id == account_id)
    {
        return false;
    }
    let mut selections = read_record(store, SELECTIONS_KEY);
    let key = selection_key(project);
    let mut selection = match selections.get(&key) {
        Some(Value::Object(selection)) => selection.clone(),
        _ => Map::new(),
    };
    selection.insert(provider.as_str().into(), Value::String(account_id.into()));
    selections.insert(key, Value::Object(selection));
    write_json(store, SELECTIONS_KEY, &selections);
    true
}

/// `providerAccountLabel`.
pub fn provider_account_label(
    store: &dyn LocalStore,
    provider: HarnessId,
    account_id: Option<&str>,
) -> String {
    provider_accounts(store, provider)
        .into_iter()
        .find(|account| Some(account.id.as_str()) == account_id)
        .map(|account| account.label)
        .unwrap_or_else(|| DEFAULT_PROVIDER_ACCOUNT_LABEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::local_store::MemoryStore;

    fn ids(store: &MemoryStore, provider: HarnessId) -> Vec<String> {
        provider_accounts(store, provider)
            .into_iter()
            .map(|a| a.id)
            .collect()
    }

    fn labels(store: &MemoryStore, provider: HarnessId) -> Vec<String> {
        provider_accounts(store, provider)
            .into_iter()
            .map(|a| a.label)
            .collect()
    }

    #[test]
    fn always_exposes_the_provider_owned_default_account() {
        let store = MemoryStore::new();
        assert_eq!(
            provider_accounts(&store, HarnessId::Claude),
            vec![ProviderAccount {
                id: DEFAULT_PROVIDER_ACCOUNT_ID.into(),
                provider: HarnessId::Claude,
                label: "Default account".into(),
                is_default: Some(true),
            }]
        );
    }

    #[test]
    fn stores_named_profiles_separately_per_provider() {
        let store = MemoryStore::new();
        let work = new_provider_account_with_uuid(
            &store,
            HarnessId::Claude,
            "  Work   account  ",
            "00000000-0000-4000-8000-000000000001",
        );
        assert_eq!(work.id, "account-00000000-0000-4000-8000-000000000001");
        save_provider_account(&store, &work);
        assert_eq!(
            labels(&store, HarnessId::Claude),
            vec!["Default account", "Work account"]
        );
        assert_eq!(provider_accounts(&store, HarnessId::Codex).len(), 1);
        assert_eq!(
            provider_account_label(&store, HarnessId::Claude, Some(&work.id)),
            "Work account"
        );
    }

    #[test]
    fn falls_back_to_the_default_account_for_malformed_stored_profiles() {
        let store = MemoryStore::new();
        store
            .set_item(ACCOUNTS_KEY, r#"{"claude":{"id":"not-an-array"}}"#)
            .unwrap();
        assert_eq!(
            ids(&store, HarnessId::Claude),
            vec![DEFAULT_PROVIDER_ACCOUNT_ID]
        );
    }

    #[test]
    fn falls_back_when_the_stored_root_is_not_a_record() {
        let store = MemoryStore::new();
        for malformed in ["null", "[]", "42"] {
            store.set_item(ACCOUNTS_KEY, malformed).unwrap();
            assert_eq!(
                ids(&store, HarnessId::Claude),
                vec![DEFAULT_PROVIDER_ACCOUNT_ID]
            );
        }
        store.set_item(SELECTIONS_KEY, "null").unwrap();
        assert_eq!(
            selected_provider_account_id(&store, HarnessId::Claude, Some("/repo")),
            DEFAULT_PROVIDER_ACCOUNT_ID
        );
    }

    #[test]
    fn replaces_malformed_provider_storage_when_saving_an_account() {
        let store = MemoryStore::new();
        store
            .set_item(ACCOUNTS_KEY, r#"{"codex":"not-an-array"}"#)
            .unwrap();
        save_provider_account(
            &store,
            &ProviderAccount::new("account-work", HarnessId::Codex, "Work"),
        );
        assert_eq!(
            labels(&store, HarnessId::Codex),
            vec!["Default account", "Work"]
        );
    }

    #[test]
    fn discards_malformed_account_entries_when_saving_an_account() {
        let store = MemoryStore::new();
        store
            .set_item(
                ACCOUNTS_KEY,
                r#"{"codex":[null,42,{"id":"account-missing-label","provider":"codex"},{"id":"account-keep","provider":"codex","label":"Keep"}]}"#,
            )
            .unwrap();
        save_provider_account(
            &store,
            &ProviderAccount::new("account-work", HarnessId::Codex, "Work"),
        );
        assert_eq!(
            labels(&store, HarnessId::Codex),
            vec!["Default account", "Keep", "Work"]
        );
    }

    #[test]
    fn remembers_a_selection_per_project_and_ignores_unknown_ids() {
        let store = MemoryStore::new();
        let work = ProviderAccount::new("account-work", HarnessId::Codex, "Work");
        save_provider_account(&store, &work);
        select_provider_account(&store, HarnessId::Codex, Some("/repo/one"), &work.id);
        assert_eq!(
            selected_provider_account_id(&store, HarnessId::Codex, Some("/repo/one")),
            work.id
        );
        assert_eq!(
            selected_provider_account_id(&store, HarnessId::Codex, Some("/repo/two")),
            DEFAULT_PROVIDER_ACCOUNT_ID
        );
        assert!(!select_provider_account(
            &store,
            HarnessId::Codex,
            Some("/repo/one"),
            "missing"
        ));
        assert_eq!(
            selected_provider_account_id(&store, HarnessId::Codex, Some("/repo/one")),
            work.id
        );
    }

    #[test]
    fn renames_a_named_account_without_changing_its_identity_or_order() {
        let store = MemoryStore::new();
        save_provider_account(
            &store,
            &ProviderAccount::new("account-work", HarnessId::Codex, "Wrk"),
        );
        save_provider_account(
            &store,
            &ProviderAccount::new("account-personal", HarnessId::Codex, "Personal"),
        );
        assert_eq!(
            rename_provider_account(
                &store,
                HarnessId::Codex,
                "account-work",
                "  Work   account  "
            ),
            Some(ProviderAccount::new(
                "account-work",
                HarnessId::Codex,
                "Work account"
            ))
        );
        assert_eq!(
            ids(&store, HarnessId::Codex),
            vec![
                DEFAULT_PROVIDER_ACCOUNT_ID,
                "account-work",
                "account-personal"
            ]
        );
    }

    #[test]
    fn removes_named_account_metadata_and_every_project_selection() {
        let store = MemoryStore::new();
        let work = ProviderAccount::new("account-work", HarnessId::Claude, "Work");
        save_provider_account(&store, &work);
        select_provider_account(&store, HarnessId::Claude, Some("/repo/one"), &work.id);
        select_provider_account(&store, HarnessId::Claude, Some("/repo/two"), &work.id);
        assert!(remove_provider_account(&store, HarnessId::Claude, &work.id));
        assert!(!provider_account_exists(
            &store,
            HarnessId::Claude,
            Some(&work.id)
        ));
        for project in ["/repo/one", "/repo/two"] {
            assert_eq!(
                selected_provider_account_id(&store, HarnessId::Claude, Some(project)),
                DEFAULT_PROVIDER_ACCOUNT_ID
            );
        }
        assert_eq!(store.get_item(SELECTIONS_KEY).as_deref(), Some("{}"));
    }

    #[test]
    fn renames_but_does_not_remove_the_provider_owned_default_account() {
        let store = MemoryStore::new();
        assert_eq!(
            rename_provider_account(&store, HarnessId::Codex, "default", "  Personal  "),
            Some(ProviderAccount {
                id: DEFAULT_PROVIDER_ACCOUNT_ID.into(),
                provider: HarnessId::Codex,
                label: "Personal".into(),
                is_default: Some(true),
            })
        );
        assert!(!remove_provider_account(
            &store,
            HarnessId::Codex,
            "default"
        ));
        assert_eq!(
            provider_accounts(&store, HarnessId::Codex)[0].label,
            "Personal"
        );
    }

    #[test]
    fn preserves_a_custom_default_name_when_named_profiles_change() {
        let store = MemoryStore::new();
        rename_provider_account(&store, HarnessId::Claude, "default", "Primary");
        save_provider_account(
            &store,
            &ProviderAccount::new("account-work", HarnessId::Claude, "Work"),
        );
        remove_provider_account(&store, HarnessId::Claude, "account-work");
        assert_eq!(
            provider_accounts(&store, HarnessId::Claude)[0].label,
            "Primary"
        );
    }

    #[test]
    fn treats_a_missing_account_id_as_the_default() {
        assert!(same_provider_account_id(None, Some("default")));
        assert!(!same_provider_account_id(Some("account-x"), None));
        assert!(supports_provider_accounts(HarnessId::Codex));
        assert!(!supports_provider_accounts(HarnessId::Cursor));
    }
}
