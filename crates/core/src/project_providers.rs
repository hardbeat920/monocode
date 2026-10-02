//! Port of src/features/sessions/model/projectProviders.ts.
//!
//! Per-project overrides for the Providers settings: which provider new
//! conversations start with, each provider's model, and which providers show
//! in the model picker. Anything unset falls back to the global defaults, so
//! a project stores only what it overrides.
//!
//! The TypeScript kept these in localStorage. Here they are a value the caller
//! loads and saves; `ProjectProviders::parse` reads the old stored JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::harness::HarnessId;
use crate::paths::path_key;

/// localStorage key that held every project's overrides as one JSON object
/// keyed by `path_key(project)`.
pub const PROJECT_PROVIDER_SETTINGS_KEY: &str = "monocode.projectProviderSettings.v1";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProviderSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_harness: Option<HarnessId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<BTreeMap<HarnessId, String>>,
    /// Providers this project keeps out of the picker and out of new sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<Vec<HarnessId>>,
}

impl ProjectProviderSettings {
    fn is_empty(&self) -> bool {
        self.default_harness.is_none()
            && self.default_model.is_none()
            && self.models.as_ref().is_none_or(BTreeMap::is_empty)
            && self.hidden.as_ref().is_none_or(Vec::is_empty)
    }

    fn normalized(mut self) -> Self {
        self.default_model = self.default_model.filter(|model| !model.is_empty());
        self.models = self
            .models
            .map(|models| {
                models
                    .into_iter()
                    .filter(|(_, model)| !model.is_empty())
                    .collect::<BTreeMap<_, _>>()
            })
            .filter(|models| !models.is_empty());
        self.hidden = self
            .hidden
            .map(|hidden| {
                let mut unique = Vec::new();
                for id in hidden {
                    if !unique.contains(&id) {
                        unique.push(id);
                    }
                }
                unique
            })
            .filter(|hidden| !hidden.is_empty());
        self
    }

    /// `normalize` applied to untyped stored JSON. Unknown harness ids are
    /// dropped.
    // TODO(port): the TypeScript kept any string as a harness id. A Rust enum
    // cannot hold an unknown id, so one written by a newer build is dropped.
    fn from_value(value: &Value) -> Option<Self> {
        let rec = value.as_object()?;
        let harness = |value: &Value| value.as_str().and_then(HarnessId::parse);
        let models = rec.get("models").and_then(Value::as_object).map(|models| {
            models
                .iter()
                .filter_map(|(key, model)| {
                    let model = model.as_str().filter(|model| !model.is_empty())?;
                    Some((HarnessId::parse(key)?, model.to_string()))
                })
                .collect()
        });
        let hidden = rec
            .get("hidden")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(harness).collect());
        Some(
            ProjectProviderSettings {
                default_harness: rec.get("defaultHarness").and_then(harness),
                default_model: rec
                    .get("defaultModel")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                models,
                hidden,
            }
            .normalized(),
        )
    }
}

/// Every project's overrides, keyed by `path_key(project)`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectProviders {
    pub by_project: BTreeMap<String, ProjectProviderSettings>,
}

impl ProjectProviders {
    /// `parse`: read the stored JSON, skipping malformed and empty entries.
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
            return Self::default();
        };
        match serde_json::from_str::<Value>(raw) {
            Ok(value) => Self::from_value(&value),
            Err(_) => Self::default(),
        }
    }

    pub fn from_value(value: &Value) -> Self {
        let Some(entries) = value.as_object() else {
            return Self::default();
        };
        let by_project = entries
            .iter()
            .filter_map(|(key, entry)| {
                let clean = ProjectProviderSettings::from_value(entry)?;
                (!clean.is_empty()).then(|| (key.clone(), clean))
            })
            .collect();
        Self { by_project }
    }

    /// The JSON the TypeScript stored under `PROJECT_PROVIDER_SETTINGS_KEY`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    /// `loadProjectProviderSettings`.
    pub fn load(&self, project: Option<&str>) -> ProjectProviderSettings {
        let Some(project) = project.filter(|project| !project.is_empty()) else {
            return ProjectProviderSettings::default();
        };
        self.by_project
            .get(&path_key(project))
            .cloned()
            .unwrap_or_default()
    }

    /// `isProviderHidden`.
    pub fn is_provider_hidden(&self, project: Option<&str>, harness: HarnessId) -> bool {
        self.load(project)
            .hidden
            .is_some_and(|hidden| hidden.contains(&harness))
    }

    /// `setProjectDefaultProvider`.
    pub fn set_project_default_provider(&mut self, project: &str, harness: HarnessId, model: &str) {
        self.update(project, |current| ProjectProviderSettings {
            default_harness: Some(harness),
            default_model: Some(model.to_string()),
            ..current
        });
    }

    /// `setProjectDefaultModel`.
    pub fn set_project_default_model(&mut self, project: &str, harness: HarnessId, model: &str) {
        self.update(project, |mut current| {
            current
                .models
                .get_or_insert_with(BTreeMap::new)
                .insert(harness, model.to_string());
            current
        });
    }

    /// `setProjectProviderHidden`.
    pub fn set_project_provider_hidden(&mut self, project: &str, harness: HarnessId, hidden: bool) {
        self.update(project, |mut current| {
            let mut next = current.hidden.take().unwrap_or_default();
            if hidden {
                if !next.contains(&harness) {
                    next.push(harness);
                }
            } else {
                next.retain(|id| *id != harness);
            }
            current.hidden = (!next.is_empty()).then_some(next);
            current
        });
    }

    /// `clearProjectProviders`.
    pub fn clear_project_providers(&mut self, project: &str) {
        self.by_project.remove(&path_key(project));
    }

    /// `rebaseProjectProviders`: follow a project rename so its overrides are
    /// not orphaned.
    pub fn rebase_project_providers(&mut self, from: &str, to: &str) {
        let from_key = path_key(from);
        let to_key = path_key(to);
        if from_key == to_key {
            return;
        }
        if let Some(entry) = self.by_project.remove(&from_key) {
            self.by_project.insert(to_key, entry);
        }
    }

    fn update(
        &mut self,
        project: &str,
        change: impl FnOnce(ProjectProviderSettings) -> ProjectProviderSettings,
    ) {
        let key = path_key(project);
        let current = self.by_project.get(&key).cloned().unwrap_or_default();
        let next = change(current).normalized();
        if next.is_empty() {
            self.by_project.remove(&key);
        } else {
            self.by_project.insert(key, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_only_what_a_project_overrides() {
        let mut store = ProjectProviders::default();
        store.set_project_provider_hidden("/repo/a/", HarnessId::Claude, true);
        store.set_project_default_model("/repo/a", HarnessId::Cursor, "cursor:composer-2.5");
        assert!(store.is_provider_hidden(Some("/repo/a"), HarnessId::Claude));
        assert!(!store.is_provider_hidden(Some("/repo/b"), HarnessId::Claude));
        assert_eq!(
            store.to_json(),
            r#"{"/repo/a":{"models":{"cursor":"cursor:composer-2.5"},"hidden":["claude"]}}"#
        );
        store.set_project_provider_hidden("/repo/a", HarnessId::Claude, false);
        store.set_project_default_model("/repo/a", HarnessId::Cursor, "");
        assert_eq!(store.to_json(), "{}");
    }

    #[test]
    fn parses_and_cleans_stored_overrides() {
        let store = ProjectProviders::parse(Some(
            r#"{"/a":{"defaultHarness":"codex","defaultModel":"","hidden":["pi","pi","nope"]},"/b":{},"/c":[1],"/d":{"models":{"claude":""}}}"#,
        ));
        assert_eq!(store.by_project.len(), 1);
        assert_eq!(
            store.load(Some("/a")),
            ProjectProviderSettings {
                default_harness: Some(HarnessId::Codex),
                hidden: Some(vec![HarnessId::Pi]),
                ..Default::default()
            }
        );
        assert_eq!(
            ProjectProviders::parse(Some("not json")),
            ProjectProviders::default()
        );
        assert_eq!(store.load(None), ProjectProviderSettings::default());
    }

    #[test]
    fn follows_a_project_rename() {
        let mut store = ProjectProviders::default();
        store.set_project_default_provider("/old", HarnessId::Grok, "grok:grok-4.6");
        store.rebase_project_providers("/old", "/new");
        assert_eq!(
            store.load(Some("/new")).default_harness,
            Some(HarnessId::Grok)
        );
        assert_eq!(store.load(Some("/old")), ProjectProviderSettings::default());
        store.clear_project_providers("/new");
        assert!(store.by_project.is_empty());
    }
}
