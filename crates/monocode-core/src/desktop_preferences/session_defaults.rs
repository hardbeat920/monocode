//! Read the same model choices and remembered controls used by newSession.
use super::DesktopPreferences;
use crate::agent::Settings;
use serde_json::Value;
use std::collections::HashMap;

impl DesktopPreferences {
    /// Defaults for a new conversation of the already selected provider.
    /// Stored conversations retain their own model and control values.
    pub fn session_defaults(&self, provider: &str, cwd: &str) -> Option<Settings> {
        if !self.is_available() {
            return None;
        }
        let projects = self.json("monocode.projectProviderSettings.v1");
        let key = crate::files::path_key(cwd);
        let project = projects.as_ref().and_then(|values| values.get(&key));
        let defaults = self.json("monocode.defaultModels");
        let last = self.json("monocode.lastModel");
        let model = project
            .and_then(|project| project["models"][provider].as_str())
            .or_else(|| {
                project
                    .filter(|project| project["defaultHarness"] == provider)
                    .and_then(|project| project["defaultModel"].as_str())
            })
            .or_else(|| {
                defaults
                    .as_ref()
                    .and_then(|models| models[provider].as_str())
            })
            .or_else(|| {
                last.as_ref()
                    .filter(|last| last["harness"] == provider)
                    .and_then(|last| last["model"].as_str())
            })
            .and_then(|model| crate::store::native_model(model, provider));
        let controls: HashMap<String, String> = self
            .json("monocode.lastModelSettings")
            .as_ref()
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
            // lastModelSettings remembers every provider's controls together.
            // newSession merges only controls defined by the chosen provider.
            .filter(|(key, value)| match provider {
                "codex" => matches!(key.as_str(), "reasoningEffort" | "serviceTier"),
                "claude" => {
                    matches!(key.as_str(), "effort" | "fast" | "context")
                        || (key == "thinking" && matches!(value.as_str(), "true" | "false"))
                }
                _ if matches!(
                    key.as_str(),
                    "effort"
                        | "reasoning"
                        | "reasoningEffort"
                        | "reasoning_effort"
                        | "thinking"
                        | "variant"
                ) =>
                {
                    effort_keys(provider).contains(&key.as_str())
                }
                _ => true,
            })
            .collect();
        if model.is_none() && controls.is_empty() {
            return None;
        }
        Some(Settings {
            model,
            effort: effort_keys(provider)
                .iter()
                .find_map(|key| controls.get(*key).cloned()),
            fast: controls.get("fast").is_some_and(|value| value == "true"),
            service_tier: controls.get("serviceTier").cloned(),
            thinking: controls
                .get("thinking")
                .and_then(|value| value.parse().ok()),
            model_settings: controls,
            ..Default::default()
        })
    }
}

fn effort_keys(provider: &str) -> &'static [&'static str] {
    match provider {
        "codex" => &["reasoningEffort"],
        "claude" | "grok" | "fx" | "devin" | "antigravity" => &["effort"],
        "cursor" => &["reasoning", "reasoning_effort"],
        "pi" | "omp" => &["thinking"],
        "opencode" => &["variant"],
        _ => &[
            "effort",
            "reasoning",
            "reasoningEffort",
            "reasoning_effort",
            "variant",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preferences(entries: &[(&str, Value)]) -> DesktopPreferences {
        DesktopPreferences {
            available: true,
            values: entries
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        }
    }

    #[test]
    fn desktop_model_keys_and_remembered_effort_seed_new_conversations() {
        let source = preferences(&[
            (
                "monocode.defaultModels",
                json!({"codex":"codex:gpt-6.1-sol"}),
            ),
            (
                "monocode.lastModelSettings",
                json!({"effort":"medium","reasoningEffort":"xhigh","serviceTier":"default","thinking":"high","variant":"xhigh"}),
            ),
        ]);
        let settings = source.session_defaults("codex", "/work").unwrap();
        assert_eq!(settings.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(settings.effort.as_deref(), Some("xhigh"));
        assert_eq!(settings.service_tier.as_deref(), Some("default"));
        assert!(!settings.model_settings.contains_key("effort"));
        let claude = source.session_defaults("claude", "/work").unwrap();
        assert_eq!(claude.effort.as_deref(), Some("medium"));
        assert!(!claude.model_settings.contains_key("reasoningEffort"));
    }

    #[test]
    fn project_models_win_and_other_provider_last_choices_do_not_leak() {
        let source = preferences(&[
            (
                "monocode.projectProviderSettings.v1",
                json!({"/work":{"defaultHarness":"claude","defaultModel":"claude:opus-4.8","models":{"codex":"codex:gpt-project"}}}),
            ),
            (
                "monocode.defaultModels",
                json!({"codex":"codex:gpt-global"}),
            ),
            (
                "monocode.lastModel",
                json!({"harness":"codex","model":"codex:gpt-last"}),
            ),
        ]);
        assert_eq!(
            source
                .session_defaults("codex", "/work")
                .unwrap()
                .model
                .as_deref(),
            Some("gpt-project")
        );
        assert_eq!(
            source
                .session_defaults("claude", "/work")
                .unwrap()
                .model
                .as_deref(),
            Some("claude-opus-4-8")
        );
        assert!(source.session_defaults("claude", "/other").is_none());
        assert!(DesktopPreferences::default()
            .session_defaults("codex", "/work")
            .is_none());
    }
}
