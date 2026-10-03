//! The pure parts of src/features/sessions/ui/ModelPicker.tsx and
//! ModelSettings.tsx: setting order, toolbar pill grouping, model groups,
//! the visible provider tabs, search, and the trigger labels.

use monocode_core::models::{
    AgentModel, LastModelChoice, ModelPickerTab, ModelPrefs, ModelSetting, ModelSettingKind,
    is_effort_setting_id,
};
use monocode_core::{HARNESSES, HarnessId, ModelSettings, ProjectProviders};

use super::model_source::ModelSource;

/// `MENU_WIDTH`.
pub const MENU_WIDTH: f32 = 250.0;
/// `MODEL_MENU_WIDTH`.
pub const MODEL_MENU_WIDTH: f32 = 310.0;
/// `SETTING_MENU_WIDTH`.
pub const SETTING_MENU_WIDTH: f32 = 210.0;
/// `SUBMENU_OVERLAP`.
pub const SUBMENU_OVERLAP: f32 = -4.0;
pub const PROVIDER_TAB_SIZE: f32 = 32.0;
pub const PROVIDER_TAB_GAP: f32 = 4.0;
pub const PROVIDER_RAIL_PADDING: f32 = 12.0;
/// `MODEL_MENU_HEIGHT`: room for Favorites and every provider tab.
pub const MODEL_MENU_HEIGHT: f32 = (HARNESSES.len() as f32 + 1.0) * PROVIDER_TAB_SIZE
    + HARNESSES.len() as f32 * PROVIDER_TAB_GAP
    + PROVIDER_RAIL_PADDING;
/// `MODEL_MENU_FRAME_HEIGHT`: the content plus the frame's border.
pub const MODEL_MENU_FRAME_HEIGHT: f32 = MODEL_MENU_HEIGHT + 2.0;

/// `SETTING_ORDER`: rows in the combined picker menu.
const SETTING_ORDER: [&str; 9] = [
    "fast",
    "effort",
    "reasoning",
    "reasoningEffort",
    "serviceTier",
    "thinking",
    "variant",
    "agent",
    "context",
];

/// `PILL_ORDER`: toolbar pills, reasoning level first.
const PILL_ORDER: [&str; 8] = [
    "effort",
    "reasoning",
    "reasoningEffort",
    "variant",
    "fast",
    "thinking",
    "serviceTier",
    "context",
];

/// ModelSettings.tsx's pill order.
const MODEL_SETTINGS_ORDER: [&str; 7] = [
    "variant",
    "agent",
    "effort",
    "reasoning",
    "thinking",
    "fast",
    "context",
];

fn order_index(order: &[&str], id: &str) -> usize {
    order.iter().position(|item| *item == id).unwrap_or(99)
}

fn sorted_by(settings: Vec<ModelSetting>, order: &[&str]) -> Vec<ModelSetting> {
    let mut settings = settings;
    settings.sort_by_key(|setting| order_index(order, &setting.id));
    settings
}

/// `isEffortSetting`.
pub fn is_effort_setting(setting: &ModelSetting) -> bool {
    is_effort_setting_id(&setting.id)
}

/// `effortSetting`: the select that sets the reasoning level.
pub fn effort_setting(model: &AgentModel) -> Option<&ModelSetting> {
    model
        .settings
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|setting| setting.kind == ModelSettingKind::Select && is_effort_setting(setting))
}

/// `menuVisibleSettings`: settings without the OpenCode agent row, which
/// never shows in the menu.
pub fn menu_visible_settings(model: &AgentModel) -> Vec<ModelSetting> {
    model
        .settings
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|setting| !(model.harness == HarnessId::Opencode && setting.id == "agent"))
        .cloned()
        .collect()
}

/// `pickerSettings`.
pub fn picker_settings(model: &AgentModel) -> Vec<ModelSetting> {
    sorted_by(menu_visible_settings(model), &SETTING_ORDER)
}

/// `pillSettings`.
pub fn pill_settings(model: &AgentModel) -> Vec<ModelSetting> {
    sorted_by(menu_visible_settings(model), &PILL_ORDER)
}

/// The settings ModelSettings.tsx shows, in its order.
pub fn model_settings_controls(harness: HarnessId, model: &AgentModel) -> Vec<ModelSetting> {
    let list = model
        .settings
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|setting| !(harness == HarnessId::Opencode && setting.id == "agent"))
        .cloned()
        .collect();
    sorted_by(list, &MODEL_SETTINGS_ORDER)
}

/// `settingLabel`.
pub fn setting_label(setting: &ModelSetting) -> String {
    if setting.id == "effort" || setting.id == "reasoning" {
        "Effort".into()
    } else {
        setting.label.clone()
    }
}

/// `settingValue`.
pub fn setting_value<'a>(setting: &'a ModelSetting, values: &'a ModelSettings) -> &'a str {
    values.get(&setting.id).unwrap_or(&setting.value)
}

/// `settingValueLabel`.
pub fn setting_value_label(setting: &ModelSetting, values: &ModelSettings) -> String {
    let value = setting_value(setting, values);
    setting
        .options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| value.to_string())
}

/// `{ ...values, [setting.id]: value }`.
pub fn with_setting(values: &ModelSettings, id: &str, value: &str) -> ModelSettings {
    let mut next = values.clone();
    next.insert(id.to_string(), value.to_string());
    next
}

/// `recentMenuModels`: recent choices the source still knows, plus the
/// current model, at most six.
pub fn recent_menu_models(
    current: &AgentModel,
    recent: &[LastModelChoice],
    source: &dyn ModelSource,
) -> Vec<AgentModel> {
    let mut models: Vec<AgentModel> = recent
        .iter()
        .filter_map(|choice| {
            source
                .find(&choice.model)
                .filter(|item| item.harness == choice.harness)
        })
        .collect();
    if !models.iter().any(|item| item.id == current.id) {
        models.push(current.clone());
    }
    models.truncate(6);
    models
}

/// `ModelGroup`: OpenCode lists models under their upstream provider.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelGroup {
    pub id: String,
    pub name: Option<String>,
    /// Each model with its index in the flat list.
    pub models: Vec<(AgentModel, usize)>,
}

/// `modelGroups`.
pub fn model_groups(tab: ModelPickerTab, models: &[AgentModel]) -> Vec<ModelGroup> {
    if tab != ModelPickerTab::Harness(HarnessId::Opencode) {
        return vec![ModelGroup {
            id: "models".into(),
            name: None,
            models: models.iter().cloned().zip(0..).collect(),
        }];
    }
    let mut groups: Vec<ModelGroup> = Vec::new();
    for (index, item) in models.iter().enumerate() {
        let (id, name) = match &item.provider {
            Some(provider) => (provider.id.clone(), provider.name.clone()),
            None => ("opencode".to_string(), "OpenCode".to_string()),
        };
        match groups.iter_mut().find(|group| group.id == id) {
            Some(group) => group.models.push((item.clone(), index)),
            None => groups.push(ModelGroup {
                id,
                name: Some(name),
                models: vec![(item.clone(), index)],
            }),
        }
    }
    groups
}

/// `pickerHarnesses`: providers the tab rail shows.
pub fn picker_harnesses(
    source: &dyn ModelSource,
    prefs: &ModelPrefs,
    projects: &ProjectProviders,
    project: Option<&str>,
    allowed: Option<&[HarnessId]>,
) -> Vec<HarnessId> {
    HARNESSES
        .into_iter()
        .filter(|id| {
            allowed.is_none_or(|allowed| allowed.contains(id))
                && !projects.is_provider_hidden(project, *id)
                && prefs.show_provider_in_model_picker(*id, source.available(*id), source.probed())
        })
        .collect()
}

/// `visibleModels`: the tab's pool, filtered by the search query.
pub fn visible_models(
    tab: ModelPickerTab,
    favorites: &[String],
    harnesses: &[HarnessId],
    source: &dyn ModelSource,
    query: &str,
) -> Vec<AgentModel> {
    let needle = monocode_core::js::trim(query).to_lowercase();
    let pool: Vec<AgentModel> = match tab {
        ModelPickerTab::Favorites => favorites
            .iter()
            .filter_map(|id| source.find(id))
            .filter(|item| harnesses.contains(&item.harness))
            .collect(),
        ModelPickerTab::Harness(harness) => source.models_for(harness),
    };
    if needle.is_empty() {
        return pool;
    }
    pool.into_iter()
        .filter(|item| {
            let provider = item.provider.as_ref();
            format!(
                "{} {} {} {}",
                item.name,
                item.harness.title(),
                provider.map(|p| p.name.as_str()).unwrap_or(""),
                provider.map(|p| p.id.as_str()).unwrap_or("")
            )
            .to_lowercase()
            .contains(&needle)
        })
        .collect()
}

/// `provenance`: provider first (OpenCode Go vs OpenCode), else harness.
pub fn provenance(item: &AgentModel) -> String {
    item.provider
        .as_ref()
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| item.harness.title().to_string())
}

/// The effort label the trigger shows, unless settings live beside it.
pub fn trigger_effort_label(
    current: &AgentModel,
    values: &ModelSettings,
    hide_settings: bool,
) -> Option<String> {
    if hide_settings {
        return None;
    }
    effort_setting(current).map(|setting| setting_value_label(setting, values))
}

/// `triggerTitle`.
pub fn trigger_title(current: &AgentModel, effort: Option<&str>) -> String {
    [
        Some(current.harness.title()),
        current.provider.as_ref().map(|p| p.name.as_str()),
        Some(current.name.as_str()),
        effort,
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// The trigger's accessible name: `Grok Build Grok 4.6, effort High`.
pub fn trigger_label(current: &AgentModel, effort: Option<&str>) -> String {
    let provider = current
        .provider
        .as_ref()
        .map(|provider| format!(", {},", provider.name))
        .unwrap_or_default();
    let effort = effort
        .map(|effort| format!(", effort {effort}"))
        .unwrap_or_default();
    format!(
        "{}{provider} {}{effort}",
        current.harness.title(),
        current.name
    )
}

/// The empty-list line in the model flyout.
pub fn empty_models_message(tab: ModelPickerTab, query: &str, source: &dyn ModelSource) -> String {
    let blank = monocode_core::js::trim(query).is_empty();
    match tab {
        ModelPickerTab::Favorites if blank => "No favorite models".into(),
        ModelPickerTab::Harness(harness) if !source.available(harness) => {
            source.unavailable_hint(harness)
        }
        ModelPickerTab::Harness(HarnessId::Codex) if blank => "Loading Codex models…".into(),
        _ => "No matching models".into(),
    }
}

/// One toolbar pill from `ModelControlPills`.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlPill {
    Toggle(ModelSetting),
    /// A select, with the settings grouped into its popover (fast mode and
    /// service tier ride with the effort select).
    Select {
        setting: ModelSetting,
        grouped: Vec<ModelSetting>,
    },
}

/// `ModelControlPills`: which pills render, in order.
pub fn control_pills(model: &AgentModel) -> Vec<ControlPill> {
    let pills = pill_settings(model);
    let effort = pills
        .iter()
        .find(|setting| setting.kind == ModelSettingKind::Select && is_effort_setting(setting))
        .cloned();
    let grouped: Vec<ModelSetting> = if effort.is_some() {
        pills
            .iter()
            .filter(|setting| setting.id == "fast" || setting.id == "serviceTier")
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    pills
        .into_iter()
        .filter(|setting| !grouped.iter().any(|item| item.id == setting.id))
        .map(|setting| match setting.kind {
            ModelSettingKind::Toggle => ControlPill::Toggle(setting),
            ModelSettingKind::Select => {
                let with_group = effort
                    .as_ref()
                    .is_some_and(|effort| effort.id == setting.id);
                ControlPill::Select {
                    setting,
                    grouped: if with_group {
                        grouped.clone()
                    } else {
                        Vec::new()
                    },
                }
            }
        })
        .collect()
}

/// `menuLabel`: "Effort", "Effort and Fast", "A, B, and C".
pub fn select_menu_label(settings: &[ModelSetting]) -> String {
    let labels: Vec<String> = settings.iter().map(setting_label).collect();
    match labels.len() {
        0..=2 => labels.join(" and "),
        n => format!("{}, and {}", labels[..n - 1].join(", "), labels[n - 1]),
    }
}

/// The flat option list of a select pill's popover: each setting's options
/// in turn.
pub fn select_menu_options(settings: &[ModelSetting]) -> Vec<(ModelSetting, String, String)> {
    settings
        .iter()
        .flat_map(|setting| {
            setting
                .options
                .iter()
                .map(move |option| (setting.clone(), option.value.clone(), option.label.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pickers::model_source::{LocalModelSource, all_available};
    use monocode_core::models::{ModelCatalog, ModelProvider, ModelSettingChoice};

    fn select(id: &str, label: &str, value: &str, options: &[(&str, &str)]) -> ModelSetting {
        ModelSetting {
            id: id.into(),
            label: label.into(),
            kind: ModelSettingKind::Select,
            value: value.into(),
            options: options
                .iter()
                .map(|(value, label)| ModelSettingChoice {
                    value: (*value).into(),
                    label: (*label).into(),
                })
                .collect(),
            description: None,
        }
    }

    fn toggle(id: &str, label: &str) -> ModelSetting {
        ModelSetting {
            kind: ModelSettingKind::Toggle,
            ..select(id, label, "false", &[("false", "Off"), ("true", "On")])
        }
    }

    fn model(id: &str, harness: HarnessId, name: &str, settings: Vec<ModelSetting>) -> AgentModel {
        let mut model = AgentModel::new(id, harness, name);
        model.settings = Some(settings);
        model
    }

    #[test]
    fn menu_rows_put_fast_first_and_hide_the_opencode_agent() {
        let opencode = model(
            "opencode:x",
            HarnessId::Opencode,
            "X",
            vec![
                select("agent", "Agent", "build", &[("build", "Build")]),
                select("variant", "Variant", "high", &[("high", "High")]),
                toggle("fast", "Fast"),
            ],
        );
        let ids: Vec<String> = picker_settings(&opencode)
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["fast", "variant"]);
        let pills: Vec<String> = pill_settings(&opencode).into_iter().map(|s| s.id).collect();
        assert_eq!(pills, vec!["variant", "fast"]);
    }

    #[test]
    fn effort_pills_group_fast_mode_and_service_tier() {
        let opus = model(
            "claude:opus-5",
            HarnessId::Claude,
            "Opus 5",
            vec![
                toggle("fast", "Fast"),
                select(
                    "effort",
                    "Reasoning",
                    "high",
                    &[("medium", "Medium"), ("high", "High")],
                ),
            ],
        );
        let pills = control_pills(&opus);
        assert_eq!(pills.len(), 1);
        let ControlPill::Select { setting, grouped } = &pills[0] else {
            panic!("effort is a select pill");
        };
        assert_eq!(setting.id, "effort");
        assert_eq!(
            grouped.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["fast"]
        );
        let mut menu = vec![setting.clone()];
        menu.extend(grouped.iter().cloned());
        assert_eq!(select_menu_label(&menu), "Effort and Fast");

        // Without an effort select, fast stays its own toggle pill.
        let composer = model(
            "cursor:c",
            HarnessId::Cursor,
            "C",
            vec![toggle("fast", "Fast")],
        );
        assert!(matches!(&control_pills(&composer)[0], ControlPill::Toggle(s) if s.id == "fast"));
    }

    #[test]
    fn menu_labels_join_with_commas_and_and() {
        let a = select("reasoningEffort", "Reasoning", "", &[]);
        let b = select("serviceTier", "Service Tier", "", &[]);
        let c = toggle("fast", "Fast");
        assert_eq!(select_menu_label(std::slice::from_ref(&a)), "Reasoning");
        assert_eq!(
            select_menu_label(&[a.clone(), b.clone()]),
            "Reasoning and Service Tier"
        );
        assert_eq!(
            select_menu_label(&[a, b, c]),
            "Reasoning, Service Tier, and Fast"
        );
    }

    #[test]
    fn groups_opencode_models_by_provider_in_first_seen_order() {
        let mut go = AgentModel::new("opencode:opencode-go/luna", HarnessId::Opencode, "Luna");
        go.provider = Some(ModelProvider {
            id: "opencode-go".into(),
            name: "OpenCode Go".into(),
        });
        let mut openai = AgentModel::new("opencode:openai/luna", HarnessId::Opencode, "Luna");
        openai.provider = Some(ModelProvider {
            id: "openai".into(),
            name: "OpenAI".into(),
        });
        let bare = AgentModel::new("opencode:glm", HarnessId::Opencode, "GLM");
        let models = vec![go.clone(), openai, go.clone(), bare];
        let groups = model_groups(ModelPickerTab::Harness(HarnessId::Opencode), &models);
        let names: Vec<_> = groups.iter().map(|g| g.name.clone().unwrap()).collect();
        assert_eq!(names, vec!["OpenCode Go", "OpenAI", "OpenCode"]);
        assert_eq!(
            groups[0].models.iter().map(|m| m.1).collect::<Vec<_>>(),
            vec![0, 2]
        );
        let flat = model_groups(ModelPickerTab::Favorites, &models);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, None);
    }

    #[test]
    fn trigger_labels_name_the_provider_model_and_effort() {
        let catalog = ModelCatalog::new();
        let grok = catalog.resolve_model(HarnessId::Grok, Some("grok:grok-4.6"));
        let values = ModelSettings::from([("effort".to_string(), "high".to_string())]);
        let effort = trigger_effort_label(&grok, &values, false);
        assert_eq!(effort.as_deref(), Some("High"));
        assert_eq!(
            trigger_label(&grok, effort.as_deref()),
            "Grok Build Grok 4.6, effort High"
        );
        assert_eq!(
            trigger_title(&grok, effort.as_deref()),
            "Grok Build · Grok 4.6 · High"
        );
        assert_eq!(trigger_effort_label(&grok, &values, true), None);
        assert_eq!(trigger_label(&grok, None), "Grok Build Grok 4.6");
    }

    #[test]
    fn search_matches_names_harness_titles_and_providers() {
        let source = LocalModelSource::new(ModelCatalog::new(), all_available());
        let harnesses = HARNESSES.to_vec();
        let grok = ModelPickerTab::Harness(HarnessId::Grok);
        let all = visible_models(grok, &[], &harnesses, &source, "");
        assert!(!all.is_empty());
        assert_eq!(
            visible_models(grok, &[], &harnesses, &source, "  grok build ").len(),
            all.len()
        );
        assert!(visible_models(grok, &[], &harnesses, &source, "zzz").is_empty());
        let favorites = vec!["grok:grok-4.6".to_string(), "missing:model".to_string()];
        let picked = visible_models(
            ModelPickerTab::Favorites,
            &favorites,
            &harnesses,
            &source,
            "",
        );
        assert_eq!(picked.len(), 1);
        // A favorite whose provider tab is hidden drops out.
        assert!(visible_models(ModelPickerTab::Favorites, &favorites, &[], &source, "").is_empty());
    }

    #[test]
    fn empty_messages_follow_the_tab_and_availability() {
        let source = LocalModelSource::new(ModelCatalog::new(), all_available());
        assert_eq!(
            empty_models_message(ModelPickerTab::Favorites, "", &source),
            "No favorite models"
        );
        assert_eq!(
            empty_models_message(ModelPickerTab::Harness(HarnessId::Codex), "", &source),
            "Loading Codex models…"
        );
        assert_eq!(
            empty_models_message(ModelPickerTab::Harness(HarnessId::Codex), "x", &source),
            "No matching models"
        );
        let none = LocalModelSource::new(ModelCatalog::new(), Default::default())
            .with_unavailable_hint(|id| format!("{id} missing"));
        assert_eq!(
            empty_models_message(ModelPickerTab::Harness(HarnessId::Pi), "", &none),
            "pi missing"
        );
    }

    #[test]
    fn recent_menu_keeps_known_choices_and_the_current_model() {
        let source = LocalModelSource::new(ModelCatalog::new(), all_available());
        let grok = source.resolve(HarnessId::Grok, Some("grok:grok-4.6"));
        let recent = vec![
            LastModelChoice {
                harness: HarnessId::Claude,
                model: "claude:opus-5".into(),
            },
            LastModelChoice {
                harness: HarnessId::Cursor,
                model: "claude:opus-5".into(),
            },
            LastModelChoice {
                harness: HarnessId::Cursor,
                model: "cursor:composer-2.5".into(),
            },
        ];
        let ids: Vec<String> = recent_menu_models(&grok, &recent, &source)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(
            ids,
            vec!["claude:opus-5", "cursor:composer-2.5", "grok:grok-4.6"]
        );
    }

    #[test]
    fn provider_rail_follows_availability_hidden_providers_and_projects() {
        let source = LocalModelSource::new(ModelCatalog::new(), all_available());
        let mut prefs = ModelPrefs::default();
        prefs.set_picker_provider_visible(HarnessId::Pi, false);
        let mut projects = ProjectProviders::default();
        projects.set_project_provider_hidden("/repo", HarnessId::Droid, true);
        let rail = picker_harnesses(&source, &prefs, &projects, Some("/repo"), None);
        assert!(!rail.contains(&HarnessId::Pi));
        assert!(!rail.contains(&HarnessId::Droid));
        assert!(rail.contains(&HarnessId::Claude));
        let only = picker_harnesses(&source, &prefs, &projects, None, Some(&[HarnessId::Codex]));
        assert_eq!(only, vec![HarnessId::Codex]);
    }
}
