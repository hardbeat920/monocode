//! Port of src/app/model/updateNotice.ts: the marker that tells the next
//! launch which version was just installed, so it can open the release notes.
//!
//! The marker lives under the same localStorage key. The Tauri app writes it
//! before relaunching into an update, and the first native launch copies
//! WebKit's localStorage into `Kv`, so an update from the Tauri app into the
//! native app still shows the notes.

use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const INSTALLED_UPDATE_KEY: &str = "monocode.installedUpdate";

/// `UpdateNoticeStore`: the three `Storage` methods the marker uses. Any of
/// them may fail, as localStorage could throw.
pub trait UpdateNoticeStore {
    fn get_item(&self, key: &str) -> anyhow::Result<Option<String>>;
    fn set_item(&self, key: &str, value: &str) -> anyhow::Result<()>;
    fn remove_item(&self, key: &str) -> anyhow::Result<()>;
}

impl UpdateNoticeStore for Kv {
    fn get_item(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(Kv::get_item(self, key))
    }

    fn set_item(&self, key: &str, value: &str) -> anyhow::Result<()> {
        Kv::set_item(self, key, value);
        Ok(())
    }

    fn remove_item(&self, key: &str) -> anyhow::Result<()> {
        Kv::remove_item(self, key);
        Ok(())
    }
}

/// `InstalledUpdate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledUpdate {
    pub version: String,
}

/// `rememberInstalledUpdate`. Blank versions and storage errors are ignored.
pub fn remember_installed_update(version: &str, store: &dyn UpdateNoticeStore) {
    let normalized = version.trim();
    if normalized.is_empty() {
        return;
    }
    let value = serde_json::json!({ "version": normalized }).to_string();
    let _ = store.set_item(INSTALLED_UPDATE_KEY, &value);
}

/// `consumeInstalledUpdate`: reads the marker once and removes it, even when
/// it is malformed. Storage errors read as no marker.
pub fn consume_installed_update(store: &dyn UpdateNoticeStore) -> Option<InstalledUpdate> {
    let stored = store.get_item(INSTALLED_UPDATE_KEY).ok()??;
    store.remove_item(INSTALLED_UPDATE_KEY).ok()?;
    let raw: Value = serde_json::from_str(&stored).ok()?;
    parse_installed_update(&raw)
}

/// `parseInstalledUpdate`.
fn parse_installed_update(raw: &Value) -> Option<InstalledUpdate> {
    let version = raw.as_object()?.get("version")?.as_str()?.trim();
    if version.is_empty() {
        return None;
    }
    Some(InstalledUpdate {
        version: version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    //! Port of src/app/model/updateNotice.test.ts.

    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct MemoryStore {
        values: RefCell<HashMap<String, String>>,
        failing: Option<&'static str>,
    }

    impl MemoryStore {
        fn fail(&self, method: &str) -> anyhow::Result<()> {
            if self.failing == Some(method) {
                anyhow::bail!("storage unavailable");
            }
            Ok(())
        }
    }

    impl UpdateNoticeStore for MemoryStore {
        fn get_item(&self, key: &str) -> anyhow::Result<Option<String>> {
            self.fail("getItem")?;
            Ok(self.values.borrow().get(key).cloned())
        }

        fn set_item(&self, key: &str, value: &str) -> anyhow::Result<()> {
            self.fail("setItem")?;
            self.values.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }

        fn remove_item(&self, key: &str) -> anyhow::Result<()> {
            self.fail("removeItem")?;
            self.values.borrow_mut().remove(key);
            Ok(())
        }
    }

    #[test]
    fn is_consumed_once() {
        let store = MemoryStore::default();
        remember_installed_update("0.1.23", &store);
        assert_eq!(
            consume_installed_update(&store),
            Some(InstalledUpdate {
                version: "0.1.23".into()
            })
        );
        assert_eq!(consume_installed_update(&store), None);
    }

    #[test]
    fn does_not_store_blank_versions() {
        for version in ["", "   "] {
            let store = MemoryStore::default();
            remember_installed_update(version, &store);
            assert_eq!(consume_installed_update(&store), None, "{version:?}");
        }
    }

    #[test]
    fn removes_malformed_markers() {
        for value in ["not json", "{}", r#"{"version":3}"#, r#"{"version":""}"#] {
            let store = MemoryStore::default();
            store.set_item(INSTALLED_UPDATE_KEY, value).unwrap();
            assert_eq!(consume_installed_update(&store), None, "{value:?}");
            assert_eq!(store.get_item(INSTALLED_UPDATE_KEY).unwrap(), None);
        }
    }

    #[test]
    fn treats_missing_storage_as_no_marker() {
        assert_eq!(consume_installed_update(&MemoryStore::default()), None);
    }

    #[test]
    fn contains_storage_errors() {
        for method in ["getItem", "setItem", "removeItem"] {
            let store = MemoryStore {
                failing: Some(method),
                ..MemoryStore::default()
            };
            remember_installed_update("0.1.23", &store);
            let _ = consume_installed_update(&store);
        }
    }

    #[test]
    fn stores_the_same_json_as_the_typescript() {
        let store = MemoryStore::default();
        remember_installed_update(" 0.1.23 ", &store);
        assert_eq!(
            store.get_item(INSTALLED_UPDATE_KEY).unwrap().as_deref(),
            Some(r#"{"version":"0.1.23"}"#)
        );
    }

    #[test]
    fn works_with_kv() {
        let kv = Kv::in_memory();
        remember_installed_update("0.6.1", &kv);
        assert_eq!(
            consume_installed_update(&kv).map(|update| update.version),
            Some("0.6.1".into())
        );
        assert_eq!(kv.get_item(INSTALLED_UPDATE_KEY), None);
    }
}
