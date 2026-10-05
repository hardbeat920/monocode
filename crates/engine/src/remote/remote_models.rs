//! Port of src/features/connections/model/remoteModels.ts: model settings
//! controls for a remote session, backed by the host's catalog and falling
//! back to the session's saved values.

use std::collections::BTreeMap;

use monocode_core::models::{ModelSetting, ModelSettingChoice, ModelSettingKind, bundled_models};
use monocode_core::{AgentModel, HarnessId};
use monocode_harness::providers::claude::catalog::claude_model_catalog;
use monocode_remote::host::protocol::{HostModelCatalog, RemoteProvider};

/// Why some settings are not described by the host's live catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelControlsFallback {
    /// `"no-catalog"`: the host has not described its models.
    NoCatalog,
    /// `"unlisted"`: the host's catalog no longer lists the saved model.
    Unlisted,
    /// `"saved"`: saved settings the catalog entry no longer describes.
    Saved,
}

/// `RemoteModelControls`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteModelControls {
    /// The host's current catalog entry, when it lists this model.
    pub model: Option<AgentModel>,
    pub settings: Vec<ModelSetting>,
    pub fallback: Option<ModelControlsFallback>,
}

/// `comparable`: an id with its harness prefix, `[1m]` suffix, `claude-`
/// prefix, and dots normalized away.
fn comparable(id: &str) -> String {
    let mut rest = id;
    if let Some(colon) = rest.find(':')
        && colon > 0
        && rest[..colon].bytes().all(|byte| byte.is_ascii_lowercase())
    {
        rest = &rest[colon + 1..];
    }
    let suffix = rest.len().checked_sub(4).filter(|start| {
        rest.get(*start..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case("[1m]"))
    });
    let rest = suffix.map_or(rest, |start| &rest[..start]);
    let lower = rest.to_lowercase();
    let lower = lower.strip_prefix("claude-").unwrap_or(&lower);
    lower.replace('.', "-")
}

/// `findRemoteModel`: match a saved model to the host catalog, tolerating id
/// scheme changes such as `claude:opus-4.6` (built-in list) and
/// `claude:opus-4-6` (live list).
pub fn find_remote_model<'a>(models: &'a [AgentModel], id: &str) -> Option<&'a AgentModel> {
    if id.is_empty() {
        return None;
    }
    let wanted = comparable(id);
    models.iter().find(|model| model.id == id).or_else(|| {
        models.iter().find(|model| {
            comparable(&model.id) == wanted
                || model
                    .native_id
                    .as_deref()
                    .is_some_and(|native| !native.is_empty() && comparable(native) == wanted)
        })
    })
}

fn choice(value: &str, label: &str) -> ModelSettingChoice {
    ModelSettingChoice {
        value: value.into(),
        label: label.into(),
    }
}

/// `LABELS[value] ?? value`.
fn label(value: &str) -> &str {
    match value {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultracode" => "Ultracode",
        "ultrathink" => "Ultrathink",
        other => other,
    }
}

fn setting(
    id: &str,
    label: &str,
    kind: ModelSettingKind,
    value: &str,
    options: Vec<ModelSettingChoice>,
) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: label.into(),
        kind,
        value: value.into(),
        options,
        description: None,
    }
}

fn effort(id: &str, values: &[&str]) -> ModelSetting {
    setting(
        id,
        "Reasoning",
        ModelSettingKind::Select,
        "high",
        values
            .iter()
            .map(|value| choice(value, label(value)))
            .collect(),
    )
}

fn toggle(id: &str, label: &str) -> ModelSetting {
    setting(
        id,
        label,
        ModelSettingKind::Toggle,
        "false",
        vec![choice("true", "On"), choice("false", "Off")],
    )
}

/// `knownSetting`: conservative definitions for settings the provider
/// adapters understand, used only when the host cannot describe the
/// session's model.
fn known_setting(provider: RemoteProvider, id: &str) -> Option<ModelSetting> {
    match provider {
        HarnessId::Codex => match id {
            "reasoningEffort" => Some(effort(id, &["low", "medium", "high", "xhigh"])),
            "serviceTier" => Some(setting(
                id,
                "Service Tier",
                ModelSettingKind::Select,
                "default",
                vec![choice("default", "Standard"), choice("fast", "Fast")],
            )),
            _ => None,
        },
        HarnessId::Claude => match id {
            "effort" => Some(effort(id, &["low", "medium", "high", "max", "ultrathink"])),
            "fast" => Some(toggle(id, "Fast")),
            "thinking" => Some(toggle(id, "Thinking")),
            "context" => Some(setting(
                id,
                "Context",
                ModelSettingKind::Select,
                "200k",
                vec![choice("200k", "200k"), choice("1m", "1M")],
            )),
            _ => None,
        },
        _ => None,
    }
}

/// A select with only the saved value, for a setting nothing describes.
fn saved_only(id: &str, value: &str) -> ModelSetting {
    setting(
        id,
        id,
        ModelSettingKind::Select,
        value,
        vec![choice(value, value)],
    )
}

fn fallback_settings(
    provider: RemoteProvider,
    model_id: &str,
    saved: &BTreeMap<String, String>,
) -> Vec<ModelSetting> {
    let builtin: Vec<AgentModel> = if provider == HarnessId::Claude {
        claude_model_catalog().to_vec()
    } else {
        bundled_models()
            .iter()
            .filter(|model| model.harness == provider)
            .cloned()
            .collect()
    };
    let known = find_remote_model(&builtin, model_id).and_then(|model| model.settings.clone());
    let mut ids: Vec<&str> = match provider {
        HarnessId::Codex => vec!["reasoningEffort"],
        HarnessId::Claude => vec!["effort"],
        _ => Vec::new(),
    };
    ids.extend(saved.keys().map(String::as_str));
    let mut settings = known.unwrap_or_default();
    for id in ids {
        if settings.iter().any(|setting| setting.id == id) {
            continue;
        }
        if let Some(setting) = known_setting(provider, id) {
            settings.push(setting);
        } else if let Some(value) = saved.get(id).filter(|value| !value.is_empty()) {
            settings.push(saved_only(id, value));
        }
    }
    settings
}

/// `withSaved`: keep a saved value selectable even when the catalog no
/// longer offers it.
fn with_saved(mut setting: ModelSetting, saved: Option<&String>) -> ModelSetting {
    let Some(saved) = saved.filter(|saved| !saved.is_empty()) else {
        return setting;
    };
    if setting.options.iter().any(|option| option.value == *saved) {
        return setting;
    }
    setting
        .options
        .push(choice(saved, &format!("{} (saved)", label(saved))));
    setting
}

/// `remoteModelControls`: settings controls for a remote session. Effort
/// must never disappear because the host's catalog is loading, failed, or no
/// longer lists the saved model; those cases fall back to the session's
/// saved values and definitions the provider adapters already understand.
pub fn remote_model_controls(
    catalog: Option<&HostModelCatalog>,
    provider: RemoteProvider,
    model_id: &str,
    saved: &BTreeMap<String, String>,
    saved_model_id: Option<&str>,
) -> RemoteModelControls {
    let listed = catalog.and_then(|catalog| catalog.models.get(&provider));
    let model = listed
        .and_then(|models| find_remote_model(models, model_id))
        .cloned();
    // A newly chosen catalog model uses only what the host says it supports.
    if let Some(model) = &model
        && Some(model_id) != saved_model_id
    {
        return RemoteModelControls {
            settings: model.settings.clone().unwrap_or_default(),
            model: Some(model.clone()),
            fallback: None,
        };
    }
    if model.is_none() && saved_model_id.is_none_or(str::is_empty) {
        return RemoteModelControls::default();
    }
    let mut settings = model
        .as_ref()
        .and_then(|model| model.settings.clone())
        .unwrap_or_default();
    let mut fallback = None;
    if model.is_none() {
        settings.extend(fallback_settings(provider, model_id, saved));
        fallback = Some(if listed.is_some() {
            ModelControlsFallback::Unlisted
        } else {
            ModelControlsFallback::NoCatalog
        });
    }
    // Saved settings the current catalog entry no longer describes stay visible.
    for (id, value) in saved {
        if settings.iter().any(|setting| setting.id == *id) {
            continue;
        }
        settings.push(known_setting(provider, id).unwrap_or_else(|| saved_only(id, value)));
        fallback.get_or_insert(ModelControlsFallback::Saved);
    }
    RemoteModelControls {
        model,
        settings: settings
            .into_iter()
            .map(|setting| {
                let saved = saved.get(&setting.id);
                with_saved(setting, saved)
            })
            .collect(),
        fallback,
    }
}

/// `carryModelSettings`: values for the next model, keeping choices it
/// supports and defaulting the rest.
pub fn carry_model_settings(
    settings: &[ModelSetting],
    current: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    settings
        .iter()
        .map(|setting| {
            let value = current
                .get(&setting.id)
                .filter(|value| setting.options.iter().any(|option| option.value == **value))
                .cloned()
                .unwrap_or_else(|| setting.value.clone());
            (setting.id.clone(), value)
        })
        .collect()
}

/// `sameModelSettings`.
pub fn same_model_settings(
    a: Option<&BTreeMap<String, String>>,
    b: Option<&BTreeMap<String, String>>,
) -> bool {
    let empty = BTreeMap::new();
    let a = a.unwrap_or(&empty);
    let b = b.unwrap_or(&empty);
    a.len() == b.len() && a.iter().all(|(key, value)| b.get(key) == Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effort_setting() -> ModelSetting {
        setting(
            "effort",
            "Reasoning",
            ModelSettingKind::Select,
            "high",
            ["low", "high", "xhigh"]
                .iter()
                .map(|value| choice(value, value))
                .collect(),
        )
    }

    fn opus() -> AgentModel {
        let mut model = AgentModel::new("claude:opus-4-6", HarnessId::Claude, "Opus")
            .with_native_id("claude-opus-4-6");
        model.settings = Some(vec![effort_setting()]);
        model
    }

    fn map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    // remoteModels.test.ts
    #[test]
    fn matches_saved_models_across_catalog_id_schemes_preferring_exact_ids() {
        let opus = opus();
        let models = [opus.clone()];
        assert_eq!(find_remote_model(&models, "claude:opus-4.6"), Some(&opus));
        assert_eq!(find_remote_model(&models, "claude-opus-4-6"), Some(&opus));
        let mut alias = opus.clone();
        alias.id = "claude:opus-4.6".into();
        alias.native_id = Some("opus".into());
        let both = [opus.clone(), alias.clone()];
        assert_eq!(find_remote_model(&both, "claude:opus-4.6"), Some(&alias));
        assert_eq!(find_remote_model(&models, "claude:sonnet-4-6"), None);
    }

    // remoteModels.test.ts
    #[test]
    fn uses_only_catalog_settings_for_a_newly_chosen_model() {
        let catalog = HostModelCatalog {
            models: BTreeMap::from([(HarnessId::Claude, vec![opus()])]),
            errors: BTreeMap::new(),
        };
        assert_eq!(
            remote_model_controls(
                Some(&catalog),
                HarnessId::Claude,
                &opus().id,
                &map(&[("context", "1m")]),
                None
            ),
            RemoteModelControls {
                model: Some(opus()),
                settings: vec![effort_setting()],
                fallback: None,
            }
        );
    }

    // remoteModels.test.ts
    #[test]
    fn describes_a_claude_model_from_built_in_metadata_when_the_host_cannot() {
        let controls = remote_model_controls(
            None,
            HarnessId::Claude,
            "claude:opus-5-5",
            &map(&[("effort", "xhigh")]),
            Some("claude:opus-5-5"),
        );
        assert_eq!(controls.fallback, Some(ModelControlsFallback::NoCatalog));
        // Opus 5.5 runs at 1M from its bare id, so there is no Context choice.
        assert_eq!(
            controls
                .settings
                .iter()
                .map(|setting| setting.id.as_str())
                .collect::<Vec<_>>(),
            ["effort", "fast"]
        );
    }

    // remoteModels.test.ts
    #[test]
    fn keeps_another_providers_saved_settings_without_inventing_claude_controls() {
        let controls = remote_model_controls(
            None,
            HarnessId::Cursor,
            "cursor:custom-model",
            &map(&[("profile", "fast")]),
            Some("cursor:custom-model"),
        );
        assert_eq!(controls.settings, vec![saved_only("profile", "fast")]);
    }

    // remoteModels.test.ts
    #[test]
    fn carries_compatible_choices_to_another_model_and_compares_settings_by_value() {
        let settings = [effort_setting()];
        assert_eq!(
            carry_model_settings(&settings, &map(&[("effort", "xhigh"), ("fast", "true")])),
            map(&[("effort", "xhigh")])
        );
        assert_eq!(
            carry_model_settings(&settings, &map(&[("effort", "max")])),
            map(&[("effort", "high")])
        );
        assert!(same_model_settings(
            Some(&map(&[("a", "1"), ("b", "2")])),
            Some(&map(&[("b", "2"), ("a", "1")]))
        ));
        assert!(!same_model_settings(
            Some(&map(&[("a", "1")])),
            Some(&map(&[("a", "1"), ("b", "2")]))
        ));
    }
}
