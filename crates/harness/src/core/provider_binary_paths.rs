//! Port of src/features/providers/model/providerBinaryPaths.ts: the
//! user-configured CLI paths. A saved path takes effect at the next launch;
//! until then the process keeps the paths it started with.
//!
//! The stored map lives in [`LocalStore`] under the same key. The runtime map
//! the TypeScript kept in a module global is [`ProviderBinaryPaths`].

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use monocode_core::harness::HarnessId;
use monocode_process::harness::{HarnessHost, harness_runtime_binary_paths};

use super::local_store::LocalStore;

/// `ConfigurableBinaryProvider`: every harness.
pub type ConfigurableBinaryProvider = HarnessId;

/// `STORAGE_KEY`.
pub const STORAGE_KEY: &str = "monocode.providerBinaryPaths.v1";

/// `readProviderBinaryPaths`: the stored string entries, keyed by provider id.
pub fn read_provider_binary_paths(store: &dyn LocalStore) -> BTreeMap<String, String> {
    let raw = store.get_item(STORAGE_KEY).unwrap_or_else(|| "{}".into());
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(provider, path)| match path {
                Value::String(path) => Some((provider, path)),
                _ => None,
            })
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn trimmed(path: Option<&String>) -> Option<String> {
    path.map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// The paths this process launched with (`runtimeBinaryPaths`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderBinaryPaths {
    runtime: BTreeMap<String, String>,
}

impl ProviderBinaryPaths {
    /// The module-load value: whatever is stored now.
    pub fn from_store(store: &dyn LocalStore) -> Self {
        Self {
            runtime: read_provider_binary_paths(store),
        }
    }

    /// `initializeProviderBinaryPaths`: hand the stored paths to the host,
    /// which keeps the first set it sees for the life of the process, and
    /// adopt whatever set is active.
    pub fn initialize(&mut self, host: &HarnessHost, store: &dyn LocalStore) {
        let stored: HashMap<String, String> =
            read_provider_binary_paths(store).into_iter().collect();
        let active = harness_runtime_binary_paths(host, stored);
        self.runtime = active.into_iter().collect();
    }

    /// `initializeProviderBinaryPaths` with the host's answer given. Test seam.
    pub fn initialize_with(&mut self, active: impl IntoIterator<Item = (String, String)>) {
        self.runtime = active.into_iter().collect();
    }

    /// `runtimeProviderBinaryPath`.
    pub fn runtime_provider_binary_path(&self, provider: HarnessId) -> Option<String> {
        trimmed(self.runtime.get(provider.as_str()))
    }

    /// `providerBinaryPathChangePending`: a saved path waits for a restart.
    pub fn provider_binary_path_change_pending(
        &self,
        store: &dyn LocalStore,
        provider: HarnessId,
    ) -> bool {
        self.runtime_provider_binary_path(provider) != load_provider_binary_path(store, provider)
    }
}

/// `loadProviderBinaryPath`: the stored path, trimmed.
pub fn load_provider_binary_path(store: &dyn LocalStore, provider: HarnessId) -> Option<String> {
    trimmed(read_provider_binary_paths(store).get(provider.as_str()))
}

/// `saveProviderBinaryPath`. False when the store could not write.
pub fn save_provider_binary_path(
    store: &dyn LocalStore,
    provider: HarnessId,
    path: Option<&str>,
) -> bool {
    let mut stored = read_provider_binary_paths(store);
    match path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(value) => {
            stored.insert(provider.as_str().into(), value.to_string());
        }
        None => {
            stored.remove(provider.as_str());
        }
    }
    let Ok(raw) = serde_json::to_string(&stored) else {
        return false;
    };
    store.set_item(STORAGE_KEY, &raw).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::local_store::MemoryStore;

    #[test]
    fn repairs_malformed_storage_when_saving_a_path() {
        let store = MemoryStore::new();
        store.set_item(STORAGE_KEY, "not json").unwrap();
        assert!(save_provider_binary_path(
            &store,
            HarnessId::Codex,
            Some("/opt/codex")
        ));
        assert_eq!(
            load_provider_binary_path(&store, HarnessId::Codex).as_deref(),
            Some("/opt/codex")
        );
    }

    #[test]
    fn filters_non_string_paths_before_runtime_initialization() {
        let store = MemoryStore::new();
        store
            .set_item(STORAGE_KEY, r#"{"cursor":null,"codex":"/opt/codex"}"#)
            .unwrap();
        let stored = read_provider_binary_paths(&store);
        assert_eq!(
            stored,
            BTreeMap::from([("codex".to_string(), "/opt/codex".to_string())])
        );
        let host = HarnessHost::default();
        let mut paths = ProviderBinaryPaths::from_store(&store);
        paths.initialize(&host, &store);
        assert_eq!(paths.runtime_provider_binary_path(HarnessId::Cursor), None);
        assert_eq!(
            paths
                .runtime_provider_binary_path(HarnessId::Codex)
                .as_deref(),
            Some("/opt/codex")
        );
        assert_eq!(
            host.runtime_binary_path("codex").as_deref(),
            Some("/opt/codex")
        );
    }

    #[test]
    fn keeps_the_active_path_unchanged_across_windows_until_restart() {
        let store = MemoryStore::new();
        store
            .set_item(STORAGE_KEY, r#"{"cursor":"/opt/cursor/old"}"#)
            .unwrap();
        let host = HarnessHost::default();
        let mut first_window = ProviderBinaryPaths::from_store(&store);
        first_window.initialize(&host, &store);
        assert_eq!(
            first_window
                .runtime_provider_binary_path(HarnessId::Cursor)
                .as_deref(),
            Some("/opt/cursor/old")
        );

        save_provider_binary_path(&store, HarnessId::Cursor, Some("/opt/cursor/new"));
        assert_eq!(
            first_window
                .runtime_provider_binary_path(HarnessId::Cursor)
                .as_deref(),
            Some("/opt/cursor/old")
        );
        assert!(first_window.provider_binary_path_change_pending(&store, HarnessId::Cursor));

        // A second window in the same process gets the host's first set.
        let mut second_window = ProviderBinaryPaths::from_store(&store);
        second_window.initialize(&host, &store);
        assert_eq!(
            second_window
                .runtime_provider_binary_path(HarnessId::Cursor)
                .as_deref(),
            Some("/opt/cursor/old")
        );
        assert!(second_window.provider_binary_path_change_pending(&store, HarnessId::Cursor));

        // A restart is a new host.
        let restarted_host = HarnessHost::default();
        let mut restarted = ProviderBinaryPaths::default();
        restarted.initialize(&restarted_host, &store);
        assert_eq!(
            restarted
                .runtime_provider_binary_path(HarnessId::Cursor)
                .as_deref(),
            Some("/opt/cursor/new")
        );
        assert!(!restarted.provider_binary_path_change_pending(&store, HarnessId::Cursor));
    }

    #[test]
    fn reports_storage_failures_without_claiming_the_path_was_saved() {
        let store = MemoryStore::failing("storage full");
        assert!(!save_provider_binary_path(
            &store,
            HarnessId::Opencode,
            Some("/opt/opencode")
        ));
    }

    #[test]
    fn clearing_a_path_removes_it() {
        let store = MemoryStore::new();
        save_provider_binary_path(&store, HarnessId::Pi, Some(" /opt/pi "));
        assert_eq!(
            load_provider_binary_path(&store, HarnessId::Pi).as_deref(),
            Some("/opt/pi")
        );
        save_provider_binary_path(&store, HarnessId::Pi, Some("  "));
        assert_eq!(load_provider_binary_path(&store, HarnessId::Pi), None);
        assert_eq!(store.get_item(STORAGE_KEY).as_deref(), Some("{}"));
    }
}
