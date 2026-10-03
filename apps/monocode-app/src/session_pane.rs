//! One session's pane: its transcript and its composer. The transcript
//! follows the session in `Sessions` as events land, and its actions go to
//! the engine: approvals through attention's `Approvals` (the router sends
//! them to the harness registry), files and diffs to the workspace, and
//! Undo, Keep, and Review to the session checkpoint.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Render, Styled as _, Subscription, Task, WeakEntity, Window, div,
};
use monocode_core::models::FAVORITES_KEY;
use monocode_core::session::session_work_cwd;
use monocode_core::settings::ModelControls;
use monocode_core::{BlockRole, Session};
use monocode_engine::attention::{Approvals, Queues};
use monocode_engine::runtime::Engine;
use monocode_engine::runtime::checkpoint::{ReviewChanged, notify_review_changed};
use monocode_engine::submit::{Submit, SubmitOptions};
use monocode_engine::workspace::workspace::{DiffSession, FileOpenOptions};
use monocode_engine::workspace::{Files, Workspace, paths::FileNavigation};
use monocode_store::checkpoint::CheckpointStatus;
use monocode_ui::widgets::{Toast, ToastKind, Toasts};
use monocode_ui::{Theme, u};
use monocode_view_composer::composer::{Composer, ComposerEvent, ComposerProps};
use monocode_view_transcript::transcript::{
    ChangedFile, TranscriptConfig, TranscriptEvent, TranscriptView,
};

use monocode_app::boot::AppServices;

use crate::composer_host::SessionComposerHost;

pub struct SessionPane {
    session_id: String,
    session: Option<Arc<Session>>,
    transcript: Entity<TranscriptView>,
    composer: Entity<Composer>,
    focused: bool,
    workspace: WeakEntity<Workspace>,
    opening: bool,
    changes: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The transcript settings from the stored appearance settings.
fn transcript_config(cx: &App) -> TranscriptConfig {
    let mut config = TranscriptConfig::default();
    if let Some(services) = AppServices::try_global(cx) {
        config.layout = services.settings.appearance.transcript_layout;
        config.anchor_prompts = services.settings.appearance.transcript_anchor;
        config.catalog = Arc::new(services.catalog.snapshot());
    }
    config.approvals = true;
    config.can_build_plans = true;
    config.can_send_drafts = true;
    config.can_open_plans = true;
    config
}

/// The composer props SessionPane.tsx passed for this session.
fn composer_props(session: Option<&Session>, focused: bool, cx: &App) -> ComposerProps {
    let mut props = ComposerProps {
        can_save_draft: true,
        btw_enabled: true,
        notes_enabled: false,
        hotkeys: focused,
        focused,
        ..ComposerProps::default()
    };
    if let Some(services) = AppServices::try_global(cx) {
        props.runner_enabled =
            monocode_settings::settings_store::load_composer_runner(&services.kv);
        props.model_controls_beside =
            monocode_settings::settings_store::load_model_controls(&services.kv)
                == ModelControls::Beside;
        if let Some(session) = session {
            props.compact_supported = services
                .registry
                .can_compact_harness_context(session.harness);
        }
    }
    if let Some(session) = session {
        props.harness = session.harness;
        props.model = session.model.clone();
        props.model_settings = session.model_settings.clone();
        props.runtime_mode = session.runtime_mode;
        props.cwd = session.cwd.clone();
        props.execution_cwd = session_work_cwd(session).to_string();
        props.session_id = Some(session.id.clone());
        props.branch = session.branch.clone().filter(|branch| !branch.is_empty());
        props.context = session.context;
        props.busy = session.is_busy();
        props.queued_messages = session.queued_messages.clone().unwrap_or_default();
        props.queue_status = session.queue_status;
        props.inbox_card = session.inbox_card.clone();
        props.note_card = session.note_card.clone();
        props.handoff_card = session.handoff_card.clone();
        props.worktree_removed = session.worktree_removed == Some(true);
    }
    props
}

impl SessionPane {
    pub fn new(
        session_id: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = transcript_config(cx);
        let transcript = cx.new(|cx| {
            let mut view = TranscriptView::new(cx);
            view.set_config(config, cx);
            view
        });
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx).get(&session_id).cloned();
        let host = Rc::new(SessionComposerHost::new(session_id.clone(), cx));
        let draft = Submit::try_global(cx)
            .and_then(|submit| submit.read(cx).drafts().get_composer_draft(&session_id));
        let props = composer_props(open.as_ref(), false, cx);
        let composer = cx.new(|cx| Composer::new(host.clone(), props, draft, window, cx));
        host.set_composer(composer.downgrade());
        let review = Engine::global(cx).review.clone();
        let mut subscriptions = vec![
            cx.subscribe_in(&transcript, window, Self::on_transcript_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
            cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.sync(cx);
                this.sync_composer(window, cx);
            }),
            cx.subscribe(&review, |this, _, event: &ReviewChanged, cx| {
                if event.session_id.is_empty() || event.session_id == this.session_id {
                    this.refresh_changes(cx);
                }
            }),
        ];
        if let Some(files) = Files::try_global(cx) {
            // The `@` index finished a scan: re-rank the open mention list.
            let composer = composer.downgrade();
            subscriptions.push(cx.observe(&files.index.clone(), move |_, _, cx| {
                composer
                    .update(cx, |composer, cx| composer.refresh_suggestions(cx))
                    .ok();
            }));
        }
        let mut pane = Self {
            session_id,
            session: None,
            transcript,
            composer,
            focused: false,
            workspace,
            opening: false,
            changes: None,
            _subscriptions: subscriptions,
        };
        pane.sync(cx);
        pane.refresh_changes(cx);
        pane
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        let composer = self.composer.clone();
        composer.update(cx, |composer, cx| composer.focus(window, cx));
    }

    /// The pane holds the window's focus: its composer takes the model
    /// hotkeys.
    pub fn set_focused(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused != focused {
            self.focused = focused;
            self.sync_composer(window, cx);
        }
    }

    /// Hand the composer the session's current props when they changed.
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let props = composer_props(self.session.as_deref(), self.focused, cx);
        if self.composer.read(cx).props() == &props {
            return;
        }
        self.composer
            .update(cx, |composer, cx| composer.set_props(props, window, cx));
    }

    fn on_composer_event(
        &mut self,
        _: &Entity<Composer>,
        event: &ComposerEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.session_id.clone();
        let submit = Submit::try_global(cx);
        match event {
            ComposerEvent::Focus => {
                self.update_workspace(cx, |workspace, cx| {
                    workspace.focus_pane(&id, cx);
                    workspace.set_composer_focused(true, cx);
                });
            }
            ComposerEvent::ModelChange { harness, model } => {
                if let Some(submit) = submit {
                    submit.update(cx, |submit, cx| submit.set_model(&id, *harness, model, cx));
                }
            }
            ComposerEvent::ModelSettingsChange(settings) => {
                if let Some(submit) = submit {
                    let settings = settings.clone();
                    submit.update(cx, |submit, cx| {
                        submit.set_model_settings(&id, settings, cx)
                    });
                }
            }
            ComposerEvent::RuntimeModeChange(mode) => {
                if let Some(submit) = submit {
                    submit.update(cx, |submit, cx| submit.set_runtime_mode(&id, *mode, cx));
                }
            }
            ComposerEvent::FavoritesChange(favorites) => {
                if let (Some(services), Ok(json)) = (
                    AppServices::try_global(cx),
                    serde_json::to_string(favorites),
                ) {
                    services.kv.set_item(FAVORITES_KEY, &json);
                }
            }
            ComposerEvent::OpenFile { path, line } => {
                let path = path.clone();
                let navigation = line.map(|line| FileNavigation { line, column: None });
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_file(&path, navigation, FileOpenOptions::default(), cx)
                        .detach();
                });
            }
            ComposerEvent::DeleteQueuedMessage(message) => Queues::delete(&id, message, cx),
            ComposerEvent::EditQueuedMessage { id: message, text } => {
                Queues::edit(&id, message, text, cx)
            }
            ComposerEvent::QueuedMessageEditing(message) => {
                Queues::set_editing(&id, message.as_deref(), cx)
            }
            ComposerEvent::SteerQueuedMessage(message) => Queues::steer(&id, message, cx),
            ComposerEvent::ResumeQueue => Queues::resume(&id, cx),
            ComposerEvent::InsertRequestConsumed(_)
            | ComposerEvent::OpenMcpSettings
            | ComposerEvent::EditingLastTurnChange(_) => {}
        }
    }

    /// Follow the session in `Sessions`. A session that is not open yet (a
    /// tab restored from the snapshot) opens from the store.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let sessions = Engine::sessions(cx);
        // Compare before cloning: every session's change notifies, and most
        // are not this one.
        let current = sessions.read(cx).get(&self.session_id);
        let unchanged = current.is_some() && self.session.as_deref() == current;
        if unchanged {
            self.opening = false;
            return;
        }
        let current = current.cloned();
        match current {
            Some(session) => {
                self.opening = false;
                let finished = self.session.as_ref().is_some_and(|before| before.is_busy())
                    && !session.is_busy();
                let session = Arc::new(session);
                self.session = Some(session.clone());
                self.transcript
                    .update(cx, |transcript, cx| transcript.set_session(session, cx));
                if finished {
                    self.refresh_changes(cx);
                }
                cx.notify();
            }
            None if !self.opening => {
                self.opening = true;
                let id = self.session_id.clone();
                sessions
                    .update(cx, |sessions, cx| sessions.ensure_open(&id, cx))
                    .detach();
            }
            None => {}
        }
    }

    /// The changes card: this session's checkpoint status.
    fn refresh_changes(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let status = Engine::checkpoints(cx).status(&session.id, session_work_cwd(&session));
        self.changes = Some(cx.spawn(async move |this, cx| {
            let Ok(status) = status.await else {
                return;
            };
            this.update(cx, |this, cx| this.show_changes(status, cx))
                .ok();
        }));
    }

    fn show_changes(&mut self, status: CheckpointStatus, cx: &mut Context<Self>) {
        let busy = self
            .session
            .as_ref()
            .is_some_and(|session| session.is_busy());
        let files = status
            .files
            .into_iter()
            .map(|file| ChangedFile {
                path: file.path,
                relative: file.relative,
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
                exact: file.exact,
                undoable: file.undoable,
            })
            .collect();
        self.transcript
            .update(cx, |transcript, cx| transcript.set_changes(files, busy, cx));
    }

    fn update_workspace(
        &self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Workspace, &mut Context<Workspace>),
    ) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, update);
        }
    }

    fn on_transcript_event(
        &mut self,
        _: &Entity<TranscriptView>,
        event: &TranscriptEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.session_id.clone();
        match event {
            TranscriptEvent::Approval {
                request_id,
                decision,
            } => Approvals::approve(&id, *request_id, *decision, cx),
            TranscriptEvent::OpenFile { path, line } => {
                let path = path.clone();
                let navigation = line.map(|line| FileNavigation { line, column: None });
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_file(&path, navigation, FileOpenOptions::default(), cx)
                        .detach();
                });
            }
            TranscriptEvent::OpenDiff { path } => {
                let session = self.diff_session();
                let path = path.clone();
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_diff(Some(&path), session, None, false, cx)
                        .detach();
                });
            }
            TranscriptEvent::ReviewChanges { path } => {
                let session = self.diff_session();
                let path = path.clone();
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_diff(path.as_deref(), session, None, true, cx)
                        .detach();
                });
            }
            TranscriptEvent::UndoChanges => self.undo_or_keep(true, cx),
            TranscriptEvent::KeepChanges => self.undo_or_keep(false, cx),
            TranscriptEvent::OpenUrl { url } => cx.open_url(url),
            TranscriptEvent::OpenPlan { block_id } => {
                let block_id = block_id.clone();
                self.update_workspace(cx, |workspace, cx| {
                    workspace.open_plan(&id, &block_id, cx);
                });
            }
            TranscriptEvent::BuildPlan { block_id } => {
                if let Some(submit) = Submit::try_global(cx) {
                    submit.update(cx, |submit, cx| submit.build_plan(&id, block_id, None, cx));
                }
            }
            TranscriptEvent::SendDraft { block_id } => self.send_draft(block_id, cx),
            TranscriptEvent::RemoveDraft { block_id } => {
                if let Some(submit) = Submit::try_global(cx) {
                    submit.update(cx, |submit, cx| submit.remove_draft(&id, block_id, cx));
                }
            }
            TranscriptEvent::Copied { .. } => {
                Toasts::push_timed(
                    Toast::new("Copied").kind(ToastKind::Success),
                    std::time::Duration::from_millis(1500),
                    cx,
                );
            }
            TranscriptEvent::EditLastTurn
            | TranscriptEvent::SaveNote { .. }
            | TranscriptEvent::SecondOpinion { .. }
            | TranscriptEvent::Handoff { .. }
            | TranscriptEvent::JumpToBottomChanged { .. } => {}
        }
    }

    fn diff_session(&self) -> Option<DiffSession> {
        self.session.as_ref().map(|session| DiffSession {
            session_id: session.id.clone(),
            cwd: session_work_cwd(session).to_string(),
        })
    }

    /// `onUndoSessionChanges` and `onKeepSessionChanges` for every file.
    fn undo_or_keep(&mut self, undo: bool, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let checkpoints = Engine::checkpoints(cx);
        let cwd = session_work_cwd(&session).to_string();
        let task = if undo {
            checkpoints.undo(&session.id, &cwd, None)
        } else {
            checkpoints.keep(&session.id, &cwd, None)
        };
        let session_id = session.id.clone();
        self.changes = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| match result {
                Ok(status) => {
                    this.show_changes(status, cx);
                    notify_review_changed(Some(&session_id), cx);
                    if undo {
                        let hooks = Engine::hooks(cx);
                        hooks.workspace.nudge_watched_files(None, cx);
                        hooks.workspace.notify_git_changed(cx);
                    }
                }
                Err(error) => {
                    let title = if undo { "Undo failed" } else { "Keep failed" };
                    Toasts::push(Toast::new(title).kind(ToastKind::Error).body(error), cx);
                }
            })
            .ok();
        }));
    }

    /// `onSendDraft`: send an unsent transcript block as the next turn.
    fn send_draft(&mut self, block_id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(block) = session
            .blocks
            .iter()
            .find(|block| block.id == block_id && block.role == BlockRole::User)
        else {
            return;
        };
        let text = block.text.clone();
        let Some(submit) = Submit::try_global(cx) else {
            return;
        };
        let options = SubmitOptions {
            draft_block_id: Some(block_id.to_string()),
            ..SubmitOptions::default()
        };
        let id = self.session_id.clone();
        submit.update(cx, |submit, cx| {
            submit.on_submit(&id, &text, Vec::new(), options, cx);
        });
    }
}

impl Render for SessionPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let composer = self.composer.clone();
        div()
            .id(gpui::SharedString::from(format!(
                "session-pane-{}",
                self.session_id
            )))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .on_mouse_down(MouseButton::Left, {
                let workspace = self.workspace.clone();
                let session_id = self.session_id.clone();
                move |_, _, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| workspace.focus_pane(&session_id, cx));
                    }
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(self.transcript.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .px(u(12.))
                    .pb(u(12.))
                    .text_color(theme.colors.content)
                    .child(composer),
            )
    }
}
