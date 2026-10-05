//! Port of `ProvidersPage`, `ProviderRow`, and `ProjectScopeIcon` in
//! SettingsView.tsx. The accounts card at the top is a slot the accounts
//! module fills.

use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyElement, AnyView, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Task, Window, div,
};
use monocode_core::harness::{HARNESSES, HarnessId};
use monocode_core::models::{
    DEFAULT_MODELS_KEY, HIDDEN_PICKER_PROVIDERS_KEY, HarnessAvailability, LAST_MODEL_KEY,
    ModelCatalog, ModelEnv, ModelPrefs,
};
use monocode_core::paths::path_key;
use monocode_core::project_providers::{
    PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviderSettings, ProjectProviders,
};
use monocode_core::settings::CLAUDE_HOOKS_KEY;
use monocode_layout::paths::project_name;
use monocode_settings::display_prefs::{self, MASK_EMAILS_KEY, SHOW_REMAINING_USAGE_KEY};
use monocode_settings::settings_store as ss;
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};

use super::binary_control::BinaryControl;
use super::chrome::{group, row};
use super::controls::{secondary_button, toggle, watch_keys};
use super::host::SlotContext;
use super::section::SectionContext;
use super::select::{Select, SelectOption};
use super::store;

/// `GLOBAL_PROVIDER_SCOPE`.
pub const GLOBAL_PROVIDER_SCOPE: &str = "global";

/// `looksLikeProject` in recents.ts.
// TODO(port): the engine's projects package has the same check; this crate
// cannot depend on it.
pub fn looks_like_project(path: &str) -> bool {
    if path.is_empty() || path == "/" || path == "~" {
        return false;
    }
    let slashed = monocode_core::paths::slash(path);
    let trimmed = slashed.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    let bytes = normalized.as_bytes();
    let drive = bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || normalized == "/" {
        return false;
    }
    if monocode_layout::paths::pretty_cwd(path) == "~" {
        return false;
    }
    !(path.contains(".app/") || path.contains(".app\\"))
}

pub fn harness_logo(harness: HarnessId) -> ProviderLogo {
    match harness {
        HarnessId::Claude => ProviderLogo::Claude,
        HarnessId::Codex => ProviderLogo::Codex,
        HarnessId::Cursor => ProviderLogo::Cursor,
        HarnessId::Grok => ProviderLogo::Grok,
        HarnessId::Opencode => ProviderLogo::Opencode,
        HarnessId::Pi => ProviderLogo::Pi,
        HarnessId::Omp => ProviderLogo::Omp,
        HarnessId::Fx => ProviderLogo::Fx,
        HarnessId::Hermes => ProviderLogo::Hermes,
        HarnessId::Droid => ProviderLogo::Droid,
        HarnessId::Antigravity => ProviderLogo::Antigravity,
    }
}

/// The scope options: Global, then each distinct project among `cwd` and
/// the recents.
pub fn scope_paths(cwd: &str, recents: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for path in std::iter::once(cwd).chain(recents.iter().map(String::as_str)) {
        if path.is_empty() || !looks_like_project(path) {
            continue;
        }
        if seen.insert(path_key(path)) {
            paths.push(path.to_string());
        }
    }
    paths
}

/// One provider's row values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRowState {
    pub harness: HarnessId,
    pub selected_model: String,
    pub is_default: bool,
    pub in_picker: bool,
    pub picker_locked: bool,
}

/// The rows' values for `project` (or the global scope).
pub fn provider_rows(
    project: Option<&str>,
    prefs: &ModelPrefs,
    projects: &ProjectProviders,
    catalog: &ModelCatalog,
    availability: &HarnessAvailability,
) -> Vec<ProviderRowState> {
    let choice = prefs.last_model.as_ref();
    let hidden_globally = &prefs.hidden_picker_providers;
    let project_settings = project
        .map(|project| projects.load(Some(project)))
        .unwrap_or_default();
    let env = ModelEnv {
        catalog,
        prefs,
        availability,
        projects,
    };
    // A project without overrides inherits the global default provider, the
    // same way `defaultSessionChoice` resolves it for new conversations.
    let effective_default = match project {
        Some(project) => Some(
            env.first_enabled_harness(
                Some(project),
                project_settings
                    .default_harness
                    .or(choice.map(|choice| choice.harness))
                    .unwrap_or(HarnessId::Cursor),
            ),
        ),
        None => choice.map(|choice| choice.harness),
    };
    let global_model = |harness: HarnessId| {
        prefs
            .default_models
            .get(&harness)
            .cloned()
            .unwrap_or_else(|| match choice {
                Some(choice) if choice.harness == harness => choice.model.clone(),
                _ => catalog.default_model_id(harness),
            })
    };
    HARNESSES
        .into_iter()
        .map(|harness| {
            let ProjectProviderSettings {
                default_harness,
                default_model,
                models,
                hidden,
            } = &project_settings;
            let in_picker = match project {
                Some(_) => {
                    !hidden
                        .as_ref()
                        .is_some_and(|hidden| hidden.contains(&harness))
                        && !hidden_globally.contains(&harness)
                }
                None => !hidden_globally.contains(&harness),
            };
            let selected_model = match project {
                Some(_) => models
                    .as_ref()
                    .and_then(|models| models.get(&harness).cloned())
                    .or_else(|| {
                        (*default_harness == Some(harness))
                            .then(|| default_model.clone())
                            .flatten()
                    })
                    .unwrap_or_else(|| global_model(harness)),
                None => global_model(harness),
            };
            ProviderRowState {
                harness,
                selected_model,
                is_default: match project {
                    Some(_) => effective_default == Some(harness),
                    None => choice.is_some_and(|choice| choice.harness == harness),
                },
                in_picker,
                picker_locked: project.is_some() && hidden_globally.contains(&harness),
            }
        })
        .collect()
}

pub struct ProvidersSection {
    ctx: SectionContext,
    cwd: String,
    recents: Vec<String>,
    scope: String,
    prefs: ModelPrefs,
    projects: ProjectProviders,
    claude_hooks: bool,
    show_remaining_usage: bool,
    mask_emails: bool,
    scope_select: Entity<Select>,
    model_selects: BTreeMap<HarnessId, Entity<Select>>,
    binaries: BTreeMap<HarnessId, Entity<BinaryControl>>,
    accounts: Option<AnyView>,
    _watch: (Vec<monocode_settings::Subscription>, Task<()>),
    _subscriptions: Vec<Subscription>,
}

impl ProvidersSection {
    pub fn new(
        ctx: SectionContext,
        slot: &SlotContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kv = ctx.kv.clone();
        let host = ctx.hosts.providers.clone();
        host.probe_harness_availability(cx);
        let availability = host.availability(cx);
        // Providers such as Droid and Hermes ship a placeholder model, so a
        // non-empty list does not mean the live catalog has loaded.
        for harness in HARNESSES {
            if availability.is_available(harness) {
                host.refresh_harness_catalog(harness, cx);
            }
        }
        let this = cx.entity().downgrade();
        let scope_select = cx.new(|cx| {
            Select::new(
                "Provider defaults scope",
                GLOBAL_PROVIDER_SCOPE,
                Vec::new(),
                cx,
            )
            .on_change(move |value, _, cx| {
                let value = value.to_string();
                this.update(cx, |this, cx| {
                    this.scope = value;
                    cx.notify();
                })
                .ok();
            })
        });
        let mut model_selects = BTreeMap::new();
        let mut binaries = BTreeMap::new();
        for harness in HARNESSES {
            let this = cx.entity().downgrade();
            model_selects.insert(
                harness,
                cx.new(|cx| {
                    Select::new(format!("{} model", harness.title()), "", Vec::new(), cx).on_change(
                        move |value, _, cx| {
                            let value = value.to_string();
                            this.update(cx, |this, cx| this.on_model_change(harness, &value, cx))
                                .ok();
                        },
                    )
                }),
            );
            let (kv, host) = (kv.clone(), host.clone());
            binaries.insert(
                harness,
                cx.new(|cx| BinaryControl::new(harness, kv, host, window, cx)),
            );
        }
        let accounts = ctx
            .hosts
            .accounts
            .clone()
            .map(|build| build(slot, window, cx));
        let watch = watch_keys(
            &kv,
            &[
                LAST_MODEL_KEY,
                DEFAULT_MODELS_KEY,
                HIDDEN_PICKER_PROVIDERS_KEY,
                PROJECT_PROVIDER_SETTINGS_KEY,
                CLAUDE_HOOKS_KEY,
                SHOW_REMAINING_USAGE_KEY,
                MASK_EMAILS_KEY,
            ],
            |this: &mut Self, cx| {
                this.reload();
                cx.notify();
            },
            cx,
        );
        Self {
            prefs: store::load_model_prefs(&kv),
            projects: store::load_project_providers(&kv),
            claude_hooks: ss::load_claude_hooks(&kv),
            show_remaining_usage: display_prefs::load_show_remaining_usage(&kv),
            mask_emails: display_prefs::load_mask_emails(&kv),
            cwd: slot.cwd.clone(),
            recents: slot.recents.clone(),
            scope: GLOBAL_PROVIDER_SCOPE.into(),
            scope_select,
            model_selects,
            binaries,
            accounts,
            ctx,
            _watch: watch,
            _subscriptions: Vec::new(),
        }
    }

    fn reload(&mut self) {
        let kv = &self.ctx.kv;
        self.prefs = store::load_model_prefs(kv);
        self.projects = store::load_project_providers(kv);
        self.claude_hooks = ss::load_claude_hooks(kv);
        self.show_remaining_usage = display_prefs::load_show_remaining_usage(kv);
        self.mask_emails = display_prefs::load_mask_emails(kv);
    }

    /// Re-reads the catalog and availability after the host reports a change.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload();
        cx.notify();
    }

    pub fn show_remaining_usage(&self) -> bool {
        self.show_remaining_usage
    }

    pub fn mask_emails(&self) -> bool {
        self.mask_emails
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn scope_select(&self) -> &Entity<Select> {
        &self.scope_select
    }

    pub fn binary(&self, harness: HarnessId) -> Option<&Entity<BinaryControl>> {
        self.binaries.get(&harness)
    }

    pub fn model_select(&self, harness: HarnessId) -> Option<&Entity<Select>> {
        self.model_selects.get(&harness)
    }

    fn project(&self) -> Option<String> {
        (self.scope != GLOBAL_PROVIDER_SCOPE).then(|| self.scope.clone())
    }

    pub fn rows(&self, cx: &gpui::App) -> Vec<ProviderRowState> {
        let host = &self.ctx.hosts.providers;
        provider_rows(
            self.project().as_deref(),
            &self.prefs,
            &self.projects,
            &host.catalog(cx),
            &host.availability(cx),
        )
    }

    /// `onModelChange`.
    pub fn on_model_change(&mut self, harness: HarnessId, model: &str, cx: &mut Context<Self>) {
        let kv = self.ctx.kv.clone();
        if let Some(project) = self.project() {
            store::update_project_providers(&kv, |providers| {
                providers.set_project_default_model(&project, harness, model)
            });
        } else {
            store::save_default_model(&kv, harness, model);
            if self.prefs.last_model.as_ref().map(|choice| choice.harness) == Some(harness) {
                store::save_last_model_choice(&kv, harness, model);
            }
        }
        self.reload();
        cx.notify();
    }

    /// `onDefault`.
    pub fn on_default(&mut self, harness: HarnessId, model: &str, cx: &mut Context<Self>) {
        let kv = self.ctx.kv.clone();
        if let Some(project) = self.project() {
            store::update_project_providers(&kv, |providers| {
                providers.set_project_default_provider(&project, harness, model)
            });
        } else {
            store::save_last_model_choice(&kv, harness, model);
        }
        self.reload();
        cx.notify();
    }

    /// `onPickerVisible`.
    pub fn on_picker_visible(&mut self, harness: HarnessId, visible: bool, cx: &mut Context<Self>) {
        let kv = self.ctx.kv.clone();
        if let Some(project) = self.project() {
            store::update_project_providers(&kv, |providers| {
                providers.set_project_provider_hidden(&project, harness, !visible)
            });
        } else {
            store::save_picker_provider_visible(&kv, harness, visible);
        }
        self.reload();
        cx.notify();
    }

    fn scope_options(&self, cx: &mut Context<Self>) -> Vec<SelectOption> {
        let theme = Theme::of(cx).clone();
        let mut options = vec![
            SelectOption::new(GLOBAL_PROVIDER_SCOPE, "Global").icon(Rc::new(move |_, _| {
                icon(IconName::Globe)
                    .size(u(14.))
                    .text_color(theme.content(0.60))
                    .into_any_element()
            })),
        ];
        for path in scope_paths(&self.cwd, &self.recents) {
            let host = self.ctx.hosts.providers.clone();
            let icon_path = path.clone();
            options.push(
                SelectOption::new(path.clone(), project_name(&path)).icon(Rc::new(
                    move |window, cx| {
                        host.project_icon(&icon_path, window, cx)
                            .unwrap_or_else(|| {
                                let theme = Theme::of(cx);
                                icon(IconName::Folder)
                                    .size(u(14.))
                                    .text_color(theme.content(0.60))
                                    .into_any_element()
                            })
                    },
                )),
            );
        }
        options
    }

    fn provider_row(
        &self,
        state: &ProviderRowState,
        catalog: &ModelCatalog,
        availability: &HarnessAvailability,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let reveal = self.ctx.reveal(cx);
        let harness = state.harness;
        let models = catalog.models_for(harness);
        let available = availability.is_available(harness);
        let current = (!models.is_empty())
            .then(|| catalog.resolve_model(harness, Some(&state.selected_model)));
        let title = harness.title();
        let mut label = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(provider_logo(harness_logo(harness)).size(16.))
            .child(title)
            .children(self.binaries.get(&harness).cloned());
        if state.is_default {
            label = label.child(
                div()
                    .rounded_full()
                    .bg(theme.content(0.10))
                    .px(u(6.))
                    .py(u(2.))
                    .text_px(theme.text.micro)
                    .medium()
                    .text_color(theme.content(0.60))
                    .child("DEFAULT"),
            );
        }
        let description = if available {
            format!(
                "{} {} available.",
                models.len(),
                if models.len() == 1 { "model" } else { "models" }
            )
        } else {
            self.ctx.hosts.providers.harness_unavailable_hint(harness)
        };
        let mut el = row(&reveal, label)
            .selector(format!("provider-row:{title}"))
            .description(description);
        if let Some(current) = &current
            && let Some(select) = self.model_selects.get(&harness)
        {
            let options = models
                .iter()
                .map(|model| SelectOption::new(model.id.clone(), model.name.clone()))
                .collect();
            let value = current.id.clone();
            select.update(cx, |select, _| select.sync(value, options));
            el = el.child(select.clone());
        }
        let current_id = current.as_ref().map(|model| model.id.clone());
        el = el.child(
            secondary_button(
                format!("use-default-{}", harness.as_str()),
                if state.is_default {
                    "Default"
                } else {
                    "Use by default"
                },
            )
            .disabled(state.is_default || current_id.is_none())
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(model) = &current_id {
                    this.on_default(harness, model, cx);
                }
            })),
        );
        if available {
            el = el.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .child(
                        div()
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.50))
                            .child(if state.picker_locked {
                                "Hidden globally"
                            } else {
                                "Show in picker"
                            }),
                    )
                    .child(
                        toggle(format!("Show {title} in the model picker"), state.in_picker)
                            .disabled(state.picker_locked)
                            .on_change(cx.listener(move |this, next: &bool, _, cx| {
                                this.on_picker_visible(harness, *next, cx)
                            })),
                    ),
            );
        }
        el.into_any_element()
    }
}

impl Render for ProvidersSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = self.ctx.reveal(cx);
        let host = self.ctx.hosts.providers.clone();
        let catalog = host.catalog(cx);
        let availability = host.availability(cx);
        let options = self.scope_options(cx);
        // Fall back to Global when the scoped project left the list.
        if !options
            .iter()
            .any(|option| option.value.as_ref() == self.scope)
        {
            self.scope = GLOBAL_PROVIDER_SCOPE.into();
        }
        let scope = self.scope.clone();
        self.scope_select
            .update(cx, |select, _| select.sync(scope, options));
        let project = self.project();
        let rows = provider_rows(
            project.as_deref(),
            &self.prefs,
            &self.projects,
            &catalog,
            &availability,
        );

        let mut clis = group(&reveal, "Agent CLIs")
            .id("agent-clis")
            .action(self.scope_select.clone())
            .description(match &project {
                Some(project) => format!("These defaults apply to {} only. A provider with Show in picker off is also kept out of new conversations started in this project. CLI paths remain global for MonoCode.", project_name(project)),
                None => "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed but are left out of the model picker, as are installed ones with Show in picker off. The model beside a provider is what its new conversations start with; Use by default picks the provider itself. CLI paths are global for MonoCode and apply to every project.".into(),
            });
        for state in &rows {
            clis = clis.child(self.provider_row(state, &catalog, &availability, cx));
        }

        // `UsageDisplaySettings`.
        let usage_display = group(&reveal, "Usage and privacy")
            .child(
                row(&reveal, "Show remaining usage")
                    .id("show-remaining-usage")
                    .description("Fill usage meters with what is left in each limit instead of what has been used.")
                    .switch_only()
                    .child(
                        toggle("Show remaining usage", self.show_remaining_usage).on_change(
                            cx.listener(|this, next: &bool, _, cx| {
                                display_prefs::save_show_remaining_usage(&this.ctx.kv, *next);
                                this.show_remaining_usage = *next;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                row(&reveal, "Mask account emails")
                    .id("mask-emails")
                    .description("Blur account emails in Settings and the usage popover until you click one, so they stay out of screenshots.")
                    .switch_only()
                    .child(
                        toggle("Mask account emails", self.mask_emails).on_change(cx.listener(
                            |this, next: &bool, _, cx| {
                                display_prefs::save_mask_emails(&this.ctx.kv, *next);
                                this.mask_emails = *next;
                                cx.notify();
                            },
                        )),
                    ),
            );

        let advanced = group(&reveal, "Advanced").child(
            row(&reveal, "Claude Code hooks")
                .id("claude-hooks")
                .description("Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.")
                .switch_only()
                .child(toggle("Claude Code hooks", self.claude_hooks).on_change(cx.listener(
                    |this, next: &bool, _, cx| {
                        ss::save_claude_hooks(&this.ctx.kv, *next);
                        this.claude_hooks = *next;
                        cx.notify();
                    },
                ))),
        );

        // `ProviderAccountsSettings` is the `provider-accounts` card. The
        // accounts module fills the slot; without it a placeholder keeps the
        // card's place and its search target.
        let theme = Theme::of(cx).clone();
        let id: gpui::SharedString = "provider-accounts".into();
        let accounts = div()
            .relative()
            .debug_selector(|| "setting-id:provider-accounts".into())
            .child(reveal.anchors.probe(id))
            .child(match self.accounts.clone() {
                Some(view) => view.into_any_element(),
                None => group(&reveal, "Accounts")
                    .first(true)
                    .description("Create isolated sign-ins for providers that support account profiles. Account switching stays available from the usage control in the footer.")
                    .child(
                        div()
                            .px(u(16.))
                            .py(u(14.))
                            .text_px(theme.text.label)
                            .text_color(theme.content(0.45))
                            .child("Provider accounts appear here."),
                    )
                    .into_any_element(),
            });
        div()
            .flex()
            .flex_col()
            .child(accounts)
            .child(usage_display)
            .child(clis)
            .child(advanced)
    }
}
