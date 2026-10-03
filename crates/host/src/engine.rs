//! Port of host/engine.ts: runs provider turns for host-owned sessions,
//! applies their events with the transcript reducer, batches streamed
//! output, and saves `HostSession` snapshots that desktops sync.
//!
//! The Node host did this on one event loop. Here one lock over the
//! engine's state stands in for that loop: commands, provider events,
//! flushes, and settlement each run under it, and provider calls run after
//! it is released. The lock is always taken before the store's own lock,
//! never inside it.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use base64::Engine as _;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use monocode_core::attachment::is_vision_image;
use monocode_core::block::{
    ApprovalDecided, Block, BlockRole, PlanBlockMeta, PlanStatus, TurnIntent, TurnModel,
};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};
use monocode_core::reducer::{SystemEnv, apply_harness_event_mut, now_ms, stop_streaming};
use monocode_core::session::{can_replace_session_title, format_session_title, title_from_prompt};
use monocode_core::{Attachment, Session};
use monocode_harness::core::SharedCatalog;
use monocode_harness::core::registry::{EventSink, TitleInput};
use monocode_harness::core::task::SharedSpawner;
use monocode_remote::host::HostStore;
use monocode_remote::host::attachments::resolve_attachments;
use monocode_remote::host::protocol::{
    ApprovalDecision, CommandReceipt, HostCommand, HostProject, HostSession, HostSessionStatus,
    HostSessionSummary, RemoteProvider, SendIntent, provider_name,
};
use monocode_remote::host::store::SessionPatch;
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::backend::{HostEngineOptions, HostHarness};
use crate::commands::parse_command;
use crate::git_worktrees::{
    named_worktree_branch, rename_host_worktree_branch, resolve_host_worktree,
};
use crate::providers::{HostProvider, HostProviders};
use crate::workspace_commands::WorkspaceCommands;

/// Streamed output is written in batches. Anything a user may need to act
/// on (approvals, questions, errors, completion) is written immediately.
pub const FLUSH_MS: u64 = 120;

const PLACEHOLDER_TITLE: &str = "New remote session";
/// How long `close` waits for stopped turns to settle.
const CLOSE_WAIT: Duration = Duration::from_secs(10);
const STORAGE_FAILED: &str =
    "Session storage failed during this turn. Inspect its work before continuing.";

/// `BATCHED`.
fn batched(event: &HarnessEvent) -> bool {
    matches!(
        event,
        HarnessEvent::MessageDelta { .. }
            | HarnessEvent::ReasoningDelta { .. }
            | HarnessEvent::ToolUpdated { .. }
            | HarnessEvent::AgentStep { .. }
            | HarnessEvent::Status { .. }
    )
}

/// `runningSessionsMessage`: names the running sessions a branch change
/// would disturb. The desktop recognizes the last sentence and asks before
/// retrying with `force`.
pub fn running_sessions_message(sessions: &[HostSessionSummary]) -> String {
    let names = sessions
        .iter()
        .take(3)
        .map(|session| format!("\"{}\"", session.title))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if sessions.len() > 3 {
        format!(" and {} more", sessions.len() - 3)
    } else {
        String::new()
    };
    if sessions.len() == 1 {
        format!(
            "{names} is running on the host. Switching branches changes the files it is working on."
        )
    } else {
        format!(
            "{} sessions are running on the host: {names}{more}. Switching branches changes the files they are working on.",
            sessions.len()
        )
    }
}

type Done = Shared<BoxFuture<'static, ()>>;

/// A running turn.
struct Active {
    done: Done,
    cancelled: bool,
    persistence_failed: bool,
}

/// A running session, including streamed events not yet written to disk.
struct Live {
    value: HostSession,
    events: Vec<HarnessEvent>,
    timer: Option<u64>,
}

#[derive(Default)]
struct State {
    switching_projects: HashSet<String>,
    running: HashMap<String, Active>,
    live: HashMap<String, Live>,
    retry_timers: HashMap<String, u64>,
    next_token: u64,
}

impl State {
    fn token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }
}

/// A test seam: return true to fail the next matching save.
#[cfg(test)]
pub(crate) type SaveFault = Box<dyn FnMut(&Value) -> bool + Send>;

struct Inner {
    store: Arc<HostStore>,
    providers: HostProviders,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    state: Mutex<State>,
    closing: AtomicBool,
    retry_delay: Duration,
    /// The real provider processes, when this engine serves a host.
    harness: Option<Arc<HostHarness>>,
    workspace: WorkspaceCommands,
    #[cfg(test)]
    save_fault: Mutex<Option<SaveFault>>,
}

/// What a command does after its transaction commits.
enum Effect {
    Run {
        prompt: Option<String>,
        intent: Option<SendIntent>,
        attachments: Vec<Attachment>,
        first_turn: Option<(String, bool)>,
    },
    Cancel,
    Approve {
        request_id: i64,
        decision: ApprovalDecision,
    },
    Answer {
        request_id: i64,
        reply: monocode_core::user_question::UserQuestionReply,
    },
}

/// `HostEngine`. Clones share one engine.
#[derive(Clone)]
pub struct HostEngine {
    inner: Arc<Inner>,
}

impl Inner {
    fn provider(&self, id: RemoteProvider) -> Result<Arc<dyn HostProvider>, String> {
        self.providers
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("{} is not available on this host", provider_name(id)))
    }

    /// `store.save`, with the test seam.
    fn persist(&self, value: HostSession, event: &Value) -> Result<Arc<HostSession>, String> {
        #[cfg(test)]
        if let Some(fault) = self.save_fault.lock().as_mut()
            && fault(event)
        {
            return Err("temporary storage error".into());
        }
        self.store.save(value, event)
    }

    /// `save`: bumps the revision and the update time in one transaction.
    fn save(&self, mut value: HostSession, event: &Value) -> Result<Arc<HostSession>, String> {
        value.revision += 1;
        value.updated_at = now_ms();
        self.store.transaction(|| self.persist(value, event))
    }

    /// `flush`: writes batched events.
    fn flush(&self, state: &mut State, id: &str) -> Result<(), String> {
        let Some(live) = state.live.get_mut(id) else {
            return Ok(());
        };
        live.timer = None;
        if live.events.is_empty() {
            return Ok(());
        }
        let event = json!({ "type": "events", "events": live.events });
        let saved = self.save(live.value.clone(), &event)?;
        live.value = (*saved).clone();
        live.events.clear();
        Ok(())
    }

    /// `scheduledFlush`: a failed write stops the provider.
    fn scheduled_flush(self: &Arc<Self>, state: &mut State, id: &str) {
        let Err(error) = self.flush(state, id) else {
            return;
        };
        if let Some(active) = state.running.get_mut(id) {
            active.persistence_failed = true;
        }
        log::error!("Session persistence failed; stopping its provider: {error}");
        let harness = state.live.get(id).map(|live| live.value.session.harness);
        if let Some(provider) = harness.and_then(|harness| self.provider(harness).ok()) {
            let id = id.to_string();
            self.spawner.spawn(
                async move {
                    let _ = provider.stop(&id).await;
                }
                .boxed(),
            );
        }
    }

    /// `settled`: ends streaming and closes a building plan.
    fn settled(
        value: &HostSession,
        status: HostSessionStatus,
        message: Option<&str>,
        ended_at: i64,
    ) -> HostSession {
        let message = message.filter(|message| !message.is_empty());
        let mut session = stop_streaming(&value.session, ended_at);
        for block in &mut session.blocks {
            if block.role == BlockRole::Plan
                && let Some(plan) = block.plan.as_mut()
                && plan.status == PlanStatus::Building
            {
                plan.status = if status == HostSessionStatus::Idle && message.is_none() {
                    PlanStatus::Built
                } else {
                    PlanStatus::Ready
                };
            }
        }
        if let Some(message) = message {
            let mut block =
                Block::new(uuid::Uuid::new_v4().to_string(), BlockRole::System, message);
            block.streaming = Some(false);
            session.blocks.push(block);
        }
        HostSession {
            status,
            session,
            ..value.clone()
        }
    }

    /// `event`: one provider event for the current run.
    fn event(self: &Arc<Self>, id: &str, run_id: &str, event: HarnessEvent) {
        let mut state = self.state.lock();
        let state = &mut *state;
        let Some(live) = state.live.get_mut(id) else {
            return;
        };
        if live.value.run_id.as_deref() != Some(run_id)
            || live.value.status != HostSessionStatus::Running
        {
            return;
        }
        if !apply_harness_event_mut(&mut SystemEnv, &mut live.value.session, &event) {
            return;
        }
        let batch = batched(&event);
        live.events.push(event);
        if !batch {
            self.scheduled_flush(state, id);
        } else if live.timer.is_none() {
            let token = state.token();
            if let Some(live) = state.live.get_mut(id) {
                live.timer = Some(token);
            }
            let engine = Arc::downgrade(self);
            let id = id.to_string();
            self.spawner.spawn(
                async move {
                    smol::Timer::after(Duration::from_millis(FLUSH_MS)).await;
                    let Some(engine) = engine.upgrade() else {
                        return;
                    };
                    let mut state = engine.state.lock();
                    if state.live.get(&id).and_then(|live| live.timer) == Some(token) {
                        engine.scheduled_flush(&mut state, &id);
                    }
                }
                .boxed(),
            );
        }
    }

    fn sink(self: &Arc<Self>, id: &str, run_id: &str) -> EventSink {
        let engine: Weak<Self> = Arc::downgrade(self);
        let (id, run_id) = (id.to_string(), run_id.to_string());
        Arc::new(move |event| {
            if let Some(engine) = engine.upgrade() {
                engine.event(&id, &run_id, event);
            }
        })
    }

    /// `run`: starts the provider turn for a saved `running` value.
    fn run(
        self: &Arc<Self>,
        state: &mut State,
        value: &HostSession,
        prompt: Option<String>,
        intent: Option<SendIntent>,
        attachments: Vec<Attachment>,
    ) {
        let session = value.session.clone();
        let run_id = value.run_id.clone().unwrap_or_default();
        let Ok(provider) = self.provider(session.harness) else {
            return;
        };
        let (done, finished) = futures::channel::oneshot::channel::<()>();
        state.running.insert(
            session.id.clone(),
            Active {
                done: finished.map(|_| ()).boxed().shared(),
                cancelled: false,
                persistence_failed: false,
            },
        );
        state.live.insert(
            session.id.clone(),
            Live {
                value: value.clone(),
                events: Vec::new(),
                timer: None,
            },
        );
        let engine = self.clone();
        self.spawner.spawn(
            async move {
                let outcome = engine
                    .turn(&session, &run_id, &provider, prompt, intent, attachments)
                    .await;
                if let Err(error) = outcome {
                    if let Some(live) = engine.state.lock().live.get_mut(&session.id) {
                        live.timer = None;
                    }
                    log::error!("Session persistence failed; stopping its provider: {error}");
                    let stopping = provider.clone();
                    let id = session.id.clone();
                    engine.spawner.spawn(
                        async move {
                            let _ = stopping.stop(&id).await;
                        }
                        .boxed(),
                    );
                    engine.retry_settlement(&session.id, &run_id, provider);
                }
                drop(done);
            }
            .boxed(),
        );
    }

    /// The body of `run`: the provider call, then settlement.
    async fn turn(
        self: &Arc<Self>,
        session: &Session,
        run_id: &str,
        provider: &Arc<dyn HostProvider>,
        prompt: Option<String>,
        intent: Option<SendIntent>,
        attachments: Vec<Attachment>,
    ) -> Result<(), String> {
        let id = session.id.as_str();
        let mut error: Option<String> = None;
        let cancelled = self
            .state
            .lock()
            .running
            .get(id)
            .is_some_and(|active| active.cancelled);
        if !self.closing.load(Ordering::SeqCst) && !cancelled {
            let input = HarnessSessionInput {
                session_id: id.to_string(),
                cwd: session.cwd.clone(),
                model: session.model.clone(),
                model_settings: Some(session.model_settings.clone()),
                provider_account_id: None,
                runtime_mode: session.runtime_mode,
                intent: intent.map(|intent| match intent {
                    SendIntent::Default => TurnIntent::Default,
                    SendIntent::Plan => TurnIntent::Plan,
                    SendIntent::Build => TurnIntent::Build,
                }),
                controls_agents: None,
                app_access: None,
            };
            let on_event = self.sink(id, run_id);
            let result = match prompt {
                None => provider.compact(input, on_event).await,
                Some(text) => match with_image_data(attachments) {
                    Ok(attachments) => {
                        provider
                            .send(
                                SendTurnInput {
                                    session: input,
                                    text,
                                    attachments: Some(attachments),
                                },
                                on_event,
                            )
                            .await
                    }
                    Err(error) => Err(error),
                },
            };
            error = result.err();
        }
        // Keep the session running until the old process has stopped.
        // Otherwise a follow-up can race cleanup and have its newly spawned
        // child killed.
        provider.stop(id).await?;
        let persisted = {
            let mut state = self.state.lock();
            let state = &mut *state;
            self.flush(state, id)?;
            state.live.remove(id);
            let latest = self.store.session(id)?;
            if latest.run_id.as_deref() == Some(run_id) {
                let closing = self.closing.load(Ordering::SeqCst);
                let (cancelled, failed) = state
                    .running
                    .get(id)
                    .map(|active| (active.cancelled, active.persistence_failed))
                    .unwrap_or_default();
                let message = if closing {
                    Some("Host stopped. This turn was interrupted.")
                } else if failed {
                    Some(STORAGE_FAILED)
                } else if cancelled {
                    Some("Stopped by you.")
                } else {
                    error.as_deref()
                };
                let status = if closing || failed {
                    HostSessionStatus::Interrupted
                } else {
                    HostSessionStatus::Idle
                };
                let mut event = json!({ "type": "settled", "cancelled": cancelled });
                if let Some(error) = &error {
                    event["error"] = json!(error);
                }
                self.save(Self::settled(&latest, status, message, now_ms()), &event)?;
            }
            state.running.remove(id);
            self.store.session(id)?.session.clone()
        };
        // Stopping released the provider's callbacks; binding keeps only its
        // conversation for an explicit follow-up.
        if let Some(provider_session_id) = &persisted.provider_session_id {
            provider.bind(id, provider_session_id, &persisted.cwd);
        }
        Ok(())
    }

    /// `retrySettlement`: settles a turn whose final write failed, every
    /// second until it succeeds.
    fn retry_settlement(self: &Arc<Self>, id: &str, run_id: &str, provider: Arc<dyn HostProvider>) {
        let token = {
            let mut state = self.state.lock();
            if self.closing.load(Ordering::SeqCst) || state.retry_timers.contains_key(id) {
                return;
            }
            let token = state.token();
            state.retry_timers.insert(id.to_string(), token);
            token
        };
        let engine = Arc::downgrade(self);
        let (id, run_id) = (id.to_string(), run_id.to_string());
        let delay = self.retry_delay;
        self.spawner.spawn(
            async move {
                smol::Timer::after(delay).await;
                let Some(engine) = engine.upgrade() else {
                    return;
                };
                {
                    let mut state = engine.state.lock();
                    if state.retry_timers.get(&id) != Some(&token) {
                        return;
                    }
                    state.retry_timers.remove(&id);
                }
                if let Err(error) = engine.retry(&id, &run_id, &provider).await {
                    log::error!("Retrying session persistence: {error}");
                    engine.retry_settlement(&id, &run_id, provider);
                }
            }
            .boxed(),
        );
    }

    async fn retry(
        self: &Arc<Self>,
        id: &str,
        run_id: &str,
        provider: &Arc<dyn HostProvider>,
    ) -> Result<(), String> {
        provider.stop(id).await?;
        let latest = {
            let mut state = self.state.lock();
            let state = &mut *state;
            self.flush(state, id)?;
            let latest = self.store.session(id)?;
            if latest.run_id.as_deref() == Some(run_id)
                && latest.status == HostSessionStatus::Running
            {
                self.save(
                    Self::settled(
                        &latest,
                        HostSessionStatus::Interrupted,
                        Some(STORAGE_FAILED),
                        latest.updated_at,
                    ),
                    &json!({ "type": "interrupted", "reason": "persistence failure" }),
                )?;
            }
            state.live.remove(id);
            state.running.remove(id);
            latest
        };
        if let Some(provider_session_id) = &latest.session.provider_session_id {
            provider.bind(id, provider_session_id, &latest.session.cwd);
        }
        Ok(())
    }

    /// `generateFirstTurnNames`: an LLM title, and a branch name for an
    /// automatically created worktree.
    fn generate_first_turn_names(
        self: &Arc<Self>,
        value: &HostSession,
        message: &str,
        generate_title: bool,
    ) {
        let Ok(provider) = self.provider(value.session.harness) else {
            return;
        };
        let id = value.session.id.clone();
        let cwd = value.session.cwd.clone();
        let harness = value.session.harness;
        if generate_title && provider.can_generate_title() {
            let engine = self.clone();
            let title = value.session.title.clone();
            let request = provider.generate_title(TitleInput {
                session_id: id.clone(),
                cwd: cwd.clone(),
                message: message.to_string(),
                provider_account_id: None,
            });
            let id = id.clone();
            self.spawner.spawn(
                async move {
                    let result: Result<(), String> = async {
                        let Some(generated) = request.await? else {
                            return Ok(());
                        };
                        let mut state = engine.state.lock();
                        engine.flush(&mut state, &id)?;
                        let current = engine.store.session(&id)?;
                        if current.session.title != title {
                            return Ok(());
                        }
                        let mut next = (*current).clone();
                        next.session.title = format_session_title(harness, &generated.title);
                        let saved =
                            engine.save(next, &json!({ "type": "session.generatedTitle" }))?;
                        if let Some(live) = state.live.get_mut(&id) {
                            live.value = (*saved).clone();
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        log::debug!("[monocode] remote session title {error}");
                    }
                }
                .boxed(),
            );
        }
        let Some(temporary) = value.auto_worktree_branch.clone() else {
            return;
        };
        if !provider.can_generate_branch_name() {
            return;
        }
        let engine = self.clone();
        let project_id = value.project_id.clone();
        let request = provider.generate_branch_name(&cwd, message);
        self.spawner.spawn(
            async move {
                let result: Result<(), String> = async {
                    let Some(branch) = request.await?.as_deref().and_then(named_worktree_branch)
                    else {
                        return Ok(());
                    };
                    // A title or branch request may finish after the
                    // conversation was deleted.
                    let before = engine.store.session(&id)?;
                    if before.auto_worktree_branch.as_deref() != Some(temporary.as_str()) {
                        return Ok(());
                    }
                    let project = engine.store.project(&project_id)?;
                    {
                        let (engine, id, temporary, branch, cwd) = (
                            engine.clone(),
                            id.clone(),
                            temporary.clone(),
                            branch.clone(),
                            cwd.clone(),
                        );
                        smol::unblock(move || {
                            let owned = || {
                                engine.store.session(&id).is_ok_and(|current| {
                                    current.auto_worktree_branch.as_deref()
                                        == Some(temporary.as_str())
                                })
                            };
                            rename_host_worktree_branch(
                                &project.cwd,
                                &cwd,
                                &temporary,
                                &branch,
                                &owned,
                            )
                        })
                        .await?;
                    }
                    let mut state = engine.state.lock();
                    engine.flush(&mut state, &id)?;
                    let current = engine.store.session(&id)?;
                    let mut next = (*current).clone();
                    next.auto_worktree_branch = None;
                    next.session.branch = Some(branch.clone());
                    let saved = engine.save(
                        next,
                        &json!({ "type": "session.generatedBranch", "branch": branch }),
                    )?;
                    if let Some(live) = state.live.get_mut(&id) {
                        live.value = (*saved).clone();
                    }
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    log::debug!("[monocode] remote worktree branch {error}");
                }
            }
            .boxed(),
        );
    }
}

/// Adds base64 data to vision images, as the provider sends them inline.
fn with_image_data(attachments: Vec<Attachment>) -> Result<Vec<Attachment>, String> {
    attachments
        .into_iter()
        .map(|mut file| {
            if is_vision_image(&file.mime_type)
                && file.size <= 20 * 1024 * 1024
                && let Some(path) = file.path.as_deref().filter(|path| !path.is_empty())
            {
                let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
                file.data = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
            }
            Ok(file)
        })
        .collect()
}

/// `value.session.model.replace(/^[^:]+:/, "")`.
fn model_slug(model: &str) -> &str {
    match model.find(':') {
        Some(index) if index > 0 => &model[index + 1..],
        _ => model,
    }
}

impl HostEngine {
    /// Builds the engine over `store` and recovers turns a previous host
    /// left running. Provider dispatch is not transactional with SQLite, so
    /// a send is never replayed after a crash; its effects may already
    /// exist.
    pub fn new(
        store: Arc<HostStore>,
        providers: HostProviders,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
    ) -> Result<Self, String> {
        Self::build(store, providers, catalog, spawner, None)
    }

    /// The engine over the real harness: every provider adapter, run by
    /// this host's process supervisor. This is what `serve` uses.
    pub fn start(store: Arc<HostStore>, options: HostEngineOptions) -> Result<Self, String> {
        let harness = Arc::new(HostHarness::start(&store, options));
        let engine = Self::build(
            store,
            harness.providers(),
            harness.catalog().clone(),
            harness.spawner(),
            Some(harness.clone()),
        );
        if engine.is_err() {
            harness.close();
        }
        engine
    }

    fn build(
        store: Arc<HostStore>,
        providers: HostProviders,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        harness: Option<Arc<HostHarness>>,
    ) -> Result<Self, String> {
        let engine = Self {
            inner: Arc::new(Inner {
                workspace: WorkspaceCommands::new(store.clone()),
                store,
                providers,
                catalog,
                spawner,
                state: Mutex::new(State::default()),
                closing: AtomicBool::new(false),
                retry_delay: Duration::from_secs(1),
                harness,
                #[cfg(test)]
                save_fault: Mutex::new(None),
            }),
        };
        let inner = &engine.inner;
        for value in inner.store.sessions(None)? {
            if value.status == HostSessionStatus::Running {
                inner.save(
                    Inner::settled(
                        &value,
                        HostSessionStatus::Interrupted,
                        Some("Host restarted. This turn was interrupted; inspect its work before continuing."),
                        value.updated_at,
                    ),
                    &json!({ "type": "interrupted" }),
                )?;
            }
            if let Some(provider_session_id) = &value.session.provider_session_id {
                inner.provider(value.session.harness)?.bind(
                    &value.session.id,
                    provider_session_id,
                    &value.session.cwd,
                );
            }
        }
        Ok(engine)
    }

    pub fn store(&self) -> &HostStore {
        &self.inner.store
    }

    pub fn store_arc(&self) -> Arc<HostStore> {
        self.inner.store.clone()
    }

    pub fn catalog(&self) -> &SharedCatalog {
        &self.inner.catalog
    }

    #[cfg(test)]
    pub(crate) fn set_save_fault(&self, fault: Option<SaveFault>) {
        *self.inner.save_fault.lock() = fault;
    }

    /// `openProject`: registers an absolute directory.
    pub fn open_project(&self, path: &str) -> Result<HostProject, String> {
        if !std::path::Path::new(path).is_absolute() || path.contains('\0') {
            return Err("Choose an absolute directory path on the host".into());
        }
        let cwd = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        if !std::fs::metadata(&cwd).is_ok_and(|meta| meta.is_dir()) {
            return Err("Project path is not a directory".into());
        }
        let name = cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.inner.store.add_project(&cwd.to_string_lossy(), &name)
    }

    /// `withIdleProject`: runs a branch change while no session in the
    /// project is running. With `force`, the change goes ahead anyway, after
    /// the desktop has asked; the running agents then see their files
    /// change, as they would locally.
    pub fn with_idle_project<T>(
        &self,
        project_id: &str,
        force: bool,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        {
            let mut state = self.inner.state.lock();
            if state.switching_projects.contains(project_id) {
                return Err("A branch switch is already in progress".into());
            }
            let running: Vec<HostSessionSummary> = self
                .inner
                .store
                .summaries(project_id)?
                .into_iter()
                .filter(|session| session.status == HostSessionStatus::Running)
                .collect();
            if !running.is_empty() && !force {
                return Err(running_sessions_message(&running));
            }
            state.switching_projects.insert(project_id.to_string());
        }
        let result = action();
        self.inner
            .state
            .lock()
            .switching_projects
            .remove(project_id);
        result
    }

    /// `updateSession`: flushes batched output, then applies the metadata
    /// change.
    pub fn update_session(
        &self,
        id: &str,
        patch: &SessionPatch,
    ) -> Result<HostSessionSummary, String> {
        let inner = &self.inner;
        let mut state = inner.state.lock();
        inner.flush(&mut state, id)?;
        let summary = inner.store.update_session(id, patch)?;
        if let Some(live) = state.live.get_mut(id) {
            live.value = (*inner.store.session(id)?).clone();
        }
        Ok(summary)
    }

    /// `command`: validates and applies one `HostCommand`. A receipt means
    /// durable host acceptance, not provider completion.
    pub fn command(&self, raw: &Value) -> Result<CommandReceipt, String> {
        let inner = &self.inner;
        if inner.closing.load(Ordering::SeqCst) {
            return Err("Host is stopping".into());
        }
        let command = parse_command(raw)?;
        let serialized = serde_json::to_string(&command).map_err(|error| error.to_string())?;
        let signature: String = Sha256::digest(serialized.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let command_id = command_id(&command).to_string();
        if let Some(previous) = inner.store.receipt(&command_id, &signature)? {
            return Ok(previous);
        }
        let mut guard = inner.state.lock();
        let state = &mut *guard;
        // Commands apply to the latest state, including batched output.
        if let Some(session_id) = session_id(&command) {
            inner.flush(state, session_id)?;
        }
        let mut effect: Option<Effect> = None;
        let (receipt, saved) = inner.store.transaction(|| {
            let mut value = self.prepare(state, &command, &mut effect)?;
            value.revision += 1;
            if !matches!(command, HostCommand::Create { .. }) {
                value.updated_at = now_ms();
            }
            let saved = inner.persist(value, &json!({ "type": "command", "command": command }))?;
            let receipt = CommandReceipt {
                command_id: command_id.clone(),
                session_id: saved.session.id.clone(),
                revision: saved.revision,
            };
            inner.store.record_receipt(&signature, &receipt)?;
            Ok((receipt, saved))
        })?;
        let id = saved.session.id.clone();
        if let Some(live) = state.live.get_mut(&id) {
            live.value = (*saved).clone();
        }
        let provider = inner.provider(saved.session.harness);
        match effect {
            None => {}
            Some(Effect::Run {
                prompt,
                intent,
                attachments,
                first_turn,
            }) => {
                inner.run(state, &saved, prompt.clone(), intent, attachments);
                drop(guard);
                if let Some((message, placeholder_title)) = first_turn {
                    inner.generate_first_turn_names(&saved, &message, placeholder_title);
                }
            }
            Some(Effect::Cancel) => {
                if let Some(active) = state.running.get_mut(&id) {
                    active.cancelled = true;
                }
                drop(guard);
                let provider = provider?;
                inner.spawner.spawn(
                    async move {
                        if provider.cancel(&id).await.is_err() {
                            let _ = provider.stop(&id).await;
                        }
                    }
                    .boxed(),
                );
            }
            Some(Effect::Approve {
                request_id,
                decision,
            }) => {
                drop(guard);
                let decision = match decision {
                    ApprovalDecision::Allow => {
                        monocode_core::harness_event::ApprovalDecision::Allow
                    }
                    ApprovalDecision::Deny => monocode_core::harness_event::ApprovalDecision::Deny,
                };
                provider?.approve(&id, request_id, decision)?;
            }
            Some(Effect::Answer { request_id, reply }) => {
                drop(guard);
                // TODO(port): as in TypeScript, a provider without questions
                // fails here after the command was recorded.
                provider?.answer(&id, request_id, reply)?;
            }
        }
        Ok(receipt)
    }

    /// The new session value for `command`, inside the command transaction.
    fn prepare(
        &self,
        state: &mut State,
        command: &HostCommand,
        effect: &mut Option<Effect>,
    ) -> Result<HostSession, String> {
        let inner = &self.inner;
        let store = &inner.store;
        if let HostCommand::Create {
            project_id,
            worktree_cwd,
            auto_worktree_branch,
            harness,
            model,
            model_settings,
            runtime_mode,
            ..
        } = command
        {
            let project = store.project(project_id)?;
            if state.switching_projects.contains(&project.id) {
                return Err("Wait for the branch switch to finish".into());
            }
            inner.provider(*harness)?;
            let cwd = resolve_host_worktree(
                &project.cwd,
                worktree_cwd
                    .as_ref()
                    .map(|cwd| Value::String(cwd.clone()))
                    .as_ref(),
            )?;
            let now = now_ms();
            let mut session = Session::blank(
                uuid::Uuid::new_v4().to_string(),
                *harness,
                model.clone(),
                cwd.clone(),
            );
            session.runtime_mode = *runtime_mode;
            session.model_settings = model_settings.clone().unwrap_or_default();
            session.title = PLACEHOLDER_TITLE.into();
            if auto_worktree_branch.is_some() {
                session.branch = auto_worktree_branch.clone();
                session.worktree_cwd = Some(cwd);
            }
            return Ok(HostSession {
                session,
                project_id: project.id,
                revision: 0,
                run_id: None,
                status: HostSessionStatus::Idle,
                created_at: Some(now),
                updated_at: now,
                archived: None,
                pinned: None,
                auto_worktree_branch: auto_worktree_branch.clone(),
                block_revisions: None,
                extra: Default::default(),
            });
        }
        let session_id = session_id(command).unwrap_or_default();
        let mut value = (*store.session(session_id)?).clone();
        let starts_turn = matches!(
            command,
            HostCommand::Send { .. } | HostCommand::Compact { .. }
        );
        if starts_turn && state.switching_projects.contains(&value.project_id) {
            return Err("Wait for the branch switch to finish".into());
        }
        let provider = inner.provider(value.session.harness)?;
        let running = value.status == HostSessionStatus::Running;
        match command {
            HostCommand::Create { .. } => unreachable!("handled above"),
            HostCommand::Configure {
                model,
                model_settings,
                runtime_mode,
                ..
            } => {
                if running {
                    return Err("Wait for the current turn before changing settings".into());
                }
                value.session.model = model.clone();
                value.session.model_settings = model_settings.clone();
                value.session.runtime_mode = *runtime_mode;
            }
            HostCommand::Draft {
                command_id,
                text,
                attachments,
                ..
            } => {
                if running || value.session.blocks.iter().any(Block::is_draft) {
                    return Err("This session cannot save another draft right now".into());
                }
                let attachments =
                    resolve_attachments(store, attachments.as_deref().unwrap_or_default())?;
                if value.session.blocks.is_empty() {
                    value.session.title =
                        title_from_prompt(text, value.session.harness, &attachments);
                }
                let mut block = Block::new(command_id.clone(), BlockRole::User, text.clone());
                if !attachments.is_empty() {
                    block.attachments = Some(attachments);
                }
                block.draft = Some(true);
                value.session.blocks.push(block);
            }
            HostCommand::RemoveDraft { draft_block_id, .. } => {
                let index = value
                    .session
                    .blocks
                    .iter()
                    .position(|block| block.id == *draft_block_id && block.is_draft())
                    .ok_or("Draft not found")?;
                let draft_id = value.session.blocks[index].id.clone();
                value.session.blocks.retain(|block| block.id != draft_id);
            }
            HostCommand::Send { .. } | HostCommand::Compact { .. } => {
                self.prepare_turn(&mut value, command, &provider, effect)?;
            }
            HostCommand::Cancel { run_id, .. }
            | HostCommand::Approve { run_id, .. }
            | HostCommand::Answer { run_id, .. } => {
                if value.run_id.as_deref() != Some(run_id.as_str()) || !running {
                    return Err("This request belongs to a finished or replaced turn".into());
                }
                match command {
                    HostCommand::Cancel { .. } => *effect = Some(Effect::Cancel),
                    HostCommand::Approve {
                        request_id,
                        decision,
                        ..
                    } => {
                        let pending = value.session.blocks.iter().any(|block| {
                            block.approval.as_ref().is_some_and(|approval| {
                                approval.request_id == *request_id && approval.decided.is_none()
                            })
                        });
                        if !pending {
                            return Err("Approval is already resolved".into());
                        }
                        apply_harness_event_mut(
                            &mut SystemEnv,
                            &mut value.session,
                            &HarnessEvent::ApprovalResolved {
                                request_id: *request_id,
                                decision: match decision {
                                    ApprovalDecision::Allow => ApprovalDecided::Allow,
                                    ApprovalDecision::Deny => ApprovalDecided::Deny,
                                },
                            },
                        );
                        *effect = Some(Effect::Approve {
                            request_id: *request_id,
                            decision: *decision,
                        });
                    }
                    HostCommand::Answer {
                        request_id, reply, ..
                    } => {
                        if value
                            .session
                            .pending_question
                            .as_ref()
                            .map(|question| question.request_id)
                            != Some(*request_id)
                        {
                            return Err("Question is already resolved".into());
                        }
                        value.session.pending_question = None;
                        *effect = Some(Effect::Answer {
                            request_id: *request_id,
                            reply: reply.clone(),
                        });
                    }
                    _ => unreachable!("matched above"),
                }
            }
        }
        Ok(value)
    }

    /// The `send` and `compact` half of `command`.
    fn prepare_turn(
        &self,
        value: &mut HostSession,
        command: &HostCommand,
        provider: &Arc<dyn HostProvider>,
        effect: &mut Option<Effect>,
    ) -> Result<(), String> {
        let inner = &self.inner;
        let (command_id, text, attachments, intent, draft_block_id, plan_block_id, send) =
            match command {
                HostCommand::Send {
                    command_id,
                    text,
                    attachments,
                    intent,
                    draft_block_id,
                    plan_block_id,
                    ..
                } => (
                    command_id,
                    text.as_str(),
                    attachments.as_deref(),
                    *intent,
                    draft_block_id.as_deref(),
                    plan_block_id.as_deref(),
                    true,
                ),
                HostCommand::Compact { command_id, .. } => {
                    (command_id, "", None, None, None, None, false)
                }
                _ => unreachable!("only send and compact start turns"),
            };
        if value.status == HostSessionStatus::Running {
            return Err("This session is already running".into());
        }
        if !send && !provider.can_compact() {
            return Err("Context compaction is unavailable for this provider".into());
        }
        let blocks = &value.session.blocks;
        let draft = draft_block_id.and_then(|id| {
            blocks
                .iter()
                .find(|block| block.id == id && block.is_draft())
        });
        if draft_block_id.is_some() && draft.is_none() {
            return Err("Draft not found".into());
        }
        let plan_index = plan_block_id.and_then(|id| {
            blocks
                .iter()
                .position(|block| block.id == id && block.role == BlockRole::Plan)
        });
        if plan_block_id.is_some() {
            let ready = plan_index.map(|index| &blocks[index]).is_some_and(|plan| {
                !monocode_core::js::trim(&plan.text).is_empty()
                    && !plan.is_streaming()
                    && !plan.plan.as_ref().is_some_and(|meta| {
                        matches!(meta.status, PlanStatus::Building | PlanStatus::Built)
                    })
            });
            if !ready {
                return Err("Plan is not ready to build".into());
            }
        }
        let attachments = if send {
            match draft.and_then(|draft| draft.attachments.clone()) {
                Some(attachments) => attachments,
                None => resolve_attachments(&inner.store, attachments.unwrap_or_default())?,
            }
        } else {
            Vec::new()
        };
        let run_id = uuid::Uuid::new_v4().to_string();
        let first_turn = send && !blocks.iter().any(|block| !block.is_draft());
        let harness = value.session.harness;
        let placeholder_title = value.session.title == PLACEHOLDER_TITLE
            || can_replace_session_title(&value.session.title, harness, harness.label());
        let model = inner
            .catalog
            .read()
            .resolve_model(harness, Some(&value.session.model));
        let turn_model = TurnModel {
            harness,
            id: value.session.model.clone(),
            name: if model.id == value.session.model {
                model.name.clone()
            } else {
                model_slug(&value.session.model).to_string()
            },
            extra: Default::default(),
        };
        let mut next_blocks: Vec<Block> = Vec::with_capacity(blocks.len() + 1);
        for (index, block) in blocks.iter().enumerate() {
            if block.is_draft() {
                continue;
            }
            let mut block = block.clone();
            if Some(index) == plan_index {
                let mut plan = block.plan.clone().unwrap_or(PlanBlockMeta {
                    status: PlanStatus::Ready,
                    ..Default::default()
                });
                plan.status = PlanStatus::Building;
                plan.approved_text = Some(block.text.clone());
                block.plan = Some(plan);
            }
            next_blocks.push(block);
        }
        let mut user = Block::new(
            command_id.clone(),
            BlockRole::User,
            if send { text } else { "/compact" },
        );
        if !attachments.is_empty() {
            user.attachments = Some(attachments.clone());
        }
        user.started_at = Some(now_ms());
        user.turn_model = Some(turn_model);
        next_blocks.push(user);
        value.status = HostSessionStatus::Running;
        value.run_id = Some(run_id);
        value.session.busy = Some(true);
        value.session.pending_question = None;
        // A new turn retries after a usage limit, as it does locally.
        value.session.usage_limit = None;
        if first_turn && placeholder_title {
            value.session.title = title_from_prompt(text, harness, &attachments);
        }
        value.session.blocks = next_blocks;
        *effect = Some(Effect::Run {
            prompt: send.then(|| text.to_string()),
            intent: if send { intent } else { None },
            attachments,
            first_turn: (first_turn && send).then(|| (text.to_string(), placeholder_title)),
        });
        Ok(())
    }

    /// `close`: stops running providers and waits for their turns to
    /// settle.
    pub fn close(&self) {
        let inner = &self.inner;
        if inner.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        let (stops, dones) = {
            let mut state = inner.state.lock();
            state.retry_timers.clear();
            let stops: Vec<_> = state
                .running
                .keys()
                .filter_map(|id| {
                    let harness = inner.store.session(id).ok()?.session.harness;
                    let provider = inner.provider(harness).ok()?;
                    Some(provider.stop(id))
                })
                .collect();
            let dones: Vec<Done> = state
                .running
                .values()
                .map(|active| active.done.clone())
                .collect();
            (stops, dones)
        };
        smol::block_on(async {
            for result in futures::future::join_all(stops).await {
                if let Err(error) = result {
                    log::error!("Could not stop a provider: {error}");
                }
            }
            // TODO(port): TypeScript waited for every turn without a limit.
            // A provider whose send never returns after it was stopped (Codex
            // while it starts) would keep the host from exiting, so the wait
            // is bounded. A turn left running is marked interrupted when the
            // host starts again.
            let settled = smol::future::or(
                async {
                    futures::future::join_all(dones).await;
                    true
                },
                async {
                    smol::Timer::after(CLOSE_WAIT).await;
                    false
                },
            )
            .await;
            if !settled {
                log::error!("A provider turn did not stop; closing the host anyway");
            }
        });
        if let Some(harness) = &inner.harness {
            harness.close();
        }
    }

    /// The real harness, when this engine runs providers.
    pub fn harness(&self) -> Option<&Arc<HostHarness>> {
        self.inner.harness.as_ref()
    }

    pub fn workspace(&self) -> &WorkspaceCommands {
        &self.inner.workspace
    }

    /// Whether `id` has a turn the engine is still running.
    pub fn is_running(&self, id: &str) -> bool {
        self.inner.state.lock().running.contains_key(id)
    }
}

fn command_id(command: &HostCommand) -> &str {
    match command {
        HostCommand::Create { command_id, .. }
        | HostCommand::Configure { command_id, .. }
        | HostCommand::Compact { command_id, .. }
        | HostCommand::Send { command_id, .. }
        | HostCommand::Draft { command_id, .. }
        | HostCommand::RemoveDraft { command_id, .. }
        | HostCommand::Cancel { command_id, .. }
        | HostCommand::Approve { command_id, .. }
        | HostCommand::Answer { command_id, .. } => command_id,
    }
}

fn session_id(command: &HostCommand) -> Option<&str> {
    match command {
        HostCommand::Create { .. } => None,
        HostCommand::Configure { session_id, .. }
        | HostCommand::Compact { session_id, .. }
        | HostCommand::Send { session_id, .. }
        | HostCommand::Draft { session_id, .. }
        | HostCommand::RemoveDraft { session_id, .. }
        | HostCommand::Cancel { session_id, .. }
        | HostCommand::Approve { session_id, .. }
        | HostCommand::Answer { session_id, .. } => Some(session_id),
    }
}

#[cfg(test)]
mod tests;
