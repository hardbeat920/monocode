//! `SideThreads`: a turn's side conversations. Ports these App.tsx
//! callbacks: `onHandoffCardDismiss` (2434-2443),
//! `dismissNoticesForContinuedSession` (4135-4149), `openSessionBeside`,
//! `onSecondOpinion`, `updateBtwThread`, `removeBtwThread`,
//! `runBtwRequest`, `onBtwSubmit`, `onBtwModelChange`, `onBtwDelete`,
//! `onBtwStop`, `onBtwRetry`, and `onHandoff` (7661-8385), and the
//! `btwRequestsRef` cleanup effects (1125-1149) as the runtime's
//! `SideThreadHooks`.
//!
//! Side threads live on the user block they were asked about
//! (`Block.btw_threads`), so views read them from `Sessions`. While a reply
//! streams, its blocks are in the thread's `pending_blocks`.
//!
//! `SideThreads` is a cloneable global handle rather than an entity. Its
//! flows call `Submit`, which can call back into this package (for example
//! `dismiss_notices_for_continued_session`), and an entity would panic on
//! that nested update. Views observe `Sessions` for the results.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::{BoxFuture, Either};
use gpui::{App, Global};
use monocode_core::block::{
    Block, BlockRole, BtwMessage, BtwMessageRole, BtwThread, BtwThreadStatus, ModelTarget,
    TurnIntent,
};
use monocode_core::btw::supports_btw_harness;
use monocode_core::handoff::HANDOFF_TITLE;
use monocode_core::project_providers::ProjectProviders;
use monocode_core::reducer::now_ms;
use monocode_core::session::{
    format_session_title, new_session, session_display_title, session_work_cwd,
};
use monocode_core::{
    Extra, HarnessAvailability, HarnessEvent, HarnessId, ModelEnv, ModelSettings, RuntimeMode,
    Session, js,
};
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::core::registry::{HarnessRegistry, TextPromptInput, event_sink};
use monocode_harness::core::task::AbortSignal;
use monocode_settings::Kv;

use super::btw::{
    apply_btw_harness_event, btw_turn_harness, build_btw_prompt, replace_btw_thread,
    seal_btw_response_blocks,
};
use super::peers::{DefaultSideThreadPeers, SideThreadPeers};
use super::requests::{BtwRequests, request_key};
use crate::runtime::engine::Engine;
use crate::runtime::hooks::SideThreadHooks;
use crate::submit::handoff::{
    build_deterministic_handoff, build_handoff_composer_card, session_through_turn,
};
use crate::submit::prefs::load_model_prefs;
use crate::submit::second_opinion::{
    SECOND_OPINION_TITLE, build_second_opinion_request, harness_for_turn, turn_edited_files,
    turn_user_request,
};
use crate::submit::{Submit, SubmitOptions};

/// What `SideThreads` needs from the app: the harness registry for text
/// prompts, the model catalog, and the settings store for model
/// preferences.
#[derive(Clone)]
pub struct SideThreadsConfig {
    pub registry: HarnessRegistry,
    pub catalog: SharedCatalog,
    pub kv: Kv,
}

impl SideThreadsConfig {
    /// The registry, catalog, and store the app's `Submit` entity uses.
    pub fn from_submit(cx: &App) -> Option<Self> {
        let submit = Submit::try_global(cx)?;
        let config = submit.read(cx).config();
        Some(Self {
            registry: config.registry.clone(),
            catalog: config.catalog.clone(),
            kv: config.kv.clone(),
        })
    }
}

/// `onBtwSubmit` arguments.
#[derive(Debug, Clone, Copy)]
pub struct BtwSubmit<'a> {
    pub session_id: &'a str,
    /// The turn the question is about, as `groupTurns` returned it.
    pub turn: &'a [Block],
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub text: &'a str,
    /// The model picked in the side sheet, when any.
    pub model: Option<&'a str>,
    pub model_settings: Option<&'a ModelSettings>,
}

/// The runtime's side-thread hooks: abort requests whose session closed,
/// and everything at shutdown.
#[derive(Clone)]
struct EngineSideThreadHooks {
    requests: BtwRequests,
    registry: HarnessRegistry,
}

impl EngineSideThreadHooks {
    /// The unmount cleanup: abort every request, then
    /// `stopHarnessTextPrompts`.
    fn stop(&self) -> BoxFuture<'static, ()> {
        self.requests.abort_all();
        let registry = self.registry.clone();
        async move {
            registry.stop_harness_text_prompts().await;
        }
        .boxed()
    }
}

impl SideThreadHooks for EngineSideThreadHooks {
    fn sessions_closed(&self, live_ids: &HashSet<String>, _cx: &mut App) {
        self.requests.abort_closed(live_ids);
    }

    fn stop_all(&self, _cx: &mut App) {
        self.registry.spawner().spawn(self.stop());
    }
}

/// A turn's side conversations: second opinions, split-pane handoffs, and
/// BTW threads. Clones share one state.
#[derive(Clone)]
pub struct SideThreads {
    inner: Rc<Inner>,
}

struct Inner {
    config: SideThreadsConfig,
    peers: RefCell<Rc<dyn SideThreadPeers>>,
    requests: BtwRequests,
}

impl Global for SideThreads {}

/// `runBtwRequest` input.
struct BtwRequestInput {
    session_id: String,
    user_block_id: String,
    source: Session,
    thread: BtwThread,
    harness: HarnessId,
}

/// What a running text prompt reports, in order.
enum PromptUpdate {
    ThreadId(String),
    Event(Box<HarnessEvent>),
}

/// Where one request writes.
#[derive(Clone)]
struct ThreadRef {
    session_id: String,
    user_block_id: String,
    thread_id: String,
}

impl ThreadRef {
    fn update(
        &self,
        cx: &mut App,
        update: impl FnOnce(Option<&BtwThread>) -> Option<BtwThread>,
    ) -> bool {
        update_btw_thread(
            &self.session_id,
            &self.user_block_id,
            &self.thread_id,
            cx,
            update,
        )
    }
}

impl SideThreads {
    pub fn new(config: SideThreadsConfig) -> Self {
        Self {
            inner: Rc::new(Inner {
                config,
                peers: RefCell::new(Rc::new(DefaultSideThreadPeers)),
                requests: BtwRequests::default(),
            }),
        }
    }

    /// Create the handle, install it as the app's, and fill in the runtime's
    /// `SideThreadHooks`. Call after `Engine::init`. The App unmount effect
    /// becomes an app quit observer, because the runtime does not call
    /// `SideThreadHooks::stop_all` itself.
    pub fn init(config: SideThreadsConfig, cx: &mut App) -> SideThreads {
        let side_threads = SideThreads::new(config);
        let hooks = EngineSideThreadHooks {
            requests: side_threads.inner.requests.clone(),
            registry: side_threads.inner.config.registry.clone(),
        };
        let on_quit = hooks.clone();
        cx.on_app_quit(move |_| on_quit.stop()).detach();
        Engine::set_hooks(cx, |engine_hooks| {
            engine_hooks.side_threads = Rc::new(hooks);
        });
        cx.set_global(side_threads.clone());
        side_threads
    }

    pub fn global(cx: &App) -> SideThreads {
        cx.global::<SideThreads>().clone()
    }

    pub fn try_global(cx: &App) -> Option<SideThreads> {
        cx.try_global::<SideThreads>().cloned()
    }

    /// Replace the calls into other packages.
    pub fn set_peers(&self, peers: Rc<dyn SideThreadPeers>) {
        *self.inner.peers.borrow_mut() = peers;
    }

    fn peers(&self) -> Rc<dyn SideThreadPeers> {
        self.inner.peers.borrow().clone()
    }

    pub fn config(&self) -> &SideThreadsConfig {
        &self.inner.config
    }

    /// A side-thread request is in flight for this thread.
    pub fn is_requesting(&self, session_id: &str, thread_id: &str) -> bool {
        self.inner
            .requests
            .contains(&request_key(session_id, thread_id))
    }

    /// `onHandoffCardDismiss`: drop the handoff chip from the composer.
    pub fn dismiss_handoff_card(&self, session_id: &str, cx: &mut App) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            if sessions
                .get(session_id)
                .is_some_and(|session| session.handoff_card.is_some())
            {
                sessions.update(session_id, cx, |session| session.handoff_card = None);
            }
        });
    }

    /// `dismissNoticesForContinuedSession`: the user continued this chat, so
    /// its due reminders and its linked work item update card go away.
    pub fn dismiss_notices_for_continued_session(&self, session_id: &str, cx: &mut App) {
        let peers = self.peers();
        peers.dismiss_due_reminders(session_id, cx);
        let sessions = Engine::sessions(cx);
        let updated_at = sessions
            .read(cx)
            .get(session_id)
            .and_then(|session| session.linked_work_item_update_card.as_ref())
            .map(|card| card.updated_at);
        let Some(updated_at) = updated_at else {
            return;
        };
        peers.mark_linked_session_update_seen(session_id, updated_at, cx);
        // `setLinkedWorkItemUpdateCard` leaves the session alone when the
        // card did not change.
        sessions.update(cx, |sessions, cx| {
            let matches = sessions
                .get(session_id)
                .and_then(|session| session.linked_work_item_update_card.as_ref())
                .is_some_and(|card| card.updated_at == updated_at);
            if matches {
                sessions.update(session_id, cx, |session| {
                    session.linked_work_item_update_card = None;
                });
            }
        });
    }

    /// `newSession` with a fresh id, the catalog, and the stored model
    /// preferences.
    fn new_session(
        &self,
        harness: HarnessId,
        cwd: &str,
        model: &str,
        runtime_mode: RuntimeMode,
    ) -> Session {
        let config = self.config();
        let catalog = config.catalog.read();
        let prefs = load_model_prefs(&config.kv);
        // `newSession` reads availability and project defaults only when it
        // has no model.
        let availability = HarnessAvailability::default();
        let projects = ProjectProviders::default();
        let env = ModelEnv {
            catalog: &catalog,
            prefs: &prefs,
            availability: &availability,
            projects: &projects,
        };
        new_session(
            &env,
            uuid::Uuid::new_v4().to_string(),
            harness,
            cwd,
            Some(model),
            Some(runtime_mode),
            None,
        )
    }

    /// `mergeModelSettings(resolveModel(harness, model), settings)`.
    fn merged_settings(&self, target: &ModelTarget) -> ModelSettings {
        let catalog = self.config().catalog.read();
        let model = catalog.resolve_model(target.harness, Some(&target.model));
        catalog.merge_model_settings(&model, Some(&target.model_settings))
    }

    /// `openSessionBeside`: open the session, then let the workspace place it
    /// beside the source pane.
    fn open_session_beside(
        &self,
        source_id: &str,
        session: Session,
        cwd: &str,
        focus_composer: bool,
        cx: &mut App,
    ) {
        let session_id = session.id.clone();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
        self.peers()
            .open_session_beside(source_id, &session_id, cwd, focus_composer, cx);
    }

    fn open_source(session_id: &str, cx: &App) -> Option<Session> {
        Engine::sessions(cx).read(cx).get(session_id).cloned()
    }

    /// `onSecondOpinion`: ask `target` to review the work `turn` did, in a
    /// new chat beside the source. Returns the new session's id.
    pub fn second_opinion(
        &self,
        source_id: &str,
        target: &ModelTarget,
        turn: &[Block],
        cx: &mut App,
    ) -> Option<String> {
        let source = Self::open_source(source_id, cx)?;
        if source.worktree_removed == Some(true) {
            return None;
        }
        let harness = target.harness;
        let cwd = session_work_cwd(&source);
        let from = harness_for_turn(&source.blocks, turn, source.harness);
        let request = build_second_opinion_request(from, harness, turn, cwd);
        let session = Session {
            worktree_cwd: source.worktree_cwd.clone(),
            branch: source.branch.clone(),
            model_settings: self.merged_settings(target),
            title: format_session_title(harness, SECOND_OPINION_TITLE),
            ..self.new_session(harness, &source.cwd, &target.model, source.runtime_mode)
        };
        let session_id = session.id.clone();
        self.open_session_beside(source_id, session, &source.cwd, false, cx);
        let options = SubmitOptions {
            ci_context: request.ci_context,
            second_opinion: Some(request.second_opinion),
            ..SubmitOptions::default()
        };
        self.peers()
            .submit(&session_id, &request.prompt, options, cx);
        Some(session_id)
    }

    /// `onHandoff`: continue the conversation through `turn` with `target`
    /// in a new chat beside the source. The recap waits on the composer as a
    /// handoff card. Returns the new session's id.
    pub fn handoff(
        &self,
        source_id: &str,
        target: &ModelTarget,
        turn: &[Block],
        cx: &mut App,
    ) -> Option<String> {
        let source = Self::open_source(source_id, cx)?;
        if source.worktree_removed == Some(true) {
            return None;
        }
        let harness = target.harness;
        let cwd = session_work_cwd(&source);
        let from = harness_for_turn(&source.blocks, turn, source.harness);
        let sliced = session_through_turn(&source, turn);
        let user_request = turn_user_request(turn);
        let files = turn_edited_files(&sliced.blocks, Some(cwd));
        let display = session_display_title(&source.title, source.harness);
        let title = if display == "New session" {
            HANDOFF_TITLE
        } else {
            display.as_str()
        };
        let session = Session {
            worktree_cwd: source.worktree_cwd.clone(),
            branch: source.branch.clone(),
            model_settings: self.merged_settings(target),
            title: format_session_title(harness, title),
            handoff_card: Some(build_handoff_composer_card(
                from,
                harness,
                &build_deterministic_handoff(&sliced, None, None),
                &user_request,
                &files,
            )),
            ..self.new_session(harness, &source.cwd, &target.model, source.runtime_mode)
        };
        let session_id = session.id.clone();
        self.open_session_beside(source_id, session, &source.cwd, true, cx);
        Some(session_id)
    }

    /// `runBtwRequest`: ask the thread's provider the latest side question
    /// through an isolated text prompt, streaming its activity into the
    /// thread's `pending_blocks`.
    fn run_btw_request(&self, input: BtwRequestInput, cx: &mut App) {
        let harness = input.harness;
        if !supports_btw_harness(Some(harness)) {
            return;
        }
        let target = ThreadRef {
            session_id: input.session_id.clone(),
            user_block_id: input.user_block_id.clone(),
            thread_id: input.thread.id.clone(),
        };
        let fail = |message: String, cx: &mut App| {
            target.update(cx, |thread| {
                thread.map(|thread| BtwThread {
                    status: BtwThreadStatus::Error,
                    updated_at: now_ms(),
                    error: Some(message),
                    ..thread.clone()
                })
            });
        };
        let cwd = session_work_cwd(&input.source).to_string();
        let model = {
            let saved = input.thread.model.as_deref().unwrap_or(&input.source.model);
            js::trim(&self.config().catalog.read().native_model_id_for(saved)).to_string()
        };
        if cwd.is_empty() || cwd == "~" {
            fail(
                "A project working directory is required for this question.".into(),
                cx,
            );
            return;
        }
        if model.is_empty() && harness == HarnessId::Codex {
            fail("The selected Codex model is unavailable.".into(), cx);
            return;
        }
        let prompt = match build_btw_prompt(&input.source.blocks, &input.thread, Some(&cwd)) {
            Ok(prompt) => prompt,
            Err(message) => {
                fail(message, cx);
                return;
            }
        };

        let requests = self.inner.requests.clone();
        let key = request_key(&input.session_id, &input.thread.id);
        let (signal, token) = requests.start(&key, &input.session_id);

        let user_message_id = last_message_id(&input.thread);
        let response_model = if model.is_empty() {
            input.source.model.clone()
        } else {
            model.clone()
        };

        let (sender, updates) = async_channel::unbounded::<PromptUpdate>();
        let thread_ids = sender.clone();
        let run = self.config().registry.run_harness_text_prompt(
            harness,
            TextPromptInput {
                cwd,
                provider_account_id: input.source.provider_account_id.clone(),
                model: (!model.is_empty()).then_some(model),
                model_settings: Some(
                    input
                        .thread
                        .model_settings
                        .clone()
                        .unwrap_or_else(|| input.source.model_settings.clone()),
                ),
                thread_id: input.thread.provider_thread_id.clone(),
                on_thread_id: Some(Arc::new(move |thread_id: String| {
                    let _ = thread_ids.try_send(PromptUpdate::ThreadId(thread_id));
                })),
                intent: Some(TurnIntent::Plan),
                prompt,
                timeout_ms: None,
                signal: Some(signal.clone()),
                on_event: Some(event_sink(move |event| {
                    let _ = sender.try_send(PromptUpdate::Event(Box::new(event)));
                })),
            },
        );

        let reply = BtwReply {
            target,
            harness,
            response_model,
            user_message_id,
            signal,
        };
        cx.spawn(async move |cx| {
            let result = drive(run, &updates, |update| {
                cx.update(|cx| reply.apply(update, cx));
            })
            .await;
            cx.update(|cx| {
                reply.settle(result, cx);
                requests.finish(&key, token);
            });
        })
        .detach();
    }

    /// `onBtwSubmit`: ask a side question about `turn`, starting a thread or
    /// continuing one. False when the question cannot go: no such session or
    /// turn, a provider without a text runner, a removed worktree, a thread
    /// that is still answering, or a thread anchored to another turn end.
    pub fn btw_submit(&self, input: BtwSubmit<'_>, cx: &mut App) -> bool {
        let BtwSubmit {
            session_id,
            turn,
            thread_id,
            message_id,
            text,
            model,
            model_settings,
        } = input;
        let source = Self::open_source(session_id, cx);
        let turn_user = turn.iter().find(|block| block.role == BlockRole::User);
        let source_user_id = turn_user.map(|block| block.id.as_str());
        let source_end_block_id = turn.last().map(|block| block.id.as_str());
        let turn_harness = source
            .as_ref()
            .map(|source| harness_for_turn(&source.blocks, turn, source.harness));
        let turn_model = turn_user
            .and_then(|block| block.turn_model.as_ref())
            .map(|turn_model| turn_model.id.clone());
        let source_block = source
            .as_ref()
            .and_then(|source| find_user_block(&source.blocks, source_user_id));
        let existing = find_thread(source_block, thread_id).cloned();
        let request_harness = source.as_ref().and_then(|source| {
            if supports_btw_harness(turn_harness) {
                turn_harness
            } else {
                existing
                    .as_ref()
                    .and_then(|thread| thread.harness)
                    .or_else(|| btw_turn_harness(&source.blocks, turn, source.harness))
            }
        });
        let (Some(source), Some(request_harness), Some(source_user_id), Some(source_end_block_id)) = (
            source.as_ref(),
            request_harness.filter(|harness| supports_btw_harness(Some(*harness))),
            source_user_id.filter(|id| !id.is_empty()),
            source_end_block_id.filter(|id| !id.is_empty()),
        ) else {
            return false;
        };
        if source.worktree_removed == Some(true) || source_block.is_none() {
            return false;
        }

        let config = self.config();
        let selected_model = model
            .map(js::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|thread| thread.model.clone())
                    .filter(|model| !model.is_empty())
            })
            .or_else(|| turn_model.filter(|model| !model.is_empty()))
            .unwrap_or_else(|| {
                js::trim(&config.catalog.read().native_model_id_for(&source.model)).to_string()
            });
        let selected_model_settings = model_settings
            .cloned()
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|thread| thread.model_settings.clone())
            })
            .unwrap_or_else(|| {
                let catalog = config.catalog.read();
                let id = if selected_model.is_empty() {
                    &source.model
                } else {
                    &selected_model
                };
                let resolved = catalog.resolve_model(request_harness, Some(id));
                let prefs = load_model_prefs(&config.kv);
                catalog.preferred_model_settings(
                    &resolved,
                    Some(&source.model_settings),
                    &prefs.last_model_settings,
                )
            });
        if existing
            .as_ref()
            .is_some_and(|thread| thread.status == BtwThreadStatus::Running)
        {
            return false;
        }
        if existing
            .as_ref()
            .is_some_and(|thread| thread.source_end_block_id != source_end_block_id)
        {
            return false;
        }

        let now = now_ms();
        let question = BtwMessage {
            id: message_id.to_string(),
            role: BtwMessageRole::User,
            text: text.to_string(),
            created_at: now,
            blocks: None,
            extra: Extra::new(),
        };
        let selected_model = (!selected_model.is_empty()).then_some(selected_model);
        let thread = match existing {
            Some(existing) => {
                let mut messages = existing.messages.clone();
                messages.push(question);
                BtwThread {
                    harness: existing.harness.or(turn_harness),
                    model: selected_model,
                    model_settings: Some(selected_model_settings),
                    status: BtwThreadStatus::Running,
                    updated_at: now,
                    error: None,
                    pending_blocks: Some(Vec::new()),
                    messages,
                    ..existing
                }
            }
            None => BtwThread {
                id: thread_id.to_string(),
                source_end_block_id: source_end_block_id.to_string(),
                created_at: now,
                updated_at: now,
                status: BtwThreadStatus::Running,
                pending_blocks: Some(Vec::new()),
                harness: Some(request_harness),
                model: selected_model,
                model_settings: Some(selected_model_settings),
                messages: vec![question],
                provider_thread_id: None,
                error: None,
                extra: Extra::new(),
            },
        };
        let stored = thread.clone();
        if !update_btw_thread(session_id, source_user_id, thread_id, cx, |_| Some(stored)) {
            return false;
        }
        // TODO(port): a thread saved without a harness takes the turn's
        // harness here even when that one cannot run BTW. The request then
        // returns early and leaves the thread running, as in TypeScript.
        let harness = thread.harness.unwrap_or(request_harness);
        self.run_btw_request(
            BtwRequestInput {
                session_id: session_id.to_string(),
                user_block_id: source_user_id.to_string(),
                source: source.clone(),
                thread,
                harness,
            },
            cx,
        );
        true
    }

    /// The thread's provider for model changes, deletes, and retries:
    /// the stored one, else the turn's BTW provider, else the turn's.
    fn thread_harness(
        source: &Session,
        turn: &[Block],
        source_user_id: Option<&str>,
        thread_id: &str,
    ) -> Option<HarnessId> {
        let turn_harness = harness_for_turn(&source.blocks, turn, source.harness);
        let source_block = find_user_block(&source.blocks, source_user_id);
        find_thread(source_block, thread_id)
            .and_then(|thread| thread.harness)
            .or_else(|| btw_turn_harness(&source.blocks, turn, source.harness))
            .or(Some(turn_harness))
    }

    /// The checks `onBtwModelChange`, `onBtwDelete`, and `onBtwRetry` share.
    /// Returns the source session and the turn's user block id.
    fn btw_source(
        session_id: &str,
        turn: &[Block],
        thread_id: &str,
        cx: &App,
    ) -> Option<(Session, String)> {
        let source = Self::open_source(session_id, cx)?;
        let source_user_id = turn_user_id(turn);
        let thread_harness = Self::thread_harness(&source, turn, source_user_id, thread_id);
        if !supports_btw_harness(thread_harness) || source.worktree_removed == Some(true) {
            return None;
        }
        let source_user_id = source_user_id.filter(|id| !id.is_empty())?.to_string();
        Some((source, source_user_id))
    }

    /// `onBtwModelChange`: pick the model for the thread's next question.
    pub fn btw_set_model(
        &self,
        session_id: &str,
        turn: &[Block],
        thread_id: &str,
        model: &str,
        model_settings: ModelSettings,
        cx: &mut App,
    ) {
        let next_model = js::trim(model);
        let Some((_, source_user_id)) = Self::btw_source(session_id, turn, thread_id, cx) else {
            return;
        };
        if next_model.is_empty() {
            return;
        }
        update_btw_thread(session_id, &source_user_id, thread_id, cx, |thread| {
            thread.map(|thread| BtwThread {
                model: Some(next_model.to_string()),
                model_settings: Some(model_settings),
                updated_at: now_ms(),
                ..thread.clone()
            })
        });
    }

    /// `onBtwDelete`: cancel the thread's request and remove the thread.
    pub fn btw_delete(&self, session_id: &str, turn: &[Block], thread_id: &str, cx: &mut App) {
        let Some((_, source_user_id)) = Self::btw_source(session_id, turn, thread_id, cx) else {
            return;
        };
        self.inner
            .requests
            .abort(&request_key(session_id, thread_id));
        remove_btw_thread(session_id, &source_user_id, thread_id, cx);
    }

    /// `onBtwStop`: keep whatever the side answer streamed, like stopping a
    /// main turn, and leave the thread ready for the next question.
    pub fn btw_stop(&self, session_id: &str, turn: &[Block], thread_id: &str, cx: &mut App) {
        let Some(source_user_id) = turn_user_id(turn).filter(|id| !id.is_empty()) else {
            return;
        };
        self.inner
            .requests
            .abort(&request_key(session_id, thread_id));
        update_btw_thread(session_id, source_user_id, thread_id, cx, |thread| {
            let thread = thread?;
            if thread.status != BtwThreadStatus::Running {
                return Some(thread.clone());
            }
            let user_message_id = last_message_id(thread);
            let pending = thread.pending_blocks.as_deref().unwrap_or_default();
            let blocks = match thread.harness {
                Some(harness) if !pending.is_empty() => seal_btw_response_blocks(
                    pending,
                    harness,
                    thread.model.as_deref().unwrap_or(""),
                    &user_message_id,
                ),
                _ => Vec::new(),
            };
            let text = js::trim(
                &blocks
                    .iter()
                    .filter(|block| block.role == BlockRole::Assistant)
                    .map(|block| block.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            )
            .to_string();
            let now = now_ms();
            let mut messages = thread.messages.clone();
            if !blocks.is_empty() {
                messages.push(BtwMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: BtwMessageRole::Assistant,
                    text,
                    created_at: now,
                    blocks: Some(blocks),
                    extra: Extra::new(),
                });
            }
            Some(BtwThread {
                status: BtwThreadStatus::Ready,
                updated_at: now,
                pending_blocks: None,
                error: None,
                messages,
                ..thread.clone()
            })
        });
    }

    /// `onBtwRetry`: ask a failed thread's last question again.
    pub fn btw_retry(&self, session_id: &str, turn: &[Block], thread_id: &str, cx: &mut App) {
        let Some((source, source_user_id)) = Self::btw_source(session_id, turn, thread_id, cx)
        else {
            return;
        };
        let thread_harness = Self::thread_harness(&source, turn, Some(&source_user_id), thread_id);
        let source_block = find_user_block(&source.blocks, Some(&source_user_id));
        let Some(existing) = find_thread(source_block, thread_id) else {
            return;
        };
        if existing.status != BtwThreadStatus::Error {
            return;
        }
        let thread = BtwThread {
            status: BtwThreadStatus::Running,
            updated_at: now_ms(),
            error: None,
            pending_blocks: Some(Vec::new()),
            ..existing.clone()
        };
        let stored = thread.clone();
        if !update_btw_thread(session_id, &source_user_id, thread_id, cx, |_| Some(stored)) {
            return;
        }
        let Some(updated) = Self::open_source(session_id, cx) else {
            return;
        };
        let Some(harness) = thread.harness.or(thread_harness) else {
            return;
        };
        self.run_btw_request(
            BtwRequestInput {
                session_id: session_id.to_string(),
                user_block_id: source_user_id,
                source: updated,
                thread,
                harness,
            },
            cx,
        );
    }
}

/// The streamed and final halves of `runBtwRequest`.
struct BtwReply {
    target: ThreadRef,
    harness: HarnessId,
    response_model: String,
    user_message_id: String,
    signal: AbortSignal,
}

impl BtwReply {
    /// `onThreadId` and `onEvent`.
    fn apply(&self, update: PromptUpdate, cx: &mut App) {
        if self.signal.is_aborted() {
            return;
        }
        match update {
            PromptUpdate::ThreadId(provider_thread_id) => {
                self.target.update(cx, |thread| {
                    let thread = thread?;
                    if thread.provider_thread_id.as_deref() == Some(provider_thread_id.as_str()) {
                        return Some(thread.clone());
                    }
                    Some(BtwThread {
                        provider_thread_id: Some(provider_thread_id),
                        updated_at: now_ms(),
                        ..thread.clone()
                    })
                });
            }
            PromptUpdate::Event(event) => {
                self.target.update(cx, |thread| {
                    let thread =
                        thread.filter(|thread| thread.status == BtwThreadStatus::Running)?;
                    let pending_blocks = apply_btw_harness_event(
                        thread.pending_blocks.as_deref().unwrap_or_default(),
                        &event,
                        self.harness,
                        &self.response_model,
                        &self.user_message_id,
                    );
                    Some(BtwThread {
                        updated_at: now_ms(),
                        pending_blocks: Some(pending_blocks),
                        ..thread.clone()
                    })
                });
            }
        }
    }

    /// The `then` and `catch` handlers.
    fn settle(&self, result: anyhow::Result<String>, cx: &mut App) {
        if self.signal.is_aborted() {
            return;
        }
        let output = result
            .map_err(|error| error.to_string())
            .and_then(|output| {
                let text = js::trim(&output).to_string();
                if text.is_empty() {
                    Err(format!(
                        "{} returned an empty side answer.",
                        self.harness.title()
                    ))
                } else {
                    Ok(text)
                }
            });
        match output {
            Ok(text) => {
                self.target.update(cx, |thread| {
                    let thread = thread?;
                    let pending = thread.pending_blocks.as_deref().unwrap_or_default();
                    let blocks = (!pending.is_empty())
                        .then(|| {
                            seal_btw_response_blocks(
                                pending,
                                self.harness,
                                &self.response_model,
                                &self.user_message_id,
                            )
                        })
                        .filter(|blocks| !blocks.is_empty());
                    let mut messages = thread.messages.clone();
                    messages.push(BtwMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: BtwMessageRole::Assistant,
                        text,
                        created_at: now_ms(),
                        blocks,
                        extra: Extra::new(),
                    });
                    Some(BtwThread {
                        status: BtwThreadStatus::Ready,
                        updated_at: now_ms(),
                        pending_blocks: None,
                        messages,
                        error: None,
                        ..thread.clone()
                    })
                });
            }
            Err(message) => {
                self.target.update(cx, |thread| {
                    thread.map(|thread| BtwThread {
                        status: BtwThreadStatus::Error,
                        updated_at: now_ms(),
                        pending_blocks: None,
                        error: Some(message),
                        ..thread.clone()
                    })
                });
            }
        }
    }
}

/// Run a text prompt and hand each update to `on_update` as it arrives.
/// Updates sent before the prompt resolved are handled before it returns.
/// A copy of submit's private `pipeline::turn::drive` for this channel.
async fn drive<T>(
    operation: BoxFuture<'static, T>,
    updates: &async_channel::Receiver<PromptUpdate>,
    mut on_update: impl FnMut(PromptUpdate),
) -> T {
    let mut operation = operation;
    loop {
        let next = updates.recv();
        futures::pin_mut!(next);
        match futures::future::select(next, &mut operation).await {
            Either::Left((Ok(update), _)) => on_update(update),
            Either::Left((Err(_), _)) => return operation.await,
            Either::Right((result, _)) => {
                while let Ok(update) = updates.try_recv() {
                    on_update(update);
                }
                return result;
            }
        }
    }
}

/// The id of the turn's user block.
fn turn_user_id(turn: &[Block]) -> Option<&str> {
    turn.iter()
        .find(|block| block.role == BlockRole::User)
        .map(|block| block.id.as_str())
}

/// The session's user block with this id.
fn find_user_block<'a>(blocks: &'a [Block], user_id: Option<&str>) -> Option<&'a Block> {
    let user_id = user_id?;
    blocks
        .iter()
        .find(|block| block.id == user_id && block.role == BlockRole::User)
}

/// The block's side thread with this id.
fn find_thread<'a>(block: Option<&'a Block>, thread_id: &str) -> Option<&'a BtwThread> {
    block?
        .btw_threads
        .as_ref()?
        .iter()
        .find(|thread| thread.id == thread_id)
}

/// The id of the thread's latest message, or the thread's own.
fn last_message_id(thread: &BtwThread) -> String {
    thread
        .messages
        .last()
        .map(|message| message.id.clone())
        .unwrap_or_else(|| thread.id.clone())
}

/// `updateBtwThread`: replace or add one side thread on a user block, then
/// save the session. `update` gets the current thread and returns `None` to
/// leave the session alone. True when the session changed.
pub(crate) fn update_btw_thread(
    session_id: &str,
    user_block_id: &str,
    thread_id: &str,
    cx: &mut App,
    update: impl FnOnce(Option<&BtwThread>) -> Option<BtwThread>,
) -> bool {
    Engine::sessions(cx).update(cx, |sessions, cx| {
        let (index, next_block) = {
            let Some(session) = sessions.get(session_id) else {
                return false;
            };
            let Some(index) = session
                .blocks
                .iter()
                .position(|block| block.id == user_block_id && block.role == BlockRole::User)
            else {
                return false;
            };
            let block = &session.blocks[index];
            let current = find_thread(Some(block), thread_id);
            let Some(next_thread) = update(current) else {
                return false;
            };
            let next_block = if current.is_some() {
                replace_btw_thread(block, next_thread).unwrap_or_else(|| block.clone())
            } else {
                let mut next = block.clone();
                next.btw_threads
                    .get_or_insert_with(Vec::new)
                    .push(next_thread);
                next
            };
            (index, next_block)
        };
        sessions.update(session_id, cx, |session| {
            session.blocks[index] = next_block;
        });
        sessions.persist(session_id, cx);
        true
    })
}

/// `removeBtwThread`: drop one side thread from a user block, then save the
/// session. True when the session changed.
pub(crate) fn remove_btw_thread(
    session_id: &str,
    user_block_id: &str,
    thread_id: &str,
    cx: &mut App,
) -> bool {
    Engine::sessions(cx).update(cx, |sessions, cx| {
        let index = {
            let Some(session) = sessions.get(session_id) else {
                return false;
            };
            let Some(index) = session
                .blocks
                .iter()
                .position(|block| block.id == user_block_id && block.role == BlockRole::User)
            else {
                return false;
            };
            let has_thread = session.blocks[index]
                .btw_threads
                .as_ref()
                .is_some_and(|threads| threads.iter().any(|thread| thread.id == thread_id));
            if !has_thread {
                return false;
            }
            index
        };
        sessions.update(session_id, cx, |session| {
            let block = &mut session.blocks[index];
            let threads: Vec<BtwThread> = block
                .btw_threads
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|thread| thread.id != thread_id)
                .collect();
            block.btw_threads = (!threads.is_empty()).then_some(threads);
        });
        sessions.persist(session_id, cx);
        true
    })
}
