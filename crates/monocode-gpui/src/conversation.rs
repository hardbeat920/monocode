mod accounts;
mod approvals;
mod attachment_drop;
mod background_tasks;
mod btw;
mod commands;
pub(crate) mod composer_features;
mod composer_shelf;
mod edit_last_turn;
mod follow_up;
pub(crate) mod git_actions;
mod history;
mod mobile;
mod mono;
mod notifications;
mod remote;
pub(crate) mod remote_plans;
mod response_menu;
#[cfg(test)]
mod submission_tests;
mod title;
use crate::{
    composer_motion::ComposerMotion,
    git_picker::{GitPicker, Mode as GitMode, Picked as GitPicked},
    input,
    pickers::{
        ModelList, ModelPicked, cached_model_name, model_display_name, model_name, remember_model,
    },
    popover::{
        Align, Anchor, AnchorSlot, Flyout, Menu, MenuAction, MenuEvent, MenuItem, MenuStyle, Side,
        menu_popover, popover,
    },
    response_actions::{Request as ResponseRequest, Transfer},
    runtime::{Database, locked, run_turn},
    theme::*,
    transcript::TranscriptView,
};
pub(crate) use accounts::UsageResumed;
use gpui::{
    AnyElement, Context, CursorStyle, Entity, EventEmitter, Focusable, MouseButton, Role,
    SharedString, StyleRefinement, Subscription, Window, canvas, div, prelude::*, px,
};
pub(crate) use mobile::{Waiting as MobileWaiting, mobile_access_modes};
pub(crate) use mono::QueueChanged as MonoQueueChanged;
pub(crate) use mono::Submitted as MonoSubmitted;
use monocode_core::{
    agent::{AccessMode, Approval, Control, Decision, Settings},
    catalog::{self, Model},
};
use monocode_core::{provider::Kind, store::Session};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
// Keep empty and docked composers aligned with the transcript column. Below
// this width the empty session drops its title.
const COMPOSER_COLUMN: f32 = COLUMN;
// RUNTIME_MODE_LABEL and RUNTIME_MODE_HINT from the React session model,
// with AccessPicker's icons; Full access is amber.
const ACCESS_MODES: &[(&str, &str, &str, &str)] = &[
    (
        "supervised",
        "Supervised",
        "Ask before commands and file changes.",
        "square-lock",
    ),
    (
        "auto-accept-edits",
        "Auto-accept edits",
        "Auto-approve edits, ask before other actions.",
        "pencil-edit",
    ),
    (
        "auto",
        "Auto",
        "An AI reviewer can approve or deny actions.",
        "sparkles",
    ),
    (
        "full-access",
        "Full access",
        "Allow commands, edits, and supported MCP confirmations in non-plan turns without prompts.",
        "shield-alert",
    ),
];
// Tailwind v4 accents the composer menus tint their icons with.
const YELLOW_300: u32 = 0xffdf20;
const SKY_300: u32 = 0x74d4ff;
const FUCHSIA_300: u32 = 0xf4a8ff;
const AMBER_400: u32 = 0xffb900;
fn tint(color: u32, alpha: f32) -> gpui::Hsla {
    let mut c: gpui::Hsla = gpui::rgb(color).into();
    c.a = alpha;
    c
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerMenu {
    Add,
    Model,
    /// ModelPicker's recent models, on right-click or `App: Switch Model`.
    Recent,
    Access,
    Account,
    Workspace,
    /// A Mono's model setting pill: 0 is effort, then each model control.
    Setting(usize),
    /// The commit pill's chevron above the composer.
    GitActions,
}
struct OpenMenu {
    kind: ComposerMenu,
    menu: Entity<Menu>,
    _events: Vec<Subscription>,
}
use response_menu::ResponseMenu;
pub(crate) struct SessionChanged;
pub(crate) struct ChooseFolder(pub(crate) AnchorSlot);
/// The review card asked to open the session's recorded changes.
pub(crate) struct OpenSessionChanges(pub(crate) crate::review::ReviewTab);
pub(crate) struct Conversation {
    editing_last_turn: edit_last_turn::State,
    btw: Option<Entity<btw::Btw>>,
    /// The composer box the /btw sheet morphs out of, and how visible the
    /// composer is under the sheet.
    btw_home: (AnchorSlot, f32),
    _btw_observer: Option<Subscription>,
    pub(crate) summary: Session,
    remote_state: remote::State,
    remote_plans: remote_plans::State,
    store: Database,
    pub(crate) transcript: Entity<TranscriptView>,
    pub(crate) review: Entity<crate::session_review::SessionReview>,
    _review_events: Subscription,
    _review_observation: Subscription,
    pub(crate) input: Entity<input::Input>,
    completion: Entity<crate::composer_completion::Completion>,
    _completion_event: Subscription,
    _command_removal: Subscription,
    _draft_observation: Subscription,
    _input_focus: Option<[Subscription; 2]>,
    draft_has_value: bool,
    folder_selected: bool,
    follow_ups: follow_up::FollowUps,
    background: background_tasks::BackgroundTasks,
    attachments: Vec<std::path::PathBuf>,
    pending_attachments: Vec<std::path::PathBuf>,
    attachment_photos: std::collections::HashMap<
        std::path::PathBuf,
        Entity<crate::transcript::attachments::Photo>,
    >,
    attaching: bool,
    file_drag: bool,
    attachment_error: Option<String>,
    app_environment: std::collections::HashMap<String, String>,
    /// The running turn's policy, independent of the next composer's mode.
    pub(crate) app_read_only: bool,
    planning_orchestration: bool,
    pub(crate) orchestration_plan: Option<monocode_core::orchestration::Plan>,
    orchestration_error: Option<String>,
    pub(crate) orchestration_progress: Option<String>,
    status: String,
    loaded: bool,
    history_unloaded: bool,
    history_actions: std::collections::VecDeque<history::HistoryAction>,
    pending_search: Option<(usize, String)>,
    loading: bool,
    pub(crate) busy: bool,
    /// The UI is idle; serialize the next launch behind the worker's last save.
    stopping: bool,
    notification_cancelled: bool,
    /// The store's saved-draft flag, until the conversation loads its own.
    pub(crate) draft_hint: bool,
    /// The provider model for the next turn; `None` uses its configured default.
    model: Option<String>,
    resolved_model: Option<String>,
    pub(crate) settings: Settings,
    pub(crate) mono_turn: u64,
    pub(crate) submission_generation: u64,
    mono_ui: mono::ChatState,
    project_appearance: monocode_core::project_appearance::ProjectAppearance,
    classic_layout: bool,
    catalog: Vec<Model>,
    catalog_epoch: u64,
    account_switching: bool,
    _usage_clock: gpui::Task<()>,
    _account_usage: Option<Subscription>,
    approvals: std::collections::VecDeque<Approval>,
    approval_ui: approvals::State,
    questions:
        std::collections::VecDeque<(Entity<crate::questions::QuestionCard>, Vec<Subscription>)>,
    question_overlay: crate::questions::Overlay,
    controls: Option<std::sync::mpsc::Sender<Control>>,
    branch: Option<String>,
    git: Option<monocode_core::git::Snapshot>,
    git_picker: Option<Entity<GitPicker>>,
    git_events: Option<Subscription>,
    preparing_worktree: bool,
    context: Option<monocode_core::context::Usage>,
    context_hovered: bool,
    menu: Option<OpenMenu>,
    add_anchor: AnchorSlot,
    model_anchor: AnchorSlot,
    access_anchor: AnchorSlot,
    account_anchor: AnchorSlot,
    workspace_anchor: AnchorSlot,
    git_action_anchor: AnchorSlot,
    /// Files this session's edit tools changed, across its whole history.
    session_edits: Vec<String>,
    /// Where `session_edits` stand in Git: uncommitted ones show the pill.
    session_git: monocode_core::git::SessionChanges,
    /// The session's latest action commit is on no remote yet: the pill offers Push.
    session_unpushed: bool,
    session_edits_generation: u64,
    branch_anchor: AnchorSlot,
    context_anchor: AnchorSlot,
    composer_anchor: AnchorSlot,
    composer_motion: ComposerMotion,
    narrow: bool,
    // A split or review pane sits beside this chat; only then does a narrow
    // pane rest the composer where it docks.
    beside_pane: bool,
    arcade: Option<Entity<crate::arcade::Arcade>>,
    runner: Option<Entity<crate::composer_runner::Runner>>,
    cancel: Arc<AtomicBool>,
    title_cancel: Option<Arc<AtomicBool>>,
    _submission: Subscription,
    _alternate_submission: Subscription,
    response_events: Option<Subscription>,
    response_menu: Option<ResponseMenu>,
    response_request: Option<(usize, monocode_core::response::TransferKind, Instant)>,
}
impl Drop for Conversation {
    fn drop(&mut self) {
        if !self.summary.cwd.starts_with("remote://") {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.cancel_title_generation();
    }
}
impl Conversation {
    /// Seed workspace metadata before an unloaded blank can render or submit.
    /// Loaded/running conversations keep their current agent settings.
    pub(crate) fn restore_worktree(
        &mut self,
        path: Option<String>,
        branch: Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if !self.loaded && self.settings.revision == 0 && self.settings.worktree_cwd != path {
            self.settings.worktree_cwd = path;
            self.git = self
                .settings
                .worktree_cwd
                .as_deref()
                .and_then(crate::git_cache::get);
            self.branch = branch.or_else(|| self.git.as_ref().and_then(|s| s.current.clone()));
            self.sync_tool_paths(cx);
            cx.notify();
        } else if !self.loaded && branch.is_some() && self.branch != branch {
            self.branch = branch;
            cx.notify();
        }
        self.settings.worktree_cwd.clone()
    }
    pub(crate) fn settings_models(&self) -> Vec<(String, String)> {
        self.catalog
            .iter()
            .map(|model| {
                (
                    model.id.clone(),
                    model_display_name(
                        Kind::from_id(&self.summary.provider),
                        &model.id,
                        &model.name,
                    )
                    .to_string(),
                )
            })
            .collect()
    }
    pub(crate) fn set_classic_layout(&mut self, classic: bool, cx: &mut Context<Self>) {
        if self.classic_layout != classic {
            self.classic_layout = classic;
            cx.notify();
        }
    }
    pub(crate) fn set_beside_pane(&mut self, beside: bool, cx: &mut Context<Self>) {
        if self.beside_pane != beside {
            self.beside_pane = beside;
            cx.notify();
        }
    }
    pub(crate) fn set_project_appearance(
        &mut self,
        appearance: monocode_core::project_appearance::ProjectAppearance,
        cx: &mut Context<Self>,
    ) {
        if self.project_appearance != appearance {
            self.project_appearance = appearance;
            cx.notify();
        }
    }
    /// SessionCardPreview.tsx's compact model/status header and workspace details.
    /// Its latest message is saved as a draft, not sent (`session.draft`).
    pub(crate) fn has_saved_draft(&self) -> bool {
        if self.loaded {
            self.follow_ups.state.draft.is_some()
        } else {
            self.draft_hint
        }
    }
    pub(crate) fn archive_shortcut_blocked(&self, cx: &gpui::App) -> bool {
        self.menu.is_some()
            || self.response_menu.is_some()
            || self.git_picker.is_some()
            || self.needs_approval()
            || self.input.read(cx).is_composing()
    }
    pub(crate) fn needs_approval(&self) -> bool {
        !self.approvals.is_empty() || !self.questions.is_empty()
    }
    pub(crate) fn responding(&self) -> bool {
        self.busy && !self.stopping
    }
    pub(crate) fn sidebar_details(&self) -> Option<(SharedString, Option<String>)> {
        self.loaded
            .then(|| (self.model_label(), self.branch.clone()))
    }
    pub(crate) fn preview(&self, done: bool) -> AnyElement {
        let kind = Kind::from_id(&self.summary.provider);
        let model = self.resolved_model.as_deref().or(self.model.as_deref());
        let name = self
            .catalog
            .iter()
            .find(|entry| Some(entry.id.as_str()) == model)
            .map(|entry| model_display_name(kind, &entry.id, &entry.name))
            .unwrap_or_else(|| model_name(kind, model));
        let cwd = self
            .settings
            .worktree_cwd
            .as_deref()
            .unwrap_or(&self.summary.cwd);
        let directory = std::env::var_os("HOME")
            .and_then(|home| {
                Path::new(cwd).strip_prefix(home).ok().map(|path| {
                    if path.as_os_str().is_empty() {
                        "~".to_owned()
                    } else {
                        format!("~/{}", path.display())
                    }
                })
            })
            .unwrap_or_else(|| cwd.to_owned());
        let linked =
            self.settings.worktree_cwd.is_some() || self.git.as_ref().is_some_and(|git| git.linked);
        let detail = |label: &'static str, value: String| {
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(content(0.45))
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(content(0.8))
                        .child(value),
                )
        };
        div()
            .px_3()
            .py_2p5()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.))
                            .text_color(content(0.55))
                            .child(provider_icon(&self.summary.provider, 14.))
                            .child(div().min_w_0().truncate().child(name)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .text_color(if self.needs_approval() {
                                gpui::rgb(0xfbbf24).into()
                            } else if self.responding() {
                                accent_color()
                            } else if done {
                                gpui::rgb(0x34d399).into()
                            } else {
                                content(0.45)
                            })
                            .child(if self.needs_approval() {
                                "Needs approval".to_owned()
                            } else if self.busy {
                                "Working…".to_owned()
                            } else if done {
                                "Done".to_owned()
                            } else if self.loading {
                                "Loading…".to_owned()
                            } else {
                                monocode_core::time::relative(
                                    self.summary.updated_at,
                                    monocode_core::time::now_ms(),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .mt_1p5()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(content(1.))
                    .child(self.display_title()),
            )
            .child(
                div()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(content(0.08))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .when_some(self.branch.clone(), |d, branch| {
                        d.child(detail("Branch", branch))
                    })
                    .child(detail(
                        if linked { "Worktree" } else { "Directory" },
                        directory,
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn new(summary: Session, store: Database, cx: &mut Context<Self>) -> Self {
        let input = cx.new(input::Input::new);
        // A new tab shows its folder's checkout from the first frame; the
        // load below confirms it rather than popping it in.
        let git = (!summary.has_messages)
            .then(|| crate::git_cache::get(&summary.cwd))
            .flatten();
        let completion = cx.new(|cx| {
            crate::composer_completion::Completion::new(input.clone(), summary.cwd.clone(), cx)
                .with_store(store.clone())
        });
        completion.update(cx, |completion, cx| {
            completion.set_provider(&summary.provider, cx)
        });
        let completion_event = cx.subscribe(
            &completion,
            |this, _, event: &crate::composer_completion::ModePicked, cx| {
                if event.0 == "add-to-folder" {
                    this.request_folder(cx);
                } else {
                    this.enable_mode(event.0, cx);
                }
            },
        );
        let command_removal = cx.subscribe(&input, |this, _, event: &input::CommandRemoved, cx| {
            this.clear_mode(event.0, cx)
        });
        let draft_observation = cx.observe(&input, |this, input, cx| {
            let content = input.read(cx).content.clone();
            let folder = monocode_core::composer::consume_command(&content, "add-to-folder");
            if folder.is_none() {
                this.folder_selected = false;
            }
            if !this.folder_selected
                && folder == Some("")
                && content.ends_with(' ')
                && !input.read(cx).is_composing()
            {
                input.update(cx, |input, cx| {
                    input.set_text(content.trim_end().into(), cx)
                });
                this.request_folder(cx);
            }
            let has_value = input.read(cx).has_message_text();
            if this.draft_has_value != has_value {
                this.draft_has_value = has_value;
                cx.notify();
            }
            this.open_typed_btw(cx);
        });
        input.update(cx, |input, _| input.set_alternate_submit(true));
        let submission = cx.subscribe(&input, |this, _, _: &input::Submitted, cx| this.send(cx));
        let alternate_submission = cx
            .subscribe(&input, |this, _, _: &input::AlternateSubmitted, cx| {
                this.send_follow_up(true, cx)
            });
        let review =
            cx.new(|_| crate::session_review::SessionReview::new(summary.id, summary.cwd.clone()));
        let review_events = cx.subscribe(&review, |this, review, event, cx| match event {
            crate::session_review::SessionReviewEvent::Open { path } => {
                let (session, cwd) = review.read(cx).session();
                cx.emit(OpenSessionChanges(crate::review::ReviewTab::Session {
                    cwd: cwd.to_owned(),
                    session,
                    path: path.clone(),
                }));
            }
            // Undo and Keep may rewrite files; the checkout's views reread.
            crate::session_review::SessionReviewEvent::Changed => {
                this.refresh_git(cx);
                cx.emit(SessionChanged);
            }
        });
        let review_observation = cx.observe(&review, |this, review, cx| {
            let shown = review.read(cx).shown();
            this.transcript
                .update(cx, |view, cx| view.set_review_shown(shown, cx));
            cx.notify();
        });
        let transcript = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.identity(&summary.provider, Kind::from_id(&summary.provider).label());
            view.file_link_base = Some(summary.cwd.clone());
            view.set_review(review.clone().into());
            view
        });
        let account = crate::accounts::get(cx)
            .map(|center| {
                center
                    .read(cx)
                    .selected(Kind::from_id(&summary.provider), &summary.cwd)
            })
            .unwrap_or_default();
        let usage_clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                if this.update(cx, |this, cx| this.tick_usage(cx)).is_err() {
                    break;
                }
            }
        });
        let account_usage = crate::accounts::get(cx).map(|center| {
            cx.observe(&center, |this, _, cx| {
                if this.settings.usage_limit.is_some() {
                    this.tick_usage(cx);
                }
            })
        });
        Self {
            editing_last_turn: Default::default(),
            btw: None,
            btw_home: (AnchorSlot::default(), 1.),
            _btw_observer: None,
            store,
            mono_turn: 0,
            submission_generation: 0,
            mono_ui: mono::ChatState::default(),
            transcript,
            review,
            _review_events: review_events,
            _review_observation: review_observation,
            input,
            completion,
            _completion_event: completion_event,
            _command_removal: command_removal,
            _draft_observation: draft_observation,
            _input_focus: None,
            draft_has_value: false,
            folder_selected: false,
            follow_ups: Default::default(),
            background: Default::default(),
            attachments: Vec::new(),
            pending_attachments: Vec::new(),
            attachment_photos: Default::default(),
            attaching: false,
            file_drag: false,
            attachment_error: None,
            app_environment: Default::default(),
            app_read_only: false,
            planning_orchestration: false,
            orchestration_plan: None,
            orchestration_error: None,
            orchestration_progress: None,
            status: "Loading conversation…".into(),
            loaded: false,
            history_unloaded: false,
            history_actions: Default::default(),
            pending_search: None,
            loading: false,
            busy: false,
            stopping: false,
            notification_cancelled: false,
            draft_hint: false,
            model: std::env::var(format!(
                "MONOCODE_{}_MODEL",
                summary.provider.to_uppercase()
            ))
            .ok(),
            resolved_model: None,
            settings: Settings {
                account,
                access: default_access(cx),
                effort: preferred_effort(&summary.provider, cx),
                ..Default::default()
            },
            project_appearance: Default::default(),
            classic_layout: false,
            catalog: Vec::new(),
            catalog_epoch: 0,
            account_switching: false,
            _usage_clock: usage_clock,
            _account_usage: account_usage,
            approvals: Default::default(),
            approval_ui: Default::default(),
            questions: Default::default(),
            question_overlay: Default::default(),
            controls: None,
            branch: git.as_ref().and_then(|s| s.current.clone()),
            git,
            git_picker: None,
            git_events: None,
            preparing_worktree: false,
            context: None,
            context_hovered: false,
            menu: None,
            add_anchor: AnchorSlot::default(),
            model_anchor: AnchorSlot::default(),
            access_anchor: AnchorSlot::default(),
            account_anchor: AnchorSlot::default(),
            workspace_anchor: AnchorSlot::default(),
            git_action_anchor: AnchorSlot::default(),
            session_edits: Vec::new(),
            session_git: Default::default(),
            session_unpushed: false,
            session_edits_generation: 0,
            branch_anchor: AnchorSlot::default(),
            context_anchor: AnchorSlot::default(),
            composer_anchor: AnchorSlot::default(),
            composer_motion: ComposerMotion::default(),
            narrow: false,
            beside_pane: false,
            arcade: None,
            runner: None,
            cancel: Arc::new(AtomicBool::new(false)),
            title_cancel: None,
            _submission: submission,
            _alternate_submission: alternate_submission,
            response_events: None,
            response_menu: None,
            response_request: None,
            summary,
            remote_state: remote::State::default(),
            remote_plans: remote_plans::State::default(),
        }
    }
    pub(crate) fn reveal_search(&mut self, row: usize, query: String, cx: &mut Context<Self>) {
        if !self.loaded || self.history_unloaded {
            self.pending_search = Some((row, query));
            self.load(cx);
            return;
        }
        self.transcript
            .update(cx, |view, cx| view.reveal_saved_search(row, &query, cx));
    }
    /// Reads this conversation's older turns for the transcript as it scrolls.
    pub(crate) fn history_pager(&self) -> crate::transcript::HistoryPager {
        let store = self.store.clone();
        let id = self.summary.id;
        Arc::new(move |before, all| locked(&store)?.load_before(id, before, all))
    }
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        if self.is_remote() {
            self.load_remote(cx);
            return;
        }
        if self.history_unloaded {
            self.reload_history(cx);
            return;
        }
        if self.loaded || self.loading {
            return;
        }
        self.loading = true;
        self.refresh_catalog(cx);
        let store = self.store.clone();
        let id = self.summary.id;
        let mono = self.settings.mono.is_some();
        let work = cx.background_executor().spawn(async move {
            let settings = locked(&store).and_then(|s| s.agent_settings(id));
            (
                locked(&store)
                    .and_then(|s| {
                        let history = s.load(id)?;
                        let edits = if mono {
                            Vec::new()
                        } else {
                            git_actions::restored_session_edits(&s, id, &history.rows)?
                        };
                        Ok((history, edits))
                    })
                    .map(|(history, edits)| {
                        (
                            crate::transcript::PreparedHistory::new(history.rows, mono),
                            edits,
                        )
                    }),
                settings,
                locked(&store).and_then(|s| s.context_usage(id)),
                locked(&store).and_then(|s| s.follow_ups(id)),
                locked(&store).and_then(|s| s.history_before(id)),
            )
        });
        cx.spawn(async move |this, cx| {
            let (result, settings, context, follow_ups, before) = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.context = context.ok().flatten();
                match follow_ups {
                    Ok(state) => {
                        this.follow_ups.persisted = state.revision;
                        this.follow_ups.state = state;
                        let draft = this.follow_ups.state.draft.clone();
                        if draft.is_some() {
                            this.summary.has_messages = true;
                        }
                        this.transcript
                            .update(cx, |view, cx| view.set_saved_draft(draft, cx));
                    }
                    Err(error) => {
                        this.status = error;
                        this.load_git(cx);
                        return;
                    }
                }
                // The composer may have a newer revision while history loads;
                // historical turn provenance still belongs to the loaded rows.
                if let Ok(Some(settings)) = &settings {
                    this.transcript
                        .update(cx, |view, _| view.set_turns(settings.turns.clone()));
                    this.settings.turns = settings.turns.clone();
                }
                if let Ok(Some(mut settings)) = settings
                    && settings.revision >= this.settings.revision
                {
                    if !this.summary.has_messages
                        && settings.revision == 0
                        && settings.account.id == monocode_core::accounts::DEFAULT
                    {
                        settings.account = crate::accounts::get(cx)
                            .map(|center| {
                                center.read(cx).selected(
                                    Kind::from_id(&this.summary.provider),
                                    &this.summary.cwd,
                                )
                            })
                            .unwrap_or_default();
                    }
                    this.model = settings.model.clone();
                    let mono = this.settings.mono.take();
                    let environment = std::mem::take(&mut this.settings.environment);
                    this.settings = settings;
                    this.attach_account(cx);
                    this.settings.mono = mono;
                    this.settings.environment = environment;
                }
                match result {
                    Ok((history, edits)) => {
                        this.remember_session_edits(edits);
                        let pager = this.history_pager();
                        this.transcript.update(cx, |v, cx| {
                            v.install_prepared_history(history, cx);
                            v.set_history_pager(before.ok().flatten(), Some(pager));
                        });
                        this.status = "Ready".into();
                        this.loaded = true;
                        if let Some((row, query)) = this.pending_search.take() {
                            this.reveal_search(row, query, cx);
                        }
                        this.sync_review(cx);
                        this.run_history_actions(cx);
                        this.pump_follow_ups(cx);
                    }
                    Err(e) => this.status = e,
                }
                // Git can take tens of milliseconds; history shows first.
                if let Some(git) = crate::git_cache::get(this.execution_cwd()) {
                    this.branch = git.current.clone();
                    this.git = Some(git);
                }
                this.load_git(cx);
                this.mono_flush_external(cx);
                cx.emit(SessionChanged);
                cx.notify();
            });
        })
        .detach();
    }
    /// The quick composer's files, sent with its prompt.
    pub(crate) fn quick_attach(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !paths.is_empty() {
            self.add_attachments(paths, cx);
        }
    }
    pub(crate) fn quick_submit(
        &mut self,
        prompt: String,
        settings: Settings,
        cx: &mut Context<Self>,
    ) {
        if self.editing_last_turn.pending() {
            self.defer_edit_transfer(settings, Some(prompt), cx);
            return;
        }
        if self.needs_history_restore() || self.stopping {
            self.defer_history_action(
                move |this, cx| {
                    if this.preserve_new_composer(cx) {
                        this.install_transfer(settings, None, cx);
                        this.enqueue_restored_prompt(prompt, cx);
                    } else {
                        this.quick_submit(prompt, settings, cx);
                    }
                },
                cx,
            );
            return;
        }
        self.loaded = true;
        self.model = settings.model.clone();
        self.settings = settings;
        self.input
            .update(cx, |input, cx| input.set_text(prompt, cx));
        self.send(cx);
    }
    fn send(&mut self, cx: &mut Context<Self>) {
        self.send_follow_up(false, cx);
    }
    /// `invert` sends a mid-turn message the opposite way from the follow-up
    /// preference: queued when it steers, steering when it queues.
    fn send_follow_up(&mut self, invert: bool, cx: &mut Context<Self>) {
        if self.is_remote() && self.remote_send_guard(cx) {
            return;
        }
        if self.needs_history_restore() {
            self.defer_history_action(move |this, cx| this.send_follow_up(invert, cx), cx);
            return;
        }
        if self.submit_edited(cx) {
            return;
        }
        if self.handle_local_command(cx) {
            return;
        }
        if self.settings.mono.is_some() {
            if self.attaching || self.input.read(cx).is_composing() {
                return;
            }
            let text = self
                .input
                .update(cx, |input, cx| input.take(cx))
                .unwrap_or_default();
            let attachments = std::mem::take(&mut self.attachments);
            if !text.trim().is_empty() || !attachments.is_empty() {
                cx.emit(mono::Submitted { text, attachments });
                cx.notify();
            }
            return;
        }
        if self.settings.worktree_removed {
            self.status = "Select a branch or worktree to continue this conversation.".into();
            cx.notify();
            return;
        }
        if !self.loaded || (self.preparing_worktree && !self.busy && !self.stopping) {
            return;
        }
        if self.draft_active(cx) {
            self.save_message_draft(cx);
            return;
        }
        if !self.folder_selected
            && monocode_core::composer::consume_command(
                &self.input.read(cx).content,
                "add-to-folder",
            )
            .is_some()
        {
            self.request_folder(cx);
            return;
        }
        if self.attaching
            || (!self.input.read(cx).has_message_text() && self.attachments.is_empty())
        {
            return;
        }
        if self.input.read(cx).is_composing() {
            return;
        }
        let raw = self
            .input
            .update(cx, |i, cx| i.take(cx))
            .unwrap_or_default();
        let raw = monocode_core::composer::consume_command(&raw, "add-to-folder")
            .map(str::to_owned)
            .unwrap_or(raw);
        self.folder_selected = false;
        let prompt = self.consume_mode_command(raw, cx);
        if prompt.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        let mode = if self.settings.orchestrator {
            Some("orchestrator")
        } else if self.settings.plan {
            Some("plan")
        } else if self.settings.operator {
            Some("operator")
        } else {
            None
        };
        if self.settings.pending_provider_switch.is_some() && self.busy {
            // Picking a provider only arms the change. Sending is what stops
            // the outgoing provider and commits the handoff.
            self.stop(cx);
        }
        if self.sends_as_follow_up() {
            self.enqueue_follow_up_inverted(prompt, mode.map(str::to_owned), invert, cx);
            if self.stopping || self.settings.pending_provider_switch.is_some() {
                // A fresh Send continues the conversation, while older queued
                // messages remain paused after Stop.
                if let Some(id) = self
                    .follow_ups
                    .state
                    .messages
                    .last()
                    .map(|message| message.id.clone())
                {
                    self.follow_ups.continuations.push_back(id);
                    if self.follow_ups.requested.is_none() {
                        self.follow_ups.requested = self.follow_ups.continuations.front().cloned();
                    }
                }
            }
            if mode.is_some() {
                self.clear_submitted_mode(cx);
            }
            return;
        }
        self.planning_orchestration = self.settings.orchestrator;
        self.orchestration_plan = None;
        self.orchestration_error = None;
        self.launch_prompt(prompt, cx);
        if self.busy && mode.is_some() {
            self.clear_submitted_mode(cx);
        }
    }
    /// A message sent now joins the follow-ups instead of starting a turn.
    fn sends_as_follow_up(&self) -> bool {
        self.stopping
            || self.busy
            || self.settings.pending_provider_switch.is_some()
            || (!self.follow_ups.state.messages.is_empty() && !self.follow_ups.state.paused)
    }
    /// The submit icon for a message that will steer or queue, if it will.
    fn follow_up_action(
        &self,
        alternate: bool,
        cx: &gpui::App,
    ) -> Option<(&'static str, &'static str)> {
        // Stop and provider switches send the message as the next turn.
        if self.settings.mono.is_some()
            || self.is_remote()
            || self.stopping
            || self.settings.pending_provider_switch.is_some()
            || !self.sends_as_follow_up()
        {
            return None;
        }
        let mode = self.settings.orchestrator || self.settings.plan || self.settings.operator;
        Some(
            if self.busy && !mode && self.steers_follow_up(alternate, cx) {
                ("Steer", "queue-steer")
            } else {
                ("Queue", "queue-list")
            },
        )
    }
    fn launch_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        if self.stopping {
            let mode = if self.settings.orchestrator {
                Some("orchestrator")
            } else if self.settings.plan {
                Some("plan")
            } else if self.settings.operator {
                Some("operator")
            } else {
                None
            };
            self.enqueue_follow_up(prompt, mode.map(str::to_owned), cx);
            if let Some(id) = self
                .follow_ups
                .state
                .messages
                .last()
                .map(|message| message.id.clone())
            {
                self.follow_ups.continuations.push_back(id);
                if self.follow_ups.requested.is_none() {
                    self.follow_ups.requested = self.follow_ups.continuations.front().cloned();
                }
            }
            return;
        }
        if !self.history_ready() {
            self.defer_history_action(move |this, cx| this.launch_prompt(prompt, cx), cx);
            return;
        }
        if self.account_switching {
            return;
        }
        let kind = Kind::from_id(&self.summary.provider);
        if self.settings.account.id != monocode_core::accounts::DEFAULT
            && crate::accounts::get(cx)
                .is_some_and(|center| !center.read(cx).book.exists(kind, &self.settings.account.id))
        {
            self.account_unavailable(cx);
            return;
        }
        self.settings.usage_limit = None;
        self.attach_account(cx);
        if !self.is_remote() {
            self.generate_title(&prompt, cx);
        }
        let model_label = self.model_label();
        self.sync_tool_paths(cx);
        self.transcript
            .update(cx, |v, _| v.identity(&self.summary.provider, &model_label));
        self.begin_turn(cx);
        let generation = self.submission_generation;
        let source_provider = self.summary.provider.clone();
        let source_account = self.settings.account.id.clone();
        self.status = if self.settings.pending_provider_switch.is_some() {
            "Preparing handoff…"
        } else {
            "Connecting…"
        }
        .into();
        self.cancel = Arc::new(AtomicBool::new(false));
        let summary = self.summary.clone();
        let store = self.store.clone();
        let cancel = self.cancel.clone();
        // Composer modes reset after Send; the thread grant and running turn's
        // access policy must outlive that reset and edits to the next draft.
        self.settings.operator_enabled |= self.settings.operator;
        self.app_read_only = self.settings.plan
            || self.settings.orchestrator
            || self.settings.access == AccessMode::ReadOnly;
        let mut settings = self.settings.clone();
        if let Some(model) = self.model_info() {
            for control in &model.controls {
                settings
                    .model_settings
                    .entry(control.id.clone())
                    .or_insert_with(|| control.current.clone());
            }
        }
        settings.turn_model = Some(monocode_core::agent::TurnInfo {
            provider: self.summary.provider.clone(),
            model: model_label.to_string(),
            model_id: self
                .model_info()
                .map(|model| model.id.clone())
                .or(self.model.clone()),
            started_at: Some(monocode_core::time::now_ms()),
            ..Default::default()
        });
        if !self.is_remote() && settings.prepared_prompt.is_none() {
            let prepared = self.completion.read(cx).prepare_prompt(&prompt);
            if prepared != prompt {
                settings.prepared_prompt = Some(prepared);
            }
        }
        settings.disable_claude_hooks = cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|p| !p.claude_hooks);
        settings.model = self.model.clone();
        settings.environment.extend(self.app_environment.clone());
        if settings.remote_action.as_ref().is_none_or(|action| {
            (action["type"] == "send"
                && action["draftBlockId"].is_null()
                && action["planBlockId"].is_null())
                || action["type"] == "draft"
        }) {
            settings
                .attachments
                .extend(std::mem::take(&mut self.attachments));
        }
        if settings
            .remote_action
            .as_ref()
            .is_some_and(|action| action["planBlockId"].is_string())
        {
            settings.attachments.clear();
        }
        self.preparing_worktree = settings.new_worktree && settings.worktree_cwd.is_none();
        if self.preparing_worktree {
            settings.worktree_base = settings.worktree_base.or(self.branch.clone());
            self.status = "Creating worktree…".into();
        }
        if settings.mono.is_none()
            && settings
                .remote_action
                .as_ref()
                .is_none_or(|action| action["type"] == "send")
        {
            self.transcript
                .update(cx, |view, cx| view.submit_prompt(&prompt, &settings, cx));
        }
        if settings.orchestrator {
            settings.plan = true;
            settings.operator = false;
            self.settings.orchestrator = false;
        }
        if self.settings.handoff.take().is_some() {
            self.save_settings(cx);
        }
        let (controls, control_rx) = std::sync::mpsc::channel();
        self.controls = Some(controls);
        self.approvals.clear();
        self.questions.clear();
        let (tx, rx) = async_channel::bounded(8);
        let mut turn_model_id = settings
            .turn_model
            .as_ref()
            .and_then(|turn| turn.model_id.clone())
            .or_else(|| settings.model.clone());
        let remote_client = self
            .is_remote()
            .then(|| self.remote_state.client.clone())
            .flatten();
        cx.background_executor()
            .spawn(async move {
                if let Some(client) = remote_client {
                    crate::runtime::remote::run_with_client(
                        client, summary, prompt, settings, control_rx, store, cancel, tx,
                    );
                } else {
                    run_turn(summary, prompt, settings, control_rx, store, cancel, tx);
                }
            })
            .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(update) = rx.recv().await {
                let finished = update.finished;
                if this
                    .update(cx, |this, cx| {
                        if this.submission_generation != generation {
                            return;
                        }
                        let stopping = this.stopping;
                        // The final update settles accepted rows and storage.
                        // Ignore streaming notifications after the user stops.
                        if stopping && !finished {
                            return;
                        }
                        let source_selected = this.summary.provider == source_provider
                            && this.settings.account.id == source_account;
                        if let Some(cwd) = update.worktree_cwd
                            && (this.settings.worktree_cwd.as_ref() != Some(&cwd)
                                || this.settings.new_worktree)
                        {
                            this.preparing_worktree = false;
                            this.settings.worktree_cwd = Some(cwd);
                            this.settings.new_worktree = false;
                            this.save_settings(cx);
                            this.refresh_git(cx);
                        }
                        if let Some(commands) = update.commands.filter(|_| source_selected) {
                            this.completion.update(cx, |completion, cx| {
                                completion.set_native_commands(commands, cx)
                            });
                        }
                        let needed_approval = this.needs_approval();
                        let incoming_notification_input = !stopping
                            && (!update.approvals.is_empty() || !update.questions.is_empty());
                        let mono_queue_changed = this.settings.mono.is_some()
                            && (update.turn_ready
                                || update.delivered.is_some()
                                || !update.follow_up_delivered.is_empty()
                                || !update.follow_up_errors.is_empty());
                        this.transcript.update(cx, |v, cx| {
                            v.patch(update.start, update.rows, cx);
                            if stopping {
                                v.stop_streaming(cx);
                            }
                        });
                        if let Some(id) = update.provider_turn_id {
                            this.transcript
                                .update(cx, |view, _| view.record_provider_turn(id));
                        }
                        if let Some(id) = update.provider_id {
                            if let Some(pending) = this.settings.pending_provider_switch.as_mut()
                                && pending.from == source_provider
                                && pending.account.id == source_account
                            {
                                pending.provider_id = Some(id);
                            } else if source_selected {
                                this.summary.provider_id = Some(id);
                            }
                        }
                        if let Some(handoff) = update.handoff {
                            this.settings.pending_provider_switch = None;
                            this.settings.handoff = handoff;
                            this.save_settings(cx);
                        }
                        if let Some(model) = update.config_model {
                            if let Some(pending) = this.settings.pending_provider_switch.as_mut()
                                && pending.from == source_provider
                                && pending.account.id == source_account
                            {
                                pending.model = Some(model);
                            } else if source_selected {
                                this.model = Some(model.clone());
                                this.settings.model = Some(model);
                            }
                        }
                        if let Some(settings) = update.config_settings {
                            if let Some(pending) = this.settings.pending_provider_switch.as_mut()
                                && pending.from == source_provider
                                && pending.account.id == source_account
                            {
                                pending.model_settings.extend(settings);
                            } else if source_selected {
                                this.settings.model_settings.extend(settings);
                            }
                            this.save_settings(cx);
                        }
                        if let Some(model) = update.model {
                            turn_model_id = Some(model.clone());
                            let name: SharedString = this
                                .catalog
                                .iter()
                                .find(|entry| source_selected && entry.id == model)
                                .map(|entry| {
                                    let name = model_display_name(
                                        Kind::from_id(&source_provider),
                                        &entry.id,
                                        &entry.name,
                                    );
                                    name.strip_prefix("Claude ")
                                        .unwrap_or(&name)
                                        .to_owned()
                                        .into()
                                })
                                .unwrap_or_else(|| {
                                    model_name(Kind::from_id(&source_provider), Some(&model))
                                });
                            if source_selected {
                                this.resolved_model = Some(model);
                            }
                            this.transcript
                                .update(cx, |v, _| v.identity(&source_provider, &name));
                        }
                        if let Some(context) = update.context.filter(|_| source_selected) {
                            this.context = Some(context);
                        }
                        if let Some(metrics) = update.metrics {
                            this.transcript.update(cx, |v, _| v.metrics(metrics));
                        }
                        if update.turn_ready && !stopping {
                            this.follow_ups.ready = true;
                        }
                        for id in update.follow_up_delivered {
                            if this.settings.mono.is_some() {
                                this.mono_message_delivered(id, cx);
                            } else {
                                this.follow_up_delivered(&id, cx);
                            }
                        }
                        for (id, error) in update.follow_up_errors {
                            if this.settings.mono.is_some() {
                                this.mono_message_failed(id, error);
                            } else {
                                this.fail_follow_up(&id, error, cx);
                            }
                        }
                        if let Some(id) = update.delivered {
                            if this.settings.mono.is_some() {
                                this.mono_message_delivered(id, cx);
                            } else {
                                this.follow_up_delivered(&id, cx);
                            }
                        }
                        if !stopping || finished {
                            this.status = update.status;
                        }
                        if let Some(tasks) = update.background.filter(|_| !stopping) {
                            this.set_background(tasks, cx);
                        }
                        if !stopping {
                            this.approvals.extend(update.approvals);
                        }
                        for id in update.approvals_resolved {
                            this.approvals.retain(|approval| approval.id != id);
                        }
                        for id in update.questions_resolved {
                            this.questions.retain(|(card, _)| card.read(cx).id() != id);
                        }
                        for request in update.questions.into_iter().filter(|_| !stopping) {
                            let card =
                                cx.new(|cx| crate::questions::QuestionCard::new(request, cx));
                            let events = cx.subscribe(
                                &card,
                                |this, _, reply: &crate::questions::Answered, cx| {
                                    if this
                                        .questions
                                        .front()
                                        .is_none_or(|(card, _)| card.read(cx).id() != reply.id)
                                    {
                                        return;
                                    }
                                    let before = this.needs_approval();
                                    let decision = reply
                                        .answers
                                        .clone()
                                        .map_or(Decision::Deny, Decision::Answer);
                                    if this.controls.as_ref().is_none_or(|controls| {
                                        controls
                                            .send(Control {
                                                id: reply.id.clone(),
                                                decision,
                                            })
                                            .is_err()
                                    }) {
                                        this.status =
                                            "The provider is no longer waiting for this answer"
                                                .into();
                                        cx.notify();
                                        return;
                                    }
                                    if let Some((card, _)) = this.questions.pop_front() {
                                        this.question_overlay.release(card, cx);
                                    }
                                    this.status = if !this.approvals.is_empty() {
                                        "Waiting for approval"
                                    } else if !this.questions.is_empty() {
                                        "Waiting for your answer"
                                    } else {
                                        "Working…"
                                    }
                                    .into();
                                    if before != this.needs_approval() {
                                        cx.emit(SessionChanged);
                                    }
                                    cx.notify();
                                },
                            );
                            let keep_open = cx.subscribe(
                                &card,
                                |this, _, request: &crate::questions::KeptOpen, cx| {
                                    if let Some(controls) = &this.controls {
                                        let _ = controls.send(Control {
                                            id: request.id.clone(),
                                            decision: Decision::KeepOpen,
                                        });
                                    }
                                    cx.emit(SessionChanged);
                                    cx.notify();
                                },
                            );
                            this.questions.push_back((card, vec![events, keep_open]));
                            this.status = "Waiting for your answer".into();
                        }
                        if !finished && !this.approvals.is_empty() {
                            this.status = "Waiting for approval".into();
                        } else if !finished && !this.questions.is_empty() {
                            this.status = "Waiting for your answer".into();
                        }
                        if finished {
                            if this.is_remote() {
                                this.settings.remote_action = None;
                                this.remote_state.refreshing = false;
                            }
                            this.summary.updated_at = monocode_core::time::now_ms();
                            this.busy = false;
                            this.stopping = false;
                            this.clear_background();
                            if this.is_remote() {
                                this.refresh_remote_snapshot(cx);
                            }
                            this.preparing_worktree = false;
                            if !stopping && let Some(limit) = update.usage_limit {
                                this.settings.usage_limit = Some(limit);
                            }
                            if !this.is_remote() {
                                this.detect_usage_limit(cx);
                            }
                            this.review
                                .update(cx, |review, cx| review.set_busy(false, cx));
                            this.follow_ups.ready = false;
                            if this.follow_ups.resuming {
                                this.follow_ups.resuming = false;
                                if !stopping && this.status == "Ready" {
                                    this.follow_ups.state.paused = false;
                                    for message in &mut this.follow_ups.state.messages {
                                        message.error = None;
                                    }
                                    this.save_follow_ups(cx);
                                }
                            }
                            if let Some(id) = this.follow_ups.sending.clone() {
                                this.fail_follow_up(
                                    &id,
                                    "The turn ended before this message was accepted".into(),
                                    cx,
                                );
                            }
                            if this.status != "Ready" && !this.follow_ups.state.messages.is_empty()
                            {
                                this.follow_ups.state.paused = true;
                                this.save_follow_ups(cx);
                            }
                            this.approvals.clear();
                            this.questions.clear();
                            this.controls = None;
                            this.transcript.update(cx, |v, cx| v.streaming(false, cx));
                            let model_id = turn_model_id.clone();
                            this.transcript.update(cx, |view, _| {
                                if let Some(turn) = view.active_turn()
                                    && let Some(info) = view.turns.get_mut(&turn)
                                {
                                    info.model_id = model_id;
                                }
                            });
                            if stopping {
                                this.planning_orchestration = false;
                                this.orchestration_progress = None;
                            } else {
                                this.finish_orchestration_plan(cx);
                            }
                            this.settings.turns = this.transcript.read(cx).window_turns();
                            this.save_settings(cx);
                            this.refresh_git(cx);
                            this.run_history_actions(cx);
                            this.mono_flush_external(cx);
                            cx.emit(SessionChanged);
                        } else if incoming_notification_input
                            || mono_queue_changed
                            || needed_approval != this.needs_approval()
                        {
                            cx.emit(SessionChanged);
                        }
                        this.pump_follow_ups(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.emit(SessionChanged);
        cx.notify();
    }
    fn begin_turn(&mut self, cx: &mut Context<Self>) {
        self.submission_generation = self.submission_generation.wrapping_add(1);
        if !self.summary.has_messages {
            self.composer_motion.capture(self.composer_anchor.get());
        }
        self.busy = true;
        self.stopping = false;
        self.notification_cancelled = false;
        self.review
            .update(cx, |review, cx| review.set_busy(true, cx));
        self.follow_ups.ready = false;
        self.mono_turn = self
            .mono_turn
            .wrapping_add(1)
            .max(monocode_core::time::now_ms() & 0xffff_ffff);
        self.summary.updated_at = monocode_core::time::now_ms();
        self.transcript.update(cx, |v, cx| v.streaming(true, cx));
        self.summary.has_messages = true;
    }
}

impl EventEmitter<SessionChanged> for Conversation {}
impl EventEmitter<ChooseFolder> for Conversation {}
impl EventEmitter<OpenSessionChanges> for Conversation {}
impl EventEmitter<Transfer> for Conversation {}
impl EventEmitter<composer_features::StartOrchestration> for Conversation {}
impl EventEmitter<composer_features::StopOrchestration> for Conversation {}

impl Conversation {
    pub(crate) fn install_transfer(
        &mut self,
        settings: Settings,
        prompt: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.editing_last_turn.pending() {
            self.defer_edit_transfer(settings, prompt, cx);
            return;
        }
        if self.needs_history_restore() || self.stopping {
            self.defer_history_action(
                move |this, cx| {
                    // Mono callers may configure their live context immediately
                    // after installing a transfer. Keep that context at replay.
                    let mono = this.settings.mono.take();
                    let environment = std::mem::take(&mut this.settings.environment);
                    let mut settings = settings;
                    if settings.mono.is_none() {
                        settings.mono = mono;
                    }
                    settings.environment.extend(environment);
                    match prompt {
                        Some(prompt) if this.preserve_new_composer(cx) => {
                            this.install_transfer(settings, None, cx);
                            this.enqueue_restored_prompt(prompt, cx);
                        }
                        prompt => this.install_transfer(settings, prompt, cx),
                    }
                },
                cx,
            );
            return;
        }
        self.model = settings.model.clone();
        self.settings = settings;
        self.attach_account(cx);
        self.loaded = true;
        self.status = "Ready".into();
        self.refresh_catalog(cx);
        self.refresh_git(cx);
        if let Some(prompt) = prompt {
            self.input.update(cx, |i, cx| i.set_text(prompt, cx));
            self.send(cx);
        } else {
            cx.notify();
        }
    }
    fn close_response_menu(&mut self, cx: &mut Context<Self>) {
        self.response_menu = None;
        self.response_request = None;
        self.transcript.update(cx, |view, cx| {
            view.target_action = None;
            cx.notify();
        });
        cx.notify();
    }
    fn response_request(
        &mut self,
        request: &ResponseRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            ResponseRequest::RemotePlan { id, text, build } => {
                self.remote_plan_request(id, text, *build, cx)
            }
            ResponseRequest::QuoteSelection { text } => self.mono_quote(text, window, cx),
            ResponseRequest::EditLastTurn { turn } => self.begin_edit_last_turn(*turn, window, cx),
            ResponseRequest::Draft { send } => self.draft_action(*send, cx),
            ResponseRequest::SaveNote {
                turn,
                text,
                selection,
            } => {
                let turn = *turn;
                let note = monocode_core::response::Note {
                    id: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64,
                    title: {
                        let title = self.display_title();
                        if *selection || title.trim().is_empty() || title == "New session" {
                            monocode_core::notes::title(text)
                        } else {
                            title
                        }
                    },
                    body: text.clone(),
                    tags: Vec::new(),
                    source_session: self.summary.id,
                    source_cwd: self.summary.cwd.clone(),
                };
                let store = self.store.clone();
                let work = cx
                    .background_executor()
                    .spawn(async move { locked(&store)?.save_note(&note) });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.transcript
                            .update(cx, |view, cx| view.note_result(turn, result, cx))
                    });
                })
                .detach();
            }
            ResponseRequest::Target { turn, kind, anchor } => {
                if self
                    .response_request
                    .is_some_and(|(at, k, _)| at == *turn && k == *kind)
                {
                    self.close_response_menu(cx);
                    return;
                }
                let turn = *turn;
                let kind = *kind;
                let token = Instant::now();
                let anchor = anchor.clone();
                self.menu = None;
                self.git_picker = None;
                self.response_request = Some((turn, kind, token));
                self.transcript.update(cx, |view, cx| {
                    view.target_action = Some((turn, kind));
                    cx.notify();
                });
                self.open_response_menu(turn, kind, token, anchor, window, cx);
                cx.notify();
            }
        }
    }
    fn finish_response_target(
        &mut self,
        turn: usize,
        kind: monocode_core::response::TransferKind,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let Ok(target) = serde_json::from_str::<monocode_core::response::Target>(id) else {
            return;
        };
        let from = self
            .transcript
            .read(cx)
            .turns
            .get(&turn)
            .map(|t| t.provider.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.summary.provider)
            .to_owned();
        let (rows, through) = self.transcript.read(cx).response_rows(turn);
        let transfer = Transfer {
            kind,
            target,
            from,
            turn: rows,
            through,
            cwd: self.summary.cwd.clone(),
            title: self.summary.title.clone(),
            settings: self.settings.clone(),
        };
        self.close_response_menu(cx);
        cx.emit(transfer);
    }
    pub(crate) fn worktree_deleted(
        &mut self,
        summary: Session,
        settings: Settings,
        cx: &mut Context<Self>,
    ) {
        self.summary = summary;
        self.settings = settings;
        self.context = None;
        self.status = "Worktree deleted".into();
        self.refresh_git(cx);
        cx.emit(SessionChanged);
        cx.notify();
    }
    fn execution_cwd(&self) -> &str {
        if self.settings.worktree_removed {
            return &self.summary.cwd;
        }
        self.settings
            .worktree_cwd
            .as_deref()
            .unwrap_or(&self.summary.cwd)
    }
    fn sync_tool_paths(&mut self, cx: &mut Context<Self>) {
        let cwd = self.execution_cwd().to_owned();
        self.transcript.update(cx, |view, cx| {
            if view.file_link_base.as_deref() != Some(cwd.as_str()) {
                view.file_link_base = Some(cwd);
                cx.notify();
            }
        });
    }
    /// The review card follows the checkout and rereads its recorded turn.
    pub(crate) fn sync_review(&mut self, cx: &mut Context<Self>) {
        self.sync_tool_paths(cx);
        if self.settings.mono.is_some() || self.is_remote() {
            return;
        }
        self.collect_session_edits(cx);
        let cwd = self.execution_cwd().to_owned();
        self.review.update(cx, |review, cx| {
            if review.session().1 == cwd {
                review.refresh(cx);
            } else {
                review.set_cwd(cwd, cx);
            }
        });
    }
    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        self.sync_review(cx);
        self.load_git(cx);
    }
    fn load_git(&mut self, cx: &mut Context<Self>) {
        let cwd = self.execution_cwd().to_owned();
        let work_cwd = cwd.clone();
        let work = cx
            .background_executor()
            .spawn(async move { crate::git_cache::load(Path::new(&work_cwd)).ok() });
        cx.spawn(async move |this, cx| {
            let git = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.execution_cwd() == cwd {
                    this.branch = git.as_ref().and_then(|s| s.current.clone());
                    this.git = git;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn prepare_worktree(&mut self, cx: &mut Context<Self>) {
        self.preparing_worktree = true;
        self.status = "Creating worktree…".into();
        let cwd = self.summary.cwd.clone();
        let base = self
            .settings
            .worktree_base
            .clone()
            .or(self.branch.clone())
            .unwrap_or_else(|| "HEAD".into());
        let id = self.summary.id;
        let work = cx
            .background_executor()
            .spawn(async move { monocode_core::git::create_worktree(Path::new(&cwd), &base, id) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.preparing_worktree = false;
                match result {
                    Ok(path) => {
                        this.settings.worktree_cwd = Some(path.to_string_lossy().into_owned());
                        this.settings.new_worktree = false;
                        this.save_settings(cx);
                        this.refresh_git(cx);
                        if this.follow_ups.state.messages.is_empty() {
                            this.send(cx);
                        } else {
                            this.pump_follow_ups(cx);
                        }
                    }
                    Err(error) => {
                        this.status = error.clone();
                        if let Some(message) = this.follow_ups.state.messages.first() {
                            let id = message.id.clone();
                            this.fail_follow_up(&id, error, cx);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn open_git(&mut self, mode: GitMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.preparing_worktree {
            return;
        }
        if self.git_picker.take().is_some() {
            self.git_events = None;
            window.focus(&self.input.focus_handle(cx), cx);
            cx.notify();
            return;
        }
        self.menu = None;
        let cwd = self.execution_cwd().into();
        let selected = if mode == GitMode::Base {
            self.settings.worktree_base.clone().or(self.branch.clone())
        } else {
            self.branch.clone()
        };
        let picker = cx.new(|cx| GitPicker::new(cwd, mode, selected, cx));
        self.git_events =
            Some(
                cx.subscribe_in(&picker, window, |this, _, event: &GitPicked, window, cx| {
                    match event {
                        GitPicked::Changed => {
                            if this.settings.worktree_removed {
                                this.settings.worktree_removed = false;
                                this.settings.worktree_cwd = None;
                                this.save_settings(cx);
                            }
                            this.refresh_git(cx);
                        }
                        GitPicked::Base(base) => {
                            this.settings.worktree_base = Some(base.clone());
                            this.save_settings(cx);
                        }
                        GitPicked::Worktree(path) => {
                            if !this.use_existing_worktree(path, cx) {
                                return;
                            }
                        }
                        GitPicked::Dismissed => {}
                    }
                    this.git_picker = None;
                    this.git_events = None;
                    window.focus(&this.input.focus_handle(cx), cx);
                    cx.notify();
                }),
            );
        window.focus(&picker.read(cx).focus(cx), cx);
        self.git_picker = Some(picker);
        cx.notify();
    }
    fn use_existing_worktree(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        if self.summary.has_messages && !self.settings.worktree_removed {
            return false;
        }
        self.settings.worktree_removed = false;
        self.settings.worktree_cwd = (monocode_core::files::path_key(path)
            != monocode_core::files::path_key(&self.summary.cwd))
        .then(|| path.to_owned());
        self.settings.new_worktree = false;
        self.save_settings(cx);
        self.refresh_git(cx);
        true
    }
    fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) {
            return;
        }
        let kind = Kind::from_id(&self.summary.provider);
        self.attach_account(cx);
        self.catalog_epoch += 1;
        let epoch = self.catalog_epoch;
        let account = self.settings.account.clone();
        let work = cx.background_executor().spawn({
            let remote_project = self.is_remote().then(|| self.summary.cwd.clone());
            async move {
                if let Some(project) = remote_project {
                    monocode_core::catalog::remote_models(&project, kind.id())
                } else {
                    catalog::models_for(kind, &account)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            if let Ok(models) = work.await {
                let _ = this.update(cx, |this, cx| {
                    if Kind::from_id(&this.summary.provider) == kind && this.catalog_epoch == epoch
                    {
                        this.catalog = models;
                        if this.is_remote() && this.model.is_none() {
                            this.model = this
                                .catalog
                                .iter()
                                .find(|m| m.default)
                                .or_else(|| this.catalog.first())
                                .map(|m| m.id.clone());
                            this.settings.model = this.model.clone();
                            this.save_settings(cx);
                        }
                        cx.emit(SessionChanged);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
    fn model_info(&self) -> Option<&Model> {
        match self.model.as_deref().or(self.resolved_model.as_deref()) {
            Some(id) => self.catalog.iter().find(|m| m.id == id),
            None => self
                .catalog
                .iter()
                .find(|m| m.default)
                .or_else(|| self.catalog.first()),
        }
    }
    /// A new tab shows the model and access it was created with from its
    /// first frame instead of flipping when the stored settings load.
    pub(crate) fn seed_new_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        if self.loaded || self.settings.revision != 0 {
            return;
        }
        if settings.model.is_some() {
            self.model = settings.model.clone();
        }
        self.settings = settings;
        cx.notify();
    }
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.revision = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut settings = self.settings.clone();
        settings.model = self.model.clone();
        let store = self.store.clone();
        let id = self.summary.id;
        let provider = self.summary.provider.clone();
        let remote = self.is_remote();
        let title = (!self.summary.has_messages).then(|| self.summary.title.clone());
        cx.background_executor()
            .spawn(async move {
                let saved = locked(&store).and_then(|s| {
                    s.configure_agent(id, &provider, &settings)?;
                    if let Some(title) = title
                        && s.agent_settings(id)?
                            .is_some_and(|saved| saved.revision == settings.revision)
                    {
                        s.title(id, &title)?;
                    }
                    Ok(())
                });
                if saved.is_ok() && remote {
                    let _ = crate::runtime::remote::configure_saved(store, id);
                }
            })
            .detach();
    }
    fn menu_choice(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.git_action_choice(id, cx) {
            return;
        }
        if let Some(choice) = id.strip_prefix("recent-model:") {
            if let Ok((provider, model)) = serde_json::from_str::<(String, Option<String>)>(choice)
                && let Some(kind) = Kind::parse(&provider)
            {
                let picked = ModelPicked(kind, model);
                if self.apply_model_pick(&picked, cx) {
                    remember_model(kind, picked.1, cx);
                }
            }
            return;
        }
        if let Some(id) = id.strip_prefix("usage-account:") {
            if let Some(center) = crate::accounts::get(cx) {
                let kind = Kind::from_id(&self.summary.provider);
                if center.read(cx).book.exists(kind, id) {
                    let profile = center.read(cx).profile(id);
                    self.switch_account(profile, cx);
                }
            }
            return;
        }
        if std::env::var_os("MONOCODE_NATIVE_TRACE").is_some() {
            eprintln!("native settings session={} action={id}", self.summary.id);
        }
        if let Some(mode) = id.strip_prefix("access:") {
            self.settings.access = AccessMode::from_id(mode);
            remember_access(self.settings.access, cx);
        } else if let Some(effort) = id.strip_prefix("effort:") {
            self.set_effort_choice(effort, cx);
        } else if let Some(tier) = id.strip_prefix("tier:") {
            self.settings.service_tier = (!tier.is_empty()).then(|| tier.to_owned());
            self.settings.model_settings.remove("serviceTier");
        } else if let Some(setting) = id.strip_prefix("model-setting:") {
            if let Ok((key, value)) = serde_json::from_str::<(String, String)>(setting) {
                if is_effort_key(&key)
                    && (!self.is_remote() || matches!(key.as_str(), "effort" | "reasoningEffort"))
                {
                    self.set_effort_choice(&value, cx);
                }
                self.settings.model_settings.insert(key, value);
            }
        } else if let Some(key) = id.strip_prefix("model-toggle:") {
            let current = self
                .settings
                .model_settings
                .get(key)
                .map(String::as_str)
                .or_else(|| {
                    self.model_info()
                        .and_then(|model| model.controls.iter().find(|c| c.id == key))
                        .map(|c| c.current.as_str())
                });
            let value = if current == Some("true") {
                "false"
            } else {
                "true"
            };
            self.settings
                .model_settings
                .insert(key.to_owned(), value.to_owned());
        } else {
            match id {
                "workspace:current" => {
                    self.settings.worktree_removed = false;
                    self.settings.new_worktree = false;
                    self.settings.worktree_cwd = None;
                    self.refresh_git(cx);
                }
                "workspace:new" => {
                    self.settings.worktree_removed = false;
                    self.settings.new_worktree = true;
                    self.settings.worktree_cwd = None;
                    self.settings.worktree_base =
                        self.settings.worktree_base.clone().or(self.branch.clone());
                    self.refresh_git(cx);
                }
                "fast" => self.settings.fast = !self.settings.fast,
                "fast:on" => self.settings.fast = true,
                "fast:off" => self.settings.fast = false,
                "thinking" => {
                    self.settings.thinking = Some(!self.settings.thinking.unwrap_or(false))
                }
                "plan" | "operator" | "orchestrator" | "draft" => {
                    self.toggle_mode(id, cx);
                    return;
                }
                _ => return,
            }
        }
        self.save_settings(cx);
        cx.notify();
    }
    fn set_effort_choice(&mut self, effort: &str, cx: &mut Context<Self>) {
        self.settings
            .model_settings
            .retain(|key, _| !is_effort_key(key));
        self.settings.effort = (!effort.is_empty()).then(|| effort.to_owned());
        let provider = self.summary.provider.clone();
        if cx.has_global::<crate::settings::Preferences>() {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                if effort.is_empty() {
                    prefs.provider_efforts.remove(&provider);
                } else {
                    prefs.provider_efforts.insert(provider, effort.to_owned());
                }
            });
        }
    }
    fn selected_effort(&self) -> Option<&str> {
        self.settings
            .effort
            .as_deref()
            .or_else(|| {
                [
                    "effort",
                    "reasoning",
                    "reasoningEffort",
                    "reasoning_effort",
                    "variant",
                ]
                .into_iter()
                .find_map(|key| self.settings.model_settings.get(key).map(String::as_str))
            })
            .or_else(|| self.model_info().and_then(|m| m.default_effort.as_deref()))
            .or_else(|| {
                self.model_info()
                    .and_then(|m| m.efforts.first().map(String::as_str))
            })
    }
    fn effort_label(&self) -> SharedString {
        effort_name(self.selected_effort().unwrap_or("default")).into()
    }
    fn effort_setting_label(&self) -> &'static str {
        if Kind::from_id(&self.summary.provider) == Kind::Codex {
            "Reasoning"
        } else {
            "Effort"
        }
    }
    fn selected_tier(&self) -> Option<&str> {
        self.settings
            .service_tier
            .as_deref()
            .or_else(|| {
                self.settings
                    .model_settings
                    .get("serviceTier")
                    .map(String::as_str)
            })
            .filter(|value| !value.is_empty() && *value != "default")
    }
    /// Combined ModelPicker trigger; beside mode renders these as separate pills.
    fn trigger_effort_label(&self) -> Option<SharedString> {
        if let Some(model) = self.model_info() {
            if let Some(control) = model.controls.iter().find(|control| {
                !model_control_toggle(control)
                    && matches!(
                        control.id.as_str(),
                        "effort"
                            | "reasoning"
                            | "reasoningEffort"
                            | "reasoning_effort"
                            | "thinking"
                            | "variant"
                    )
            }) {
                let selected = self
                    .settings
                    .model_settings
                    .get(&control.id)
                    .unwrap_or(&control.current);
                return Some(
                    control
                        .choices
                        .iter()
                        .find(|(value, _)| value == selected)
                        .map(|(_, label)| label)
                        .unwrap_or(selected)
                        .clone()
                        .into(),
                );
            }
            return (!model.efforts.is_empty()).then(|| self.effort_label());
        }
        // Keep the saved effort visible while the live catalog is loading.
        self.settings.effort.as_ref().map(|_| self.effort_label())
    }
    fn effort_items(&self) -> Vec<MenuAction> {
        self.model_info().map_or_else(Vec::new, |model| {
            model
                .efforts
                .iter()
                .map(|effort| {
                    MenuAction::new(format!("effort:{effort}"), effort_name(effort))
                        .checked(self.selected_effort() == Some(effort.as_str()))
                })
                .collect()
        })
    }
    /// The pill's label, which drops the catalog names' "Claude " prefix.
    fn model_label(&self) -> SharedString {
        let name = self
            .model_info()
            .map(|m| model_display_name(Kind::from_id(&self.summary.provider), &m.id, &m.name))
            .unwrap_or_else(|| {
                model_name(
                    Kind::from_id(&self.summary.provider),
                    self.model.as_deref().or(self.resolved_model.as_deref()),
                )
            });
        if Kind::from_id(&self.summary.provider) == Kind::Claude {
            name.strip_prefix("Claude ")
                .map_or(name.clone(), |short| short.to_owned().into())
        } else {
            name
        }
    }
    /// A different provider is armed for the next submission. The running
    /// provider retains its controls, account and immutable turn identity.
    pub(super) fn apply_model_pick(
        &mut self,
        picked: &ModelPicked,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.editing_last_turn.pending() || self.status == "Preparing handoff…" {
            return false;
        }
        let changed_model = self.summary.provider != picked.0.id() || self.model != picked.1;
        if std::env::var_os("MONOCODE_NATIVE_TRACE").is_some() {
            eprintln!(
                "native model session={} provider={} model={:?}",
                self.summary.id,
                picked.0.id(),
                picked.1
            );
        }
        if self.summary.provider != picked.0.id() {
            let reverting = self
                .settings
                .pending_provider_switch
                .as_ref()
                .is_some_and(|pending| pending.from == picked.0.id());
            let restored = reverting
                .then(|| self.settings.pending_provider_switch.clone())
                .flatten();
            let changed = locked(&self.store).and_then(|store| {
                store.change_provider(self.summary.id, picked.0.id())?;
                if let Some(id) = restored
                    .as_ref()
                    .and_then(|pending| pending.provider_id.as_deref())
                {
                    store.provider_id(self.summary.id, id)?;
                }
                Ok(())
            });
            if let Err(error) = changed {
                self.status = error;
                cx.notify();
                return false;
            }
            if let Some(pending) = restored {
                self.settings.account = pending.account;
                self.summary.provider_id = pending.provider_id;
                self.settings.pending_provider_switch = None;
                self.settings.handoff = None;
            } else {
                let has_history = self.busy
                    || self
                        .transcript
                        .read(cx)
                        .source_rows()
                        .iter()
                        .any(|row| row.is_prompt());
                if has_history && self.settings.pending_provider_switch.is_none() {
                    self.settings.pending_provider_switch =
                        Some(monocode_core::agent::ProviderSwitch {
                            from: self.summary.provider.clone(),
                            model_label: self.model_label().to_string(),
                            cwd: self.execution_cwd().to_owned(),
                            model: self.model.clone(),
                            model_settings: self.settings.model_settings.clone(),
                            effort: self.settings.effort.clone(),
                            fast: self.settings.fast,
                            service_tier: self.settings.service_tier.clone(),
                            thinking: self.settings.thinking,
                            skip_outgoing_recap: self.settings.usage_limit.is_some(),
                            account: self.settings.account.clone(),
                            provider_id: self.summary.provider_id.clone(),
                        });
                }
                self.settings.account = crate::accounts::get(cx)
                    .map(|center| center.read(cx).selected(picked.0, &self.summary.cwd))
                    .unwrap_or_default();
                self.summary.provider_id = None;
                if let Some(pending) = &self.settings.pending_provider_switch {
                    self.settings.handoff = Some(monocode_core::agent::Handoff {
                        from: pending.from.clone(),
                        brief: monocode_core::response::handoff(
                            self.transcript.read(cx).source_rows(),
                        ),
                    });
                }
            }
            self.summary.provider = picked.0.id().into();
            self.completion.update(cx, |completion, cx| {
                completion.set_provider(picked.0.id(), cx)
            });
            self.catalog.clear();
            if !self.summary.has_messages {
                self.summary.title = format!("New {} session", picked.0.label());
            }
        }
        self.model = picked.1.clone();
        if let Some(context) = &mut self.context {
            context.window = None;
        }
        let store = self.store.clone();
        let id = self.summary.id;
        let context = self.context;
        cx.background_executor()
            .spawn(async move {
                if let Some(context) = context {
                    let _ = locked(&store).and_then(|s| s.save_context_usage(id, &context));
                }
            })
            .detach();
        self.resolved_model = None;
        self.settings.effort = preferred_effort(&self.summary.provider, cx);
        self.settings.fast = false;
        self.settings.thinking = None;
        self.settings.service_tier = None;
        self.settings.model_settings.clear();
        if changed_model && self.settings.usage_limit.is_some() && !self.busy {
            self.recover_usage_after_model_change(cx);
        }
        self.refresh_catalog(cx);
        self.save_settings(cx);
        cx.emit(SessionChanged);
        true
    }
    fn menu_is(&self, kind: ComposerMenu) -> bool {
        self.menu.as_ref().is_some_and(|open| open.kind == kind)
    }
    fn anchor(&self, kind: ComposerMenu) -> &AnchorSlot {
        match kind {
            ComposerMenu::Add => &self.add_anchor,
            ComposerMenu::Model | ComposerMenu::Recent => &self.model_anchor,
            ComposerMenu::Access => &self.access_anchor,
            ComposerMenu::Account => &self.account_anchor,
            ComposerMenu::Workspace => &self.workspace_anchor,
            ComposerMenu::GitActions => &self.git_action_anchor,
            ComposerMenu::Setting(ix) => self
                .mono_ui
                .setting_anchors
                .get(ix)
                .unwrap_or(&self.model_anchor),
        }
    }
    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }
    fn toggle_menu(&mut self, kind: ComposerMenu, window: &mut Window, cx: &mut Context<Self>) {
        if kind == ComposerMenu::Model
            && cx
                .try_global::<crate::settings::Preferences>()
                .is_some_and(|p| p.model_controls == "beside")
        {
            self.toggle_mono_models(window, cx);
            return;
        }
        self.mono_ui.models = None;
        if self.menu.as_ref().is_some_and(|open| open.kind == kind) {
            self.close_menu(window, cx);
            return;
        }
        // Another menu gives way at once rather than overlapping this one.
        self.menu = None;
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        self.git_picker = None;
        self.git_events = None;
        let mut events = Vec::new();
        let (label, items, width, style) = self.menu_items(kind, &mut events, window, cx);
        // The recent models open on the current one.
        let current = (kind == ComposerMenu::Recent)
            .then(|| {
                items.iter().find_map(|item| match item {
                    MenuItem::Action(action) if action.checked => Some(action.id.clone()),
                    _ => None,
                })
            })
            .flatten();
        let menu = cx.new(|cx| {
            let menu = Menu::new(label, items, window, cx)
                .width(width)
                .style(style);
            match current {
                Some(id) => menu.select_value(&id),
                None => menu,
            }
        });
        events.push(
            cx.subscribe_in(&menu, window, |this, menu, event: &MenuEvent, _, cx| {
                if let MenuEvent::Picked(id, _) = event {
                    let toggle = menu.read(cx).toggle_state(id);
                    if id.as_ref() == "upload" {
                        this.choose_attachments(cx);
                    } else {
                        this.menu_choice(id, cx);
                    }
                    if let Some(on) = toggle {
                        menu.update(cx, |menu, cx| menu.set_toggle(id, !on, cx));
                        cx.notify();
                        return;
                    }
                }
                this.menu = None;
                cx.notify();
            }),
        );
        self.menu = Some(OpenMenu {
            kind,
            menu,
            _events: events,
        });
        cx.notify();
    }
    fn menu_items(
        &self,
        kind: ComposerMenu,
        events: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (&'static str, Vec<MenuItem>, f32, MenuStyle) {
        let provider = Kind::from_id(&self.summary.provider);
        match kind {
            ComposerMenu::Account => {
                let items = crate::accounts::get(cx)
                    .map(|center| {
                        center
                            .read(cx)
                            .book
                            .list(provider)
                            .into_iter()
                            .map(|account| {
                                MenuAction::new(
                                    format!("usage-account:{}", account.id),
                                    account.label,
                                )
                                .checked(account.id == self.settings.account.id)
                                .into()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ("Switch account", items, 250., MenuStyle::Explorer)
            }
            ComposerMenu::Workspace => {
                let picker = cx.new(|cx| {
                    GitPicker::new(
                        self.summary.cwd.clone().into(),
                        GitMode::Worktree,
                        Some(self.execution_cwd().to_owned()),
                        cx,
                    )
                    .existing_worktrees()
                });
                events.push(cx.subscribe_in(
                    &picker,
                    window,
                    |this, _, picked: &GitPicked, window, cx| match picked {
                        GitPicked::Worktree(path) => {
                            if this.use_existing_worktree(path, cx) {
                                this.close_menu(window, cx);
                            }
                        }
                        GitPicked::Dismissed => this.close_menu(window, cx),
                        _ => {}
                    },
                ));
                let focus = picker.read(cx).focus(cx);
                (
                    "Workspace",
                    vec![
                        MenuItem::Header("Workspace".into()),
                        MenuAction::new("workspace:current", "Current checkout")
                            .icon("folder")
                            .checked(
                                !self.settings.new_worktree && self.settings.worktree_cwd.is_none(),
                            )
                            .into(),
                        MenuAction::new("workspace:new", "New worktree")
                            .icon("folder-tree")
                            .checked(self.settings.new_worktree)
                            .into(),
                        MenuItem::Separator,
                        MenuAction::new("workspace:existing", "Existing worktrees")
                            .icon("folder-tree")
                            .flyout(Flyout {
                                view: picker.into(),
                                width: 300.,
                                padding: 0.,
                                height: None,
                                max_height: Some(320.),
                                focus: Some(focus),
                                surface: None,
                            })
                            .into(),
                    ],
                    240.,
                    MenuStyle::Settings,
                )
            }
            // Composer.tsx's plus menu with native mode and attachment actions.
            ComposerMenu::Add => {
                let mut items = vec![MenuItem::Header("Add to message".into())];
                items.extend(
                    [
                        MenuAction::new("upload", "Upload file")
                            .icon("file-add")
                            .description("Attach files or images"),
                        MenuAction::new("plan", "Plan mode")
                            .icon("ai-idea")
                            .icon_color(tint(YELLOW_300, 0.8))
                            .description("Review a plan before building"),
                        MenuAction::new("operator", "Operator")
                            .disabled(self.is_remote())
                            .icon("cursor-magic")
                            .icon_color(tint(SKY_300, 0.8))
                            .description("Give this thread access to MonoCode"),
                        MenuAction::new("orchestrator", "Orchestrator")
                            .disabled(self.is_remote())
                            .icon("share")
                            .icon_color(tint(FUCHSIA_300, 0.65))
                            .badge("v1")
                            .description("Plan and coordinate agent work"),
                        MenuAction::new("draft", "Draft")
                            .icon("circle-dashed-hi")
                            .description("Save this message without starting the agent")
                            .disabled(self.busy || self.follow_ups.state.draft.is_some()),
                    ]
                    .into_iter()
                    .map(|action| {
                        let checked = match action.id.as_ref() {
                            "plan" => self.settings.plan,
                            "draft" => self.draft_active(cx),
                            "operator" => self.settings.operator,
                            "orchestrator" => self.settings.orchestrator,
                            _ => false,
                        };
                        action.checked(checked).into()
                    }),
                );
                ("Add to message", items, 250., MenuStyle::Composer)
            }
            ComposerMenu::GitActions => (
                "Git actions",
                self.git_action_menu_items(cx),
                250.,
                MenuStyle::Composer,
            ),
            // ModelPicker: a "Model" row whose flyout lists the models.
            ComposerMenu::Model => {
                let list = cx.new(|cx| {
                    let hidden = cx
                        .try_global::<crate::settings::Preferences>()
                        .map(|p| p.hidden_for(&self.summary.cwd))
                        .unwrap_or_default();
                    ModelList::new_for(
                        provider,
                        self.model.clone(),
                        &self.summary.cwd,
                        self.settings.account.clone(),
                        cx,
                    )
                    .hide_providers(hidden)
                    .allow_provider_switch(true)
                });
                events.push(cx.subscribe_in(
                    &list,
                    window,
                    |this, _, picked: &ModelPicked, window, cx| {
                        if this.apply_model_pick(picked, cx) {
                            this.close_menu(window, cx);
                        }
                    },
                ));
                let focus = list.read(cx).search_focus(cx);
                let mut items = if cx
                    .try_global::<crate::settings::Preferences>()
                    .is_none_or(|p| p.model_controls != "beside")
                {
                    self.mono_model_options()
                } else {
                    Vec::new()
                };
                items.push(
                    MenuAction::new("model", "Model")
                        .value_image(provider.id())
                        .value(self.model_info().map_or_else(
                            || model_name(provider, self.model.as_deref()),
                            |model| model_display_name(provider, &model.id, &model.name),
                        ))
                        .flyout(Flyout {
                            view: list.into(),
                            width: 310.,
                            padding: 0.,
                            height: Some(442.),
                            max_height: None,
                            focus: Some(focus),
                            surface: None,
                        })
                        .into(),
                );
                ("Model and settings", items, 250., MenuStyle::Settings)
            }
            ComposerMenu::Recent => (
                "Recently used models",
                self.recent_model_items(cx),
                250.,
                MenuStyle::Recent,
            ),
            ComposerMenu::Access => {
                let items = self.mono_access_options();
                ("Access", items, 288., MenuStyle::Access)
            }
            ComposerMenu::Setting(ix) => self.mono_setting_menu(ix),
        }
    }
    /// The model trigger: the composer's model menu.
    #[cfg(test)]
    pub(crate) fn toggle_model_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.mono.is_some() {
            self.toggle_mono_models(window, cx);
            return;
        }
        self.toggle_menu(ComposerMenu::Model, window, cx);
    }
    /// `App: Switch Model` (Cmd+.): toggles the recent models, as ModelPicker's
    /// hotkey does.
    pub(crate) fn toggle_recent_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mono_ui.models = None;
        self.toggle_menu(ComposerMenu::Recent, window, cx);
    }
    /// Right-clicking the model trigger opens the recent models over any menu.
    fn open_recent_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.menu_is(ComposerMenu::Recent) {
            self.toggle_recent_models(window, cx);
        }
    }
    /// recentMenuModels: up to six remembered choices, with the current model
    /// added when it is not among them.
    fn recent_model_items(&self, cx: &gpui::App) -> Vec<MenuItem> {
        let provider = Kind::from_id(&self.summary.provider);
        let current = (provider, self.model.clone());
        let hidden = cx
            .try_global::<crate::settings::Preferences>()
            .map(|p| p.hidden_for(&self.summary.cwd))
            .unwrap_or_default();
        let mut choices: Vec<(Kind, Option<String>)> = cx
            .try_global::<crate::settings::Preferences>()
            .into_iter()
            .flat_map(|p| &p.recent_models)
            .filter_map(|choice| {
                let kind = Kind::parse(&choice.provider)?;
                choice.model.as_ref()?;
                (kind == provider || !hidden.iter().any(|id| id == kind.id()))
                    .then(|| (kind, choice.model.clone()))
            })
            .collect();
        choices.dedup();
        if !choices.contains(&current) {
            choices.push(current.clone());
        }
        choices.truncate(6);
        choices
            .into_iter()
            .map(|(kind, model)| {
                let selected = (kind, model.clone()) == current;
                let name = if selected {
                    self.model_label()
                } else {
                    let name = cached_model_name(kind, model.as_deref());
                    match name.strip_prefix("Claude ") {
                        Some(short) if kind == Kind::Claude => short.to_owned().into(),
                        _ => name,
                    }
                };
                let id = serde_json::to_string(&(kind.id(), &model)).unwrap_or_default();
                MenuAction::new(format!("recent-model:{id}"), name)
                    .image(kind.id())
                    .description(kind.label())
                    .checked(selected)
                    .into()
            })
            .collect()
    }
    /// `Session: Stop` on Escape: cancels a running turn.
    pub(crate) fn stop_if_busy(&mut self, cx: &mut Context<Self>) {
        if self.cancel_edit_last_turn(cx) {
            return;
        }
        if self.busy {
            self.stop(cx);
        }
    }
    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.busy || self.stopping {
            return;
        }
        self.stopping = true;
        self.busy = false;
        self.follow_ups.resuming = false;
        self.follow_ups.continuations.clear();
        self.notification_cancelled = true;
        self.cancel.store(true, Ordering::Relaxed);
        if !self.follow_ups.state.messages.is_empty() {
            self.follow_ups.state.paused = true;
            self.follow_ups.requested = None;
            self.save_follow_ups(cx);
        }
        self.follow_ups.ready = false;
        self.clear_background();
        self.approvals.clear();
        self.questions.clear();
        self.controls = None;
        self.status = "Ready".into();
        self.transcript
            .update(cx, |view, cx| view.stop_streaming(cx));
        self.review
            .update(cx, |review, cx| review.set_busy(false, cx));
        cx.emit(SessionChanged);
        cx.notify();
    }
    // Composer.tsx in the inset layout: a frosted, rounded box holding the
    // field and its toolbar, with the workspace bar sitting under it.
    fn composer_runner(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::composer_runner::Runner>> {
        // A Mono already signs its turns with its own mascot.
        if self.settings.mono.is_some()
            || cx
                .try_global::<crate::settings::Preferences>()
                .is_some_and(|p| !p.composer_mascot)
        {
            return None;
        }
        if self.busy && self.runner.is_none() {
            let transcript = self.transcript.clone();
            self.runner = Some(cx.new(|cx| crate::composer_runner::Runner::new(transcript, cx)));
        }
        let runner = self.runner.clone()?;
        let name = monocode_core::project_appearance::MASCOTS
            [self.project_appearance.mascot(&self.summary.cwd)];
        let color = self.project_appearance.color(&self.summary.cwd);
        runner.update(cx, |r, cx| r.configure(self.busy, name, color, cx));
        Some(runner)
    }
    fn composer(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let radius = 12.;
        let command = if self.follow_ups.draft_selected {
            Some(input::Command::Draft)
        } else if self.settings.orchestrator {
            Some(input::Command::Orchestrator)
        } else if self.settings.plan {
            Some(input::Command::Plan)
        } else if self.settings.operator {
            Some(input::Command::Operator)
        } else {
            None
        };
        self.input
            .update(cx, |input, cx| input.selected_command(command, cx));
        let runner = self.composer_runner(cx);
        let beside = cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|p| p.model_controls == "beside");
        let trigger_effort = (!beside).then(|| self.trigger_effort_label()).flatten();
        let controls = if beside {
            self.model_controls(cx)
        } else {
            Vec::new()
        };
        let input = self.input.read(cx);
        let has_value = input.has_message_text() || !self.attachments.is_empty();
        self.completion.update(cx, |completion, cx| {
            completion.set_cwd(
                self.settings
                    .worktree_cwd
                    .as_deref()
                    .unwrap_or(&self.summary.cwd),
                cx,
            )
        });
        let kind = Kind::from_id(&self.summary.provider);
        let menu: Vec<_> = self
            .menu
            .as_ref()
            .map(|open| {
                menu_popover(&open.menu, Anchor::Slot(self.anchor(open.kind).clone()), cx)
                    .side(Side::Top)
                    .when(open.kind == ComposerMenu::Account, |p| {
                        p.width(250.).max_height(260.)
                    })
            })
            .into_iter()
            .collect();
        div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(self.composer_anchor.probe())
            .children(menu)
            .children(self.model_list_popover(cx))
            .children(self.orchestration_card(cx))
            .children(if self.is_remote() { None } else { self.usage_notice(false, cx) })
            .child(self.background_shelf(window, cx))
            .child(self.message_queue(window, cx))
            .children(if self.is_remote() { None } else { self.auth_notice(cx) })
            .when(self.settings.worktree_removed,|d|d.child(div().mb_2().p_3().rounded_lg().border_1().border_color(content(0.1)).flex().items_center().gap_3().text_size(px(12.)).child(div().flex_1().child("Worktree deleted. Select a branch or worktree to continue this conversation.")).child(tool_pill("reattach-worktree","Select worktree").child("Select worktree").on_click(cx.listener(|this,_,window,cx|this.open_git(GitMode::Worktree,window,cx))))))
            .when_some(self.settings.handoff.clone(), |d, handoff| {
                d.child(
                    div()
                        .mb(px(6.))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(10.))
                        .bg(content(0.05))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .child(icon("response-handoff", 14.))
                        .child(div().min_w_0().flex_1().child(format!(
                            "Handoff from {} · Add your next instruction below",
                            Kind::from_id(&handoff.from).label()
                        )))
                        .child(
                            tool_pill("dismiss-handoff", "Dismiss handoff")
                                .child(icon("close", 12.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.handoff = None;
                                    this.save_settings(cx);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(self.git_picker.clone(), |d, picker| {
                let anchor = if picker.read(cx).mode() == GitMode::Worktree {
                    self.workspace_anchor.clone()
                } else {
                    self.branch_anchor.clone()
                };
                let weak = cx.entity().downgrade();
                d.child(
                    popover("composer-git-picker", Anchor::Slot(anchor))
                        .side(Side::Top)
                        .width(300.)
                        .height(320.)
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.git_picker = None;
                                this.git_events = None;
                                window.focus(&this.input.focus_handle(cx), cx);
                                cx.notify();
                            });
                        })
                        .child(picker),
                )
            })
            .child(self.approval_shelf(window, cx))
            .child(
                div()
                    .id("composer-box")
                    .debug_selector(|| "composer-box".into())
                    .capture_action(cx.listener(|this, _: &input::Paste, _, cx| {
                        if this.paste_attachments(cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .relative()
                    .rounded(px(radius))
                    .border_1()
                    .border_color(composer_border(self.input.focus_handle(cx).is_focused(window)))
                    .shadow(composer_shadow())
                    .when(self.file_drag, |d| d.border_color(accent_color().opacity(0.6)))
                    .flex()
                    .flex_col()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            window.focus(&this.input.focus_handle(cx), cx)
                        }),
                    )
                    .child(self.completion.read(cx).composer_probe())
                    .child(self.btw_home.0.probe())
                    .child(composer_surface(radius))
                    .children(self.attachment_chips(cx))
                    .child(
                        div()
                            .px_3()
                            .pt(px(14.))
                            .pb(px(14.))
                            .child(self.completion.clone()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "composer-toolbar".into())
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .pb(px(10.))
                            .child(
                                tool_pill("composer-add", "Add files or choose a mode")
                                    .aria_expanded(self.menu_is(ComposerMenu::Add))
                                    .when(self.menu_is(ComposerMenu::Add), |d| d.bg(content(0.2)))
                                    .child(self.add_anchor.probe())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_menu(ComposerMenu::Add, window, cx)
                                    }))
                                    .w(px(26.))
                                    .justify_center()
                                    .text_color(content(0.5))
                                    .child(
                                        icon("plus", 14.)
                                            .text_color(content(0.5))
                                            .group_hover("composer-add", |s| {
                                                s.text_color(content(1.))
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        tool_pill("composer-model", "Model")
                                            .min_w_0()
                                            .max_w(px(220.))
                                            .flex_shrink(1.)
                                            .aria_label(format!(
                                                "{}, {}{}",
                                                kind.label(),
                                                self.model_label(),
                                                trigger_effort
                                                    .as_ref()
                                                    .map(|effort| format!(", effort {effort}"))
                                                    .unwrap_or_default()
                                            ))
                                            .child(self.model_anchor.probe())
                                            .aria_expanded(self.menu_is(ComposerMenu::Model) || self.menu_is(ComposerMenu::Recent) || self.mono_ui.models.is_some())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_menu(ComposerMenu::Model, window, cx)
                                            }))
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                cx.listener(|this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.open_recent_models(window, cx)
                                                }),
                                            )
                                            .child(provider_icon(&self.summary.provider, 16.))
                                            .child(pill_label(self.model_label()))
                                            .when_some(trigger_effort, |d, effort| {
                                                d.child(
                                                    div()
                                                        .debug_selector(|| "composer-model-effort".into())
                                                        .flex_shrink_0()
                                                        .text_size(px(11.))
                                                        .text_color(content(0.5))
                                                        .child(effort),
                                                )
                                            })
                                            .child(icon("chevron-down", 12.)),
                                    )
                                    .children(controls)
                                    .child(
                                        tool_pill("composer-access", "Access")
                                            .aria_label(format!(
                                                "Access, {}",
                                                self.settings.access.label()
                                            ))
                                            .aria_expanded(self.menu_is(ComposerMenu::Access))
                                            .child(self.access_anchor.probe())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_menu(ComposerMenu::Access, window, cx)
                                            }))
                                            .child(
                                                icon(
                                                    match self.settings.access {
                                                        AccessMode::FullAccess => "shield-alert",
                                                        AccessMode::AutoAcceptEdits => {
                                                            "pencil-edit"
                                                        }
                                                        AccessMode::Auto => "sparkles",
                                                        _ => "square-lock",
                                                    },
                                                    14.,
                                                )
                                                .text_color(
                                                    if self.settings.access
                                                        == AccessMode::FullAccess
                                                    {
                                                        tint(AMBER_400, 0.9)
                                                    } else {
                                                        content(1.)
                                                    },
                                                ),
                                            )
                                            .child(pill_label(self.settings.access.label()))
                                            .child(icon("chevron-down", 12.)),
                                    ),
                            )
                            .child(self.action(has_value, window, cx)),
                    )
                    .children(self.attachment_drop_overlay(12.)),
            )
            .children(runner.map(|runner| {
                runner.cached(crate::composer_runner::Runner::placement())
            }))
            .children(self.remote_notice(cx))
            .child(self.workspace_bar(cx))
            .into_any_element()
    }
    fn workspace_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = !self.summary.has_messages;
        let linked =
            self.git.as_ref().is_some_and(|s| s.linked) || self.settings.worktree_cwd.is_some();
        let new = draft && self.settings.new_worktree;
        let label = if new {
            "New worktree"
        } else if linked {
            "Worktree"
        } else {
            "Current checkout"
        };
        let enabled = !self.busy && !self.preparing_worktree && self.git.is_some();
        // Still reading git is not unavailable: don't dim, then brighten.
        let dimmed = !enabled && (self.loaded || self.busy || self.preparing_worktree);
        let workspace = if draft {
            bar_trigger(
                "composer-workspace",
                if new || linked {
                    "folder-tree"
                } else {
                    "folder"
                },
                label.into(),
            )
            .aria_label(format!("Workspace {label}"))
            .aria_expanded(self.menu_is(ComposerMenu::Workspace))
            .child(self.workspace_anchor.probe())
            .when(dimmed, |d| d.opacity(0.4))
            .on_click(cx.listener(move |this, _, window, cx| {
                if enabled {
                    this.refresh_git(cx);
                    this.toggle_menu(ComposerMenu::Workspace, window, cx);
                }
            }))
            .into_any_element()
        } else {
            // WorkspaceIdentity: a started conversation owns this working copy.
            div()
                .id("composer-workspace")
                .role(Role::Group)
                .aria_label(format!("Workspace {label}"))
                .flex_shrink_0()
                .h(px(24.))
                .ml(px(-6.))
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(content(0.55 * 0.4))
                .child(
                    icon(if linked { "folder-tree" } else { "folder" }, 14.)
                        .text_color(content(0.55 * 0.4)),
                )
                .child(label)
                .into_any_element()
        };
        let branch = if new {
            self.settings.worktree_base.clone().or(self.branch.clone())
        } else {
            self.branch.clone()
        };
        let ratio = self.context.and_then(|usage| usage.ratio());
        let show_context = ratio.is_some() || draft;
        let headline = ratio
            .map(|r| format!("{:.0}% context used", r * 100.))
            .unwrap_or_else(|| "No context used yet".into());
        let detail = self.context.map(|usage| {
            format!(
                "{} / {} tokens",
                monocode_core::context::format_tokens(usage.used),
                monocode_core::context::format_tokens(usage.window.unwrap_or(0))
            )
        });
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .pt(px(6.))
            .px(px(15.))
            .child(workspace)
            .when_some(branch, |d, branch| {
                let detached = !new && self.git.as_ref().is_some_and(|s| s.detached);
                let label = if detached {
                    format!("detached {branch}")
                } else {
                    branch
                };
                d.child(
                    bar_trigger("composer-branch", "git-branch", label.clone().into())
                        .aria_label(format!(
                            "{} {label}",
                            if new { "Worktree base" } else { "Branch" }
                        ))
                        .aria_description(self.execution_cwd().to_owned())
                        .max_w(px(256.))
                        .child(self.branch_anchor.probe())
                        .when(dimmed, |d| d.opacity(0.4))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if enabled {
                                this.open_git(
                                    if new { GitMode::Base } else { GitMode::Branch },
                                    window,
                                    cx,
                                );
                            }
                        })),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(self.editing_last_turn.pending(), |d| {
                        d.child(
                            div()
                                .id("cancel-edit-last-turn")
                                .role(Role::Button)
                                .aria_label("Cancel editing")
                                .text_size(px(11.))
                                .text_color(content(0.6))
                                .child("Cancel edit")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_edit_last_turn(cx);
                                })),
                        )
                    })
                    .when(
                        self.status != "Ready"
                            && !self.busy
                            && !self.loading
                            && self.settings.usage_limit.is_none(),
                        |d| {
                            d.child(
                                div()
                                    .id("provider-status")
                                    .debug_selector(|| "provider-status".into())
                                    .role(Role::Status)
                                    .aria_label(self.status.clone())
                                    .max_w(px(220.))
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(self.status.clone()),
                            )
                        },
                    )
                    .when(show_context, |d| {
                        d.child(
                            div()
                                .id("context-meter")
                                .role(Role::Image)
                                .aria_label(format!(
                                    "{headline}{}",
                                    detail
                                        .as_ref()
                                        .map(|s| format!(", {s}"))
                                        .unwrap_or_default()
                                ))
                                .child(self.context_anchor.probe())
                                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                    this.context_hovered = *hovered;
                                    cx.notify();
                                }))
                                .child(crate::context_meter::ring(ratio.unwrap_or(0.)))
                                .when(self.context_hovered, |d| {
                                    d.child(
                                        popover(
                                            "context-tooltip",
                                            Anchor::Slot(self.context_anchor.clone()),
                                        )
                                        .side(Side::Top)
                                        .align(Align::End)
                                        .max_width(240.)
                                        .padding(10.)
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(content(1.))
                                                .child(headline)
                                                .when_some(detail, |d, detail| {
                                                    d.child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(content(0.5))
                                                            .child(detail),
                                                    )
                                                }),
                                        ),
                                    )
                                }),
                        )
                    }),
            )
    }

    // ComposerAction: a solid submit, dimmed while empty; Stop while busy.
    fn action(&self, has_value: bool, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.responding() && !has_value;
        let draft = self.draft_active(cx);
        let modifiers = window.modifiers();
        let alternate = if cfg!(target_os = "macos") {
            modifiers.platform
        } else {
            modifiers.control
        };
        let follow_up = (has_value && !draft)
            .then(|| self.follow_up_action(alternate, cx))
            .flatten();
        let enabled = !self.attaching
            && (!self.preparing_worktree || self.busy || self.stopping)
            && (busy || (has_value && self.loaded))
            && (busy || !draft || (!self.busy && self.follow_ups.state.draft.is_none()));
        send_button(
            "send-or-stop",
            if busy {
                "Stop"
            } else if draft {
                "Save draft"
            } else {
                follow_up.map_or("Send", |(label, _)| label)
            },
            busy,
            follow_up.map_or("send", |(_, glyph)| glyph),
            enabled,
        )
        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
            if !enabled {
                return;
            }
            if busy {
                this.stop(cx);
            } else {
                let modifiers = event.modifiers();
                let alternate = if cfg!(target_os = "macos") {
                    modifiers.platform
                } else {
                    modifiers.control
                };
                this.send_follow_up(alternate, cx);
            }
        }))
    }
    // EmptySession.tsx: the composer centred under its question, or resting
    // where it will dock once a neighbouring pane squeezes it narrower than
    // the column. A narrow window alone keeps it centred.
    fn empty(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let games = cx
            .try_global::<crate::settings::Preferences>()
            .is_none_or(|p| p.empty_session_games)
            && cx
                .try_global::<crate::settings::Preferences>()
                .is_none_or(|p| p.chat_background.is_none());
        let arcade = if games {
            Some(
                self.arcade
                    .get_or_insert_with(|| cx.new(crate::arcade::Arcade::new))
                    .clone(),
            )
        } else {
            None
        };
        let this = cx.entity().downgrade();
        let narrow = self.narrow;
        let docked = narrow && self.beside_pane;
        self.attachment_drop_target("empty-session", cx)
            .relative()
            .size_full()
            .flex()
            .justify_center()
            .children(arcade)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let now = bounds.size.width < px(COMPOSER_COLUMN);
                        if now != narrow {
                            cx.defer(move |cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.narrow = now;
                                    cx.notify();
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .w_full()
                    .max_w(px(COMPOSER_COLUMN))
                    .flex()
                    .flex_col()
                    .px(px(10.))
                    .pt(px(24.))
                    .pb(px(6.))
                    .child(div().flex_grow(1.).flex_basis(px(0.)))
                    .when(!docked, |d| {
                        d.child(
                            div()
                                .id("empty-session-title")
                                .role(Role::Heading)
                                .flex_shrink_0()
                                .mb_4()
                                .px(px(10.))
                                .text_size(px(18.))
                                .line_height(px(28.))
                                .text_color(content(1.))
                                .truncate()
                                .child(format!(
                                    "What should we work on in {}?",
                                    self.project_appearance.label(&self.summary.cwd)
                                )),
                        )
                    })
                    .child(self.composer(window, cx))
                    .when(!docked, |d| {
                        d.child(div().flex_grow(1.).flex_shrink(1.).flex_basis(px(18.)))
                    }),
            )
            .into_any()
    }
}

/// The access mode the last composer pick left as the default for new chats.
pub(crate) fn default_access(cx: &gpui::App) -> AccessMode {
    cx.try_global::<crate::settings::Preferences>()
        .and_then(|prefs| prefs.default_access.as_deref())
        .map(AccessMode::from_id)
        .unwrap_or_default()
}
pub(crate) fn remember_access(mode: AccessMode, cx: &mut gpui::App) {
    if cx.has_global::<crate::settings::Preferences>() {
        cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
            prefs.default_access = Some(mode.id().into());
        });
    }
}
fn is_effort_key(key: &str) -> bool {
    matches!(
        key,
        "effort" | "reasoning" | "reasoningEffort" | "reasoning_effort"
    )
}
pub(crate) fn preferred_effort(provider: &str, cx: &gpui::App) -> Option<String> {
    cx.try_global::<crate::settings::Preferences>()
        .and_then(|prefs| prefs.provider_efforts.get(provider).cloned())
}
pub(crate) fn effort_name(value: &str) -> String {
    match value {
        "default" => "Default",
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra High",
        "max" => "Max",
        "ultra" => "Ultra",
        "ultracode" => "Ultracode",
        "ultrathink" => "Ultrathink",
        other => other,
    }
    .into()
}

fn model_control_toggle(control: &catalog::ModelControl) -> bool {
    control.choices.len() == 2
        && control.choices.iter().any(|(value, _)| value == "true")
        && control.choices.iter().any(|(value, _)| value == "false")
}

#[cfg(test)]
mod composer_motion_tests {
    use super::*;
    use gpui::{
        Bounds, EntityInputHandler, TestAppContext, WindowBounds, WindowOptions, point, size,
    };
    use monocode_core::store::Store;
    use std::{sync::Mutex, time::Duration};

    fn window(cx: &mut TestAppContext, width: f32) -> gpui::WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/work/monocode").unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(width), px(700.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| Conversation::new(summary, store, cx)),
            )
            .unwrap()
        });
        window
            .update(cx, |c, w, cx| {
                c.loaded = true;
                c.status = "Ready".into();
                w.focus(&c.input.focus_handle(cx), cx);
                c.input.update(cx, |i, cx| {
                    i.replace_text_in_range(None, "First message", w, cx)
                });
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
    }
    fn begin(
        window: gpui::WindowHandle<Conversation>,
        cx: &mut TestAppContext,
    ) -> Bounds<gpui::Pixels> {
        let from = window
            .update(cx, |c, _, cx| {
                let from = c.composer_anchor.get().unwrap();
                // Use the first-send presentation path without a provider request.
                c.input.update(cx, |i, cx| {
                    i.take(cx);
                });
                c.begin_turn(cx);
                cx.notify();
                from
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        from
    }
    #[gpui::test]
    fn composer_uses_react_surface_and_equal_text_padding_with_attachments(
        cx: &mut TestAppContext,
    ) {
        let preferences = crate::settings::Preferences::default();
        crate::theme::configure(&preferences, false);
        cx.update(|cx| {
            cx.set_global(preferences);
            cx.set_reduce_motion(true);
        });
        for width in [480., 1000.] {
            let window = window(cx, width);
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            for docked in [false, true] {
                for attached in [false, true] {
                    window
                        .update(visual, |c, _, cx| {
                            c.summary.has_messages = docked;
                            c.attachments = if attached {
                                vec![std::path::PathBuf::from("/work/notes.txt")]
                            } else {
                                Vec::new()
                            };
                            c.input.update(cx, |input, cx| {
                                input.set_text(
                                    "First typed line\nSecond typed line\nThird typed line".into(),
                                    cx,
                                );
                            });
                            cx.notify();
                        })
                        .unwrap();
                    visual.run_until_parked();
                    visual.refresh().unwrap();
                    let composer = visual.debug_bounds("composer-box").unwrap();
                    let field = visual.debug_bounds("composer-text-field").unwrap();
                    let toolbar = visual.debug_bounds("composer-toolbar").unwrap();
                    let top = if attached {
                        visual
                            .debug_bounds("composer-attachments")
                            .unwrap()
                            .bottom()
                    } else {
                        composer.top() + px(1.)
                    };
                    assert_eq!(field.top() - top, px(14.));
                    assert_eq!(toolbar.top() - field.bottom(), px(14.));
                    assert_eq!(field.size.height, px(66.));
                    window
                        .update(visual, |_, window, _| {
                            let scale = window.scale_factor();
                            let surface = window
                                .painted_quads()
                                .into_iter()
                                .find(|quad| {
                                    (quad.corner_radii.top_left.0 / scale - 11.).abs() < 0.1
                                        && quad.background.as_solid() == Some(content(0.03))
                                        && (quad.bounds.size.width.0 / scale
                                            - f32::from(composer.size.width))
                                        .abs()
                                            <= 2.
                                })
                                .expect("the composer paints React's content-3% surface");
                            #[cfg(any(target_os = "macos", target_os = "windows"))]
                            assert_eq!(
                                surface.border_color,
                                gpui::backdrop_blur_marker(24. * scale)
                            );
                            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                            let _ = surface;
                            assert!(window.painted_quads().iter().any(|quad| {
                                quad.border_color == content(0.2)
                                    && (quad.bounds.size.width.0 / scale
                                        - f32::from(composer.size.width))
                                    .abs()
                                        < 0.1
                            }));
                        })
                        .unwrap();
                }
            }
        }
    }

    #[gpui::test]
    fn first_submit_moves_the_painted_composer_down_without_resizing_chat(cx: &mut TestAppContext) {
        let window = window(cx, 1000.);
        let from = begin(window, cx);
        let (initial, transcript) = window
            .update(cx, |c, _, cx| {
                (
                    c.composer_anchor.get().unwrap(),
                    c.transcript.read(cx).list.viewport_bounds(),
                )
            })
            .unwrap();
        assert!(
            (initial.top() - from.top()).abs() < px(1.),
            "the first docked frame must stay at the empty-session slot"
        );
        window
            .update(cx, |c, _, cx| {
                c.composer_motion.elapsed(Duration::from_millis(120));
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let middle = window
            .update(cx, |c, w, cx| {
                let bounds = c.composer_anchor.get().unwrap();
                let painted = w
                    .painted_quads()
                    .into_iter()
                    .find(|q| {
                        (q.corner_radii.top_left.0 / w.scale_factor() - 12.).abs() < 0.1
                            && q.bounds.size.width.0 / w.scale_factor() > 500.
                    })
                    .expect("the composer box must be painted at its animated position");
                assert!(
                    (painted.bounds.top().0 / w.scale_factor() - f32::from(bounds.top())).abs()
                        < 1.
                );
                assert!(c.input.focus_handle(cx).is_focused(w));
                c.composer_motion.elapsed(Duration::from_millis(480));
                cx.notify();
                bounds
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let docked = window
            .update(cx, |c, _, _| c.composer_anchor.get().unwrap())
            .unwrap();
        assert!(
            middle.top() > from.top() + px(30.),
            "the composer must slide down"
        );
        assert!(
            middle.top() < docked.top() - px(5.),
            "an intermediate frame must not already be docked"
        );
        assert!((docked.bottom() - px(694.)).abs() < px(1.));
        assert_eq!(
            window
                .update(cx, |c, _, cx| c.transcript.read(cx).list.viewport_bounds())
                .unwrap(),
            transcript,
            "composer motion must not relayout the transcript"
        );
    }
    #[gpui::test]
    fn narrow_layout_and_opening_history_do_not_slide(cx: &mut TestAppContext) {
        let narrow = window(cx, 820.);
        narrow
            .update(cx, |c, _, cx| c.set_beside_pane(true, cx))
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let from = begin(narrow, cx);
        narrow
            .update(cx, |c, _, _| {
                assert!((c.composer_anchor.get().unwrap().top() - from.top()).abs() < px(1.));
            })
            .unwrap();
        let history = window(cx, 1000.);
        let from = history
            .update(cx, |c, _, cx| {
                let from = c.composer_anchor.get().unwrap();
                c.summary.has_messages = true;
                cx.notify();
                from
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        history
            .update(cx, |c, _, _| {
                assert!(c.composer_anchor.get().unwrap().top() > from.top() + px(100.));
            })
            .unwrap();
    }
    #[gpui::test]
    fn reduced_motion_docks_the_first_submit_immediately(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = window(cx, 1000.);
        let from = begin(window, cx);
        window
            .update(cx, |c, _, _| {
                assert!(c.composer_anchor.get().unwrap().top() > from.top() + px(100.));
            })
            .unwrap();
    }
}
impl Render for Conversation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self._input_focus.is_none() {
            let focus = self.input.focus_handle(cx);
            self._input_focus = Some([
                cx.on_focus(&focus, window, |_, _, cx| cx.notify()),
                cx.on_blur(&focus, window, |_, _, cx| cx.notify()),
            ]);
        }
        if self.response_events.is_none() {
            self.response_events = Some(cx.subscribe_in(
                &self.transcript,
                window,
                |this, _, request: &ResponseRequest, window, cx| {
                    this.response_request(request, window, cx)
                },
            ));
        }
        if self.settings.mono.is_some() {
            return self.render_mono(window, cx);
        }
        if !self.summary.has_messages {
            return self.empty(window, cx);
        }
        self.attachment_drop_target("session-chat", cx)
            .relative()
            .capture_action(
                cx.listener(|this, _: &crate::shell::DismissMenu, window, cx| {
                    if this.close_remote_plan(cx) {
                        cx.stop_propagation();
                        return;
                    }
                    // BtwSheet: Esc anywhere in this pane folds an open sheet away.
                    if let Some(btw) = this.btw.clone()
                        && btw.update(cx, |btw, cx| btw.dismiss(window, cx))
                    {
                        cx.stop_propagation();
                    }
                }),
            )
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .when_some(self.response_menu.as_ref(), |d, menu| {
                let panel =
                    menu_popover(&menu.menu, Anchor::Slot(menu.anchor.clone()), cx).side(Side::Top);
                d.child(
                    menu.regions
                        .iter()
                        .fold(panel, |panel, region| panel.ignore(region.clone())),
                )
            })
            .child({
                let pill = self.git_action_pill(cx);
                let beside = pill.is_some();
                self.transcript
                    .update(cx, |t, cx| t.set_jump_beside_pill(beside, cx));
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        self.transcript
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                    // Level with the transcript's jump button, which moves to
                    // the column's other end.
                    .children(pill.map(|pill| {
                        div()
                            .absolute()
                            .bottom(px(8.))
                            .left_0()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(COMPOSER_COLUMN))
                                    .px(px(16.))
                                    .flex()
                                    .child(pill),
                            )
                    }))
            })
            .child(
                div()
                    .flex_shrink_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COMPOSER_COLUMN))
                            .opacity(self.btw_home.1)
                            .flex()
                            .flex_col()
                            .px(px(10.))
                            .pb(px(6.))
                            .child({
                                let composer = self
                                    .composer_motion
                                    .clone()
                                    .wrap(self.composer(window, cx))
                                    .into_any_element();
                                let focus = self.input.focus_handle(cx);
                                let card = self.questions.front().map(|(card, _)| card);
                                self.question_overlay
                                    .wrap(composer, card, &focus, window, cx)
                            }),
                    ),
            )
            .when_some(self.btw.as_ref(), |d, sheet| d.child(sheet.clone()))
            .children(self.remote_plan_reader(cx))
            .into_any()
    }
}

// The composer's `h-6.5` toolbar pills share `bg-selection` and its hover.
fn tool_pill(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .group(id)
        .cursor_pointer()
        .relative()
        .flex_shrink_0()
        .h(px(26.))
        .px(px(6.))
        .rounded_md()
        .flex()
        .items_center()
        .gap_1()
        .bg(content(0.1))
        .text_color(content(1.))
        .hover(|s| s.bg(content(0.15)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
/// `[data-composer-box]::before`: the frosted (or, in light themes, solid)
/// surface inside the composer's border.
fn composer_surface(radius: f32) -> impl IntoElement {
    let blur = composer_blur();
    let fill = composer_fill();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let surface = gpui::fill(bounds, fill).corner_radii(px(radius - 1.));
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let surface = if blur > 0. {
                surface.border_color(gpui::backdrop_blur_marker(blur * window.scale_factor()))
            } else {
                surface
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = blur;
            window.paint_quad(surface);
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
/// ComposerAction: the solid send button, or a stop square while busy.
fn send_button(
    id: &'static str,
    label: &'static str,
    busy: bool,
    glyph: &'static str,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let accent = primary_action(enabled);
    // Without an accent the button is the inverse of the surface: white with
    // black ink on dark, black with white ink on light.
    let light = crate::theme::is_light();
    let solid: fn(f32) -> gpui::Hsla = if light { black } else { white };
    let ink: fn(f32) -> gpui::Hsla = if light { white } else { black };
    div()
        .id(id)
        .debug_selector(|| id.into())
        .role(Role::Button)
        .aria_label(label)
        .cursor_pointer()
        .flex_shrink_0()
        .size(px(26.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .map(|d| match accent {
            Some((fill, _)) => d
                .bg(fill)
                .when(enabled, |d| d.hover(|s| s.bg(fill.blend(content(0.12))))),
            None => d
                .bg(solid(if enabled { 1. } else { 0.3 }))
                .when(enabled, |d| d.hover(|s| s.bg(solid(0.9)))),
        })
        .map(|d| {
            let ink = accent.map_or(ink(if enabled { 1. } else { 0.4 }), |(_, ink)| ink);
            if busy {
                d.child(div().size(px(10.)).rounded(px(2.)).bg(ink))
            } else {
                d.child(icon(glyph, 14.).text_color(ink))
            }
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
// GitPickerTrigger: the quiet `h-6` controls in the bar under the box.
fn bar_trigger(
    id: &'static str,
    path: &'static str,
    label: SharedString,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .group(id)
        .cursor_pointer()
        .min_w_0()
        .h(px(24.))
        .ml(px(-6.))
        .px(px(6.))
        .rounded_md()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(content(0.55))
        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            icon(path, 14.)
                .text_color(content(0.55))
                .group_hover(id, |s| s.text_color(content(1.))),
        )
        .child(div().min_w_0().truncate().child(label))
}
fn pill_label(label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .min_w_0()
        .truncate()
        .text_size(px(11.))
        .child(label.into())
}

/// The checked-out branch (or short detached commit) for `cwd`, read from the
/// repository's HEAD, including linked worktrees whose `.git` is a file.
pub(crate) fn git_branch(cwd: &Path) -> Option<String> {
    let root = cwd.ancestors().find(|d| d.join(".git").exists())?;
    let dot = root.join(".git");
    let git = if dot.is_dir() {
        dot
    } else {
        let link = std::fs::read_to_string(&dot).ok()?;
        root.join(link.strip_prefix("gitdir:")?.trim())
    };
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => branch.to_owned(),
        None => head.chars().take(7).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::git_branch;
    #[test]
    fn reads_branch_from_repositories_and_linked_worktrees() {
        let root = std::env::temp_dir().join(format!("monocode-branch-{}", std::process::id()));
        let repo = root.join("repo");
        let nested = repo.join("src/deep");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(git_branch(&nested).as_deref(), Some("feature/x"));

        let linked = root.join("linked");
        let meta = repo.join(".git/worktrees/linked");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::write(linked.join(".git"), format!("gitdir: {}\n", meta.display())).unwrap();
        std::fs::write(meta.join("HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(git_branch(&linked).as_deref(), Some("0123456"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle, point};
    use monocode_core::store::Store;
    use std::sync::Mutex;
    #[gpui::test]
    fn combined_picker_paints_effort_and_beside_mode_moves_it_to_its_own_control(
        cx: &mut TestAppContext,
    ) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.summary.provider = "codex".into();
                view.model = Some("gpt-6.1-sol".into());
                view.settings.effort = Some("xhigh".into());
                view.catalog = vec![Model {
                    alias_target: None,
                    id: "gpt-6.1-sol".into(),
                    name: "GPT-6.1 Sol".into(),
                    efforts: vec!["high".into(), "xhigh".into()],
                    default_effort: Some("high".into()),
                    fast: false,
                    thinking: false,
                    default: true,
                    tiers: Vec::new(),
                    controls: Vec::new(),
                }];
                cx.set_global(crate::settings::Preferences::default());
                cx.notify();
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let effort = visual
            .debug_bounds("composer-model-effort")
            .expect("combined model picker includes effort");
        let model = window
            .update(visual, |view, _, _| view.model_anchor.get().unwrap())
            .unwrap();
        assert!(effort.left() > model.left() && effort.right() < model.right());
        window
            .update(visual, |_, _, cx| {
                cx.global_mut::<crate::settings::Preferences>()
                    .model_controls = "beside".into();
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("composer-model-effort").is_none());
        window
            .update(visual, |view, _, _| {
                assert!(
                    view.mono_ui.setting_anchors[0].get().is_some(),
                    "beside mode paints its effort control"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn composer_labels_use_the_saved_model_effort_and_the_catalog_default(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, _| {
                view.summary.provider = "codex".into();
                view.model = Some("gpt-6.1-sol".into());
                view.settings.effort = Some("xhigh".into());
                view.catalog.clear();
                assert_eq!(view.model_label(), "GPT-6.1-Sol");
                assert_eq!(view.trigger_effort_label().as_deref(), Some("Extra High"));
                view.catalog.push(Model {
                    alias_target: None,
                    id: "gpt-6.1-sol".into(),
                    name: "GPT-6.1 Sol".into(),
                    efforts: vec!["high".into(), "xhigh".into()],
                    default_effort: Some("high".into()),
                    fast: false,
                    thinking: false,
                    default: true,
                    tiers: Vec::new(),
                    controls: Vec::new(),
                });
                assert_eq!(view.model_label(), "GPT-6.1 Sol");
                assert_eq!(view.trigger_effort_label().as_deref(), Some("Extra High"));
                view.model = None;
                view.settings.effort = None;
                assert_eq!(view.model_label(), "GPT-6.1 Sol");
                assert_eq!(view.trigger_effort_label().as_deref(), Some("High"));
                view.model = Some("gpt-custom-model".into());
                assert_eq!(view.model_label(), "GPT-custom-model");
                assert!(
                    view.model_info().is_none(),
                    "unknown saved models must not inherit a different model's controls"
                );
                assert!(view.trigger_effort_label().is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn tool_paths_follow_restored_and_transferred_working_copies(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.loaded = false;
                view.settings.revision = 0;
                view.restore_worktree(Some("/work/feature".into()), None, cx);
                assert_eq!(
                    view.transcript.read(cx).file_link_base.as_deref(),
                    Some("/work/feature")
                );
                // History/transfer/workspace refreshes all pass through sync_review.
                view.settings.worktree_cwd = Some("/work/transferred".into());
                view.sync_review(cx);
                assert_eq!(
                    view.transcript.read(cx).file_link_base.as_deref(),
                    Some("/work/transferred")
                );
                view.settings.worktree_removed = true;
                view.sync_review(cx);
                assert_eq!(
                    view.transcript.read(cx).file_link_base.as_deref(),
                    Some("/work/monocode")
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn response_target_picker_preserves_model_effort_and_working_copy(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let picked = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = picked.clone();
        let _subscription = window
            .update(cx, |view, window, cx| {
                let subscription = cx.subscribe(&cx.entity(), move |_, _, event: &Transfer, _| {
                    *result.borrow_mut() = Some(event.clone())
                });
                view.summary.has_messages = true;
                view.settings.worktree_cwd = Some("/work/isolated-copy".into());
                view.transcript.update(cx, |transcript, cx| {
                    transcript.patch(
                        0,
                        vec![
                            monocode_core::transcript::Row {
                                role: "user".into(),
                                text: "Review the footer".into(),
                                first: true,
                                sent_at: None,
                            },
                            monocode_core::transcript::Row {
                                role: "assistant".into(),
                                text: "The footer is ready.".into(),
                                first: true,
                                sent_at: None,
                            },
                        ],
                        cx,
                    )
                });
                let model = Model {
                    alias_target: None,
                    id: "gpt-review".into(),
                    name: "Review model".into(),
                    efforts: vec!["low".into(), "high".into()],
                    default_effort: Some("low".into()),
                    fast: false,
                    thinking: false,
                    default: true,
                    tiers: Vec::new(),
                    controls: vec![catalog::ModelControl {
                        id: "context".into(),
                        name: "Context".into(),
                        current: "1m".into(),
                        choices: vec![("1m".into(), "1M".into())],
                    }],
                };
                view.make_response_menu(
                    1,
                    monocode_core::response::TransferKind::Handoff,
                    AnchorSlot::default(),
                    Some(vec![(Kind::Codex, Some(Ok(vec![model])))]),
                    window,
                    cx,
                );
                cx.notify();
                subscription
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "right right down enter");
        cx.run_until_parked();
        let result = picked.borrow();
        let result = result
            .as_ref()
            .expect("a target is picked through provider, model and effort");
        assert_eq!(result.target.provider, "codex");
        assert_eq!(result.target.model.as_deref(), Some("gpt-review"));
        assert_eq!(result.target.effort.as_deref(), Some("low"));
        assert_eq!(
            result.target.settings.get("context").map(String::as_str),
            Some("1m")
        );
        assert_eq!(
            result.settings.worktree_cwd.as_deref(),
            Some("/work/isolated-copy")
        );
        assert_eq!(
            monocode_core::response::request(&result.turn),
            "Review the footer"
        );
    }

    #[gpui::test]
    fn new_tab_shows_its_folders_branch_at_once_and_no_loading_status(cx: &mut TestAppContext) {
        crate::git_cache::remember(
            "/work/remembered",
            &monocode_core::git::Snapshot {
                current: Some("main".into()),
                ..Default::default()
            },
        );
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/work/remembered").unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut c = Conversation::new(summary, store, cx);
                    // The first frame of a tab whose history is still loading.
                    c.loading = true;
                    c
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |c, _, _| {
                assert_eq!(c.branch.as_deref(), Some("main"));
                assert!(c.git.is_some());
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("provider-status").is_none());
        // Once loaded, a real status (an error here) still shows.
        window
            .update(visual, |c, _, cx| {
                c.loading = false;
                c.status = "Couldn't load".into();
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("provider-status").is_some());
    }

    #[gpui::test]
    fn picked_access_becomes_the_default_for_new_chats(cx: &mut TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let first = store.create("codex", "/work").unwrap();
        let second = store.create("codex", "/work").unwrap();
        let store = Arc::new(Mutex::new(store));
        cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(first, store.clone(), cx))
            })
            .unwrap()
        });
        window
            .update(cx, |c, _, cx| {
                assert_eq!(c.settings.access, AccessMode::Supervised);
                c.menu_choice("access:auto", cx);
            })
            .unwrap();
        cx.update(|cx| {
            assert_eq!(
                cx.global::<crate::settings::Preferences>()
                    .default_access
                    .as_deref(),
                Some("auto")
            );
            let next = cx.new(|cx| Conversation::new(second, store, cx));
            assert_eq!(next.read(cx).settings.access, AccessMode::Auto);
        });
    }

    #[gpui::test]
    fn effort_choices_survive_provider_switches_and_new_projects(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
        let window = conversation(cx);
        window
            .update(cx, |c, _, cx| {
                c.menu_choice("effort:medium", cx);
                c.apply_model_pick(&ModelPicked(Kind::Codex, Some("gpt-6.1-sol".into())), cx);
                c.menu_choice("effort:xhigh", cx);
                c.apply_model_pick(
                    &ModelPicked(Kind::Claude, Some("claude-opus-5-5".into())),
                    cx,
                );
                assert_eq!(c.settings.effort.as_deref(), Some("medium"));
                c.apply_model_pick(&ModelPicked(Kind::Codex, Some("gpt-6.1-sol".into())), cx);
                assert_eq!(c.settings.effort.as_deref(), Some("xhigh"));
                c.menu_choice("access:auto", cx);
                let encoded =
                    serde_json::to_string(cx.global::<crate::settings::Preferences>()).unwrap();
                let restored: crate::settings::Preferences =
                    serde_json::from_str(&encoded).unwrap();
                assert_eq!(restored.provider_efforts["claude"], "medium");
                assert_eq!(restored.provider_efforts["codex"], "xhigh");
                assert_eq!(restored.default_access.as_deref(), Some("auto"));
                let store = Store::open(Path::new(":memory:")).unwrap();
                let summary = store.create("codex", "/different/project").unwrap();
                let next = cx.new(|cx| Conversation::new(summary, Arc::new(Mutex::new(store)), cx));
                assert_eq!(next.read(cx).settings.effort.as_deref(), Some("xhigh"));
                assert_eq!(next.read(cx).settings.access, AccessMode::Auto);
            })
            .unwrap();
    }

    fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("claude", "/work/monocode").unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(summary, store, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |c, w, cx| w.focus(&c.input.focus_handle(cx), cx))
            .unwrap();
        cx.run_until_parked();
        window
    }
    fn open(window: WindowHandle<Conversation>, kind: ComposerMenu, cx: &mut TestAppContext) {
        window
            .update(cx, |c, w, cx| c.toggle_menu(kind, w, cx))
            .unwrap();
        cx.run_until_parked();
    }
    fn input_focused(window: WindowHandle<Conversation>, cx: &mut TestAppContext) -> bool {
        window
            .update(cx, |c, w, cx| c.input.focus_handle(cx).is_focused(w))
            .unwrap()
    }

    #[gpui::test]
    fn workspace_existing_worktrees_open_as_a_submenu_and_preserve_the_draft(
        cx: &mut TestAppContext,
    ) {
        use monocode_core::git::{Snapshot, Worktree};
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
        });
        let cwd = "/nonexistent/monocode-composer-existing-submenu";
        let feature = format!("{cwd}-feature-b");
        crate::git_cache::remember(
            cwd,
            &Snapshot {
                current: Some("main".into()),
                worktrees: vec![
                    Worktree {
                        path: cwd.into(),
                        branch: Some("main".into()),
                        main: true,
                        ..Default::default()
                    },
                    Worktree {
                        path: format!("{cwd}-feature-a"),
                        branch: Some("feature-a".into()),
                        ..Default::default()
                    },
                    Worktree {
                        path: feature.clone(),
                        branch: Some("feature-b".into()),
                        ..Default::default()
                    },
                    Worktree {
                        path: format!("{cwd}-missing"),
                        branch: Some("missing".into()),
                        missing: true,
                        ..Default::default()
                    },
                    Worktree {
                        path: format!("{cwd}-detached"),
                        head: "123456789abc".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("claude", cwd).unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(summary, store, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |c, w, cx| {
                c.input.update(cx, |input, cx| {
                    input.set_text("Keep my unsent draft".into(), cx)
                });
                c.settings.new_worktree = true;
                c.toggle_menu(ComposerMenu::Workspace, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_keystrokes("down down right");
        visual.run_until_parked();
        let parent = visual
            .debug_bounds("menu-action-workspace:existing")
            .unwrap();
        let submenu = visual
            .debug_bounds("existing-worktrees-submenu")
            .expect("existing copies open beside the workspace menu");
        assert!(submenu.left() >= parent.right());
        assert!(submenu.size.height > px(0.) && submenu.size.height <= px(320.));
        assert!(
            visual.debug_bounds("git-option-2").is_some(),
            "detached copies remain available"
        );
        assert!(
            visual.debug_bounds("git-option-3").is_none(),
            "main and missing copies are excluded"
        );
        window
            .update(visual, |c, _, _| {
                assert!(c.menu_is(ComposerMenu::Workspace));
                assert!(
                    c.git_picker.is_none(),
                    "the submenu never replaces its parent"
                );
            })
            .unwrap();

        visual.simulate_keystrokes("left");
        visual.run_until_parked();
        assert!(visual.debug_bounds("existing-worktrees-submenu").is_none());
        assert!(
            visual
                .debug_bounds("menu-action-workspace:existing")
                .is_some()
        );
        visual.simulate_keystrokes("right escape");
        visual.run_until_parked();
        window
            .update(visual, |c, w, cx| {
                assert!(c.menu.is_none());
                assert!(c.input.focus_handle(cx).is_focused(w));
                assert!(
                    c.settings.new_worktree,
                    "dismissal does not change workspace mode"
                );
                c.toggle_menu(ComposerMenu::Workspace, w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        let existing = visual
            .debug_bounds("menu-action-workspace:existing")
            .unwrap();
        visual.simulate_mouse_move(existing.center(), None, Default::default());
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("existing-worktrees-submenu").is_some(),
            "hover also opens the submenu"
        );
        visual.simulate_input("feature-b");
        visual.run_until_parked();
        assert!(visual.debug_bounds("git-option-0").is_some());
        assert!(visual.debug_bounds("git-option-1").is_none());
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        window
            .update(visual, |c, w, cx| {
                assert!(c.menu.is_none());
                assert_eq!(c.execution_cwd(), feature);
                assert_eq!(
                    c.transcript.read(cx).file_link_base.as_deref(),
                    Some(feature.as_str())
                );
                assert!(!c.settings.new_worktree);
                assert!(!c.busy);
                assert_eq!(c.input.read(cx).content.as_ref(), "Keep my unsent draft");
                assert!(c.input.focus_handle(cx).is_focused(w));
                assert_eq!(
                    locked(&c.store)
                        .unwrap()
                        .agent_settings(c.summary.id)
                        .unwrap()
                        .unwrap()
                        .worktree_cwd
                        .as_deref(),
                    Some(feature.as_str())
                );
                c.toggle_menu(ComposerMenu::Workspace, w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        let current = visual
            .debug_bounds("menu-action-workspace:current")
            .unwrap();
        visual.simulate_click(current.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |c, _, cx| {
                assert!(c.settings.worktree_cwd.is_none());
                assert_eq!(c.execution_cwd(), cwd);
                assert_eq!(c.input.read(cx).content.as_ref(), "Keep my unsent draft");
            })
            .unwrap();
    }

    #[gpui::test]
    fn mode_menu_edits_the_inline_prefix_and_removal_preserves_the_draft(cx: &mut TestAppContext) {
        use gpui::EntityInputHandler;
        let window = conversation(cx);
        window
            .update(cx, |c, _, cx| {
                c.input
                    .update(cx, |i, cx| i.set_text("Review this".into(), cx));
                c.toggle_mode("plan", cx);
                assert_eq!(c.input.read(cx).content, "/plan Review this");
                c.toggle_mode("operator", cx);
                assert_eq!(c.input.read(cx).content, "/operator Review this");
                c.toggle_mode("orchestrator", cx);
                assert_eq!(c.input.read(cx).content, "/orchestrator Review this");
                c.toggle_mode("orchestrator", cx);
                assert_eq!(c.input.read(cx).content, "Review this");
                c.toggle_mode("plan", cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |c, w, cx| {
                c.input.update(cx, |i, cx| {
                    i.select_range(0..i.content.len(), cx);
                    i.replace_text_in_range(None, "/operator New draft", w, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |c, _, cx| {
                assert!(!c.settings.plan);
                assert_eq!(c.input.read(cx).content, "/operator New draft");
                c.enable_mode("operator", cx);
                c.input.update(cx, |i, cx| i.select_range(1..1, cx));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "backspace");
        cx.run_until_parked();
        window
            .update(cx, |c, _, cx| {
                assert!(!c.settings.operator);
                assert_eq!(c.input.read(cx).content, "operator New draft");
            })
            .unwrap();
    }

    #[gpui::test]
    fn submitted_modes_clear_for_a_started_turn_and_stay_cleared_after_completion(
        cx: &mut TestAppContext,
    ) {
        for mode in ["operator", "orchestrator", "plan"] {
            let window = conversation(cx);
            window
                .update(cx, |view, _, cx| {
                    // An unsupported synthetic provider stops before any CLI I/O.
                    view.summary.provider = "fixture".into();
                    view.loaded = true;
                    view.input
                        .update(cx, |i, cx| i.set_text("check the composer".into(), cx));
                    view.toggle_mode(mode, cx);
                    view.send(cx);
                    assert!(view.busy);
                    assert!(view.input.read(cx).content.is_empty());
                    assert!(
                        !view.settings.plan
                            && !view.settings.operator
                            && !view.settings.orchestrator
                    );
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    let saved = locked(&view.store)
                        .unwrap()
                        .agent_settings(view.summary.id)
                        .unwrap()
                        .unwrap();
                    assert!(!saved.plan && !saved.operator && !saved.orchestrator);
                    assert!(view.input.read(cx).content.is_empty());
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn model_menu_picks_by_keyboard_and_returns_focus(cx: &mut TestAppContext) {
        let window = conversation(cx);
        open(window, ComposerMenu::Model, cx);
        assert!(
            !input_focused(window, cx),
            "the menu takes focus while open"
        );
        // Open the Model row's flyout, which focuses its search field with
        // Default highlighted, then take the first catalog model.
        cx.simulate_keystrokes(window.into(), "right down enter");
        cx.run_until_parked();
        window
            .update(cx, |c, _, _| {
                assert!(c.menu.is_none());
                assert_eq!(c.model.as_deref(), Some("sonnet"));
                // The pill drops the catalog's "Claude " prefix, as React's does.
                assert_eq!(c.model_label(), "Sonnet");
            })
            .unwrap();
        assert!(input_focused(window, cx));

        open(window, ComposerMenu::Model, cx);
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |c, _, _| {
                assert!(c.menu.is_none());
                assert_eq!(c.model.as_deref(), Some("sonnet"));
            })
            .unwrap();
        assert!(input_focused(window, cx));
    }

    #[gpui::test]
    fn recent_models_open_on_right_click_and_pick_by_keyboard(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |c, _, cx| {
                c.model = Some("opus".into());
                let prefs = crate::settings::Preferences {
                    recent_models: [
                        ("codex", Some("gpt-6.1-sol")),
                        ("claude", Some("opus")),
                        ("claude", None),
                        ("codex", Some("gpt-6-luna")),
                    ]
                    .into_iter()
                    .map(|(provider, model)| crate::settings::ModelChoice {
                        provider: provider.into(),
                        model: model.map(Into::into),
                    })
                    .collect(),
                    ..Default::default()
                };
                cx.set_global(prefs);
                cx.notify();
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let pill = window
            .update(visual, |c, _, _| c.model_anchor.get().unwrap())
            .unwrap()
            .center();
        visual.simulate_event(gpui::MouseDownEvent {
            position: pill,
            button: MouseButton::Right,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        });
        visual.run_until_parked();
        window
            .update(visual, |c, _, cx| {
                assert!(c.menu_is(ComposerMenu::Recent));
                // Recents with a model, in order; the default is not one.
                let items = c.recent_model_items(cx);
                let labels: Vec<_> = items
                    .iter()
                    .filter_map(|item| match item {
                        MenuItem::Action(a) => Some((a.label.to_string(), a.checked)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    labels,
                    [
                        ("GPT-6.1-Sol".to_string(), false),
                        ("Opus".to_string(), true),
                        ("GPT-6-luna".to_string(), false),
                    ]
                );
            })
            .unwrap();
        // The current model is highlighted; Down then Enter takes the next.
        visual.simulate_keystrokes("down enter");
        visual.run_until_parked();
        window
            .update(visual, |c, _, cx| {
                assert!(c.menu.is_none());
                assert_eq!(c.summary.provider, "codex");
                assert_eq!(c.model.as_deref(), Some("gpt-6-luna"));
                let recent = &cx.global::<crate::settings::Preferences>().recent_models[0];
                assert_eq!(recent.model.as_deref(), Some("gpt-6-luna"));
                // App: Switch Model toggles the same menu.
            })
            .unwrap();
        window
            .update(visual, |c, w, cx| c.toggle_recent_models(w, cx))
            .unwrap();
        window
            .update(visual, |c, _, _| assert!(c.menu_is(ComposerMenu::Recent)))
            .unwrap();
        window
            .update(visual, |c, w, cx| c.toggle_recent_models(w, cx))
            .unwrap();
        window
            .update(visual, |c, _, _| assert!(c.menu.is_none()))
            .unwrap();
    }

    #[gpui::test]
    fn pressing_outside_dismisses_and_the_trigger_toggles(cx: &mut TestAppContext) {
        let window = conversation(cx);
        open(window, ComposerMenu::Access, cx);
        cx.update_window(window.into(), |_, w, cx| {
            w.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    position: point(px(1.), px(1.)),
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |c, _, _| assert!(c.menu.is_none()))
            .unwrap();
        open(window, ComposerMenu::Add, cx);
        open(window, ComposerMenu::Add, cx);
        window
            .update(cx, |c, _, _| assert!(c.menu.is_none()))
            .unwrap();
        assert!(input_focused(window, cx));
    }
}
