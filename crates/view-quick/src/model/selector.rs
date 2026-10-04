//! The data half of src/features/quick-composer/ui/QuickModelSelector.tsx,
//! plus the model helpers of quickComposer.ts the panel uses
//! (`filterQuickModels`, `resolveQuickModel`, `filterQuickProjects`).

use monocode_core::block::ModelSettings;
use monocode_core::js;
use monocode_core::models::{
    AgentModel, LastModelChoice, ModelCatalog, ModelPickerTab, ModelPrefs, ModelSetting,
    ModelSettingKind, model_effort_setting,
};
use monocode_core::{HARNESSES, HarnessId};
use monocode_layout::paths::{pretty_cwd, project_name};

/// `filterQuickModels`: the model, its upstream provider, or its harness.
pub fn filter_quick_models(models: &[AgentModel], query: &str) -> Vec<AgentModel> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return models.to_vec();
    }
    models
        .iter()
        .filter(|model| {
            [
                model.name.as_str(),
                model.id.as_str(),
                model
                    .provider
                    .as_ref()
                    .map_or("", |provider| provider.name.as_str()),
                model.harness.title(),
            ]
            .join(" ")
            .to_lowercase()
            .contains(&needle)
        })
        .cloned()
        .collect()
}

/// `resolveQuickModel`: keep a live-only model while its catalog loads.
pub fn resolve_quick_model(catalog: &ModelCatalog, choice: &LastModelChoice) -> Option<AgentModel> {
    if !catalog.has_live_catalog(choice.harness)
        && !catalog.models_for(choice.harness).iter().any(|model| {
            model.id == choice.model || model.native_id.as_deref() == Some(choice.model.as_str())
        })
    {
        return None;
    }
    let resolved = catalog.resolve_model(choice.harness, Some(&choice.model));
    (resolved.harness == choice.harness).then_some(resolved)
}

/// The placeholder model while the choice's catalog loads:
/// `{ ...choice, id: choice.model, name: "Loading model…" }`.
pub fn loading_model(choice: &LastModelChoice) -> AgentModel {
    AgentModel::new(&choice.model, choice.harness, "Loading model…")
}

/// `/^[A-Za-z]:$/`.
fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `parentPath` in src/shared/lib/paths.ts.
///
/// TODO(port): copies `monocode_engine::submit::paths::parent_path`, which
/// this crate cannot depend on. Delete it when the helper moves to core.
pub fn parent_path(path: &str) -> String {
    let slashed = monocode_core::paths::slash(path);
    let trimmed = match slashed.trim_end_matches('/') {
        "" => "/",
        trimmed => trimmed,
    };
    // `//server/share` stays as it is.
    if let Some(rest) = trimmed.strip_prefix("//") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|part| !part.is_empty()) {
            return trimmed.to_string();
        }
    }
    if is_drive(trimmed) {
        return format!("{trimmed}/");
    }
    let Some(index) = trimmed.rfind('/').filter(|index| *index > 0) else {
        return "/".into();
    };
    let parent = &trimmed[..index];
    if is_drive(parent) {
        return format!("{parent}/");
    }
    parent.to_string()
}

/// `prettyParent`.
pub fn pretty_parent(path: &str) -> String {
    pretty_cwd(&parent_path(path))
}

/// `filterQuickProjects`: the project name or its parent folder.
pub fn filter_quick_projects(projects: &[String], query: &str) -> Vec<String> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return projects.to_vec();
    }
    projects
        .iter()
        .filter(|path| {
            project_name(path).to_lowercase().contains(&needle)
                || pretty_parent(path).to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// The providers the selector shows: installed ones the user has not
/// hidden. `available` is `None` until a workspace reported availability.
pub fn selector_providers(prefs: &ModelPrefs, available: Option<&[HarnessId]>) -> Vec<HarnessId> {
    HARNESSES
        .into_iter()
        .filter(|id| {
            prefs.show_provider_in_model_picker(
                *id,
                available.is_some_and(|list| list.contains(id)),
                available.is_some(),
            )
        })
        .collect()
}

/// `tabs`: Favorites, then the providers.
pub fn selector_tabs(providers: &[HarnessId]) -> Vec<ModelPickerTab> {
    std::iter::once(ModelPickerTab::Favorites)
        .chain(providers.iter().copied().map(ModelPickerTab::Harness))
        .collect()
}

/// `visibleTab`.
pub fn visible_tab(tab: ModelPickerTab, tabs: &[ModelPickerTab]) -> ModelPickerTab {
    if tabs.contains(&tab) {
        tab
    } else {
        ModelPickerTab::Favorites
    }
}

/// `pool`: the favorites that still resolve to a shown provider, or one
/// provider's models.
pub fn selector_pool(
    catalog: &ModelCatalog,
    tab: ModelPickerTab,
    favorites: &[String],
    providers: &[HarnessId],
) -> Vec<AgentModel> {
    match tab {
        ModelPickerTab::Favorites => favorites
            .iter()
            .filter_map(|id| catalog.find_model(id))
            .filter(|model| providers.contains(&model.harness))
            .cloned()
            .collect(),
        ModelPickerTab::Harness(harness) => catalog.models_for(harness).to_vec(),
    }
}

/// `enabled`: a model can be picked when its provider is available.
pub fn model_enabled(model: &AgentModel, available: Option<&[HarnessId]>) -> bool {
    available.is_some_and(|list| list.contains(&model.harness))
}

/// The row to highlight: the selected model, else the first.
pub fn active_row(models: &[AgentModel], selected: &str) -> usize {
    models
        .iter()
        .position(|item| item.id == selected)
        .unwrap_or(0)
}

/// `/^(effort|reasoning effort|reasoning)$/i`.
fn is_effort_label(label: &str) -> bool {
    matches!(
        label.to_lowercase().as_str(),
        "effort" | "reasoning effort" | "reasoning"
    )
}

/// `effort`: the reasoning effort select, by id or by label.
pub fn effort_setting(model: &AgentModel) -> Option<&ModelSetting> {
    model_effort_setting(model).or_else(|| {
        model.settings.as_deref()?.iter().find(|setting| {
            setting.kind == ModelSettingKind::Select && is_effort_label(&setting.label)
        })
    })
}

/// `valueFor`.
pub fn value_for<'a>(values: &'a ModelSettings, setting: &'a ModelSetting) -> &'a str {
    values
        .get(&setting.id)
        .map(String::as_str)
        .unwrap_or(&setting.value)
}

/// `effortIndex`: the current option, else the first.
pub fn effort_index(effort: &ModelSetting, values: &ModelSettings) -> usize {
    let current = value_for(values, effort);
    effort
        .options
        .iter()
        .position(|option| option.value == current)
        .unwrap_or(0)
}

/// The fast mode setting and its on and off values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastToggle {
    pub id: String,
    pub default: String,
    pub on: String,
    pub off: String,
}

fn matches_any(value: &str, choices: &[&str]) -> bool {
    let value = value.to_lowercase();
    choices.contains(&value.as_str())
}

/// `fast`, `fastOn`, `fastOff`: present only when both values exist
/// (`canToggleFast`).
pub fn fast_toggle(model: &AgentModel) -> Option<FastToggle> {
    let fast = model.settings.as_deref()?.iter().find(|setting| {
        setting.id == "fast"
            || setting.id == "serviceTier"
            || matches_any(&setting.label, &["fast", "fast mode"])
    })?;
    let on = fast.options.iter().find(|option| {
        matches_any(&option.value, &["true", "fast", "priority"])
            || matches_any(&option.label, &["on", "fast"])
    })?;
    let off = fast.options.iter().find(|option| {
        matches_any(&option.value, &["false", "default", "standard", "normal"])
            || matches_any(&option.label, &["off", "standard", "normal"])
    })?;
    Some(FastToggle {
        id: fast.id.clone(),
        default: fast.value.clone(),
        on: on.value.clone(),
        off: off.value.clone(),
    })
}

impl FastToggle {
    /// `fastEnabled`.
    pub fn enabled(&self, values: &ModelSettings) -> bool {
        values.get(&self.id).unwrap_or(&self.default) == &self.on
    }

    /// The values after the lightning button: flip on and off.
    pub fn toggled(&self, values: &ModelSettings) -> ModelSettings {
        let mut next = values.clone();
        let value = if self.enabled(values) {
            &self.off
        } else {
            &self.on
        };
        next.insert(self.id.clone(), value.clone());
        next
    }
}

/// `changeSetting`.
pub fn change_setting(values: &ModelSettings, id: &str, value: &str) -> ModelSettings {
    let mut next = values.clone();
    next.insert(id.to_string(), value.to_string());
    next
}

/// `resetSettings`: effort and fast mode go back to the saved defaults
/// (`preferredModelSettings`); other values stay. A default the model does
/// not have clears the value, as `{ effort: undefined }` did.
pub fn reset_settings(
    catalog: &ModelCatalog,
    model: &AgentModel,
    values: &ModelSettings,
    last_settings: &ModelSettings,
) -> ModelSettings {
    let defaults = catalog.preferred_model_settings(model, None, last_settings);
    let mut next = values.clone();
    let mut reset = |id: &str| match defaults.get(id) {
        Some(value) => {
            next.insert(id.to_string(), value.clone());
        }
        None => {
            next.remove(id);
        }
    };
    if let Some(effort) = effort_setting(model) {
        reset(&effort.id);
    }
    if let Some(fast) = fast_toggle(model) {
        reset(&fast.id);
    }
    next
}

/// The empty list's message.
pub fn empty_label(query: &str, tab: ModelPickerTab) -> &'static str {
    if !query.is_empty() {
        "No matching models"
    } else if tab == ModelPickerTab::Favorites {
        "No favorite models"
    } else {
        "Loading models…"
    }
}

/// The favorites after the star on `id`.
pub fn toggle_favorite(favorites: &[String], id: &str) -> Vec<String> {
    if favorites.iter().any(|item| item == id) {
        favorites
            .iter()
            .filter(|item| *item != id)
            .cloned()
            .collect()
    } else {
        let mut next = favorites.to_vec();
        next.push(id.to_string());
        next
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use monocode_core::HarnessId;
    use monocode_core::models::{AgentModel, ModelSetting, ModelSettingChoice, ModelSettingKind};

    fn choice(label: &str, value: &str) -> ModelSettingChoice {
        ModelSettingChoice {
            label: label.into(),
            value: value.into(),
        }
    }

    /// The Claude model of QuickModelSelector.test.ts.
    pub fn claude() -> AgentModel {
        let mut model = AgentModel::new("test-claude", HarnessId::Claude, "Test Claude");
        model.settings = Some(vec![
            ModelSetting {
                id: "effort".into(),
                label: "Effort".into(),
                kind: ModelSettingKind::Select,
                value: "low".into(),
                options: vec![choice("Low", "low"), choice("High", "high")],
                description: None,
            },
            ModelSetting {
                id: "fast".into(),
                label: "Fast".into(),
                kind: ModelSettingKind::Toggle,
                value: "false".into(),
                options: vec![choice("Off", "false"), choice("On", "true")],
                description: None,
            },
        ]);
        model
    }

    pub fn grok() -> AgentModel {
        AgentModel::new("test-grok", HarnessId::Grok, "Test Grok")
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{claude, grok};
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> ModelSettings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn catalog() -> ModelCatalog {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(HarnessId::Claude, vec![claude()]);
        catalog.set_harness_models(HarnessId::Grok, vec![grok()]);
        catalog
    }

    #[test]
    fn filters_models_by_name_provider_and_harness() {
        let models = vec![claude(), grok()];
        assert_eq!(filter_quick_models(&models, " ").len(), 2);
        assert_eq!(
            filter_quick_models(&models, "grok build")[0].id,
            "test-grok"
        );
        assert_eq!(
            filter_quick_models(&models, "CLAUDE CODE")[0].id,
            "test-claude"
        );
        assert!(filter_quick_models(&models, "nothing").is_empty());
    }

    #[test]
    fn shows_favorites_and_available_providers() {
        let prefs = ModelPrefs::default();
        let providers = selector_providers(&prefs, Some(&[HarnessId::Claude, HarnessId::Grok]));
        assert_eq!(providers, [HarnessId::Claude, HarnessId::Grok]);
        assert_eq!(selector_tabs(&providers).len(), 3);
        // Before the first probe every provider shows.
        assert_eq!(selector_providers(&prefs, None).len(), HARNESSES.len());
        let catalog = catalog();
        let favorites = vec!["test-claude".to_string(), "missing".to_string()];
        let pool = selector_pool(&catalog, ModelPickerTab::Favorites, &favorites, &providers);
        assert_eq!(pool.len(), 1);
        assert_eq!(
            selector_pool(
                &catalog,
                ModelPickerTab::Favorites,
                &favorites,
                &[HarnessId::Grok]
            )
            .len(),
            0
        );
        assert_eq!(
            visible_tab(
                ModelPickerTab::Harness(HarnessId::Codex),
                &selector_tabs(&providers)
            ),
            ModelPickerTab::Favorites
        );
    }

    #[test]
    fn reads_effort_and_fast_mode() {
        let model = claude();
        let effort = effort_setting(&model).unwrap();
        assert_eq!(effort_index(effort, &settings(&[("effort", "high")])), 1);
        assert_eq!(effort_index(effort, &settings(&[("effort", "max")])), 0);
        let fast = fast_toggle(&model).unwrap();
        assert!(!fast.enabled(&settings(&[("effort", "low")])));
        assert_eq!(
            fast.toggled(&settings(&[("effort", "low")])),
            settings(&[("effort", "low"), ("fast", "true")])
        );
        assert!(fast_toggle(&grok()).is_none());
    }

    #[test]
    fn finds_effort_by_label_when_the_id_is_unusual() {
        let mut model = claude();
        if let Some(settings) = model.settings.as_mut() {
            settings[0].id = "level".into();
            settings[0].label = "Reasoning Effort".into();
        }
        assert_eq!(effort_setting(&model).unwrap().id, "level");
    }

    #[test]
    fn resets_to_saved_defaults_and_keeps_other_values() {
        let catalog = catalog();
        let next = reset_settings(
            &catalog,
            &claude(),
            &settings(&[("effort", "low"), ("fast", "true"), ("context", "256k")]),
            &settings(&[("effort", "high"), ("fast", "false")]),
        );
        assert_eq!(
            next,
            settings(&[("effort", "high"), ("fast", "false"), ("context", "256k")])
        );
    }

    #[test]
    fn resolves_only_models_the_catalog_knows() {
        let catalog = catalog();
        let choice = LastModelChoice {
            harness: HarnessId::Claude,
            model: "test-claude".into(),
        };
        assert_eq!(
            resolve_quick_model(&catalog, &choice).unwrap().name,
            "Test Claude"
        );
        let missing = LastModelChoice {
            harness: HarnessId::Codex,
            model: "live-only".into(),
        };
        assert!(resolve_quick_model(&ModelCatalog::new(), &missing).is_none());
        assert_eq!(loading_model(&missing).name, "Loading model…");
    }

    #[test]
    fn filters_projects_by_name_or_parent() {
        let projects = vec![
            "/Users/me/code/agent-terminal".to_string(),
            "/Users/me/work/site".to_string(),
        ];
        assert_eq!(pretty_parent("/Users/me/code/agent-terminal"), "~/code");
        assert_eq!(filter_quick_projects(&projects, "agent").len(), 1);
        assert_eq!(filter_quick_projects(&projects, "~/work")[0], projects[1]);
        assert_eq!(filter_quick_projects(&projects, "").len(), 2);
    }

    #[test]
    fn stars_toggle_favorites() {
        let favorites = toggle_favorite(&[], "test-claude");
        assert_eq!(favorites, ["test-claude"]);
        assert!(toggle_favorite(&favorites, "test-claude").is_empty());
        assert_eq!(
            empty_label("", ModelPickerTab::Favorites),
            "No favorite models"
        );
        assert_eq!(
            empty_label("x", ModelPickerTab::Favorites),
            "No matching models"
        );
        assert_eq!(
            empty_label("", ModelPickerTab::Harness(HarnessId::Claude)),
            "Loading models…"
        );
    }
}
