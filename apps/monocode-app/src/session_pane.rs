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
use monocode_view_transcript::cards::{QuestionForm, QuestionFormEvent, TranscriptCardEvent};
use monocode_view_transcript::threads::{
    BtwConversationProps, BtwSheet, BtwSheetEvent, BtwSheetProps,
};
use monocode_view_transcript::transcript::{
    ChangedFile, TranscriptConfig, TranscriptEvent, TranscriptView,
};

use monocode_app::boot::AppServices;

use crate::composer_host::SessionComposerHost;
use crate::session_threads::{SessionBtwHost, SessionOrchestration};

pub struct SessionPane {
    session_id: String,
    session: Option<Arc<Session>>,
    transcript: Entity<TranscriptView>,
    navigation: Entity<crate::session_navigation::SessionNavigation>,
    composer: Entity<Composer>,
    toolbar: Entity<crate::session_toolbar::SessionToolbar>,
    cards: Entity<crate::session_cards::ComposerCards>,
    headers: Entity<crate::session_cards::ComposerHeaders>,
    linked: Entity<crate::session_cards::LinkedActivity>,
    empty: Entity<crate::session_empty::SessionEmpty>,
    background: Entity<crate::session_empty::SessionBackground>,
    sign_in: Entity<crate::session_empty::SessionSignIn>,
    welcome: Entity<crate::session_empty::ModelWelcomeLayer>,
    bounds: monocode_view_composer::pickers::anchor::BoundsCell,
    centered: bool,
    visible: bool,
    btw_sheet: Option<Entity<BtwSheet>>,
    question: Option<Entity<QuestionForm>>,
    question_subscription: Option<Subscription>,
    revealed_search: Option<(String, String)>,
    focused: bool,
    workspace: WeakEntity<Workspace>,
    opening: bool,
    changes: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _settings_watch: Option<(monocode_settings::Subscription, Task<()>)>,
    _catalog_watch: Option<CatalogWatch>,
}

struct CatalogWatch {
    catalog: monocode_harness::core::catalog::SharedCatalog,
    availability: monocode_harness::HarnessAvailabilityStore,
    catalog_id: u64,
    availability_id: u64,
    _task: Task<()>,
}
impl Drop for CatalogWatch {
    fn drop(&mut self) {
        self.catalog.unsubscribe(self.catalog_id);
        self.availability
            .unsubscribe_harness_availability(self.availability_id);
    }
}

/// The transcript settings from the stored appearance settings.
fn transcript_config(cx: &App) -> TranscriptConfig {
    let mut config = TranscriptConfig::default();
    if let Some(services) = AppServices::try_global(cx) {
        let appearance = monocode_settings::load_app_settings(
            &services.kv,
            monocode_core::platform::Platform::current(),
        )
        .appearance;
        config.layout = appearance.transcript_layout;
        config.anchor_prompts = appearance.transcript_anchor;
        config.catalog = Arc::new(services.catalog.snapshot());
        config.can_save_notes = monocode_settings::settings_store::load_notes_enabled(&services.kv);
    }
    config.approvals = true;
    config.can_build_plans = true;
    config.can_build_plan_targets = true;
    config.can_send_drafts = true;
    config.can_open_plans = true;
    config.can_add_to_chat = true;
    config.can_second_opinion = true;
    config.can_handoff = true;
    config
}

/// The composer props SessionPane.tsx passed for this session.
fn composer_props(session: Option<&Session>, focused: bool, cx: &App) -> ComposerProps {
    let mut props = ComposerProps {
        can_save_draft: true,
        btw_enabled: true,
        notes_enabled: true,
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
        props.notes_enabled = monocode_settings::settings_store::load_notes_enabled(&services.kv);
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
        props.folders_enabled = monocode_engine::history::paths::is_local_project(&session.cwd);
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
        props.hide_top_bar = session.inbox_ask.is_some();
        props.hide_project_picker = session.inbox_ask.is_some();
        props.hide_branch_picker =
            session.inbox_ask.is_some() || session.orchestration_lead_id.is_some();
        props.can_save_draft = !session.is_busy()
            && monocode_core::session::session_draft_block(&session.blocks).is_none()
            && session.inbox_ask.is_none()
            && session.inbox_card.is_none()
            && session.note_card.is_none()
            && session.handoff_card.is_none();
        props.disabled = session.pending_question.is_some();
        props.edit_last_turn_supported =
            monocode_engine::submit::edit_last_turn::can_edit_last_turn(session);
        props.last_turn_recall = monocode_engine::submit::edit_last_turn::last_turn_recall(session)
            .map(|turn| monocode_view_composer::composer::LastTurnRecall {
                text: turn.text,
                attachments: turn.attachments,
            });
        props.btw_enabled = monocode_engine::side_threads::btw::session_has_btw_eligible_turn(
            &session.blocks,
            session.harness,
            session.orchestration_lead_id.is_some(),
        );
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
        crate::adapters::search::SearchReveals::init(cx);
        let config = transcript_config(cx);
        let transcript = cx.new(|cx| {
            let mut view = TranscriptView::new(cx);
            view.set_config(config, cx);
            view
        });
        let navigation = cx.new(|cx| {
            crate::session_navigation::SessionNavigation::new(transcript.clone(), window, cx)
        });
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx).get(&session_id).cloned();
        let host = Rc::new(SessionComposerHost::new(session_id.clone(), cx));
        let draft = Submit::try_global(cx)
            .and_then(|submit| submit.read(cx).drafts().get_composer_draft(&session_id));
        let props = composer_props(open.as_ref(), false, cx);
        let composer = cx.new(|cx| Composer::new(host.clone(), props, draft, window, cx));
        host.set_composer(composer.downgrade());
        let toolbar = cx.new(|_| {
            crate::session_toolbar::SessionToolbar::new(
                session_id.clone(),
                workspace.clone(),
                composer.downgrade(),
            )
        });
        let cards = cx.new(|cx| crate::session_cards::ComposerCards::new(session_id.clone(), cx));
        let headers =
            cx.new(|cx| crate::session_cards::ComposerHeaders::new(session_id.clone(), cx));
        let linked = cx.new(|cx| {
            crate::session_cards::LinkedActivity::new(
                session_id.clone(),
                workspace.clone(),
                composer.downgrade(),
                cx,
            )
        });
        let empty = cx.new(|cx| crate::session_empty::SessionEmpty::new(composer.clone(), cx));
        let background = cx.new(crate::session_empty::SessionBackground::new);
        let sign_in = cx.new(|cx| crate::session_empty::SessionSignIn::new(session_id.clone(), cx));
        let welcome = cx.new(crate::session_empty::ModelWelcomeLayer::new);
        composer.update(cx, |composer, cx| {
            composer.set_top_bar_views(vec![toolbar.clone().into()], cx);
            composer.set_card_views(vec![cards.clone().into()], cx);
            composer.set_header_views(vec![headers.clone().into()], cx);
        });
        let btw_catalog = AppServices::try_global(cx).map(|services| services.catalog.clone());
        let btw_sheet = btw_catalog.map(|catalog| {
            let btw_host = Rc::new(SessionBtwHost {
                session_id: session_id.clone(),
                composer: host.clone(),
                catalog,
            });
            let sheet = cx.new(|cx| BtwSheet::new(btw_host, cx));
            host.set_btw_sheet(sheet.downgrade());
            sheet
        });
        transcript.update(cx, |view, cx| {
            if monocode_engine::orchestration::Orchestration::try_global(cx).is_some() {
                let orchestration = Rc::new(SessionOrchestration);
                view.set_orchestration(orchestration.clone(), Some(orchestration), cx);
            }
            if let Some(source) = crate::session_threads::model_menu_source(cx) {
                view.set_model_menu_source(source, cx);
            }
        });
        let review = Engine::global(cx).review.clone();
        let search_reveals = crate::adapters::search::SearchReveals::entity(cx);
        let mut subscriptions = vec![
            cx.subscribe_in(&transcript, window, Self::on_transcript_event),
            cx.subscribe_in(&composer, window, Self::on_composer_event),
            cx.subscribe_in(&transcript, window, Self::on_card_event),
            cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.sync(cx);
                this.sync_composer(window, cx);
            }),
            cx.subscribe(&review, |this, _, event: &ReviewChanged, cx| {
                if event.session_id.is_empty() || event.session_id == this.session_id {
                    this.refresh_changes(cx);
                }
            }),
            cx.observe(&search_reveals, |this, _, cx| {
                this.revealed_search = None;
                this.reveal_search_target(cx);
            }),
        ];
        if let Some(sheet) = &btw_sheet {
            subscriptions.push(cx.subscribe_in(
                sheet,
                window,
                |this, _, event: &BtwSheetEvent, window, cx| {
                    if let BtwSheetEvent::Transcript(event) = event {
                        let transcript = this.transcript.clone();
                        this.on_transcript_event(&transcript, event, window, cx);
                    }
                },
            ));
        }
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
            navigation,
            composer,
            toolbar,
            cards,
            headers,
            linked,
            empty,
            background,
            sign_in,
            welcome,
            bounds: Default::default(),
            centered: false,
            visible: true,
            btw_sheet,
            question: None,
            question_subscription: None,
            revealed_search: None,
            focused: false,
            workspace,
            opening: false,
            changes: None,
            _subscriptions: subscriptions,
            _settings_watch: None,
            _catalog_watch: None,
        };
        if let Some(services) = AppServices::try_global(cx) {
            let (tx, rx) = async_channel::bounded(1);
            let subscription = services.kv.subscribe(move |change| {
                if change.key.starts_with("monocode.") && !change.key.contains("draft") {
                    let _ = tx.try_send(());
                }
            });
            let watch = cx.spawn_in(window, async move |this, cx| {
                while rx.recv().await.is_ok() {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.sync_composer(window, cx);
                            this.sync_controls(window, cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            pane._settings_watch = Some((subscription, watch));
        }
        if let Some(services) = AppServices::try_global(cx) {
            let catalog = services.catalog.clone();
            let availability = services.availability.clone();
            let (tx, rx) = async_channel::bounded(1);
            let catalog_tx = tx.clone();
            let catalog_id = catalog.subscribe(move |_| {
                let _ = catalog_tx.try_send(());
            });
            let availability_id = availability.subscribe_harness_availability(move || {
                let _ = tx.try_send(());
            });
            let task = cx.spawn_in(window, async move |this, cx| {
                while rx.recv().await.is_ok() {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.composer
                                .update(cx, |composer, cx| composer.refresh_suggestions(cx));
                            this.sync_controls(window, cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            pane._catalog_watch = Some(CatalogWatch {
                catalog,
                availability,
                catalog_id,
                availability_id,
                _task: task,
            });
        }
        pane.sync(cx);
        pane.reveal_search_target(cx);
        pane.sync_composer(window, cx);
        pane.refresh_changes(cx);
        pane
    }

    fn reveal_search_target(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = crate::adapters::search::SearchReveals::current(&self.session_id, cx)
        {
            let key = (target.block_id.clone(), target.query.clone());
            if self.revealed_search.as_ref() != Some(&key)
                && self.transcript.update(cx, |view, cx| {
                    view.navigate_to_block(Some(&target.block_id), &target.query, cx)
                })
            {
                self.revealed_search = Some(key);
            }
        }
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        let composer = self.composer.clone();
        composer.update(cx, |composer, cx| composer.focus(window, cx));
    }

    pub fn list_navigation_allowed(&self, cx: &App) -> bool {
        let composer = self.composer.read(cx);
        composer.navigation_empty() && !composer.any_picker_open(cx)
    }

    pub fn session_shortcuts_blocked(&self, cx: &App) -> bool {
        let composer = self.composer.read(cx);
        composer.any_picker_open(cx)
            || composer.prompt().read(cx).is_composing()
            || self
                .btw_sheet
                .as_ref()
                .is_some_and(|sheet| sheet.read(cx).is_rendered())
    }

    pub fn switch_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |composer, cx| composer.open_model_picker(window, cx));
    }

    pub fn toggle_workspace_mode(&mut self, cx: &mut Context<Self>) {
        self.toolbar
            .update(cx, |toolbar, cx| toolbar.toggle_workspace_mode(cx));
    }

    pub fn add_to_chat(
        &mut self,
        item: &monocode_engine::workspace::chat_context::ChatContextItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(item) = serde_json::to_value(item).and_then(serde_json::from_value) {
            self.composer.update(cx, |composer, cx| {
                let items = monocode_view_composer::composer::model::chat_context::add_chat_context(
                    composer.context_items(),
                    item,
                );
                composer.set_context_items(items, cx);
                composer.focus(window, cx);
            });
        }
    }

    /// The pane holds the window's focus: its composer takes the model
    /// hotkeys.
    pub fn set_focused(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused != focused {
            self.focused = focused;
            self.sync_composer(window, cx);
        }
    }

    pub fn set_visible(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible != visible || self.centered != self.should_center(cx) {
            self.visible = visible;
            if !visible {
                self.welcome.update(cx, |welcome, cx| welcome.dismiss(cx));
            }
            self.sync_composer(window, cx);
        }
    }

    fn should_center(&self, cx: &App) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session.blocks.is_empty()
                && session.inbox_ask.is_none()
                && session.pending_question.is_none()
                && self.workspace.upgrade().is_none_or(|workspace| {
                    workspace
                        .read(cx)
                        .active_tab()
                        .is_none_or(|tab| monocode_layout::leaf_ids(&tab.layout).len() <= 1)
                })
        })
    }

    /// Hand the composer the session's current props when they changed.
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.centered = self.should_center(cx);
        let mut props = composer_props(self.session.as_deref(), self.focused && self.visible, cx);
        props.enabled = self.visible;
        props.shell = self.centered;
        if self.composer.read(cx).props() != &props {
            self.composer
                .update(cx, |composer, cx| composer.set_props(props, window, cx));
        }
        self.sync_controls(window, cx);
    }

    fn sync_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.as_deref();
        self.navigation.update(cx, |navigation, cx| {
            navigation.set_session(session, self.visible, self.focused, cx)
        });
        self.empty.update(cx, |empty, cx| {
            empty.set_session(session, self.centered, self.visible, cx)
        });
        self.background
            .update(cx, |background, cx| background.set_session(session, cx));
        self.toolbar
            .update(cx, |toolbar, cx| toolbar.set_session(session, window, cx));
        self.cards
            .update(cx, |cards, cx| cards.set_session(session, cx));
        self.headers
            .update(cx, |headers, cx| headers.set_session(session, cx));
        let visible_session =
            session.filter(|session| self.visible && self.focused && session.inbox_ask.is_none());
        self.linked
            .update(cx, |linked, cx| linked.set_session(visible_session, cx));
        let mut config = transcript_config(cx);
        config.visible = self.visible;
        if let Some(session) = self.session.as_ref() {
            config.managed = session.orchestration_lead_id.is_some();
            let regular = session.inbox_ask.is_none() && session.worktree_removed != Some(true);
            config.approvals = session.worktree_removed != Some(true);
            config.can_second_opinion = regular;
            config.can_handoff = regular;
            config.can_build_plans = session.worktree_removed != Some(true);
            config.can_edit_last_turn =
                monocode_engine::submit::edit_last_turn::can_edit_last_turn(session);
        }
        config.editing_last_turn = self.composer.read(cx).is_editing_last_turn();
        self.transcript.update(cx, |view, cx| {
            view.set_config(config.clone(), cx);
        });
        if let Some(sheet) = &self.btw_sheet {
            let mut props = BtwSheetProps {
                transcript: config,
                ..BtwSheetProps::default()
            };
            if let Some(session) = &self.session {
                props.cwd = Some(session_work_cwd(session).to_string());
                props.composer = composer_props(Some(session), self.focused, cx);
                props.conversation = BtwConversationProps {
                    available: session.worktree_removed != Some(true),
                    blocks: Arc::new(session.blocks.clone()),
                    harness: session.harness,
                    managed: session.orchestration_lead_id.is_some(),
                    model: session.model.clone(),
                    model_settings: session.model_settings.clone(),
                };
            }
            sheet.update(cx, |sheet, cx| sheet.set_props(props, cx));
        }
        let prompt = self
            .session
            .as_ref()
            .and_then(|session| session.pending_question.clone());
        match (prompt, &self.question) {
            (Some(prompt), Some(question)) => {
                question.update(cx, |question, cx| question.set_prompt(prompt, window, cx))
            }
            (Some(prompt), None) => {
                let question = cx.new(|cx| QuestionForm::new(prompt, window, cx));
                self.question_subscription =
                    Some(cx.subscribe(&question, |this, _, event, cx| match event {
                        QuestionFormEvent::Reply { request_id, reply } => {
                            Approvals::answer_question(&this.session_id, *request_id, reply, cx)
                        }
                        QuestionFormEvent::Interaction { request_id } => {
                            Approvals::question_interaction(&this.session_id, *request_id, cx)
                        }
                    }));
                self.question = Some(question);
            }
            (None, _) => {
                self.question = None;
                self.question_subscription = None;
            }
        }
        cx.notify();
    }

    fn on_card_event(
        &mut self,
        _: &Entity<TranscriptView>,
        event: &TranscriptCardEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let TranscriptCardEvent::AddToChat { text } = event;
        self.composer.update(cx, |composer, cx| {
            let quote = text
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            let draft = if composer.draft().is_empty() {
                quote
            } else {
                format!("{}\n\n{quote}", composer.draft())
            };
            composer.set_text(&draft, cx);
            composer.focus(window, cx);
        });
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
                self.welcome
                    .update(cx, |welcome, cx| welcome.picked(*harness, model, cx));
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
            ComposerEvent::OpenMcpSettings => {
                if let Some(services) = AppServices::try_global(cx) {
                    services.kv.set_item("monocode.settingsSection", "mcp");
                }
                monocode_app::bridge::shell::ShellRequests::send(
                    monocode_app::bridge::shell::ShellRequest::OpenPage(
                        monocode_app::bridge::shell::ShellPage::Settings,
                    ),
                    cx,
                );
            }
            ComposerEvent::EditingLastTurnChange(editing) => {
                self.transcript.update(cx, |view, cx| {
                    let mut config = view.config().clone();
                    config.editing_last_turn = *editing;
                    view.set_config(config, cx);
                });
            }
            ComposerEvent::InsertRequestConsumed(_) => {}
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
                self.reveal_search_target(cx);
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
        window: &mut Window,
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
            TranscriptEvent::BuildPlanWithTarget { block_id, target } => {
                if let Some(submit) = Submit::try_global(cx) {
                    submit.update(cx, |submit, cx| {
                        submit.build_plan(&id, block_id, Some(target.clone()), cx)
                    });
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
            TranscriptEvent::EditLastTurn => {
                self.composer.update(cx, |composer, cx| {
                    composer.recall_last_turn(window, cx);
                    composer.focus(window, cx);
                });
            }
            TranscriptEvent::SaveNote { text } => self.save_note(text, cx),
            TranscriptEvent::SecondOpinion { turn_id, target } => {
                if let Some(session) = self.session.clone() {
                    crate::session_threads::pick_side_model(&session, turn_id, target, false, cx);
                }
            }
            TranscriptEvent::Handoff { turn_id, target } => {
                if let Some(session) = self.session.clone() {
                    crate::session_threads::pick_side_model(&session, turn_id, target, true, cx);
                }
            }
            TranscriptEvent::JumpToBottomChanged { .. } => {}
        }
    }

    fn save_note(&self, text: &str, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(history) = monocode_engine::history::HistoryPackage::try_global(cx) else {
            return;
        };
        let notes = history.notes.clone();
        let input = monocode_engine::history::notes::NewNote {
            body: Some(text.to_string()),
            source_session_id: Some(session.id.clone()),
            source_cwd: Some(session.cwd.clone()),
            ..Default::default()
        };
        let task = notes.update(cx, |notes, cx| notes.create_note(&input, cx));
        cx.spawn(async move |_, cx| {
            let result = task.await;
            cx.update(|cx| match result {
                Ok(_) => Toasts::push_timed(
                    Toast::new("Note saved").kind(ToastKind::Success),
                    std::time::Duration::from_millis(1500),
                    cx,
                ),
                Err(error) => Toasts::push(
                    Toast::new("Could not save note")
                        .kind(ToastKind::Error)
                        .body(error),
                    cx,
                ),
            });
        })
        .detach();
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
        let attachments = block.attachments.clone().unwrap_or_default();
        let Some(submit) = Submit::try_global(cx) else {
            return;
        };
        let options = SubmitOptions {
            draft_block_id: Some(block_id.to_string()),
            ..SubmitOptions::default()
        };
        let id = self.session_id.clone();
        submit.update(cx, |submit, cx| {
            submit.on_submit(&id, &text, attachments, options, cx);
        });
    }
}

impl Render for SessionPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let composer = self.composer.clone();
        let band = self.bounds.get().and_then(|pane| {
            composer.read(cx).bounds().map(|composer| {
                monocode_view_workbench::panes::welcome::ComposerBand {
                    top: (composer.top() - pane.top()).into(),
                    bottom: (composer.bottom() - pane.top()).into(),
                }
            })
        });
        self.welcome
            .update(cx, |welcome, cx| welcome.set_composer_band(band, cx));
        div()
            .id(gpui::SharedString::from(format!(
                "session-pane-{}",
                self.session_id
            )))
            .flex()
            .relative()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.navigation.update(cx, |navigation, cx| {
                    navigation.handle_key(&event.keystroke, window, cx)
                }) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, {
                let workspace = self.workspace.clone();
                let session_id = self.session_id.clone();
                move |_, _, cx| {
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| workspace.focus_pane(&session_id, cx));
                    }
                }
            })
            .child(self.bounds.probe())
            .child(self.background.clone())
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        if self.session.as_ref().is_some_and(|session| {
                            session.blocks.is_empty() && session.inbox_ask.is_none()
                        }) {
                            self.empty.clone().into_any_element()
                        } else {
                            self.transcript.clone().into_any_element()
                        },
                    )
                    .child(self.navigation.clone()),
            )
            .children((!self.centered).then(|| {
                div()
                    .flex()
                    .justify_center()
                    .flex_none()
                    .px(u(12.))
                    .pb(u(12.))
                    .text_color(theme.colors.content)
                    .child(div().w_full().max_w(u(896.)).child(match &self.question {
                        Some(question) => question.clone().into_any_element(),
                        None => composer.into_any_element(),
                    }))
            }))
            .children(self.btw_sheet.clone())
            .children((self.focused && self.visible).then(|| self.linked.clone()))
            .children(self.visible.then(|| self.sign_in.clone()))
            .children(self.visible.then(|| self.welcome.clone()))
    }
}
