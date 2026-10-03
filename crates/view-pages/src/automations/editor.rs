//! AutomationsView.tsx `AutomationEditor`: the name, actions, enabled
//! switch, project, and delete menu; the Settings tab (triggers with their
//! sentences, instructions with the model and access pickers, session
//! options, the missed-run grace); and the Run history tab.
//!
//! The editor owns its copy of the draft and reports each change through
//! `on_change`, the way React's controlled `draft` prop and `onChange` did.
//! The page rebuilds the editor when it replaces the draft from outside
//! (reset, save, another automation).

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseDownEvent, ParentElement as _, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::{HarnessId, ModelSettings};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::anchor::{
    BoundsCell, Side, anchored_popover, popover_layer, popover_surface, submenu_layer,
};
use monocode_view_composer::pickers::{
    AccessPicker, ModelControlPills, ModelPicker, ModelPickerProps, SearchableSelect,
    SearchableSelectOption, SelectVariant, SkillPromptField,
};

use super::data::{AutomationsData, ProviderConnections};
use super::model::{
    AutomationDraft, AutomationRun, AutomationScheduleKind, AutomationTrigger,
    AutomationTriggerKind, AutomationWorkspaceMode, TriggerExtras, apply_triggers,
    create_automation_trigger, format_automation_run_at, format_automation_run_duration,
    gmt_offset_label, next_automation_run_at, next_run_preview,
};
use super::trigger_mark;
use super::triggers::{
    CONVERSATION_OPTIONS, GRACE_OPTIONS, MAX_TRIGGERS, RunTone, WORKSPACE_OPTIONS, day_options,
    draft_is_valid, event_sentence_stem, minute_options, run_status_label, run_status_tone,
    run_trigger_meta, time_options, time_sentence_prefix, trigger_categories, trigger_events,
    trigger_name,
};
use crate::data::ProjectsData;
use crate::format::{looks_like_project, now_ms};
use crate::widgets::{
    ProjectPicker, page_tab, plain_input, spinner_icon, toggle_switch, vertical_rule,
};

type ActionFn = Rc<dyn Fn(&mut Window, &mut App)>;
type ChangeFn = Rc<dyn Fn(&AutomationDraft, &mut Window, &mut App)>;
type SessionFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// `RUN_GRID` column widths: `minmax(0,1.4fr) 9.5rem 6.75rem 3.5rem`.
const RUN_COLUMNS: [f32; 3] = [152., 108., 56.];

/// Which tab the editor shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorTab {
    #[default]
    Settings,
    History,
}

/// The page state the editor shows but does not own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorStatus {
    pub runs: Vec<AutomationRun>,
    pub saving: bool,
    pub running: bool,
    /// The draft has unsaved changes.
    pub dirty: bool,
}

/// How to build one sentence pill.
struct PillSpec {
    trigger_id: String,
    label: &'static str,
    value: String,
    options: Vec<SearchableSelectOption>,
    placeholder: Option<&'static str>,
    /// Writes the picked value into the trigger.
    set: fn(&mut AutomationTrigger, &str),
}

/// The pills of one trigger sentence.
struct TriggerPills {
    day: Entity<SearchableSelect>,
    minute: Entity<SearchableSelect>,
    time: Entity<SearchableSelect>,
    branch: Entity<SearchableSelect>,
    actor: Entity<SearchableSelect>,
}

pub struct AutomationEditor {
    data: Rc<dyn AutomationsData>,
    projects: Rc<dyn ProjectsData>,
    draft: AutomationDraft,
    status: EditorStatus,
    tab: EditorTab,
    focus: FocusHandle,
    name: Entity<InputState>,
    picker: Entity<ProjectPicker>,
    model: Entity<ModelPicker>,
    access: Entity<AccessPicker>,
    pills: Entity<ModelControlPills>,
    prompt: Entity<SkillPromptField>,
    workspace: Entity<SearchableSelect>,
    conversation: Entity<SearchableSelect>,
    /// The folder select and whether it searches (more than six options).
    folder: Option<(bool, Entity<SearchableSelect>)>,
    grace: Entity<SearchableSelect>,
    trigger_pills: HashMap<String, TriggerPills>,
    connections: ProviderConnections,
    branches: Vec<String>,
    menu_open: bool,
    menu_bounds: BoundsCell,
    trigger_open: bool,
    trigger_query: Entity<InputState>,
    trigger_category: Option<AutomationTriggerKind>,
    add_trigger_bounds: BoundsCell,
    submenu_bounds: BoundsCell,
    advanced_open: bool,
    /// The project's folders changed; resync on the next render.
    folders_changed: bool,
    scroll: ScrollHandle,
    on_change: Option<ChangeFn>,
    on_close: Option<ActionFn>,
    on_submit: Option<ActionFn>,
    on_run: Option<ActionFn>,
    on_delete: Option<ActionFn>,
    on_open_session: Option<SessionFn>,
    _subscriptions: Vec<Subscription>,
}

fn options(pairs: &[(&str, &str)]) -> Vec<SearchableSelectOption> {
    pairs
        .iter()
        .map(|(value, label)| SearchableSelectOption::new(value.to_string(), label.to_string()))
        .collect()
}

fn owned_options(pairs: Vec<(String, String)>) -> Vec<SearchableSelectOption> {
    pairs
        .into_iter()
        .map(|(value, label)| SearchableSelectOption::new(value, label))
        .collect()
}

impl AutomationEditor {
    pub fn new(
        draft: AutomationDraft,
        data: Rc<dyn AutomationsData>,
        projects: Rc<dyn ProjectsData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_name = draft.name.clone();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Untitled")
                .default_value(initial_name)
        });
        let trigger_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search triggers"));
        let weak = cx.weak_entity();
        let picker = cx.new(|cx| {
            ProjectPicker::new(&draft.cwd, projects.clone(), window, cx).on_select(
                move |path, window, cx| {
                    let path = path.to_string();
                    weak.update(cx, |this, cx| {
                        this.change(window, cx, |draft| {
                            draft.cwd = path;
                            draft.session_folder_id.clear();
                        });
                        this.sync_folder_options(window, cx);
                        this.load_branches(cx);
                    })
                    .ok();
                },
            )
        });
        let source = data.model_source(cx);
        let weak = cx.weak_entity();
        let settings_weak = cx.weak_entity();
        let props = ModelPickerProps {
            harness: Some(draft.harness),
            model: draft.model.clone(),
            values: draft.model_settings.clone(),
            project: Some(draft.cwd.clone()),
            hide_settings: data.model_controls_beside(cx),
            ..Default::default()
        };
        let prefs = data.model_prefs(cx);
        let project_providers = data.project_providers(cx);
        let model_source = source.clone();
        let model = cx.new(|cx| {
            ModelPicker::new(props, model_source, prefs, project_providers, window, cx)
                .on_change(move |harness, model, window, cx| {
                    let model = model.to_string();
                    weak.update(cx, |this, cx| this.set_model(harness, &model, window, cx))
                        .ok();
                })
                .on_settings_change(move |settings, window, cx| {
                    let settings = settings.clone();
                    settings_weak
                        .update(cx, |this, cx| this.set_model_settings(settings, window, cx))
                        .ok();
                })
        });
        let weak = cx.weak_entity();
        let access = cx.new(|cx| {
            AccessPicker::new(draft.runtime_mode, cx).on_change(move |mode, window, cx| {
                weak.update(cx, |this, cx| {
                    this.change(window, cx, |draft| draft.runtime_mode = mode)
                })
                .ok();
            })
        });
        let weak = cx.weak_entity();
        let pills = cx.new(|cx| {
            ModelControlPills::new(
                draft.harness,
                draft.model.clone(),
                draft.model_settings.clone(),
                source.clone(),
                cx,
            )
            .on_settings_change(move |settings, window, cx| {
                let settings = settings.clone();
                weak.update(cx, |this, cx| this.set_model_settings(settings, window, cx))
                    .ok();
            })
        });
        let completions = data.skill_completions(draft.harness, &draft.cwd, cx);
        let weak = cx.weak_entity();
        let initial_prompt = draft.prompt.clone();
        let prompt = cx.new(|cx| {
            SkillPromptField::new(&initial_prompt, completions, window, cx).on_change(
                move |value, window, cx| {
                    let value = value.to_string();
                    weak.update(cx, |this, cx| {
                        if this.draft.prompt != value {
                            this.change(window, cx, |draft| draft.prompt = value);
                        }
                    })
                    .ok();
                },
            )
        });
        let weak = cx.weak_entity();
        let workspace = cx.new(|cx| {
            SearchableSelect::new(
                "Working copy",
                workspace_value(draft.workspace_mode),
                options(&WORKSPACE_OPTIONS),
                window,
                cx,
            )
            .variant(SelectVariant::Pill)
            .searchable(false)
            .on_change(move |value, window, cx| {
                let worktree = value == "worktree";
                weak.update(cx, |this, cx| {
                    this.change(window, cx, |draft| {
                        draft.workspace_mode = if worktree {
                            AutomationWorkspaceMode::Worktree
                        } else {
                            AutomationWorkspaceMode::Current
                        };
                        draft.worktree_cwd.clear();
                        draft.reuse_session = !worktree && draft.reuse_session;
                    });
                    this.sync_conversation(window, cx);
                })
                .ok();
            })
        });
        let weak = cx.weak_entity();
        let conversation = cx.new(|cx| {
            let mut select = SearchableSelect::new(
                "Conversation",
                if draft.reuse_session {
                    "reuse"
                } else {
                    "fresh"
                },
                options(&CONVERSATION_OPTIONS),
                window,
                cx,
            )
            .variant(SelectVariant::Pill)
            .searchable(false)
            .on_change(move |value, window, cx| {
                let reuse = value == "reuse";
                weak.update(cx, |this, cx| {
                    this.change(window, cx, |draft| draft.reuse_session = reuse)
                })
                .ok();
            });
            select.set_disabled(
                draft.workspace_mode == AutomationWorkspaceMode::Worktree,
                window,
                cx,
            );
            select
        });
        let weak = cx.weak_entity();
        let grace = cx.new(|cx| {
            SearchableSelect::new(
                "Missed-run grace",
                draft.missed_run_grace_minutes.to_string(),
                options(&GRACE_OPTIONS),
                window,
                cx,
            )
            .variant(SelectVariant::Pill)
            .searchable(false)
            .on_change(move |value, window, cx| {
                let minutes = value.parse().unwrap_or(0);
                weak.update(cx, |this, cx| {
                    this.change(window, cx, |draft| draft.missed_run_grace_minutes = minutes)
                })
                .ok();
            })
        });
        let name_events = cx.subscribe_in(&name, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                if value != this.draft.name {
                    this.change(window, cx, |draft| draft.name = value);
                }
            }
        });
        let query_events = cx.subscribe_in(&trigger_query, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_trigger_category(cx);
                cx.notify();
            }
        });
        let weak = cx.weak_entity();
        let provider_changes = data.subscribe_provider_changes(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| this.load_connections(cx)).ok();
            }),
            cx,
        );
        let weak = cx.weak_entity();
        let folder_changes = data.subscribe_session_folders(
            &draft.cwd,
            Box::new(move |cx| {
                weak.update(cx, |this, cx| {
                    this.folders_changed = true;
                    cx.notify();
                })
                .ok();
            }),
            cx,
        );
        name.update(cx, |input, cx| input.focus(window, cx));
        let mut editor = Self {
            data,
            projects,
            draft,
            status: EditorStatus::default(),
            tab: EditorTab::Settings,
            focus: cx.focus_handle(),
            name,
            picker,
            model,
            access,
            pills,
            prompt,
            workspace,
            conversation,
            folder: None,
            grace,
            trigger_pills: HashMap::new(),
            connections: ProviderConnections::default(),
            branches: Vec::new(),
            menu_open: false,
            menu_bounds: BoundsCell::default(),
            trigger_open: false,
            trigger_query,
            trigger_category: None,
            add_trigger_bounds: BoundsCell::default(),
            submenu_bounds: BoundsCell::default(),
            advanced_open: false,
            folders_changed: false,
            scroll: ScrollHandle::new(),
            on_change: None,
            on_close: None,
            on_submit: None,
            on_run: None,
            on_delete: None,
            on_open_session: None,
            _subscriptions: vec![name_events, query_events, provider_changes, folder_changes],
        };
        editor.sync_folder_options(window, cx);
        editor.load_connections(cx);
        editor.load_branches(cx);
        editor
    }

    pub fn on_change(
        mut self,
        f: impl Fn(&AutomationDraft, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Cancel (new) or Reset (existing).
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Create or Save.
    pub fn on_submit(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// Run now; shown for a saved automation.
    pub fn on_run(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_run = Some(Rc::new(f));
        self
    }

    /// Delete automation; shown for a saved automation.
    pub fn on_delete(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_delete = Some(Rc::new(f));
        self
    }

    pub fn on_open_session(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_session = Some(Rc::new(f));
        self
    }

    pub fn draft(&self) -> &AutomationDraft {
        &self.draft
    }

    pub fn status(&self) -> &EditorStatus {
        &self.status
    }

    pub fn tab(&self) -> EditorTab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: EditorTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub fn set_status(&mut self, status: EditorStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            cx.notify();
        }
    }

    pub fn name_input(&self) -> &Entity<InputState> {
        &self.name
    }

    pub fn prompt_field(&self) -> &Entity<SkillPromptField> {
        &self.prompt
    }

    pub fn is_trigger_menu_open(&self) -> bool {
        self.trigger_open
    }

    pub fn trigger_category(&self) -> Option<AutomationTriggerKind> {
        self.trigger_category
    }

    pub fn is_actions_menu_open(&self) -> bool {
        self.menu_open
    }

    /// The `…` button.
    pub fn toggle_actions_menu(&mut self, cx: &mut Context<Self>) {
        self.menu_open = !self.menu_open;
        cx.notify();
    }

    /// The Save or Create button is enabled.
    pub fn can_submit(&self) -> bool {
        draft_is_valid(&self.draft)
            && !self.status.saving
            && (self.draft.id.is_none() || self.status.dirty)
    }

    /// `update`: change the draft and tell the page.
    fn change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut AutomationDraft),
    ) {
        f(&mut self.draft);
        self.status.dirty = true;
        if let Some(on_change) = self.on_change.clone() {
            let draft = self.draft.clone();
            window.defer(cx, move |window, cx| on_change(&draft, window, cx));
        }
        cx.notify();
    }

    fn set_model(
        &mut self,
        harness: HarnessId,
        model: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = model.to_string();
        self.change(window, cx, |draft| {
            draft.harness = harness;
            draft.model = model;
        });
        let draft = self.draft.clone();
        self.model.update(cx, |picker, cx| {
            picker.set_selection(
                draft.harness,
                draft.model.clone(),
                draft.model_settings.clone(),
                cx,
            )
        });
        self.pills.update(cx, |pills, cx| {
            pills.set_selection(
                draft.harness,
                draft.model.clone(),
                draft.model_settings.clone(),
                cx,
            )
        });
        let completions = self.data.skill_completions(draft.harness, &draft.cwd, cx);
        self.prompt
            .update(cx, |prompt, cx| prompt.set_completions(completions, cx));
    }

    fn set_model_settings(
        &mut self,
        settings: ModelSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change(window, cx, |draft| draft.model_settings = settings);
        let draft = self.draft.clone();
        self.model.update(cx, |picker, cx| {
            picker.set_selection(
                draft.harness,
                draft.model.clone(),
                draft.model_settings.clone(),
                cx,
            )
        });
        self.pills.update(cx, |pills, cx| {
            pills.set_selection(
                draft.harness,
                draft.model.clone(),
                draft.model_settings.clone(),
                cx,
            )
        });
    }

    fn sync_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let worktree = self.draft.workspace_mode == AutomationWorkspaceMode::Worktree;
        let reuse = self.draft.reuse_session;
        self.conversation.update(cx, |select, cx| {
            select.set_disabled(worktree, window, cx);
            select.set_value(if reuse { "reuse" } else { "fresh" }, cx);
        });
    }

    /// `folderOptions`: None, the project's folders, and a removed folder the
    /// draft still names.
    pub fn folder_options(&self, cx: &App) -> Vec<SearchableSelectOption> {
        let folders = self.data.session_folders(&self.draft.cwd, cx);
        let mut options = vec![SearchableSelectOption::new("", "None")];
        options.extend(folders.iter().map(|folder| {
            SearchableSelectOption::new(folder.id.clone(), folder.name.clone())
                .keywords(folder.name.clone())
        }));
        let current = &self.draft.session_folder_id;
        if !current.is_empty() && !folders.iter().any(|folder| folder.id == *current) {
            options.push(
                SearchableSelectOption::new(current.clone(), "Removed folder")
                    .keywords(current.clone()),
            );
        }
        options
    }

    fn sync_folder_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let options = self.folder_options(cx);
        let searchable = options.len() > 6;
        let value = self.draft.session_folder_id.clone();
        if let Some((current, select)) = &self.folder
            && *current == searchable
        {
            select.update(cx, |select, cx| {
                select.set_options(options, cx);
                select.set_value(value, cx);
            });
            return;
        }
        let weak = cx.weak_entity();
        let select = cx.new(|cx| {
            SearchableSelect::new("Session folder", value, options, window, cx)
                .variant(SelectVariant::Pill)
                .searchable(searchable)
                .on_change(move |value, window, cx| {
                    let value = value.to_string();
                    weak.update(cx, |this, cx| {
                        this.change(window, cx, |draft| draft.session_folder_id = value)
                    })
                    .ok();
                })
        });
        self.folder = Some((searchable, select));
    }

    fn load_connections(&mut self, cx: &mut Context<Self>) {
        let task = self.data.provider_connections(cx);
        cx.spawn(async move |this, cx| {
            let connections = task.await;
            this.update(cx, |this, cx| {
                this.connections = connections;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// TriggerRow's branch list: only when a push trigger needs it.
    fn load_branches(&mut self, cx: &mut Context<Self>) {
        let wants = self
            .draft
            .triggers
            .iter()
            .any(|trigger| trigger.event == "push_to_branch");
        if !wants || !looks_like_project(&self.draft.cwd) {
            self.branches.clear();
            return;
        }
        let task = self.data.git_branches(&self.draft.cwd, cx);
        cx.spawn(async move |this, cx| {
            let branches = task.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.branches = branches;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Triggers.

    fn set_triggers(
        &mut self,
        triggers: Vec<AutomationTrigger>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = apply_triggers(&self.draft, triggers);
        self.change(window, cx, |draft| *draft = next);
        let ids: Vec<String> = self.draft.triggers.iter().map(|t| t.id.clone()).collect();
        self.trigger_pills.retain(|id, _| ids.contains(id));
    }

    fn update_trigger(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut AutomationTrigger),
    ) {
        let mut triggers = self.draft.triggers.clone();
        if let Some(trigger) = triggers.iter_mut().find(|trigger| trigger.id == id) {
            f(trigger);
        }
        self.set_triggers(triggers, window, cx);
    }

    pub fn remove_trigger(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let triggers = self
            .draft
            .triggers
            .iter()
            .filter(|trigger| trigger.id != id)
            .cloned()
            .collect();
        self.set_triggers(triggers, window, cx);
    }

    pub fn toggle_trigger_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.triggers.len() >= MAX_TRIGGERS {
            return;
        }
        self.trigger_query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.trigger_category = None;
        self.trigger_open = !self.trigger_open;
        if self.trigger_open {
            self.trigger_query
                .update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    fn close_trigger_menu(&mut self, cx: &mut Context<Self>) {
        self.trigger_open = false;
        self.trigger_category = None;
        cx.notify();
    }

    /// The categories the trigger search shows.
    pub fn visible_categories(&self, cx: &App) -> Vec<(AutomationTriggerKind, &'static str)> {
        trigger_categories(&self.trigger_query.read(cx).value())
    }

    fn sync_trigger_category(&mut self, cx: &App) {
        if let Some(category) = self.trigger_category
            && !self
                .visible_categories(cx)
                .iter()
                .any(|(kind, _)| *kind == category)
        {
            self.trigger_category = None;
        }
    }

    /// Hovering or clicking a category: open its events when the provider
    /// is connected.
    pub fn show_category(&mut self, kind: AutomationTriggerKind, cx: &mut Context<Self>) {
        self.trigger_category = self.connections.ready(kind).then_some(kind);
        cx.notify();
    }

    /// `selectTrigger`.
    pub fn select_trigger(
        &mut self,
        kind: AutomationTriggerKind,
        event: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft.triggers.len() >= MAX_TRIGGERS || !self.connections.ready(kind) {
            self.close_trigger_menu(cx);
            return;
        }
        let mut triggers = self.draft.triggers.clone();
        triggers.push(create_automation_trigger(
            kind,
            event,
            TriggerExtras::default(),
        ));
        self.set_triggers(triggers, window, cx);
        self.close_trigger_menu(cx);
        self.load_branches(cx);
    }

    fn pill(
        &self,
        spec: PillSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SearchableSelect> {
        let weak = cx.weak_entity();
        let PillSpec {
            trigger_id,
            label,
            value,
            options,
            placeholder,
            set,
        } = spec;
        let searchable = options.len() > 8;
        cx.new(|cx| {
            let mut select = SearchableSelect::new(label, value, options, window, cx)
                .variant(SelectVariant::Pill)
                .searchable(searchable)
                .empty_label("No branches found")
                .on_change(move |value, window, cx| {
                    let value = value.to_string();
                    let trigger_id = trigger_id.clone();
                    weak.update(cx, |this, cx| {
                        this.update_trigger(&trigger_id, window, cx, |trigger| set(trigger, &value))
                    })
                    .ok();
                });
            if let Some(placeholder) = placeholder {
                select = select.placeholder(placeholder);
            }
            select
        })
    }

    /// The pills of one trigger, created on first use and kept in step with
    /// the draft.
    fn pills_for(
        &mut self,
        trigger: &AutomationTrigger,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> &TriggerPills {
        let id = trigger.id.clone();
        if !self.trigger_pills.contains_key(&id) {
            let pills = TriggerPills {
                day: self.pill(
                    PillSpec {
                        trigger_id: id.clone(),
                        label: "Day",
                        value: trigger.day_of_week.to_string(),
                        options: owned_options(day_options()),
                        placeholder: None,
                        set: |trigger, value| trigger.day_of_week = value.parse().unwrap_or(0),
                    },
                    window,
                    cx,
                ),
                minute: self.pill(
                    PillSpec {
                        trigger_id: id.clone(),
                        label: "Minute",
                        value: trigger.minute.to_string(),
                        options: owned_options(minute_options()),
                        placeholder: None,
                        set: |trigger, value| trigger.minute = value.parse().unwrap_or(0),
                    },
                    window,
                    cx,
                ),
                time: self.pill(
                    PillSpec {
                        trigger_id: id.clone(),
                        label: "Time",
                        value: trigger.time.clone(),
                        options: owned_options(time_options(&trigger.time)),
                        placeholder: None,
                        set: |trigger, value| trigger.time = value.to_string(),
                    },
                    window,
                    cx,
                ),
                branch: self.pill(
                    PillSpec {
                        trigger_id: id.clone(),
                        label: "Branch",
                        value: trigger.branch.clone(),
                        options: Vec::new(),
                        placeholder: Some("Select a branch"),
                        set: |trigger, value| trigger.branch = value.to_string(),
                    },
                    window,
                    cx,
                ),
                actor: self.pill(
                    PillSpec {
                        trigger_id: id.clone(),
                        label: "Actor",
                        value: if trigger.actor.is_empty() {
                            "anyone".into()
                        } else {
                            trigger.actor.clone()
                        },
                        options: options(&[("anyone", "Anyone")]),
                        placeholder: None,
                        set: |trigger, value| trigger.actor = value.to_string(),
                    },
                    window,
                    cx,
                ),
            };
            self.trigger_pills.insert(id.clone(), pills);
        }
        let project_chosen = looks_like_project(&self.draft.cwd);
        let mut branch_options: Vec<SearchableSelectOption> = self
            .branches
            .iter()
            .map(|name| SearchableSelectOption::new(name.clone(), name.clone()))
            .collect();
        if !trigger.branch.is_empty()
            && !branch_options
                .iter()
                .any(|option| option.value.as_ref() == trigger.branch)
        {
            branch_options.insert(
                0,
                SearchableSelectOption::new(trigger.branch.clone(), trigger.branch.clone()),
            );
        }
        let pills = &self.trigger_pills[&id];
        let trigger = trigger.clone();
        pills.day.update(cx, |select, cx| {
            select.set_value(trigger.day_of_week.to_string(), cx)
        });
        pills.minute.update(cx, |select, cx| {
            select.set_value(trigger.minute.to_string(), cx)
        });
        pills.time.update(cx, |select, cx| {
            select.set_options(owned_options(time_options(&trigger.time)), cx);
            select.set_value(trigger.time.clone(), cx);
        });
        pills.branch.update(cx, |select, cx| {
            select.set_options(branch_options, cx);
            select.set_value(trigger.branch.clone(), cx);
            select.set_disabled(!project_chosen, window, cx);
        });
        let actor = if trigger.actor.is_empty() {
            "anyone".to_string()
        } else {
            trigger.actor.clone()
        };
        pills
            .actor
            .update(cx, |select, cx| select.set_value(actor, cx));
        &self.trigger_pills[&id]
    }
}

fn workspace_value(mode: AutomationWorkspaceMode) -> &'static str {
    if mode == AutomationWorkspaceMode::Worktree {
        "worktree"
    } else {
        "current"
    }
}

/// `ACTION_OUTLINE`: `h-7 border border-content/15 text-content/80`.
fn outline_action(id: &'static str, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let ink = theme.colors.content;
    let border = theme.content(0.30);
    let fill = theme.content(0.10);
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .flex()
        .flex_none()
        .h(u(28.))
        .items_center()
        .gap(u(6.))
        .px(u(12.))
        .rounded(u(theme.radius.md))
        .border_1()
        .border_color(theme.content(0.15))
        .text_px(theme.text.label)
        .text_color(theme.content(0.80))
        .hover(move |s| s.border_color(border).bg(fill).text_color(ink))
}

/// `ACTION_FILLED`: `h-6.5 bg-content font-medium text-background-base`.
fn filled_action(id: &'static str, enabled: bool, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let hover = theme.content(0.80);
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .flex()
        .flex_none()
        .h(u(26.))
        .items_center()
        .gap(u(6.))
        .px(u(12.))
        .rounded(u(theme.radius.md))
        .bg(theme.colors.content)
        .text_px(theme.text.label)
        .medium()
        .text_color(theme.colors.background_base)
        .map(|button| {
            if enabled {
                button.hover(move |s| s.bg(hover))
            } else {
                button.opacity(0.4)
            }
        })
}

/// `SectionTitle`.
fn section_title(text: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .px(u(4.))
        .text_px(theme.text.label)
        .medium()
        .text_color(theme.content(0.50))
        .child(text)
}

/// `SettingsRow`.
fn settings_row(
    label: &'static str,
    hint: &'static str,
    control: impl IntoElement,
    theme: &Theme,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(u(16.))
        .px(u(16.))
        .py(u(10.))
        .child(
            div()
                .min_w_0()
                .child(
                    div()
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.75))
                        .child(label),
                )
                .child(
                    div()
                        .mt(u(2.))
                        .text_px(theme.text.caption)
                        .leading(theme.leading.snug)
                        .text_color(theme.content(0.40))
                        .child(hint),
                ),
        )
        .child(div().flex_none().child(control))
}

/// The run status pill.
fn status_pill(run: &AutomationRun, theme: &Theme) -> impl IntoElement + use<> {
    let (ink, fill) = match run_status_tone(run.status) {
        RunTone::Success => (theme.colors.success, theme.colors.success.opacity(0.12)),
        RunTone::Failure => (theme.colors.danger, theme.colors.danger.opacity(0.12)),
        RunTone::Warning => (theme.colors.warning, theme.colors.warning.opacity(0.12)),
        RunTone::Muted => (theme.content(0.50), theme.content(0.08)),
        RunTone::Info => (theme.colors.accent, theme.accent(0.12)),
    };
    let running = run.status == super::model::AutomationRunStatus::Running;
    div()
        .flex()
        .flex_none()
        .h(u(20.))
        .max_w_full()
        .items_center()
        .gap(u(4.))
        .px(u(8.))
        .rounded_full()
        .bg(fill)
        .text_px(theme.text.caption)
        .medium()
        .text_color(ink)
        .when(running, |pill| {
            pill.child(spinner_icon(
                ElementId::Name(format!("run-spin-{}", run.id).into()),
                10.,
                ink,
            ))
        })
        .child(run_status_label(run.status))
}

impl AutomationEditor {
    fn render_header(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let draft = &self.draft;
        let has_id = draft.id.is_some();
        let mut actions = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(u(8.));
        if !has_id || self.status.dirty {
            actions = actions.child(
                outline_action("automation-close", theme)
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(close) = this.on_close.clone() {
                            close(window, cx);
                        }
                    }))
                    .child(if has_id { "Reset" } else { "Cancel" }),
            );
        }
        if has_id && self.on_run.is_some() {
            let running = self.status.running;
            let mut run = outline_action("automation-run", theme);
            run = if running {
                run.opacity(0.4).child(spinner_icon(
                    "automation-run-spin",
                    14.,
                    theme.content(0.80),
                ))
            } else {
                run.on_click(cx.listener(|this, _, window, cx| {
                    if let Some(run) = this.on_run.clone() {
                        run(window, cx);
                    }
                }))
                .child(
                    icon(IconName::Play)
                        .size(u(14.))
                        .text_color(theme.content(0.80)),
                )
            };
            actions = actions.child(run.child("Run now"));
        }
        let can_submit = self.can_submit();
        let mut submit = filled_action("automation-submit", can_submit, theme);
        if self.status.saving {
            submit = submit.child(spinner_icon(
                "automation-saving",
                14.,
                theme.colors.background_base,
            ));
        }
        if can_submit {
            submit = submit.on_click(cx.listener(|this, _, window, cx| {
                if let Some(submit) = this.on_submit.clone() {
                    submit(window, cx);
                }
            }));
        }
        actions = actions.child(submit.child(if has_id { "Save" } else { "Create" }));

        let enabled = draft.enabled;
        let mut meta = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_px(theme.text.label)
            .text_color(theme.content(0.50))
            .child(
                toggle_switch(
                    "automation-enabled",
                    enabled,
                    true,
                    if enabled {
                        "Pause automation"
                    } else {
                        "Enable automation"
                    },
                    theme,
                )
                .debug_selector(|| "automation-enabled".into())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change(window, cx, |draft| draft.enabled = !enabled)
                })),
            )
            .child(
                div()
                    .flex_none()
                    .child(if enabled { "Active" } else { "Inactive" }),
            )
            .child(vertical_rule(theme).ml(u(8.)))
            .child(self.picker.clone());
        if has_id && self.on_delete.is_some() {
            let open = self.menu_open;
            let hover = theme.content(0.05);
            let ink = theme.colors.content;
            let mut menu = div().relative().child(self.menu_bounds.probe()).child(
                div()
                    .id("automation-actions")
                    .debug_selector(|| "Automation actions".into())
                    .flex()
                    .size(u(26.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .group("automation-actions")
                    .tooltip(tooltip("Automation actions"))
                    .map(|button| {
                        if open {
                            button.bg(theme.colors.selection)
                        } else {
                            button.hover(move |s| s.bg(hover))
                        }
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    }))
                    .child(
                        icon(IconName::MoreHorizontal)
                            .size(u(14.))
                            .text_color(if open { ink } else { theme.content(0.50) })
                            .group_hover("automation-actions", move |s| s.text_color(ink)),
                    ),
            );
            if open {
                let danger = theme.colors.danger_soft;
                let fill = monocode_ui::color::with_alpha(theme.colors.danger_fill, 0.15);
                let item = div()
                    .id("automation-delete")
                    .debug_selector(|| "Delete automation".into())
                    .flex()
                    .h(u(32.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .px(u(8.))
                    .rounded(u(theme.radius.lg))
                    .text_px(theme.text.body)
                    .text_color(danger.opacity(0.9))
                    .hover(move |s| s.bg(fill))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.menu_open = false;
                        cx.notify();
                        if let Some(delete) = this.on_delete.clone() {
                            delete(window, cx);
                        }
                    }))
                    .child(
                        icon(IconName::Trash2)
                            .size(u(14.))
                            .text_color(danger.opacity(0.9)),
                    )
                    .child("Delete automation");
                let outside = cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if !this.menu_bounds.contains(event.position) {
                        this.menu_open = false;
                        cx.notify();
                    }
                });
                let surface = popover_surface(
                    "automation-actions-menu",
                    Some(200.),
                    None,
                    outside,
                    div().p(u(4.)).child(item),
                );
                menu = menu.child(anchored_popover(
                    Side::Bottom,
                    4.,
                    popover_layer(cx),
                    window,
                    surface,
                ));
            }
            meta = meta.child(vertical_rule(theme)).child(menu);
        }
        let mut column = div()
            .mx_auto()
            .flex()
            .flex_col()
            .w_full()
            .max_w(u(1024.))
            .gap(u(10.))
            .px(u(32.))
            .pt(u(20.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(u(24.))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h(u(25.))
                            .items_center()
                            .text_px(theme.text.title)
                            .semibold()
                            .leading(theme.leading.tight)
                            .debug_selector(|| "Automation name".into())
                            .child(plain_input(&self.name, Some(theme.content(0.35)), cx)),
                    )
                    .child(actions),
            )
            .child(meta);
        if has_id {
            let history = self.tab == EditorTab::History;
            column = column.child(
                div()
                    .flex()
                    .h(u(36.))
                    .items_stretch()
                    .gap(u(16.))
                    .child(
                        page_tab("automation-tab-settings", "Settings", !history, theme)
                            .debug_selector(|| "automation-tab-settings".into())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_tab(EditorTab::Settings, cx)),
                            ),
                    )
                    .child(
                        page_tab("automation-tab-history", "Run history", history, theme)
                            .debug_selector(|| "automation-tab-history".into())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_tab(EditorTab::History, cx)),
                            ),
                    ),
            );
        } else {
            column = column.child(div().pb(u(20.)));
        }
        div()
            .relative()
            .w_full()
            .flex_none()
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(column)
            .into_any_element()
    }

    fn render_trigger_row(
        &mut self,
        trigger: AutomationTrigger,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pills = self.pills_for(&trigger, window, cx);
        let (day, minute, time, branch, actor) = (
            pills.day.clone(),
            pills.minute.clone(),
            pills.time.clone(),
            pills.branch.clone(),
            pills.actor.clone(),
        );
        let word = |text: &str| div().flex_none().child(text.to_string());
        let mut sentence = div()
            .flex()
            .flex_1()
            .flex_wrap()
            .min_w_0()
            .items_center()
            .gap_x(u(6.))
            .gap_y(u(4.))
            .text_px(theme.text.body)
            .text_color(theme.content(0.70));
        if trigger.kind == AutomationTriggerKind::Time {
            sentence = sentence.child(word(time_sentence_prefix(trigger.schedule_kind)));
            if trigger.schedule_kind == AutomationScheduleKind::Weekly {
                sentence = sentence.child(day).child(word("at"));
            }
            sentence = if trigger.schedule_kind == AutomationScheduleKind::Hourly {
                sentence.child(minute)
            } else {
                sentence.child(time)
            };
            let next = next_automation_run_at(&trigger, now_ms());
            sentence = sentence
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.content(0.45))
                        .child(gmt_offset_label(now_ms())),
                )
                .child(
                    div()
                        .flex_none()
                        .ml(u(4.))
                        .text_color(theme.content(0.35))
                        .debug_selector(|| "next-run".into())
                        .child(next_run_preview(next)),
                );
        } else {
            sentence = sentence.child(word(&event_sentence_stem(&trigger)));
            if trigger.event == "push_to_branch" {
                sentence = sentence.child(word("on")).child(branch);
            }
            sentence = sentence.child(word("by")).child(actor);
        }
        let id = trigger.id.clone();
        let group: SharedString = format!("trigger-row-{id}").into();
        let hover = theme.content(0.08);
        let ink = theme.colors.content;
        div()
            .id(ElementId::Name(group.clone()))
            .group(group.clone())
            .flex()
            .min_h(u(40.))
            .items_center()
            .gap(u(10.))
            .py(u(4.))
            .child(trigger_mark(trigger.kind, 14., theme.content(0.45)))
            .child(sentence)
            .child(
                div()
                    .id(ElementId::Name(format!("remove-trigger-{id}").into()))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("remove-trigger {id}")
                    })
                    .flex()
                    .flex_none()
                    .size(u(28.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .hover(move |s| s.bg(hover).opacity(1.))
                    .tooltip(tooltip("Remove trigger"))
                    .on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.remove_trigger(&id, window, cx)
                        }),
                    )
                    .child(
                        icon(IconName::X)
                            .size(u(14.))
                            .text_color(theme.content(0.35)),
                    )
                    .text_color(ink),
            )
            .into_any_element()
    }

    fn render_trigger_menu(
        &self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let categories = self.visible_categories(cx);
        let search = div()
            .flex()
            .flex_none()
            .h(u(44.))
            .items_center()
            .gap(u(10.))
            .px(u(12.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .text_px(theme.text.body)
                    .child(plain_input(
                        &self.trigger_query,
                        Some(theme.content(0.35)),
                        cx,
                    )),
            );
        let mut list = div().flex().flex_col().p(u(4.));
        let hover = theme.content(0.05);
        let ink = theme.colors.content;
        for (kind, label) in categories.iter().copied() {
            let ready = self.connections.ready(kind);
            let open = self.trigger_category == Some(kind);
            let row = div()
                .id(ElementId::Name(
                    format!("trigger-category-{}", kind.as_str()).into(),
                ))
                .debug_selector(move || format!("trigger-category {label}"))
                .flex()
                .h(u(36.))
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(8.))
                .rounded(u(theme.radius.lg))
                .text_px(theme.text.body)
                .map(|row| {
                    if !ready {
                        row.text_color(theme.content(0.35))
                            .tooltip(tooltip(format!("Connect {label} in Settings")))
                    } else if open {
                        row.bg(theme.colors.selection).text_color(ink)
                    } else {
                        row.text_color(theme.content(0.70))
                            .hover(move |s| s.bg(hover).text_color(ink))
                    }
                })
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.show_category(kind, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.connections.ready(kind) {
                        this.show_category(kind, cx);
                    }
                }))
                .child(trigger_mark(
                    kind,
                    14.,
                    if ready {
                        theme.content(0.70)
                    } else {
                        theme.content(0.35)
                    },
                ))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(if ready {
                    icon(IconName::ChevronRight)
                        .size(u(14.))
                        .text_color(theme.content(0.45))
                        .into_any_element()
                } else {
                    div()
                        .flex_none()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.35))
                        .child("Not connected")
                        .into_any_element()
                });
            let mut cell = div().relative().child(row);
            if open && ready {
                let mut events = div()
                    .flex()
                    .flex_col()
                    .p(u(4.))
                    .child(self.submenu_bounds.probe());
                for event in trigger_events(kind) {
                    let value = event.value;
                    let event_label = event.label;
                    events = events.child(
                        div()
                            .id(ElementId::Name(
                                format!("trigger-event-{}-{value}", kind.as_str()).into(),
                            ))
                            .debug_selector(move || format!("trigger-event {event_label}"))
                            .flex()
                            .h(u(36.))
                            .w_full()
                            .items_center()
                            .px(u(8.))
                            .rounded(u(theme.radius.lg))
                            .text_px(theme.text.body)
                            .text_color(theme.content(0.75))
                            .hover(move |s| s.bg(hover).text_color(ink))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_trigger(kind, value, window, cx)
                            }))
                            .child(event_label),
                    );
                }
                let submenu = popover_surface(
                    ElementId::Name(format!("trigger-events-{}", trigger_name(kind)).into()),
                    Some(220.),
                    Some(360.),
                    |_, _, _| {},
                    events,
                );
                cell = cell.child(anchored_popover(
                    Side::Right,
                    4.,
                    submenu_layer(cx),
                    window,
                    submenu,
                ));
            }
            list = list.child(cell);
        }
        if categories.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(20.))
                    .text_center()
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.40))
                    .child("No matching triggers"),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            if !this.add_trigger_bounds.contains(event.position)
                && !this.submenu_bounds.contains(event.position)
            {
                this.close_trigger_menu(cx);
            }
        });
        popover_surface(
            "trigger-menu",
            Some(250.),
            None,
            outside,
            div()
                .flex()
                .flex_col()
                .debug_selector(|| "trigger-menu".into())
                .child(search)
                .child(list),
        )
        .into_any_element()
    }

    fn render_settings(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.folders_changed {
            self.folders_changed = false;
            self.sync_folder_options(window, cx);
        }
        let triggers = self.draft.triggers.clone();
        let mut box_ = div()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10));
        if !triggers.is_empty() {
            let mut list = div().px(u(12.)).py(u(6.));
            for trigger in triggers.iter().cloned() {
                list = list.child(self.render_trigger_row(trigger, theme, window, cx));
            }
            box_ = box_
                .child(list)
                .child(div().border_t_1().border_color(theme.content(0.08)));
        }
        let full = triggers.len() >= MAX_TRIGGERS;
        let hover = theme.content(0.04);
        let ink = theme.colors.content;
        let mut add = div()
            .relative()
            .child(self.add_trigger_bounds.probe())
            .child(
                div()
                    .id("add-trigger")
                    .debug_selector(|| "Add Trigger".into())
                    .flex()
                    .h(u(48.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .px(u(12.))
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.55))
                    .group("add-trigger")
                    .map(|button| {
                        if full {
                            button.opacity(0.4)
                        } else {
                            button.hover(move |s| s.bg(hover).text_color(ink)).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_trigger_menu(window, cx)
                                }),
                            )
                        }
                    })
                    .child(
                        icon(IconName::Plus)
                            .size(u(14.))
                            .text_color(theme.content(0.55))
                            .group_hover("add-trigger", move |s| s.text_color(ink)),
                    )
                    .child("Add Trigger"),
            );
        if self.trigger_open {
            let menu = self.render_trigger_menu(theme, window, cx);
            add = add.child(anchored_popover(
                Side::Bottom,
                4.,
                popover_layer(cx),
                window,
                menu,
            ));
        }
        box_ = box_.child(add);

        let mut toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(4.))
            .child(self.model.clone());
        if self.data.model_controls_beside(cx) {
            toolbar = toolbar.child(self.pills.clone());
        }
        if self.draft.harness != HarnessId::Fx {
            toolbar = toolbar.child(self.access.clone());
        }
        let instructions = div()
            .relative()
            .mt(u(12.))
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.03))
            .child(self.prompt.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(4.))
                    .px(u(8.))
                    .pb(u(8.))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .child(toolbar),
                    ),
            );
        let folder = self
            .folder
            .as_ref()
            .map(|(_, select)| select.clone().into_any_element())
            .unwrap_or_else(|| div().into_any_element());
        let session = div()
            .mt(u(12.))
            .flex()
            .flex_col()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .child(settings_row(
                "Working copy",
                "This repo, or a fresh worktree",
                self.workspace.clone(),
                theme,
            ))
            .child(div().h(px(1.)).bg(theme.content(0.07)))
            .child(settings_row(
                "Conversation",
                "New chat, or continue the last run",
                self.conversation.clone(),
                theme,
            ))
            .child(div().h(px(1.)).bg(theme.content(0.07)))
            .child(settings_row(
                "Session folder",
                "Where runs appear in the sidebar",
                folder,
                theme,
            ));
        let advanced_open = self.advanced_open;
        let mut advanced = div()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .child(
                div()
                    .id("automation-advanced")
                    .debug_selector(|| "automation-advanced".into())
                    .flex()
                    .min_h(u(56.))
                    .items_center()
                    .justify_between()
                    .gap(u(12.))
                    .px(u(16.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced_open = !this.advanced_open;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_px(theme.text.body)
                                    .medium()
                                    .text_color(theme.content(0.75))
                                    .child("Advanced"),
                            )
                            .child(
                                div()
                                    .mt(u(2.))
                                    .text_px(theme.text.caption)
                                    .text_color(theme.content(0.40))
                                    .child("Catch-up window for missed runs"),
                            ),
                    )
                    .child(
                        icon(if advanced_open {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(u(14.))
                        .text_color(theme.content(0.40)),
                    ),
            );
        if advanced_open {
            advanced = advanced.child(div().border_t_1().border_color(theme.content(0.08)).child(
                settings_row(
                    "Missed-run grace",
                    "Catch up if a scheduled run was missed",
                    self.grace.clone(),
                    theme,
                ),
            ));
        }
        div()
            .mx_auto()
            .flex()
            .flex_col()
            .w_full()
            .max_w(u(1024.))
            .gap(u(32.))
            .px(u(32.))
            .pt(u(20.))
            .pb(u(40.))
            .child(
                div()
                    .child(section_title("Triggers", theme))
                    .child(box_.mt(u(12.))),
            )
            .child(
                div()
                    .child(section_title("Instructions", theme))
                    .child(instructions)
                    .child(
                        div()
                            .mt(u(8.))
                            .px(u(4.))
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.35))
                            .child("Skills, @file references, and built-in commands work here."),
                    ),
            )
            .child(div().child(section_title("Session", theme)).child(session))
            .child(advanced)
            .into_any_element()
    }

    fn render_history(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let runs = &self.status.runs;
        let mut section = div()
            .mx_auto()
            .w_full()
            .max_w(u(1024.))
            .px(u(32.))
            .pt(u(20.))
            .pb(u(40.))
            .child(section_title("Run history", theme));
        if runs.is_empty() {
            return section
                .child(
                    div()
                        .mt(u(12.))
                        .rounded(u(theme.radius.md))
                        .border_1()
                        .border_dashed()
                        .border_color(theme.content(0.10))
                        .px(u(16.))
                        .py(u(64.))
                        .text_center()
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.40))
                        .child("This automation has not run yet."),
                )
                .into_any_element();
        }
        let grid = |row: gpui::Div| row.flex().items_center().gap(u(16.)).px(u(16.));
        let cell = |width: f32| div().flex_none().w(u(width)).min_w_0();
        let header = grid(div())
            .h(u(40.))
            .text_px(theme.text.caption)
            .text_color(theme.content(0.40))
            .child(div().flex_1().min_w_0().child("Trigger"))
            .child(cell(RUN_COLUMNS[0]).child("Triggered"))
            .child(cell(RUN_COLUMNS[1]).child("Status"))
            .child(cell(RUN_COLUMNS[2]).text_right().child("Duration"));
        let mut table = div()
            .mt(u(12.))
            .overflow_hidden()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.10))
            .child(header);
        let hover = theme.content(0.05);
        let now = now_ms();
        for run in runs.iter().take(100) {
            let (kind, label) = run_trigger_meta(run, &self.draft);
            let session = run.session_id.clone();
            let hint = if session.is_some() {
                "Open session".to_string()
            } else {
                run.error
                    .clone()
                    .unwrap_or_else(|| "This run has no session yet".into())
            };
            let at = if run.scheduled_for != 0 {
                run.scheduled_for
            } else {
                run.created_at
            };
            let id = run.id.clone();
            let row = grid(div())
                .h(u(44.))
                .w_full()
                .text_px(theme.text.body)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(u(8.))
                        .text_color(theme.colors.content)
                        .child(trigger_mark(kind, 14., theme.content(0.40)))
                        .child(div().min_w_0().truncate().child(label)),
                )
                .child(
                    cell(RUN_COLUMNS[0])
                        .truncate()
                        .text_color(theme.content(0.70))
                        .child(format_automation_run_at(at)),
                )
                .child(cell(RUN_COLUMNS[1]).flex().child(status_pill(run, theme)))
                .child(
                    cell(RUN_COLUMNS[2])
                        .text_right()
                        .tabular()
                        .text_color(theme.content(0.55))
                        .child(format_automation_run_duration(run.into(), now)),
                );
            let mut row = div()
                .id(ElementId::Name(format!("run-{id}").into()))
                .debug_selector(move || format!("run {id}"))
                .border_t_1()
                .border_color(theme.content(0.08))
                .tooltip(tooltip(hint))
                .child(row);
            if let Some(session) = session {
                row = row.hover(move |s| s.bg(hover)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        if let Some(open) = this.on_open_session.clone() {
                            open(&session, window, cx);
                        }
                    },
                ));
            }
            table = table.child(row);
        }
        section = section.child(table);
        section.into_any_element()
    }
}

impl Focusable for AutomationEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AutomationEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let header = self.render_header(&theme, window, cx);
        let showing_history = self.draft.id.is_some() && self.tab == EditorTab::History;
        let body = if showing_history {
            self.render_history(&theme, cx)
        } else {
            self.render_settings(&theme, window, cx)
        };
        let _ = &self.projects;
        div()
            .id("automation-editor")
            .key_context("AutomationEditor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && (this.trigger_open || this.menu_open) {
                    this.trigger_open = false;
                    this.trigger_category = None;
                    this.menu_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(header)
            .child(
                div()
                    .id("automation-editor-body")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(body),
            )
    }
}
