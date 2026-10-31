// The pickers that open as flyouts beside a menu row: ModelPicker.tsx's
// ModelFlyout and SearchableProjectPicker.tsx's ProjectPickerPopover. Each
// owns a search field; arrows and Enter in it move and pick in the list.
use crate::{input, theme::*, workspace::project_name};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, Role, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use monocode_core::catalog::{self, Model};
use monocode_core::provider::{self, Kind};

/// Friendly names used before a provider's live catalog arrives.
pub(crate) const CLAUDE_MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-5", "Claude Sonnet 5"),
    ("claude-opus-5", "Claude Opus 5"),
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-fable-5", "Claude Fable 5"),
    ("claude-opus-4-6", "Opus 4.6"),
    ("claude-sonnet-4-6", "Sonnet 4.6"),
    ("claude-haiku-4-5", "Haiku 4.5"),
    ("claude-opus-4-5", "Opus 4.5"),
];

/// The display name for a model id; `None` is the provider's default.
pub(crate) fn model_name(kind: Kind, model: Option<&str>) -> SharedString {
    if model.is_some_and(contains_account_id) {
        return kind.label().into();
    }
    match model {
        None => kind.label().into(),
        Some("sonnet") => "Sonnet".into(),
        Some("opus") => "Opus".into(),
        Some("haiku") => "Haiku".into(),
        Some(id) => CLAUDE_MODELS
            .iter()
            .find(|(model, _)| *model == id)
            .map_or_else(
                || {
                    if let Some(name) = id.strip_prefix("gpt-") {
                        let parts = name
                            .split('-')
                            .map(|part| match part {
                                "codex" => "Codex".into(),
                                "sol" => "Sol".into(),
                                "astra" => "Astra".into(),
                                "mini" => "Mini".into(),
                                "max" => "Max".into(),
                                other => other.to_owned(),
                            })
                            .collect::<Vec<String>>()
                            .join("-");
                        format!("GPT-{parts}").into()
                    } else {
                        id.to_owned().into()
                    }
                },
                |(_, name)| (*name).into(),
            ),
    }
}

/// Account metadata is never a model label, including stale fixture responses.
pub(crate) fn model_display_name(kind: Kind, id: &str, name: &str) -> SharedString {
    if name.trim().is_empty() || contains_account_id(name) {
        model_name(kind, Some(id))
    } else {
        name.to_owned().into()
    }
}
/// A model's name from a cached catalog, else its friendly fallback.
pub(crate) fn cached_model_name(kind: Kind, id: Option<&str>) -> SharedString {
    id.and_then(|id| catalog::cached_name(kind, id).map(|name| model_display_name(kind, id, &name)))
        .unwrap_or_else(|| model_name(kind, id))
}
/// saveRecentModelChoice: the pick moves to the front of the six recents,
/// and becomes the provider's last used model for new chats.
pub(crate) fn remember_model(kind: Kind, model: Option<String>, cx: &mut gpui::App) {
    if cx.has_global::<crate::settings::Preferences>() {
        cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
            prefs.last_models.insert(kind.id().into(), model.clone());
            let choice = crate::settings::ModelChoice {
                provider: kind.id().into(),
                model,
            };
            prefs.recent_models.retain(|old| old != &choice);
            prefs.recent_models.insert(0, choice);
            prefs.recent_models.truncate(6);
        });
    }
}
/// The provider and model that write `task`'s text: the chosen one, else
/// `fallback`'s default.
pub(crate) fn text_task_choice(
    task: crate::settings::TextTask,
    fallback: Kind,
    cx: &gpui::App,
) -> (Kind, Option<String>) {
    cx.try_global::<crate::settings::Preferences>()
        .and_then(|prefs| prefs.text_model_for(task))
        .map_or((fallback, None), |choice| {
            (Kind::from_id(&choice.provider), choice.model.clone())
        })
}
fn contains_account_id(value: &str) -> bool {
    value.match_indices("account-").any(|(offset, _)| {
        let tail = &value[offset + "account-".len()..];
        let end = tail
            .find(|c: char| !c.is_ascii_hexdigit() && c != '-')
            .unwrap_or(tail.len());
        let token = tail[..end].trim_end_matches('-');
        let canonical = token.len() == 36
            && token.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            });
        (token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())) || canonical
    })
}

#[cfg(test)]
mod model_label_tests {
    use super::*;

    #[gpui::test]
    fn remote_model_availability_comes_from_the_host(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let list = cx.new(|cx| {
                ModelList::new_for(
                    Kind::Codex,
                    None,
                    "remote://env/home/code",
                    Default::default(),
                    cx,
                )
                .hide_providers(vec!["claude".into()])
            });
            list.update(cx, |list, cx| {
                list.remote_providers = Some(vec![Kind::Claude]);
                list.missing = vec![Kind::Codex];
                list.tab = ModelTab::Harness(Kind::Claude);
                assert!(list.hidden.is_empty());
                assert!(list.enabled(Kind::Claude));
                assert!(!list.enabled(Kind::Codex));
                assert!(!list.enabled(Kind::Cursor));
                // Until its host catalog arrives, no local Claude aliases
                // are advertised as available remote models.
                let entries = list.entries(cx);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].id, None);
            });
        });
    }

    #[gpui::test]
    fn the_text_model_picker_opens_on_the_chosen_model(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences {
                commit_model: Some(crate::settings::ModelChoice {
                    provider: "codex".into(),
                    model: Some("gpt-mini".into()),
                }),
                ..Default::default()
            });
            let commit = cx.new(|cx| {
                ModelList::for_text_task(
                    crate::settings::TextTask::Commit,
                    Kind::Claude,
                    "/work",
                    cx,
                )
            });
            assert_eq!(commit.read(cx).provider, Kind::Codex);
            assert_eq!(commit.read(cx).current.as_deref(), Some("gpt-mini"));
            // Other tasks are unaffected and open on the fallback's default.
            let title = cx.new(|cx| {
                ModelList::for_text_task(
                    crate::settings::TextTask::Title,
                    Kind::Claude,
                    "/work",
                    cx,
                )
            });
            assert_eq!(title.read(cx).provider, Kind::Claude);
            assert_eq!(title.read(cx).current, None);
        });
    }

    #[gpui::test]
    fn a_pick_becomes_the_providers_last_model(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences::default());
            remember_model(Kind::Claude, Some("sonnet".into()), cx);
            remember_model(Kind::Codex, Some("gpt-mini".into()), cx);
            remember_model(Kind::Claude, None, cx);
            let prefs = cx.global::<crate::settings::Preferences>();
            assert_eq!(prefs.last_models.get("claude"), Some(&None));
            assert_eq!(
                prefs.model_for_new_chat("codex", "/work").as_deref(),
                Some("gpt-mini")
            );
        });
    }

    #[gpui::test]
    fn hidden_models_leave_the_picker_until_searched(cx: &mut gpui::TestAppContext) {
        let model = |id: &str| Model {
            id: id.into(),
            name: id.into(),
            alias_target: None,
            efforts: Vec::new(),
            default_effort: None,
            fast: false,
            thinking: false,
            default: false,
            tiers: Vec::new(),
            controls: Vec::new(),
        };
        let choice = |id: &str| crate::settings::ModelChoice {
            provider: "codex".into(),
            model: Some(id.into()),
        };
        cx.update(|cx| {
            let mut prefs = crate::settings::Preferences {
                hidden_models: vec![choice("gpt-global"), choice("gpt-current")],
                ..Default::default()
            };
            prefs
                .project_providers
                .entry("/work".into())
                .or_default()
                .hidden_models = vec![choice("gpt-project")];
            cx.set_global(prefs);
            let list = cx.new(|cx| {
                ModelList::new_for(
                    Kind::Codex,
                    Some("gpt-current".into()),
                    "/work",
                    Default::default(),
                    cx,
                )
            });
            list.update(cx, |list, _| {
                list.catalogs = vec![(
                    Kind::Codex,
                    ["gpt-global", "gpt-project", "gpt-shown", "gpt-current"]
                        .map(model)
                        .to_vec(),
                )];
            });
            let ids = |list: &Entity<ModelList>, cx: &gpui::App| {
                list.read(cx)
                    .entries(cx)
                    .into_iter()
                    .map(|e| (e.id, e.hidden))
                    .collect::<Vec<_>>()
            };
            // The session's own model stays even though it is hidden.
            assert_eq!(
                ids(&list, cx),
                [
                    (None, false),
                    (Some("gpt-shown".into()), false),
                    (Some("gpt-current".into()), true),
                ]
            );
            // A search reveals hidden matches after the visible ones.
            let search = list.read(cx).search.clone();
            search.update(cx, |input, cx| input.set_text("gpt".into(), cx));
            assert_eq!(
                ids(&list, cx),
                [
                    (Some("gpt-shown".into()), false),
                    (Some("gpt-current".into()), true),
                    (Some("gpt-global".into()), true),
                    (Some("gpt-project".into()), true),
                ]
            );
        });
    }

    #[test]
    fn account_metadata_never_becomes_a_model_label() {
        for id in [
            "account-de37caea9106642f26b59835a2e9aa4f",
            "fixture-account-de37caea-9106-642f-26b5-9835a2e9aa4f",
        ] {
            let label = format!("{id} model");
            assert_eq!(model_name(Kind::Codex, Some(id)).as_ref(), "Codex");
            assert_eq!(
                model_display_name(Kind::Codex, id, &label).as_ref(),
                "Codex"
            );
            assert_eq!(
                model_display_name(Kind::Codex, "gpt-6.1-sol", &label).as_ref(),
                "GPT-6.1-Sol"
            );
        }
        assert_eq!(
            model_display_name(Kind::Codex, "custom-model", "My custom model").as_ref(),
            "My custom model"
        );
        assert_eq!(
            model_name(Kind::Codex, Some("custom-model")).as_ref(),
            "custom-model"
        );
    }

    #[gpui::test]
    fn remembered_favorites_and_recents_keep_provider_identity_and_busy_rules(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let prefs = crate::settings::Preferences {
                model_picker_tab: Some("favorites".into()),
                favorite_models: vec![
                    crate::settings::ModelChoice {
                        provider: "codex".into(),
                        model: Some("gpt-saved".into()),
                    },
                    crate::settings::ModelChoice {
                        provider: "claude".into(),
                        model: Some("opus".into()),
                    },
                ],
                recent_models: vec![crate::settings::ModelChoice {
                    provider: "codex".into(),
                    model: Some("gpt-recent".into()),
                }],
                ..Default::default()
            };
            cx.set_global(prefs);
            let list = cx.new(|cx| {
                ModelList::new_for(
                    Kind::Codex,
                    Some("gpt-saved".into()),
                    "/work",
                    Default::default(),
                    cx,
                )
                .allow_provider_switch(false)
            });
            let favorites = list.read(cx).entries(cx);
            assert_eq!(favorites.len(), 2);
            assert_eq!(favorites[0].kind, Kind::Codex);
            assert!(favorites[0].enabled);
            assert_eq!(favorites[1].kind, Kind::Claude);
            assert!(!favorites[1].enabled);
            list.update(cx, |list, _| list.tab = ModelTab::Harness(Kind::Codex));
            let rows = list.read(cx).entries(cx);
            assert_eq!(rows[1].id.as_deref(), Some("gpt-recent"));
            assert_eq!(rows[2].id.as_deref(), Some("gpt-saved"));
            assert_eq!(list.read(cx).current_index(cx), 2);
        });
    }
}

fn search_field<T: 'static>(
    label: &'static str,
    placeholder: &'static str,
    placeholder_alpha: f32,
    cx: &mut Context<T>,
) -> Entity<input::Input> {
    cx.new(|cx| input::Input::field(label, placeholder, content(placeholder_alpha), cx))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelTab {
    Favorites,
    Harness(Kind),
}

struct ModelEntry {
    kind: Kind,
    id: Option<String>,
    name: SharedString,
    enabled: bool,
    /// Hidden in Settings; listed only while a search names it.
    hidden: bool,
}

/// The model the next turn uses; `None` is the provider's default.
pub(crate) struct ModelPicked(pub Kind, pub Option<String>);

/// ModelFlyout: providers down a rail on the left, a search field over the
/// selected provider's models on the right. Switching providers is available
/// between turns.
pub(crate) struct ModelList {
    provider: Kind,
    project: String,
    remote_project: Option<String>,
    remote_providers: Option<Vec<Kind>>,
    current: Option<String>,
    profiles: std::collections::HashMap<Kind, monocode_core::accounts::Profile>,
    catalogs: Vec<(Kind, Vec<Model>)>,
    errors: Vec<(Kind, String)>,
    loading: Vec<Kind>,
    missing: Vec<Kind>,
    switch_allowed: bool,
    hidden: Vec<String>,
    /// QuickModelSelector.tsx: providers as tabs above the search and list.
    quick: bool,
    tab: ModelTab,
    highlighted: usize,
    search: Entity<input::Input>,
    _changed: Subscription,
}
impl EventEmitter<ModelPicked> for ModelList {}
impl ModelList {
    pub(crate) fn new_for(
        provider: Kind,
        current: Option<String>,
        project: &str,
        profile: monocode_core::accounts::Profile,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut profiles = crate::accounts::get(cx)
            .map(|center| {
                Kind::ALL
                    .into_iter()
                    .map(|kind| (kind, center.read(cx).selected(kind, project)))
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default();
        profiles.insert(provider, profile);
        let search = search_field("Search models", "Search models", 0.4, cx);
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.highlighted = this.current_index(cx);
            cx.notify();
        });
        let mut list = Self {
            provider,
            project: project.to_owned(),
            remote_project: monocode_core::remote::parse_remote_path(project)
                .map(|_| project.to_owned()),
            remote_providers: None,
            current,
            profiles,
            catalogs: Vec::new(),
            errors: Vec::new(),
            loading: Vec::new(),
            missing: Vec::new(),
            switch_allowed: true,
            hidden: Vec::new(),
            quick: false,
            tab: cx
                .try_global::<crate::settings::Preferences>()
                .and_then(|prefs| prefs.model_picker_tab.as_deref())
                .map(|tab| {
                    if tab == "favorites" {
                        ModelTab::Favorites
                    } else {
                        ModelTab::Harness(Kind::parse(tab).unwrap_or(provider))
                    }
                })
                .unwrap_or(ModelTab::Harness(provider)),
            highlighted: 0,
            search,
            _changed: changed,
        };
        list.highlighted = list.current_index(cx);
        list.probe_installed(cx);
        list.discover(
            match list.tab {
                ModelTab::Harness(kind) => kind,
                ModelTab::Favorites => provider,
            },
            cx,
        );
        list
    }
    /// The normal model picker, opened on the model that writes `task`'s
    /// text, or on `fallback`'s default while none is chosen.
    pub(crate) fn for_text_task(
        task: crate::settings::TextTask,
        fallback: Kind,
        project: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let choice = cx
            .try_global::<crate::settings::Preferences>()
            .and_then(|prefs| prefs.text_model_for(task))
            .cloned();
        Self::for_choice(choice.as_ref(), fallback, project, cx)
    }
    /// The normal model picker, opened on `choice`, or on `fallback`'s default.
    pub(crate) fn for_choice(
        choice: Option<&crate::settings::ModelChoice>,
        fallback: Kind,
        project: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let (kind, model) = choice.map_or((fallback, None), |c| {
            (Kind::from_id(&c.provider), c.model.clone())
        });
        let profile = crate::accounts::get(cx)
            .map(|center| center.read(cx).selected(kind, project))
            .unwrap_or_default();
        let hidden = cx
            .try_global::<crate::settings::Preferences>()
            .map(|prefs| prefs.hidden_for(project))
            .unwrap_or_default();
        Self::new_for(kind, model, project, profile, cx)
            .hide_providers(hidden)
            .allow_provider_switch(true)
    }
    /// Hides `more` providers on top of the ones already hidden.
    pub(crate) fn also_hide_providers(mut self, more: impl IntoIterator<Item = String>) -> Self {
        let mut hidden = std::mem::take(&mut self.hidden);
        hidden.extend(more);
        self.hide_providers(hidden)
    }
    pub(crate) fn hide_providers(mut self, hidden: Vec<String>) -> Self {
        if self.remote_project.is_some() {
            return self;
        }
        if let ModelTab::Harness(kind) = self.tab
            && kind != self.provider
            && hidden.iter().any(|id| id == kind.id())
        {
            self.tab = ModelTab::Favorites;
        }
        self.hidden = hidden;
        self
    }
    /// The quick composer's inline selector: providers in a row of tabs,
    /// a favorite star on every model, and no hover-to-switch.
    pub(crate) fn quick(mut self, cx: &mut Context<Self>) -> Self {
        self.quick = true;
        self.search.update(cx, |search, cx| {
            search.set_placeholder("Search models…", cx)
        });
        self
    }
    /// Follow a pick made through this list, keeping it open.
    pub(crate) fn set_current(
        &mut self,
        kind: Kind,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.provider = kind;
        self.current = model;
        cx.notify();
    }
    /// The live catalog entry behind a choice, once discovery has run.
    pub(crate) fn catalog_model(&self, kind: Kind, id: Option<&str>) -> Option<&Model> {
        let models = &self.catalogs.iter().find(|(k, _)| *k == kind)?.1;
        match id {
            Some(id) => models.iter().find(|m| m.id == id),
            None => models.iter().find(|m| m.default).or(models.first()),
        }
    }
    fn favorite(&self, kind: Kind, id: Option<&str>, cx: &gpui::App) -> bool {
        cx.try_global::<crate::settings::Preferences>()
            .is_some_and(|prefs| {
                prefs
                    .favorite_models
                    .iter()
                    .any(|c| c.provider == kind.id() && c.model.as_deref() == id)
            })
    }
    fn toggle_favorite(&mut self, kind: Kind, id: Option<String>, cx: &mut Context<Self>) {
        if !cx.has_global::<crate::settings::Preferences>() {
            return;
        }
        cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
            let before = prefs.favorite_models.len();
            prefs
                .favorite_models
                .retain(|c| !(c.provider == kind.id() && c.model == id));
            if prefs.favorite_models.len() == before {
                prefs.favorite_models.push(crate::settings::ModelChoice {
                    provider: kind.id().into(),
                    model: id,
                });
            }
        });
        cx.notify();
    }
    pub(crate) fn allow_provider_switch(mut self, allowed: bool) -> Self {
        self.switch_allowed = allowed;
        self
    }
    /// Like the Tauri picker, hide providers whose CLI isn't installed once the
    /// probe settles. The session's own provider always stays.
    fn probe_installed(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || self.remote_project.is_some() {
            return;
        }
        let work = cx.background_executor().spawn(async move {
            Kind::ALL
                .into_iter()
                .filter(|kind| monocode_core::provider::resolve(*kind).is_none())
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let missing = work.await;
            let _ = this.update(cx, |this, cx| {
                for kind in missing {
                    if !this.missing.contains(&kind) {
                        this.missing.push(kind);
                    }
                }
                if let ModelTab::Harness(selected) = this.tab
                    && selected != this.provider
                    && this.missing.contains(&selected)
                {
                    this.tab = ModelTab::Harness(this.provider);
                }
                if this.query(cx).is_empty() {
                    this.highlighted = this.current_index(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn discover(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if cfg!(test) || self.loading.contains(&kind) {
            return;
        }
        if let Some(project) = self.remote_project.clone() {
            self.loading = Kind::ALL.to_vec();
            let work = cx
                .background_executor()
                .spawn(async move { catalog::remote_catalog(&project) });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.loading.clear();
                    match result {
                        Ok(catalog) => {
                            this.missing = Kind::ALL
                                .into_iter()
                                .filter(|kind| !catalog.providers.contains(kind))
                                .collect();
                            this.remote_providers = Some(catalog.providers);
                            this.catalogs = catalog.models.into_iter().collect();
                            this.errors = catalog.errors.into_iter().collect();
                            if let ModelTab::Harness(selected) = this.tab
                                && this.missing.contains(&selected)
                                && selected != this.provider
                            {
                                this.tab = ModelTab::Harness(this.provider);
                            }
                        }
                        Err(error) => {
                            this.errors = Kind::ALL
                                .into_iter()
                                .map(|kind| (kind, error.clone()))
                                .collect();
                        }
                    }
                    this.highlighted = this.current_index(cx);
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        self.loading.push(kind);
        self.errors.retain(|(k, _)| *k != kind);
        let work = cx.background_executor().spawn({
            let profile = self.profiles.get(&kind).cloned().unwrap_or_default();
            async move {
                let result = catalog::models_for(kind, &profile);
                // Missing is a fact about the binary, not about the message it
                // produced. Resolve on the worker so the UI never probes paths.
                let missing = result.is_err() && provider::resolve(kind).is_none();
                (result, missing)
            }
        });
        cx.spawn(async move |this, cx| {
            let (result, missing) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading.retain(|k| *k != kind);
                match result {
                    Ok(models) => {
                        this.catalogs.retain(|(k, _)| *k != kind);
                        this.catalogs.push((kind, models));
                    }
                    Err(error) => this.errors.push((kind, error)),
                }
                this.missing.retain(|k| *k != kind);
                if missing {
                    this.missing.push(kind);
                }
                if this.query(cx).is_empty() {
                    this.highlighted = this.current_index(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn search_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
    fn query(&self, cx: &gpui::App) -> String {
        self.search.read(cx).content.to_lowercase()
    }
    fn entries(&self, cx: &gpui::App) -> Vec<ModelEntry> {
        let ModelTab::Harness(kind) = self.tab else {
            let query = self.query(cx);
            let entries = cx
                .try_global::<crate::settings::Preferences>()
                .into_iter()
                .flat_map(|prefs| &prefs.favorite_models)
                .filter_map(|choice| {
                    let kind = Kind::parse(&choice.provider)?;
                    if kind != self.provider && self.hidden.iter().any(|id| id == kind.id()) {
                        return None;
                    }
                    let name = self.saved_model_name(kind, choice.model.as_deref());
                    let name: SharedString = format!("{} · {}", kind.label(), name).into();
                    if !query.is_empty() && !name.to_lowercase().contains(&query) {
                        return None;
                    }
                    Some(ModelEntry {
                        kind,
                        id: choice.model.clone(),
                        name,
                        enabled: self.enabled(kind),
                        hidden: false,
                    })
                })
                .collect();
            return self.without_hidden(entries, &query, cx);
        };
        let enabled = self.enabled(kind);
        let mut entries = vec![ModelEntry {
            kind,
            id: None,
            name: "Default".into(),
            enabled,
            hidden: false,
        }];
        if let Some(prefs) = cx.try_global::<crate::settings::Preferences>() {
            for choice in &prefs.recent_models {
                if choice.provider == kind.id()
                    && choice.model.is_some()
                    && !entries.iter().any(|entry| entry.id == choice.model)
                {
                    entries.push(ModelEntry {
                        kind,
                        id: choice.model.clone(),
                        name: self.saved_model_name(kind, choice.model.as_deref()),
                        enabled,
                        hidden: false,
                    });
                }
            }
        }
        if let Some((_, catalog)) = self.catalogs.iter().find(|(k, _)| *k == kind) {
            let additional: Vec<_> = catalog
                .iter()
                .filter(|model| {
                    model.id != "default"
                        && !entries
                            .iter()
                            .any(|entry| entry.id.as_deref() == Some(model.id.as_str()))
                })
                .map(|model| ModelEntry {
                    kind,
                    id: Some(model.id.clone()),
                    name: model_display_name(kind, &model.id, &model.name),
                    enabled,
                    hidden: false,
                })
                .collect();
            entries.extend(additional);
        } else if kind == Kind::Claude && self.remote_project.is_none() {
            // Stable CLI aliases remain usable while discovery is loading.
            let additional: Vec<_> = [("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku")]
                .into_iter()
                .filter(|(id, _)| !entries.iter().any(|entry| entry.id.as_deref() == Some(*id)))
                .map(|(id, name)| ModelEntry {
                    kind,
                    id: Some(id.into()),
                    name: name.into(),
                    enabled,
                    hidden: false,
                })
                .collect();
            entries.extend(additional);
        }
        // An override from the environment that the catalog doesn't name.
        if kind == self.provider
            && let Some(id) = &self.current
            && !entries.iter().any(|e| e.id.as_ref() == Some(id))
        {
            entries.push(ModelEntry {
                kind,
                id: Some(id.clone()),
                name: model_name(kind, Some(id)),
                enabled,
                hidden: false,
            });
        }
        let query = self.query(cx);
        let entries = entries
            .into_iter()
            .filter(|e| {
                query.is_empty()
                    || format!("{} {}", e.name, kind.label())
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        self.without_hidden(entries, &query, cx)
    }
    /// Leave hidden models out, except the session's own and, while
    /// searching, the matches, which follow the visible ones.
    fn without_hidden(
        &self,
        entries: Vec<ModelEntry>,
        query: &str,
        cx: &gpui::App,
    ) -> Vec<ModelEntry> {
        let Some(prefs) = cx.try_global::<crate::settings::Preferences>() else {
            return entries;
        };
        let hidden = prefs.hidden_models_for(&self.project);
        if hidden.is_empty() {
            return entries;
        }
        let (mut visible, mut found) = (Vec::new(), Vec::new());
        for mut entry in entries {
            entry.hidden = entry.id.is_some()
                && hidden
                    .iter()
                    .any(|c| c.provider == entry.kind.id() && c.model == entry.id);
            let current = entry.kind == self.provider && entry.id == self.current;
            if !entry.hidden || current {
                visible.push(entry);
            } else if !query.is_empty() {
                found.push(entry);
            }
        }
        visible.extend(found);
        visible
    }
    fn enabled(&self, kind: Kind) -> bool {
        (kind == self.provider || self.switch_allowed)
            && !self.missing.contains(&kind)
            && (self.remote_project.is_none()
                || self
                    .remote_providers
                    .as_ref()
                    .is_some_and(|providers| providers.contains(&kind)))
    }
    fn saved_model_name(&self, kind: Kind, id: Option<&str>) -> SharedString {
        self.catalogs
            .iter()
            .find(|(provider, _)| *provider == kind)
            .and_then(|(_, models)| models.iter().find(|model| Some(model.id.as_str()) == id))
            .map(|model| model_display_name(kind, &model.id, &model.name))
            .unwrap_or_else(|| model_name(kind, id))
    }
    fn current_index(&self, cx: &gpui::App) -> usize {
        self.entries(cx)
            .iter()
            .position(|entry| entry.kind == self.provider && entry.id == self.current)
            .unwrap_or(0)
    }
    fn select_tab(&mut self, tab: ModelTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        if cx.has_global::<crate::settings::Preferences>() {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.model_picker_tab = Some(
                    match tab {
                        ModelTab::Favorites => "favorites",
                        ModelTab::Harness(kind) => kind.id(),
                    }
                    .into(),
                );
            });
        }
        if let ModelTab::Harness(kind) = tab
            && !self.catalogs.iter().any(|(k, _)| *k == kind)
            && !self.errors.iter().any(|(k, _)| *k == kind)
        {
            self.discover(kind, cx);
        }
        self.search.update(cx, |search, cx| {
            search.take(cx);
        });
        self.highlighted = self.current_index(cx);
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }
    fn step(&mut self, direction: isize, cx: &mut Context<Self>) {
        let count = self.entries(cx).len();
        if count > 0 {
            self.highlighted =
                (self.highlighted as isize + direction).clamp(0, count as isize - 1) as usize;
            cx.notify();
        }
    }
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries(cx).into_iter().nth(ix)
            && entry.enabled
        {
            remember_model(entry.kind, entry.id.clone(), cx);
            cx.emit(ModelPicked(entry.kind, entry.id));
        }
    }
    fn tab(&self, tab: ModelTab, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.tab == tab;
        let (id, label): (&'static str, SharedString) = match tab {
            ModelTab::Favorites => ("model-tab-favorites", "Favorites".into()),
            ModelTab::Harness(kind) => (
                if kind == Kind::Claude {
                    "model-tab-claude"
                } else if kind == Kind::Codex {
                    "model-tab-codex"
                } else {
                    kind.id()
                },
                kind.label().into(),
            ),
        };
        div()
            .id(id)
            .role(Role::Tab)
            .aria_label(label)
            .aria_selected(selected)
            .group(id)
            .cursor_pointer()
            .flex_shrink_0()
            .size(px(32.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .when(selected, |d| d.bg(content(0.12)))
            .when(!selected, |d| d.hover(|s| s.bg(content(0.08))))
            .child(match tab {
                ModelTab::Favorites => icon(if selected { "star-filled" } else { "star" }, 16.)
                    .text_color(content(if selected { 1. } else { 0.45 }))
                    .group_hover(id, |s| s.text_color(content(1.)))
                    .into_any_element(),
                ModelTab::Harness(kind) => provider_icon(kind.id(), 16.).into_any_element(),
            })
            // Hovering an unselected provider selects it, as in the rail.
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                if *hovered && !this.quick {
                    this.select_tab(tab, window, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.select_tab(tab, window, cx)))
            .into_any()
    }
}
impl Render for ModelList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.quick {
            return self.render_quick(cx);
        }
        let entries = self.entries(cx);
        let kind = match self.tab {
            ModelTab::Harness(kind) => Some(kind),
            _ => None,
        };
        let loading = kind.is_some_and(|kind| self.loading.contains(&kind));
        let error = kind.and_then(|kind| {
            self.errors
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, error)| error.clone())
        });
        let empty = match (self.tab, self.query(cx).is_empty()) {
            (ModelTab::Favorites, true) if entries.is_empty() => Some("No favorite models"),
            _ if entries.is_empty() => Some("No matching models"),
            _ => None,
        };
        let rows: Vec<AnyElement> = entries
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| {
                let highlighted = self.highlighted == ix && entry.enabled;
                let current = entry.kind == self.provider && entry.id == self.current;
                div()
                    .id(("model-option", ix))
                    .role(Role::ListBoxOption)
                    .aria_label(entry.name.clone())
                    .aria_selected(current)
                    .when(!entry.enabled, |d| {
                        d.aria_description(if self.missing.contains(&entry.kind) {
                            if self.remote_project.is_some() { "This provider is not installed on the connected machine. Install it there, then retry model discovery" } else { "This provider is not installed. Install it, then retry model discovery" }
                        } else {
                            "Wait for the current turn to finish before switching providers"
                        })
                    })
                    .cursor_pointer()
                    .flex_shrink_0()
                    .h(px(32.))
                    .px_1()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(13.))
                    .text_color(content(match (entry.enabled, entry.hidden) {
                        (false, _) => 0.3,
                        (true, true) => 0.5,
                        (true, false) => 1.,
                    }))
                    .when(highlighted, |d| d.bg(content(0.1)))
                    .when(!highlighted && entry.enabled, |d| {
                        d.hover(|s| s.bg(content(0.05)))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(6.))
                            .truncate()
                            .child(entry.name),
                    )
                    .when(entry.hidden, |d| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .px_1()
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child("Hidden"),
                        )
                    })
                    .when(current, |d| {
                        d.child(
                            div()
                                .size(px(24.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("check-2", 14.).text_color(content(0.55))),
                        )
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.highlighted = ix;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                    .into_any()
            })
            .collect();
        div()
            .id("model-flyout")
            .role(Role::Dialog)
            .aria_label("Models")
            .size_full()
            .min_h_0()
            .flex()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.highlighted, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id("model-providers")
                    .role(Role::TabList)
                    .aria_label("Providers")
                    .w(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .items_center()
                    .gap_1()
                    .p(px(6.))
                    .border_r_1()
                    .border_color(content(0.07))
                    .child(self.tab(ModelTab::Favorites, cx))
                    .children(
                        Kind::ALL
                            .into_iter()
                            .filter(|kind| {
                                if self.remote_project.is_some() {
                                    *kind == self.provider
                                        || self
                                            .remote_providers
                                            .as_ref()
                                            .is_some_and(|providers| providers.contains(kind))
                                } else {
                                    *kind == self.provider
                                        || (!self.hidden.iter().any(|id| id == kind.id())
                                            && !self.missing.contains(kind))
                                }
                            })
                            .map(|kind| self.tab(ModelTab::Harness(kind), cx)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py(px(10.))
                            .border_b_1()
                            .border_color(content(0.07))
                            .text_color(content(1.))
                            .child(icon("search", 14.).text_color(content(0.5)))
                            .child(div().flex_1().min_w_0().child(self.search.clone())),
                    )
                    .child(
                        div()
                            .id("model-list")
                            .role(Role::ListBox)
                            .aria_label("Models")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_1()
                            .flex()
                            .flex_col()
                            .when_some(empty, |d, text| {
                                d.child(
                                    div()
                                        .px_2()
                                        .py_3()
                                        .text_size(px(12.))
                                        .text_color(content(0.5))
                                        .child(text),
                                )
                            })
                            .children(rows)
                            .when(loading, |d| {
                                d.child(
                                    div()
                                        .px_2()
                                        .py_2()
                                        .text_size(px(12.))
                                        .text_color(content(0.5))
                                        .child("Loading models…"),
                                )
                            })
                            .when_some(error, |d, error| {
                                d.child(
                                    div()
                                        .px_2()
                                        .py_2()
                                        .text_size(px(12.))
                                        .text_color(content(0.5))
                                        .child(error)
                                        .child(
                                            div()
                                                .id("retry-models")
                                                .role(Role::Button)
                                                .aria_label("Retry model discovery")
                                                .py_2()
                                                .child("Retry")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(kind) = kind {
                                                        this.discover(kind, cx);
                                                        cx.notify();
                                                    }
                                                })),
                                        ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}
impl ModelList {
    fn render_quick(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entries = self.entries(cx);
        let kind = match self.tab {
            ModelTab::Harness(kind) => Some(kind),
            _ => None,
        };
        let loading = kind.is_some_and(|kind| self.loading.contains(&kind));
        let query = !self.query(cx).is_empty();
        let empty = entries.is_empty().then(|| {
            if query {
                "No matching models"
            } else if kind.is_none() {
                "No favorite models"
            } else if loading {
                "Loading models…"
            } else {
                "No models"
            }
        });
        let favorites = kind.is_none();
        let rows: Vec<AnyElement> = entries
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| {
                let highlighted = self.highlighted == ix;
                let current = entry.kind == self.provider && entry.id == self.current;
                let starred = self.favorite(entry.kind, entry.id.as_deref(), cx);
                let (star_kind, star_id) = (entry.kind, entry.id.clone());
                div()
                    .id(("quick-model", ix))
                    .group("quick-model-row")
                    .flex_shrink_0()
                    .h(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .when(highlighted, |d| d.bg(content(0.1)))
                    .when(!highlighted, |d| d.hover(|s| s.bg(content(0.06))))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.highlighted = ix;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .id(("quick-model-option", ix))
                            .role(Role::ListBoxOption)
                            .aria_label(entry.name.clone())
                            .aria_selected(current)
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(13.))
                            .text_color(content(1.))
                            .when(!entry.enabled, |d| d.opacity(0.35))
                            .when(entry.enabled && entry.hidden, |d| d.opacity(0.5))
                            .when(entry.enabled, |d| d.cursor_pointer())
                            .when(favorites, |d| d.child(provider_icon(entry.kind.id(), 14.)))
                            .child(div().min_w_0().truncate().child(entry.name.clone()))
                            .when(current, |d| {
                                d.child(icon("check-2", 14.).ml_auto().text_color(accent_color()))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx))),
                    )
                    .child(
                        div()
                            .id(("quick-model-star", ix))
                            .role(Role::Button)
                            .aria_label(format!(
                                "{} {} {} favorites",
                                if starred { "Remove" } else { "Add" },
                                entry.name,
                                if starred { "from" } else { "to" }
                            ))
                            .group("quick-model-star")
                            .cursor_pointer()
                            .size(px(32.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                icon(if starred { "star-filled" } else { "star" }, 14.)
                                    .text_color(content(0.35))
                                    .group_hover("quick-model-star", |s| s.text_color(content(1.))),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_favorite(star_kind, star_id.clone(), cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let tabs: Vec<ModelTab> = std::iter::once(ModelTab::Favorites)
            .chain(
                Kind::ALL
                    .into_iter()
                    .filter(|kind| {
                        *kind == self.provider
                            || (!self.hidden.iter().any(|id| id == kind.id())
                                && !self.missing.contains(kind))
                    })
                    .map(ModelTab::Harness),
            )
            .collect();
        div()
            .id("quick-model-selector")
            .role(Role::Group)
            .aria_label("Model selector")
            .w_full()
            .flex()
            .flex_col()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.highlighted, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .id("quick-model-providers")
                    .role(Role::TabList)
                    .aria_label("Providers")
                    .h(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .border_b_1()
                    .border_color(content(0.08))
                    .children(tabs.into_iter().map(|tab| {
                        let selected = self.tab == tab;
                        let id: SharedString = match tab {
                            ModelTab::Favorites => "quick-tab-favorites".into(),
                            ModelTab::Harness(kind) => format!("quick-tab-{}", kind.id()).into(),
                        };
                        let label = match tab {
                            ModelTab::Favorites => "Favorites",
                            ModelTab::Harness(kind) => kind.label(),
                        };
                        div()
                            .id(id.clone())
                            .role(Role::Tab)
                            .aria_label(label)
                            .aria_selected(selected)
                            .group(id.clone())
                            .cursor_pointer()
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(selected, |d| d.bg(content(0.1)))
                            .when(!selected, |d| {
                                d.opacity(0.4).hover(|s| s.opacity(1.).bg(content(0.06)))
                            })
                            .child(match tab {
                                ModelTab::Favorites => {
                                    icon(if selected { "star-filled" } else { "star" }, 16.)
                                        .text_color(content(1.))
                                        .into_any_element()
                                }
                                ModelTab::Harness(kind) => {
                                    provider_icon(kind.id(), 16.).into_any_element()
                                }
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_tab(tab, window, cx)
                            }))
                    })),
            )
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(content(0.08))
                    .text_size(px(13.))
                    .text_color(content(1.))
                    .child(icon("search", 14.).text_color(content(0.4)))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("quick-model-list")
                    .role(Role::ListBox)
                    .aria_label("Models")
                    .h(px(240.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .when_some(empty, |d, text| {
                        d.child(
                            div()
                                .px_2()
                                .py_6()
                                .flex()
                                .justify_center()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child(text),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }
}

pub(crate) enum ProjectPicked {
    Project(String),
    Mono(String),
    New,
    Remote,
}

pub(crate) struct PickerMono {
    pub id: String,
    pub name: String,
    pub mascot: String,
    pub color: u32,
}

/// ProjectPickerPopover: a search field over the projects, the current one
/// first and checked, and "New project" at the foot.
pub(crate) struct ProjectPicker {
    appearance:
        std::collections::HashMap<String, monocode_core::project_appearance::ProjectAppearance>,
    projects: Vec<String>,
    monos: Vec<PickerMono>,
    active_mono: Option<String>,
    current: String,
    active: usize,
    allow_remote: bool,
    search: Entity<input::Input>,
    _changed: Subscription,
}
impl EventEmitter<ProjectPicked> for ProjectPicker {}
impl ProjectPicker {
    pub(crate) fn new(projects: &[String], current: &str, cx: &mut Context<Self>) -> Self {
        let search = search_field("Search projects", "Search projects...", 0.35, cx);
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            cx.notify();
        });
        let mut projects: Vec<String> = projects.to_vec();
        if let Some(ix) = projects.iter().position(|p| p == current) {
            let first = projects.remove(ix);
            projects.insert(0, first);
        }
        Self {
            appearance: Default::default(),
            projects,
            monos: Vec::new(),
            active_mono: None,
            current: current.into(),
            active: 0,
            allow_remote: false,
            search,
            _changed: changed,
        }
    }
    pub(crate) fn search_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
    pub(crate) fn allow_remote(mut self) -> Self {
        self.allow_remote = true;
        self
    }
    pub(crate) fn appearance(
        mut self,
        appearance: std::collections::HashMap<
            String,
            monocode_core::project_appearance::ProjectAppearance,
        >,
    ) -> Self {
        self.appearance = appearance;
        self
    }
    pub(crate) fn monos(
        mut self,
        monos: Vec<PickerMono>,
        active: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        self.active_mono = active.filter(|id| monos.iter().any(|mono| &mono.id == id));
        if !monos.is_empty() {
            self.search.update(cx, |search, cx| {
                search.set_placeholder("Search monos and projects...", cx)
            });
        }
        self.monos = monos;
        self
    }
    fn matching_monos(&self, cx: &gpui::App) -> Vec<&PickerMono> {
        let query = self.search.read(cx).content.trim().to_lowercase();
        self.monos
            .iter()
            .filter(|mono| mono.name.to_lowercase().contains(&query))
            .collect()
    }
    fn label(&self, path: &str) -> String {
        self.appearance
            .get(path)
            .map_or_else(|| project_name(path), |a| a.label(path))
    }
    fn matches(&self, cx: &gpui::App) -> Vec<String> {
        let query = self.search.read(cx).content.to_lowercase();
        self.projects
            .iter()
            .filter(|path| {
                query.is_empty()
                    || format!("{}\n{}", self.label(path), path)
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect()
    }
    fn step(&mut self, direction: isize, cx: &mut Context<Self>) {
        let count = self.matching_monos(cx).len() + self.matches(cx).len();
        if count > 0 {
            self.active = (self.active as isize + direction).clamp(0, count as isize - 1) as usize;
            cx.notify();
        }
    }
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        let monos = self.matching_monos(cx);
        if let Some(mono) = monos.get(ix) {
            cx.emit(ProjectPicked::Mono(mono.id.clone()));
        } else if let Some(path) = self
            .matches(cx)
            .into_iter()
            .nth(ix.saturating_sub(monos.len()))
        {
            cx.emit(ProjectPicked::Project(path));
        }
    }
}
/// `prettyParent`: the folder holding a project, with the home folder as `~`.
pub(crate) fn pretty_parent(path: &str) -> String {
    let parent = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && parent.starts_with(&home) => {
            format!("~{}", &parent[home.len()..])
        }
        _ => parent,
    }
}
impl Render for ProjectPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let matches = self.matches(cx);
        let focused = self.search.focus_handle(cx).is_focused(window);
        let matching_monos = self.matching_monos(cx);
        let mono_count = matching_monos.len();
        let mut rows: Vec<AnyElement> = matching_monos
            .into_iter()
            .enumerate()
            .map(|(ix, mono)| {
                let active = self.active == ix;
                let current = self.active_mono.as_ref() == Some(&mono.id);
                div()
                    .id(("project-picker-mono", ix))
                    .debug_selector({
                        let id = mono.id.clone();
                        move || format!("project-picker-mono-{id}")
                    })
                    .role(Role::ListBoxOption)
                    .aria_label(format!("{}, Mono", mono.name))
                    .aria_selected(current)
                    .cursor_pointer()
                    .flex_shrink_0()
                    .h(px(36.))
                    .px(px(10.))
                    .gap(px(10.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .text_color(content(if active { 1. } else { 0.75 }))
                    .when(active, |d| d.bg(content(0.1)))
                    .when(!active, |d| {
                        d.hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                    })
                    .child(
                        div()
                            .size(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if current {
                                icon("check-2", 14.)
                                    .text_color(content(1.))
                                    .into_any_element()
                            } else {
                                crate::monos::avatar::avatar(
                                    &mono.mascot,
                                    mono.color,
                                    "idle",
                                    16.,
                                    true,
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(mono.name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child("Mono"),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.active = ix;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                    .into_any_element()
            })
            .collect();
        rows.extend(matches.into_iter().enumerate().map(|(project_ix, path)| {
            let ix = mono_count + project_ix;
            let active = self.active == ix;
            let current = self.active_mono.is_none() && path == self.current;
            div()
                .id(("project-option", ix))
                .debug_selector(move || format!("project-picker-project-{project_ix}"))
                .role(Role::ListBoxOption)
                .aria_label(self.label(&path))
                .aria_description(path.clone())
                .aria_selected(current)
                .cursor_pointer()
                .flex_shrink_0()
                .h(px(36.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .text_color(content(if active { 1. } else { 0.75 }))
                .when(active, |d| d.bg(content(0.1)))
                .when(!active, |d| {
                    d.hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                })
                .child(
                    div()
                        .size(px(16.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if current {
                            icon("check-2", 14.)
                                .text_color(content(1.))
                                .into_any_element()
                        } else {
                            if path.starts_with("remote://") {
                                icon("globe", 14.)
                                    .text_color(content(0.6))
                                    .into_any_element()
                            } else {
                                crate::project_settings::project_icon(
                                    &self.appearance.get(&path).cloned().unwrap_or_default(),
                                    &path,
                                    14.,
                                )
                            }
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink_0()
                        .max_w(px(286. - 12. - 20. - 36.))
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(self.label(&path)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .max_w(px(112.))
                        .truncate()
                        .font_family("Menlo")
                        .text_size(px(11.))
                        .text_color(content(0.4))
                        .child(pretty_parent(&path)),
                )
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.active = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                .into_any()
        }));
        let empty = rows.is_empty();
        div()
            .id("project-picker")
            .debug_selector(|| "project-picker".into())
            .role(Role::Dialog)
            .aria_label("Project picker")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.active, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .h(px(44.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px_3()
                    .border_b_1()
                    .border_color(content(0.07))
                    .text_color(content(1.))
                    .child(icon("search", 16.).text_color(content(if focused {
                        0.7
                    } else {
                        0.45
                    })))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("project-list")
                    .role(Role::ListBox)
                    .aria_label(if self.monos.is_empty() {
                        "Projects"
                    } else {
                        "Monos and projects"
                    })
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(6.))
                    .flex()
                    .flex_col()
                    .when(empty, |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(20.))
                                .flex()
                                .justify_center()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child(if self.monos.is_empty() {
                                    "No projects found"
                                } else {
                                    "No monos or projects found"
                                }),
                        )
                    })
                    .children(rows),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(content(0.07))
                    .p(px(6.))
                    .child(
                        div()
                            .id("project-new")
                            .role(Role::Button)
                            .aria_label("New project")
                            .group("project-new")
                            .cursor_pointer()
                            .h(px(36.))
                            .px(px(10.))
                            .gap(px(10.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .text_color(content(0.75))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child(
                                icon("plus", 16.)
                                    .text_color(content(0.75))
                                    .group_hover("project-new", |s| s.text_color(content(1.))),
                            )
                            .child("New project")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ProjectPicked::New))),
                    )
                    .when(self.allow_remote, |d| {
                        d.child(
                            div()
                                .id("project-remote")
                                .role(Role::Button)
                                .aria_label("Open folder on a machine")
                                .cursor_pointer()
                                .h(px(36.))
                                .px(px(10.))
                                .gap(px(10.))
                                .rounded(px(8.))
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .text_color(content(0.75))
                                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                .child(icon("globe", 16.).text_color(content(0.75)))
                                .child("Open folder on a machine…")
                                .on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(ProjectPicked::Remote)),
                                ),
                        )
                    }),
            )
    }
}
