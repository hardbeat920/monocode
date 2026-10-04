//! The view half of src/features/connections/ui/RemoteSession.tsx: a tab in
//! a project on another machine. The host owns the session; this pane draws
//! it with the normal transcript and composer, with a status line over them
//! and the placeholder for a machine that is not connected here.
//!
//! The data hooks of RemoteSession.tsx (polling and pushed changes, the
//! command outbox, optimistic turns, configuration changes, the usage limit
//! choices) live in the engine's remote package. It hands the pane what they
//! derive as [`RemoteSessionProps`], answers the composer through
//! [`RemoteSessionHost`], and acts on [`RemoteSessionEvent`]s.

mod notice;

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::session::{ComposerTurnOptions, session_draft_block, session_work_cwd};
use monocode_core::{Attachment, HarnessId, ModelPrefs, ModelSettings, RuntimeMode, Session};
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, UiStyled as _, u};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{
    Composer, ComposerEvent, ComposerHost, ComposerProps, ComposerSubmission, RemoteFeatures,
    SkillContext,
};
use monocode_view_composer::pickers::ModelSource;
use monocode_view_transcript::transcript::{TranscriptConfig, TranscriptEvent, TranscriptView};

pub use notice::{
    FailedTurn, NoticeAction, RemoteNotice, RemoteSessionStatus, host_file_path, remote_notice,
};

use crate::style::{ButtonKind, action_button, tinted_text};

#[cfg(test)]
mod tests;

/// `max-w-4xl`: the docked composer's width.
const COMPOSER_MAX_WIDTH: f32 = 896.;

/// Whether the tab's machine is connected on this computer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemoteMachineState {
    /// The machine is paired and the session can be shown.
    #[default]
    Connected,
    /// `!project`: the tab's project lost its machine details.
    MissingProject,
    /// The machine list has not loaded yet.
    Connecting,
    /// No paired machine has the project's environment.
    NotConnected,
}

impl RemoteMachineState {
    /// The placeholder text, for every state but `Connected`.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::Connected => None,
            Self::MissingProject => Some(
                "This project’s machine details are missing. Add the project again from the project rail.",
            ),
            Self::Connecting => Some("Connecting to the machine…"),
            Self::NotConnected => {
                Some("The machine for this project isn’t connected on this computer.")
            }
        }
    }
}

/// What the pane shows. The engine derives it from the host snapshot, the
/// outbox, and the composer selection; the field comments name the
/// `RemoteSessionOverrides` and RemoteSession.tsx values they stand for.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteSessionProps {
    pub machine: RemoteMachineState,
    /// The machine's name, for the status line.
    pub machine_name: String,
    /// The machine's environment, the root of its `remote://` paths.
    pub environment_id: String,
    /// `session`: the host's copy with this computer's unconfirmed turn
    /// appended, leaving drafts removed, the composer selection applied,
    /// and `cwd` under the machine's `remote://` root.
    pub session: Option<Arc<Session>>,
    /// `executionCwd`: the host checkout the session runs in, for relative
    /// paths in the transcript.
    pub execution_cwd: String,
    /// The machine answered the last request.
    pub online: bool,
    /// `remoteFeatures`: what this host version supports.
    pub features: RemoteFeatures,
    /// `remoteSessionLoading`: the tab has a host session whose transcript
    /// has not arrived yet.
    pub loading: bool,
    /// `remoteSessionStarted`: the tab has a host session.
    pub started: bool,
    /// `allowedModelHarnesses`: the session's provider once it started,
    /// else the providers the host has.
    pub allowed_model_harnesses: Vec<HarnessId>,
    pub status: RemoteSessionStatus,
    /// False while another tab is in front.
    pub visible: bool,
    /// The pane holds the window's focus.
    pub focused: bool,
    /// `canCompactHarnessContext(session.harness)`.
    pub compact_supported: bool,
    /// `monocode.modelControls === "beside"`.
    pub model_controls_beside: bool,
    /// The composer runner setting.
    pub runner_enabled: bool,
    /// An empty session in a split or an inbox question keeps its composer docked.
    pub force_docked: bool,
    /// Turn animations off, for screenshots and tests.
    pub animate: bool,
    /// The transcript settings (layout, prompt anchoring, model catalog).
    /// The pane sets the remote-specific flags itself.
    pub transcript: TranscriptConfig,
}

impl Default for RemoteSessionProps {
    fn default() -> Self {
        Self {
            machine: RemoteMachineState::Connected,
            machine_name: String::new(),
            environment_id: String::new(),
            session: None,
            execution_cwd: String::new(),
            online: false,
            features: RemoteFeatures::default(),
            loading: false,
            started: false,
            allowed_model_harnesses: vec![HarnessId::Codex, HarnessId::Claude],
            status: RemoteSessionStatus::default(),
            visible: true,
            focused: false,
            compact_supported: false,
            model_controls_beside: false,
            runner_enabled: true,
            force_docked: false,
            animate: true,
            transcript: TranscriptConfig::default(),
        }
    }
}

impl RemoteSessionProps {
    /// The status line, if any.
    pub fn notice(&self) -> Option<RemoteNotice> {
        remote_notice(&self.status, &self.machine_name)
    }

    /// `dockComposer`: the composer sits under the transcript, not in the
    /// middle of an empty session.
    pub fn docks_composer(&self) -> bool {
        let Some(session) = self.session.as_deref() else {
            return true;
        };
        self.loading
            || (session_draft_block(&session.blocks).is_none()
                && (!session.blocks.is_empty() || self.force_docked))
    }

    /// The empty session's heading.
    pub fn empty_title(&self) -> String {
        match self
            .session
            .as_deref()
            .and_then(|session| project_label(&session.cwd))
        {
            Some(project) => format!("What should we work on in {project}?"),
            None => "What should we work on?".into(),
        }
    }
}

/// The folder name at the end of a project path.
fn project_label(cwd: &str) -> Option<String> {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && !name.ends_with(':'))
        .map(str::to_string)
}

/// The composer props SessionPane.tsx passed for a host session.
pub fn composer_props(props: &RemoteSessionProps) -> ComposerProps {
    let mut composer = ComposerProps {
        enabled: props.visible,
        focused: props.focused && props.visible,
        hotkeys: props.focused && props.visible,
        remote_session: true,
        remote_features: Some(props.features),
        allowed_model_harnesses: Some(props.allowed_model_harnesses.clone()),
        compact_supported: props.compact_supported,
        model_controls_beside: props.model_controls_beside,
        runner_enabled: props.runner_enabled,
        // Notes, side questions, and folders are this computer's features.
        notes_enabled: false,
        btw_enabled: false,
        folders_enabled: false,
        animate: props.animate,
        shell: !props.docks_composer(),
        ..ComposerProps::default()
    };
    if let Some(session) = props.session.as_deref() {
        let busy = session.is_busy();
        composer.harness = session.harness;
        composer.model = session.model.clone();
        composer.model_settings = session.model_settings.clone();
        composer.runtime_mode = session.runtime_mode;
        composer.cwd = session.cwd.clone();
        composer.execution_cwd = session_work_cwd(session).to_string();
        composer.session_id = Some(session.id.clone());
        composer.branch = session.branch.clone().filter(|branch| !branch.is_empty());
        composer.context = session.context;
        composer.busy = busy;
        composer.queued_messages = session.queued_messages.clone().unwrap_or_default();
        composer.queue_status = session.queue_status;
        composer.worktree_removed = session.worktree_removed == Some(true);
        composer.disabled = session.pending_question.is_some();
        composer.can_save_draft = props.features.draft
            && !busy
            && session_draft_block(&session.blocks).is_none()
            && session.inbox_ask.is_none()
            && session.inbox_card.is_none()
            && session.note_card.is_none()
            && session.handoff_card.is_none();
    }
    composer
}

/// The transcript settings SessionPane.tsx passed for a host session: no
/// edits of the last turn, second opinions, handoffs, or build targets.
pub fn transcript_config(props: &RemoteSessionProps) -> TranscriptConfig {
    let mut config = props.transcript.clone();
    let session = props.session.as_deref();
    let removed = session.is_some_and(|session| session.worktree_removed == Some(true));
    config.visible = props.visible;
    config.approvals = !removed;
    config.can_build_plans = !removed;
    config.can_build_plan_targets = false;
    config.can_open_plans = true;
    config.can_send_drafts =
        session.is_some_and(|session| session_draft_block(&session.blocks).is_some());
    config.can_edit_last_turn = false;
    config.editing_last_turn = false;
    config.can_second_opinion = false;
    config.can_handoff = false;
    config.can_save_notes = false;
    config
}

/// What the composer needs from the engine for a host session. Callbacks
/// that return a value (whether the host took a turn) are methods; the rest
/// are [`RemoteSessionEvent`]s.
pub trait RemoteSessionHost: 'static {
    /// `submit` in RemoteSession.tsx: send a turn, or create the host
    /// session with it. Returns false when the turn was not taken (offline,
    /// busy, a request already pending, or a settings change not applied
    /// yet), so the composer keeps the text.
    fn submit(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        options: ComposerTurnOptions,
        window: &mut Window,
        cx: &mut App,
    ) -> bool;

    /// `saveDraft`: save the text as a draft turn on the host.
    fn save_draft(
        &self,
        _text: String,
        _attachments: Vec<Attachment>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> bool {
        false
    }

    /// `stopTurn`: cancel the running turn.
    fn stop(&self, _window: &mut Window, _cx: &mut App) {}

    /// `compact`: `/compact` through the host's provider.
    fn compact(&self, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }

    /// Attachments for dropped or picked files. The engine uploads them to
    /// the host before the turn is recorded.
    fn attachments_from_paths(&self, _paths: Vec<String>, _cx: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }

    fn attachments_from_files(
        &self,
        _files: Vec<ClipboardFile>,
        _cx: &mut App,
    ) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }

    fn pick_attachments(&self, _window: &mut Window, _cx: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }

    /// The model source over the host's model catalog
    /// (`ModelSourceContext` in RemoteSession.tsx).
    fn model_source(&self, _cx: &mut App) -> Option<Rc<dyn ModelSource>> {
        None
    }

    /// The model picker's saved favorites and recents.
    fn model_prefs(&self, _cx: &mut App) -> ModelPrefs {
        ModelPrefs::default()
    }

    /// The composer text changed, for the draft cache.
    fn draft_changed(&self, _text: &str, _cx: &mut App) {}
}

/// Adapts a [`RemoteSessionHost`] to the composer. Host sessions have no
/// local skills or `@` file mentions.
struct RemoteComposerHost(Rc<dyn RemoteSessionHost>);

impl ComposerHost for RemoteComposerHost {
    fn submit(&self, submission: ComposerSubmission, window: &mut Window, cx: &mut App) -> bool {
        self.0.submit(
            submission.text,
            submission.attachments,
            submission.options,
            window,
            cx,
        )
    }

    fn stop(&self, window: &mut Window, cx: &mut App) {
        self.0.stop(window, cx);
    }

    fn save_draft(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        self.0.save_draft(text, attachments, window, cx)
    }

    fn compact_context(&self, window: &mut Window, cx: &mut App) -> bool {
        self.0.compact(window, cx)
    }

    fn draft_changed(&self, text: &str, cx: &mut App) {
        self.0.draft_changed(text, cx);
    }

    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App) -> Task<Vec<Attachment>> {
        self.0.attachments_from_paths(paths, cx)
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> Task<Vec<Attachment>> {
        self.0.attachments_from_files(files, cx)
    }

    fn pick_attachments(&self, window: &mut Window, cx: &mut App) -> Task<Vec<Attachment>> {
        self.0.pick_attachments(window, cx)
    }

    fn skills(&self, _: &SkillContext, _: &mut App) -> Vec<Skill> {
        Vec::new()
    }

    fn mention_files(&self, _: &str, _: &mut App) -> Vec<ProjectFile> {
        Vec::new()
    }

    fn rank_mentions(&self, _: &str, _: &str, _: &mut App) -> Vec<RankedFile> {
        Vec::new()
    }

    fn model_source(&self, cx: &mut App) -> Option<Rc<dyn ModelSource>> {
        self.0.model_source(cx)
    }

    fn model_prefs(&self, cx: &mut App) -> ModelPrefs {
        self.0.model_prefs(cx)
    }
}

/// What the pane asks of its owner. File paths are already `remote://`
/// paths on the session's machine.
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteSessionEvent {
    /// `approve`: answer a tool approval in the running turn.
    Approve {
        request_id: i64,
        decision: ApprovalDecision,
    },
    /// Open a file in the shared remote tabs.
    OpenFile {
        path: String,
        line: Option<i64>,
    },
    /// Open the session's diff, at `path` when one file was picked.
    OpenDiff {
        path: Option<String>,
    },
    OpenUrl {
        url: String,
    },
    /// Open a plan block as a read-only plan tab.
    OpenPlan {
        block_id: String,
    },
    /// `buildPlan`: build the approved plan on the host.
    BuildPlan {
        block_id: String,
    },
    /// `onRemoveDraft`: remove a draft turn, or the whole session when the
    /// draft is all it has.
    RemoveDraft {
        block_id: String,
    },
    /// The composer picked another model.
    ModelChange {
        harness: HarnessId,
        model: String,
    },
    /// Effort or other model settings changed.
    ModelSettingsChange(ModelSettings),
    /// The permission mode changed.
    RuntimeModeChange(RuntimeMode),
    /// The model picker's favorites changed.
    FavoritesChange(Vec<String>),
    /// The status line's button.
    Notice(NoticeAction),
    /// `OPEN_CONNECTIONS_EVENT`: show Settings → Connections.
    ManageMachines,
    /// The composer was clicked or focused.
    Focus,
    /// Text the transcript copied, for the copy cue.
    Copied {
        text: String,
    },
}

/// The pane for a session on another machine.
pub struct RemoteSessionPane {
    host: Rc<dyn RemoteSessionHost>,
    props: RemoteSessionProps,
    transcript: Entity<TranscriptView>,
    composer: Entity<Composer>,
    empty_view: Option<AnyView>,
    navigation_view: Option<AnyView>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RemoteSessionEvent> for RemoteSessionPane {}

impl RemoteSessionPane {
    pub fn new(
        host: Rc<dyn RemoteSessionHost>,
        props: RemoteSessionProps,
        initial_draft: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = transcript_config(&props);
        let session = props.session.clone();
        let transcript = cx.new(|cx| {
            let mut view = TranscriptView::new(cx);
            view.set_config(config, cx);
            if let Some(session) = session {
                view.set_session(session, cx);
            }
            view
        });
        let composer_host: Rc<dyn ComposerHost> = Rc::new(RemoteComposerHost(host.clone()));
        let composer_props = composer_props(&props);
        let composer =
            cx.new(|cx| Composer::new(composer_host, composer_props, initial_draft, window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&transcript, window, Self::on_transcript_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
        ];
        Self {
            host,
            props,
            transcript,
            composer,
            empty_view: None,
            navigation_view: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn props(&self) -> &RemoteSessionProps {
        &self.props
    }

    pub fn transcript(&self) -> &Entity<TranscriptView> {
        &self.transcript
    }

    pub fn composer(&self) -> &Entity<Composer> {
        &self.composer
    }
    /// A persistent empty session with the same composer entity and its arcade.
    pub fn set_empty_view(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.empty_view = Some(view);
        cx.notify();
    }
    pub fn set_navigation_view(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.navigation_view = Some(view);
        cx.notify();
    }

    /// The engine's state changed.
    pub fn set_props(
        &mut self,
        props: RemoteSessionProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session_changed = match (&self.props.session, &props.session) {
            (Some(old), Some(new)) => !Arc::ptr_eq(old, new) && old != new,
            (None, None) => false,
            _ => true,
        };
        self.props = props;
        let config = transcript_config(&self.props);
        let session = self.props.session.clone();
        self.transcript.update(cx, |transcript, cx| {
            transcript.set_config(config, cx);
            if session_changed && let Some(session) = session {
                transcript.set_session(session, cx);
            }
        });
        let next = composer_props(&self.props);
        if self.composer.read(cx).props() != &next {
            self.composer
                .update(cx, |composer, cx| composer.set_props(next, window, cx));
        }
        cx.notify();
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        let composer = self.composer.clone();
        composer.update(cx, |composer, cx| composer.focus(window, cx));
    }

    fn host_path(&self, path: &str) -> String {
        host_file_path(&self.props.environment_id, &self.props.execution_cwd, path)
    }

    fn on_transcript_event(
        &mut self,
        _: &Entity<TranscriptView>,
        event: &TranscriptEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let event = match event {
            TranscriptEvent::Approval {
                request_id,
                decision,
            } => RemoteSessionEvent::Approve {
                request_id: *request_id,
                decision: *decision,
            },
            TranscriptEvent::OpenFile { path, line } => RemoteSessionEvent::OpenFile {
                path: self.host_path(path),
                line: *line,
            },
            TranscriptEvent::OpenDiff { path } => RemoteSessionEvent::OpenDiff {
                path: Some(self.host_path(path)),
            },
            TranscriptEvent::OpenUrl { url } => RemoteSessionEvent::OpenUrl { url: url.clone() },
            TranscriptEvent::OpenPlan { block_id } => RemoteSessionEvent::OpenPlan {
                block_id: block_id.clone(),
            },
            TranscriptEvent::BuildPlan { block_id } => RemoteSessionEvent::BuildPlan {
                block_id: block_id.clone(),
            },
            TranscriptEvent::RemoveDraft { block_id } => RemoteSessionEvent::RemoveDraft {
                block_id: block_id.clone(),
            },
            TranscriptEvent::SendDraft { block_id } => {
                self.send_draft(block_id, window, cx);
                return;
            }
            TranscriptEvent::Copied { text } => RemoteSessionEvent::Copied { text: text.clone() },
            // Host sessions have no edits of the last turn, notes, second
            // opinions, handoffs, or local change review.
            TranscriptEvent::BuildPlanWithTarget { .. }
            | TranscriptEvent::EditLastTurn
            | TranscriptEvent::SaveNote { .. }
            | TranscriptEvent::SecondOpinion { .. }
            | TranscriptEvent::Handoff { .. }
            | TranscriptEvent::UndoChanges
            | TranscriptEvent::KeepChanges
            | TranscriptEvent::ReviewChanges { .. }
            | TranscriptEvent::JumpToBottomChanged { .. } => return,
        };
        cx.emit(event);
    }

    /// `onSendDraft`: send a draft block as the next turn.
    fn send_draft(&mut self, block_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(block) = self
            .props
            .session
            .as_deref()
            .and_then(|session| session.blocks.iter().find(|block| block.id == block_id))
            .cloned()
        else {
            return;
        };
        let options = ComposerTurnOptions {
            draft_block_id: Some(block.id.clone()),
            ..ComposerTurnOptions::default()
        };
        self.host.submit(
            block.text.clone(),
            block.attachments.clone().unwrap_or_default(),
            options,
            window,
            cx,
        );
    }

    fn on_composer_event(
        &mut self,
        _: &Entity<Composer>,
        event: &ComposerEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let event = match event {
            ComposerEvent::Focus => RemoteSessionEvent::Focus,
            ComposerEvent::ModelChange { harness, model } => RemoteSessionEvent::ModelChange {
                harness: *harness,
                model: model.clone(),
            },
            ComposerEvent::ModelSettingsChange(settings) => {
                RemoteSessionEvent::ModelSettingsChange(settings.clone())
            }
            ComposerEvent::RuntimeModeChange(mode) => RemoteSessionEvent::RuntimeModeChange(*mode),
            ComposerEvent::FavoritesChange(favorites) => {
                RemoteSessionEvent::FavoritesChange(favorites.clone())
            }
            ComposerEvent::OpenFile { path, line } => RemoteSessionEvent::OpenFile {
                path: self.host_path(path),
                line: *line,
            },
            // Queues, MCP settings, and edits of the last turn are this
            // computer's features (the no-op overrides in RemoteSession.tsx).
            _ => return,
        };
        cx.emit(event);
    }

    fn render_placeholder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let machine = self.props.machine;
        let mut column = div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(u(12.))
            .p(u(24.))
            .child(
                div()
                    .text_px(theme.text.body)
                    .text_color(theme.content(0.60))
                    .debug_selector(|| "remote-placeholder".into())
                    .child(machine.message().unwrap_or_default()),
            );
        if machine == RemoteMachineState::NotConnected {
            column = column.child(
                action_button("manage-machines", "Manage machines")
                    .small()
                    .medium(false)
                    .on_click(
                        cx.listener(|_, _, _, cx| cx.emit(RemoteSessionEvent::ManageMachines)),
                    ),
            );
        }
        column
    }

    fn render_notice(&self, notice: RemoteNotice, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut spans: Vec<(SharedString, Option<gpui::Hsla>)> =
            vec![(notice.text.clone().into(), None)];
        if !notice.detail.is_empty() {
            spans.push((
                format!(" {}", notice.detail).into(),
                Some(theme.content(0.40)),
            ));
        }
        let mut text = div()
            .id("remote-notice-text")
            .min_w_0()
            .flex_1()
            .truncate()
            .child(tinted_text(spans));
        if !notice.detail.is_empty() {
            text = text.tooltip(tooltip(SharedString::from(notice.detail.clone())));
        }
        let action = notice.action;
        let role = if notice.alert { "alert" } else { "status" };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(12.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(16.))
            .py(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.65))
            .debug_selector(move || format!("remote-notice:{role}"))
            .child(text)
            .child(
                action_button("remote-notice-action", action.label())
                    .kind(ButtonKind::Ghost)
                    .small()
                    .selector(format!("notice:{}", action.label()))
                    .disabled(!notice.action_enabled(self.props.online))
                    .on_click(
                        cx.listener(move |_, _, _, cx| cx.emit(RemoteSessionEvent::Notice(action))),
                    ),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(view) = &self.empty_view {
            return div().size_full().child(view.clone());
        }
        if self.props.docks_composer() {
            return div().size_full();
        }
        let theme = Theme::of(cx).clone();
        div().size_full().flex().justify_center().child(
            div()
                .flex()
                .flex_col()
                .justify_center()
                .w_full()
                .max_w(u(COMPOSER_MAX_WIDTH))
                .px(u(6.))
                .py(u(48.))
                .child(
                    div()
                        .mb(u(16.))
                        .px(u(10.))
                        .truncate()
                        .text_px(18.)
                        .text_color(theme.colors.content)
                        .debug_selector(|| "empty-session-title".into())
                        .child(self.props.empty_title()),
                )
                .child(
                    div()
                        .w_full()
                        .debug_selector(|| "composer:centered".into())
                        .child(self.composer.clone()),
                ),
        )
    }
}

impl Render for RemoteSessionPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let root = div()
            .id("remote-session")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .leading(theme.leading.normal)
            .text_color(theme.colors.content);
        if self.props.machine != RemoteMachineState::Connected {
            return root.child(self.render_placeholder(cx));
        }
        let docked = self.props.docks_composer();
        let empty = self
            .props
            .session
            .as_deref()
            .is_none_or(|session| session.blocks.is_empty());
        let body = if self.props.loading {
            div().flex_1().min_h_0()
        } else if empty {
            div().flex_1().min_h_0().child(self.render_empty(cx))
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .debug_selector(|| "remote-transcript".into())
                .child(self.transcript.clone())
        };
        let mut root = root;
        if let Some(notice) = self.props.notice() {
            root = root.child(self.render_notice(notice, cx));
        }
        root = root.child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, _, cx| cx.emit(RemoteSessionEvent::Focus)),
                )
                .child(body),
        );
        if docked {
            root = root.child(
                div().flex().flex_none().justify_center().w_full().child(
                    div()
                        .w_full()
                        .max_w(u(COMPOSER_MAX_WIDTH))
                        .debug_selector(|| "composer:docked".into())
                        .child(self.composer.clone()),
                ),
            );
        }
        root.children(self.navigation_view.clone())
    }
}
