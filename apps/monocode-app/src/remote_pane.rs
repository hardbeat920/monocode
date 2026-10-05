//! A workspace tab backed by the paired host's session and model catalog.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Task, WeakEntity, Window, div,
};
use monocode_app::boot::AppServices;
use monocode_core::models::{AgentModel, FAVORITES_KEY, ModelPrefs};
use monocode_core::session::{ComposerTurnOptions, session_work_cwd};
use monocode_core::settings::ModelControls;
use monocode_core::{Attachment, HarnessId};
use monocode_engine::attention::Approvals;
use monocode_engine::remote::{RemoteGlobal, RemoteSession, RemoteTab, RemoteTurnOptions};
use monocode_engine::runtime::Engine;
use monocode_engine::submit::Submit;
use monocode_engine::workspace::workspace::{DiffSession, FileOpenOptions};
use monocode_engine::workspace::{Workspace, paths::FileNavigation};
use monocode_ui::widgets::{Toast, ToastKind, Toasts, button};
use monocode_ui::{Theme, u};
use monocode_view_composer::composer::ComposerHost as _;
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::pickers::ModelSource;
use monocode_view_remote::session::{FailedTurn, NoticeAction, RemoteSessionStatus};
use monocode_view_remote::{
    RemoteMachineState, RemoteSessionEvent, RemoteSessionHost, RemoteSessionPane,
    RemoteSessionProps,
};
use monocode_view_transcript::cards::{QuestionForm, QuestionFormEvent, TranscriptCardEvent};
use monocode_view_transcript::transcript::TranscriptConfig;

use crate::composer_host::SessionComposerHost;
use crate::session_threads::SessionOrchestration;
use crate::session_toolbar::SessionToolbar;

#[derive(Default)]
struct HostModels {
    models: BTreeMap<HarnessId, Vec<AgentModel>>,
    selected: Option<AgentModel>,
    available: Vec<HarnessId>,
    probed: bool,
    machine_name: String,
}

/// Picker methods have no GPUI context. Each host change refreshes these values.
struct HostModelSource {
    id: String,
    models: RefCell<HostModels>,
    session: RefCell<Option<WeakEntity<RemoteSession>>>,
    app: AsyncApp,
}

impl HostModelSource {
    fn sync(&self, remote: &Entity<RemoteSession>, cx: &App) {
        let session = remote.read(cx);
        let selection = session.configuration();
        *self.models.borrow_mut() = HostModels {
            models: monocode_core::HARNESSES
                .into_iter()
                .map(|harness| {
                    let models = session
                        .models_for(harness)
                        .into_iter()
                        .map(|model| session.resolve_model(harness, &model.id))
                        .collect();
                    (harness, models)
                })
                .collect(),
            selected: Some(session.resolve_model(selection.harness, &selection.model)),
            available: monocode_core::HARNESSES
                .into_iter()
                .filter(|harness| session.model_available(*harness))
                .collect(),
            probed: session.models_probed(),
            machine_name: session.machine().name.clone(),
        };
        *self.session.borrow_mut() = Some(remote.downgrade());
    }
}

impl ModelSource for HostModelSource {
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        self.models
            .borrow()
            .models
            .get(&harness)
            .cloned()
            .unwrap_or_default()
    }

    fn resolve(&self, harness: HarnessId, id: Option<&str>) -> AgentModel {
        let models = self.models.borrow();
        if let Some(selected) = &models.selected
            && selected.harness == harness
            && id.is_none_or(|id| selected.id == id)
        {
            return selected.clone();
        }
        if let Some(model) = models.models.get(&harness).and_then(|models| match id {
            Some(id) => models.iter().find(|model| model.id == id),
            None => models.first(),
        }) {
            return model.clone();
        }
        let id = id.unwrap_or_default();
        AgentModel::new(
            id,
            harness,
            if id.is_empty() {
                "Loading models..."
            } else {
                id
            },
        )
    }

    fn find(&self, id: &str) -> Option<AgentModel> {
        self.models
            .borrow()
            .models
            .values()
            .flatten()
            .find(|model| model.id == id)
            .cloned()
    }

    fn available(&self, harness: HarnessId) -> bool {
        self.models.borrow().available.contains(&harness)
    }

    fn probed(&self) -> bool {
        self.models.borrow().probed
    }

    fn refresh(&self, _: &[HarnessId]) {
        if let Some(session) = self.session.borrow().clone() {
            self.app
                .spawn(async move |cx| {
                    session
                        .update(cx, |session, cx| session.refresh_catalog(cx))
                        .ok();
                })
                .detach();
        }
    }

    fn unavailable_hint(&self, harness: HarnessId) -> String {
        format!(
            "{} is not available on {}.",
            harness.title(),
            self.models.borrow().machine_name
        )
    }
}

struct HostComposer {
    session_id: String,
    models: Rc<HostModelSource>,
    attachments: SessionComposerHost,
}

impl RemoteSessionHost for HostComposer {
    fn submit(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        options: ComposerTurnOptions,
        _: &mut Window,
        cx: &mut App,
    ) -> bool {
        RemoteGlobal::submit(
            &self.session_id,
            &text,
            &attachments,
            &RemoteTurnOptions {
                intent: options.intent,
                draft_block_id: options.draft_block_id,
            },
            cx,
        )
    }

    fn save_draft(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        _: &mut Window,
        cx: &mut App,
    ) -> bool {
        RemoteGlobal::save_draft(&self.session_id, &text, &attachments, cx)
    }

    fn stop(&self, _: &mut Window, cx: &mut App) {
        RemoteGlobal::stop(&self.session_id, cx);
    }

    fn compact(&self, _: &mut Window, cx: &mut App) -> bool {
        RemoteGlobal::compact(&self.session_id, cx)
    }

    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App) -> Task<Vec<Attachment>> {
        self.attachments.attachments_from_paths(paths, cx)
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> Task<Vec<Attachment>> {
        self.attachments.attachments_from_files(files, cx)
    }

    fn pick_attachments(&self, window: &mut Window, cx: &mut App) -> Task<Vec<Attachment>> {
        self.attachments.pick_attachments(window, cx)
    }

    fn model_source(&self, _: &mut App) -> Option<Rc<dyn ModelSource>> {
        Some(self.models.clone())
    }

    fn model_prefs(&self, cx: &mut App) -> ModelPrefs {
        AppServices::try_global(cx)
            .map(|services| ModelPrefs::from_local_storage(|key| services.kv.get_item(key)))
            .unwrap_or_default()
    }

    fn draft_changed(&self, text: &str, cx: &mut App) {
        if let Some(submit) = Submit::try_global(cx) {
            submit
                .read(cx)
                .drafts()
                .set_composer_draft(&self.session_id, text);
        }
    }
}

pub struct RemotePane {
    session_id: String,
    workspace: WeakEntity<Workspace>,
    view: Entity<RemoteSessionPane>,
    toolbar: Entity<SessionToolbar>,
    empty: Entity<crate::session_empty::SessionEmpty>,
    background: Entity<crate::session_empty::SessionBackground>,
    welcome: Entity<crate::session_empty::ModelWelcomeLayer>,
    navigation: Entity<crate::session_navigation::SessionNavigation>,
    bounds: monocode_view_composer::pickers::anchor::BoundsCell,
    models: Rc<HostModelSource>,
    remote: Option<Entity<RemoteSession>>,
    remote_subscription: Option<Subscription>,
    question: Option<Entity<QuestionForm>>,
    question_subscription: Option<Subscription>,
    revealed_search: Option<(String, String)>,
    focused: bool,
    visible: bool,
    opening: bool,
    _subscriptions: Vec<Subscription>,
    _settings_watch: Option<(monocode_settings::Subscription, Task<()>)>,
}

impl RemotePane {
    #[cfg(test)]
    pub(crate) fn toolbar(&self) -> &Entity<SessionToolbar> {
        &self.toolbar
    }

    pub fn new(
        session_id: String,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let models = Rc::new(HostModelSource {
            id: format!("remote:{session_id}"),
            models: RefCell::new(HostModels::default()),
            session: RefCell::new(None),
            app: cx.to_async(),
        });
        let host = Rc::new(HostComposer {
            session_id: session_id.clone(),
            models: models.clone(),
            attachments: SessionComposerHost::new(session_id.clone(), cx),
        });
        let draft = Submit::try_global(cx)
            .and_then(|submit| submit.read(cx).drafts().get_composer_draft(&session_id));
        let view = cx.new(|cx| {
            RemoteSessionPane::new(host, RemoteSessionProps::default(), draft, window, cx)
        });
        let transcript = view.read(cx).transcript().clone();
        let composer = view.read(cx).composer().clone();
        let toolbar = cx.new(|_| {
            SessionToolbar::new(session_id.clone(), workspace.clone(), composer.downgrade())
        });
        let empty = cx.new(|cx| crate::session_empty::SessionEmpty::new(composer, cx));
        let background = cx.new(crate::session_empty::SessionBackground::new);
        let welcome = cx.new(crate::session_empty::ModelWelcomeLayer::new);
        let navigation = cx.new(|cx| {
            crate::session_navigation::SessionNavigation::new(transcript.clone(), window, cx)
        });
        view.update(cx, |view, cx| view.set_empty_view(empty.clone().into(), cx));
        view.update(cx, |view, cx| {
            view.set_navigation_view(navigation.clone().into(), cx)
        });
        transcript.update(cx, |view, cx| {
            if monocode_engine::orchestration::Orchestration::try_global(cx).is_some() {
                let orchestration = Rc::new(SessionOrchestration);
                view.set_orchestration(orchestration.clone(), Some(orchestration), cx);
            }
        });
        crate::adapters::search::SearchReveals::init(cx);
        let reveals = crate::adapters::search::SearchReveals::entity(cx);
        let sessions = Engine::sessions(cx);
        let mut subscriptions = vec![
            cx.subscribe_in(&view, window, Self::on_event),
            cx.subscribe_in(&transcript, window, Self::on_card_event),
            cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.sync(window, cx)
            }),
            cx.observe(&reveals, |this, _, cx| {
                this.revealed_search = None;
                this.reveal_search(cx);
            }),
        ];
        if let Some(remote) = RemoteGlobal::try_global(cx) {
            subscriptions.push(cx.observe_in(
                &remote.connections.clone(),
                window,
                |this, _, window, cx| this.sync(window, cx),
            ));
        }
        let mut pane = Self {
            session_id,
            workspace,
            view,
            toolbar,
            empty,
            background,
            welcome,
            navigation,
            bounds: Default::default(),
            models,
            remote: None,
            remote_subscription: None,
            question: None,
            question_subscription: None,
            revealed_search: None,
            focused: false,
            visible: true,
            opening: false,
            _subscriptions: subscriptions,
            _settings_watch: None,
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
                        .update_in(cx, |this, window, cx| this.sync(window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            pane._settings_watch = Some((subscription, watch));
        }
        pane.sync(window, cx);
        pane
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sessions = Engine::sessions(cx);
        let shell = sessions.read(cx).get(&self.session_id).cloned();
        if shell.is_none() && !self.opening {
            self.opening = true;
            sessions
                .update(cx, |sessions, cx| {
                    sessions.ensure_open(&self.session_id, cx)
                })
                .detach();
        }
        let tab = match (
            shell.as_ref(),
            RemoteGlobal::try_global(cx).map(|remote| remote.sessions.clone()),
        ) {
            (Some(shell), Some(sessions)) => {
                self.opening = false;
                sessions.update(cx, |sessions, cx| sessions.open(shell, self.visible, cx))
            }
            _ => RemoteTab::Connecting,
        };
        let remote = match &tab {
            RemoteTab::Connected(remote) => Some(remote.clone()),
            _ => None,
        };
        if self.remote.as_ref().map(Entity::entity_id) != remote.as_ref().map(Entity::entity_id) {
            self.remote_subscription = remote.as_ref().map(|remote| {
                cx.observe_in(remote, window, |this, _, window, cx| this.sync(window, cx))
            });
            self.remote = remote.clone();
        }
        let mut props = RemoteSessionProps {
            visible: self.visible,
            focused: self.focused,
            machine: match tab {
                RemoteTab::Connected(_) => RemoteMachineState::Connected,
                RemoteTab::Connecting => RemoteMachineState::Connecting,
                RemoteTab::MissingProject => RemoteMachineState::MissingProject,
                RemoteTab::NotConnected => RemoteMachineState::NotConnected,
            },
            ..RemoteSessionProps::default()
        };
        if let Some(remote) = &remote {
            self.models.sync(remote, cx);
            let remote = remote.read(cx);
            let status = remote.status();
            let features = remote.features();
            props.machine_name = remote.machine().name.clone();
            props.environment_id = remote.machine().environment_id.clone();
            props.execution_cwd = remote.execution_cwd().to_string();
            props.online = remote.online();
            props.features = monocode_view_composer::composer::RemoteFeatures {
                attachments: features.attachments,
                plan: features.plan,
                draft: features.draft,
            };
            props.loading = remote.loading(cx);
            props.started = remote.started();
            props.allowed_model_harnesses = remote.allowed_model_harnesses();
            props.status = RemoteSessionStatus {
                pending: status.pending,
                sending: status.sending,
                failed_turn: status.failed_draft.map(|draft| FailedTurn { draft }),
                error: status.error,
                catalog_problem: status.catalog_problem,
            };
            props.session = Some(Arc::new(remote.session(cx)));
        } else {
            *self.models.session.borrow_mut() = None;
            *self.models.models.borrow_mut() = HostModels::default();
        }
        if let Some(services) = AppServices::try_global(cx) {
            let appearance = monocode_settings::load_app_settings(
                &services.kv,
                monocode_core::platform::Platform::current(),
            )
            .appearance;
            props.transcript = TranscriptConfig {
                layout: appearance.transcript_layout,
                anchor_prompts: appearance.transcript_anchor,
                catalog: Arc::new(services.catalog.snapshot()),
                can_add_to_chat: true,
                ..TranscriptConfig::default()
            };
            props.model_controls_beside =
                monocode_settings::settings_store::load_model_controls(&services.kv)
                    == ModelControls::Beside;
            props.runner_enabled =
                monocode_settings::settings_store::load_composer_runner(&services.kv);
            props.compact_supported = props.session.as_ref().is_some_and(|session| {
                services
                    .registry
                    .can_compact_harness_context(session.harness)
            });
        }
        props.force_docked = props.session.as_ref().is_some_and(|session| {
            session.inbox_ask.is_some() || session.pending_question.is_some()
        }) || self.workspace.upgrade().is_some_and(|workspace| {
            workspace
                .read(cx)
                .active_tab()
                .is_some_and(|tab| monocode_layout::leaf_ids(&tab.layout).len() > 1)
        });
        let hint = props.session.as_ref().and_then(|session| {
            (self.models.probed() && !self.models.available(session.harness))
                .then(|| self.models.unavailable_hint(session.harness))
        });
        self.empty.update(cx, |empty, cx| {
            empty.set_remote_session(
                props.session.as_deref(),
                !props.docks_composer(),
                self.visible && props.machine == RemoteMachineState::Connected && !props.loading,
                hint,
                cx,
            )
        });
        self.background.update(cx, |background, cx| {
            background.set_session(props.session.as_deref(), cx)
        });
        self.navigation.update(cx, |navigation, cx| {
            navigation.set_session(props.session.as_deref(), self.visible, self.focused, cx)
        });
        self.sync_question(
            props
                .session
                .as_ref()
                .and_then(|session| session.pending_question.clone()),
            window,
            cx,
        );
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_remote_session(remote.clone(), cx);
            toolbar.set_session(props.session.as_deref(), window, cx);
        });
        if self.view.read(cx).props() != &props {
            self.view
                .update(cx, |view, cx| view.set_props(props, window, cx));
        }
        self.reveal_search(cx);
    }

    fn sync_question(
        &mut self,
        prompt: Option<monocode_core::user_question::UserQuestionPrompt>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                cx.notify();
            }
            (None, _) => {
                if self.question.take().is_some() {
                    cx.notify();
                }
                self.question_subscription = None;
            }
        }
    }

    fn reveal_search(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = crate::adapters::search::SearchReveals::current(&self.session_id, cx)
        {
            let key = (target.block_id.clone(), target.query.clone());
            let transcript = self.view.read(cx).transcript().clone();
            if self.revealed_search.as_ref() != Some(&key)
                && transcript.update(cx, |view, cx| {
                    view.navigate_to_block(Some(&target.block_id), &target.query, cx)
                })
            {
                self.revealed_search = Some(key);
            }
        }
    }

    pub fn set_visible(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            if let Some(remote) = &self.remote {
                remote.update(cx, |remote, cx| remote.set_visible(visible, cx));
            }
            if !visible {
                self.welcome.update(cx, |welcome, cx| welcome.dismiss(cx));
            }
            self.sync(window, cx);
            cx.notify();
        }
    }

    pub fn set_focused(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused != focused {
            self.focused = focused;
            self.sync(window, cx);
        }
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        self.view
            .update(cx, |view, cx| view.focus_composer(window, cx));
    }

    pub fn list_navigation_allowed(&self, cx: &App) -> bool {
        let composer = self.view.read(cx).composer().read(cx);
        composer.navigation_empty() && !composer.any_picker_open(cx)
    }

    pub fn session_shortcuts_blocked(&self, cx: &App) -> bool {
        let composer = self.view.read(cx).composer().read(cx);
        composer.any_picker_open(cx) || composer.prompt().read(cx).is_composing()
    }

    pub fn switch_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let composer = self.view.read(cx).composer().clone();
        composer.update(cx, |composer, cx| composer.open_model_picker(window, cx));
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
            let composer = self.view.read(cx).composer().clone();
            composer.update(cx, |composer, cx| {
                let items = monocode_view_composer::composer::model::chat_context::add_chat_context(
                    composer.context_items(),
                    item,
                );
                composer.set_context_items(items, cx);
                composer.focus(window, cx);
            });
        }
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

    fn on_card_event(
        &mut self,
        _: &Entity<monocode_view_transcript::transcript::TranscriptView>,
        event: &TranscriptCardEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let TranscriptCardEvent::AddToChat { text } = event;
        let composer = self.view.read(cx).composer().clone();
        composer.update(cx, |composer, cx| {
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

    fn on_event(
        &mut self,
        _: &Entity<RemoteSessionPane>,
        event: &RemoteSessionEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.session_id.clone();
        match event {
            RemoteSessionEvent::Approve {
                request_id,
                decision,
            } => Approvals::approve(&id, *request_id, *decision, cx),
            RemoteSessionEvent::OpenFile { path, line } => {
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_file(
                            path,
                            line.map(|line| FileNavigation { line, column: None }),
                            FileOpenOptions::default(),
                            cx,
                        )
                        .detach();
                })
            }
            RemoteSessionEvent::OpenDiff { path } => {
                let session =
                    self.view
                        .read(cx)
                        .props()
                        .session
                        .as_ref()
                        .map(|session| DiffSession {
                            session_id: id.clone(),
                            cwd: session_work_cwd(session).to_string(),
                        });
                self.update_workspace(cx, |workspace, cx| {
                    workspace
                        .open_diff(path.as_deref(), session, None, false, cx)
                        .detach()
                });
            }
            RemoteSessionEvent::OpenUrl { url } => cx.open_url(url),
            RemoteSessionEvent::OpenPlan { block_id } => {
                self.update_workspace(cx, |workspace, cx| workspace.open_plan(&id, block_id, cx))
            }
            RemoteSessionEvent::BuildPlan { block_id } => {
                RemoteGlobal::build_plan(&id, block_id, None, cx)
            }
            RemoteSessionEvent::RemoveDraft { block_id } => {
                if let Some(remote) = &self.remote {
                    remote.update(cx, |remote, cx| remote.remove_draft(block_id, cx));
                }
            }
            RemoteSessionEvent::ModelChange { harness, model } => {
                if let Some(remote) = &self.remote {
                    remote.update(cx, |remote, cx| remote.set_model(*harness, model, cx));
                }
                let selected = self.models.resolve(*harness, Some(model));
                self.welcome
                    .update(cx, |welcome, cx| welcome.picked_model(&selected, cx));
            }
            RemoteSessionEvent::ModelSettingsChange(settings) => {
                if let Some(remote) = &self.remote {
                    remote.update(cx, |remote, cx| {
                        remote.set_model_settings(settings.clone(), cx)
                    });
                }
            }
            RemoteSessionEvent::RuntimeModeChange(mode) => {
                if let Some(remote) = &self.remote {
                    remote.update(cx, |remote, cx| remote.set_runtime_mode(*mode, cx));
                }
            }
            RemoteSessionEvent::FavoritesChange(favorites) => {
                if let Some(services) = AppServices::try_global(cx) {
                    services.kv.set_item(
                        FAVORITES_KEY,
                        &serde_json::to_string(favorites).unwrap_or_default(),
                    );
                }
            }
            RemoteSessionEvent::Notice(action) => {
                if let Some(remote) = &self.remote {
                    let action = match action {
                        NoticeAction::RetryPending => {
                            monocode_engine::remote::NoticeAction::RetryPending
                        }
                        NoticeAction::TryAgain => monocode_engine::remote::NoticeAction::TryAgain,
                        NoticeAction::Dismiss => monocode_engine::remote::NoticeAction::Dismiss,
                        NoticeAction::RetryCatalog => {
                            monocode_engine::remote::NoticeAction::RetryCatalog
                        }
                    };
                    remote.update(cx, |remote, cx| remote.run_notice_action(action, cx));
                }
            }
            RemoteSessionEvent::ManageMachines => {
                if let Some(services) = AppServices::try_global(cx) {
                    services
                        .kv
                        .set_item("monocode.settingsSection", "connections");
                }
                monocode_app::bridge::shell::ShellRequests::send(
                    monocode_app::bridge::shell::ShellRequest::OpenPage(
                        monocode_app::bridge::shell::ShellPage::Settings,
                    ),
                    cx,
                );
            }
            RemoteSessionEvent::Focus => {
                self.update_workspace(cx, |workspace, cx| workspace.focus_pane(&id, cx))
            }
            RemoteSessionEvent::Copied { .. } => {
                Toasts::push_timed(
                    Toast::new("Copied").kind(ToastKind::Success),
                    std::time::Duration::from_secs(2),
                    cx,
                );
            }
        }
    }
}

impl Render for RemotePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let composer = self.view.read(cx).composer().clone();
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
        let theme = Theme::of(cx).clone();
        let usage = self
            .view
            .read(cx)
            .props()
            .session
            .as_ref()
            .and_then(|session| session.usage_limit);
        let usage_bar = usage.and_then(|limit| {
            let remote = self.remote.clone()?;
            let enabled = remote.read(cx).online() && !remote.read(cx).busy();
            let automatic = limit.resume_at_reset == Some(true);
            let reset = limit.resets_at.map(|at| {
                monocode_engine::attention::usage_limit::format_usage_limit_reset(
                    at,
                    monocode_engine::remote::now_ms(),
                )
            });
            let label = reset.map_or_else(
                || "Usage limit reached".to_string(),
                |reset| format!("Usage limit reached. Resets {reset}"),
            );
            let resume = remote.clone();
            let arm = remote.clone();
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .px(u(12.))
                    .py(u(8.))
                    .text_color(theme.content(0.65))
                    .child(div().flex_1().child(label))
                    .child(
                        button("remote-limit-resume", "Resume")
                            .compact()
                            .disabled(!enabled)
                            .on_click(move |_, _, cx| {
                                resume.update(cx, |session, cx| session.resume_after_limit(cx))
                            }),
                    )
                    .child(
                        button("remote-limit-auto", "Resume at reset")
                            .compact()
                            .selected(automatic)
                            .disabled(limit.resets_at.is_none())
                            .on_click(move |_, _, cx| {
                                arm.update(cx, |session, cx| {
                                    session.set_resume_at_reset(!automatic, cx)
                                })
                            }),
                    )
                    .child(
                        button("remote-limit-dismiss", "Dismiss")
                            .ghost()
                            .compact()
                            .on_click(move |_, _, cx| {
                                remote.update(cx, |session, cx| session.dismiss_usage_limit(cx))
                            }),
                    ),
            )
        });
        div()
            .relative()
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let handled = this.navigation.update(cx, |navigation, cx| {
                    navigation.handle_key(&event.keystroke, window, cx)
                });
                if handled {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(self.bounds.probe())
            .children(self.remote.as_ref().map(|_| self.background.clone()))
            .children(self.remote.as_ref().map(|_| self.toolbar.clone()))
            .children(usage_bar)
            .child(self.view.clone())
            .children(self.question.as_ref().map(|question| {
                div()
                    .flex_none()
                    .px(u(12.))
                    .pb(u(12.))
                    .child(question.clone())
            }))
            .children(self.visible.then(|| self.welcome.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::Session;
    use monocode_engine::remote::remote_projects::remember_remote_project;
    use monocode_engine::remote::testing::{FakeTransport, Reply, machine};
    use monocode_engine::remote::{RemoteConfig, connections::remember_remote_session};
    use monocode_remote::host::protocol::HostProject;
    use monocode_settings::Kv;
    use serde_json::json;

    #[gpui::test]
    fn picker_uses_host_models_and_keeps_saved_settings_when_the_catalog_omits_them(
        cx: &mut TestAppContext,
    ) {
        monocode_engine::runtime::testing::init_test_engine(cx);
        let transport = FakeTransport::new();
        transport.set_machines(vec![machine("host", "env")]);
        transport.set_handler(|_, method, _| Some(match method {
            "environment.describe" => Reply::Value(json!({
                "protocolVersion": 1, "environmentId": "env", "name": "Home",
                "providers": ["codex", "claude"], "capabilities": ["changes.wait"],
                "hostVersion": "0.6.0"
            })),
            "models.list" => Reply::Value(json!({ "models": {
                "codex": [{"id": "codex:host-model", "harness": "codex", "name": "Host model"}],
                "claude": [{"id": "claude:host-model", "harness": "claude", "name": "Other provider"}]
            }, "errors": {} })),
            "sessions.sync" => Reply::Value(json!({ "kind": "snapshot", "value": {
                "projectId": "project", "revision": 1, "status": "idle", "updatedAt": 1,
                "session": { "id": "host-session", "harness": "codex", "model": "codex:host-model",
                    "modelSettings": {"serviceTier": "fast"}, "runtimeMode": "supervised",
                    "title": "Host work", "cwd": "/repo", "blocks": [] }
            }})),
            "sessions.list" => Reply::Value(json!([])),
            _ => Reply::Hold,
        }));
        let kv = Kv::in_memory();
        remember_remote_project(
            &kv,
            "env",
            &HostProject {
                id: "project".into(),
                cwd: "/repo".into(),
                name: "repo".into(),
            },
        );
        remember_remote_session(&kv, "tab", Some("host-session"));
        cx.update(|cx| {
            RemoteGlobal::init(
                RemoteConfig {
                    transport: Arc::new(transport.clone()),
                    kv,
                    clock: Arc::new(|| 1_000),
                },
                cx,
            )
        });
        cx.run_until_parked();
        let shell = Session::blank(
            "tab",
            HarnessId::Claude,
            "claude:local-only",
            "remote://env/repo",
        );
        let remote = cx.update(|cx| {
            Engine::sessions(cx).update(cx, |sessions, cx| sessions.insert(shell.clone(), cx));
            RemoteGlobal::sessions(cx).update(cx, |sessions, cx| sessions.open(&shell, true, cx))
        });
        let RemoteTab::Connected(remote) = remote else {
            panic!("paired remote tab must open");
        };
        cx.run_until_parked();
        cx.update(|cx| {
            let source = HostModelSource {
                id: "env".into(),
                models: RefCell::new(HostModels::default()),
                session: RefCell::new(None),
                app: cx.to_async(),
            };
            source.sync(&remote, cx);
            assert_eq!(
                source.models_for(HarnessId::Codex)[0].id,
                "codex:host-model"
            );
            assert!(source.find("claude:local-only").is_none());
            assert!(source.available(HarnessId::Codex));
            assert!(!source.available(HarnessId::Claude));
            let selected = source.resolve(HarnessId::Codex, None);
            assert_eq!(selected.id, "codex:host-model");
            assert!(
                selected
                    .settings
                    .unwrap()
                    .iter()
                    .any(|setting| setting.id == "serviceTier"
                        && setting.options.iter().any(|option| option.value == "fast"))
            );
        });
    }
}
