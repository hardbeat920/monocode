//! The control request executor from src/app/App.tsx (lines 8998-9221). The
//! loopback control server hands each authenticated request to
//! `ControlExecutor`; the engine runs it and answers through `control_reply`.
//! The `control` namespace goes to the orchestrator, the `app` namespace to
//! the agent app API.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::{LocalBoxFuture, Shared};
use gpui::{App, AppContext as _, AsyncApp, Task, WeakEntity};
use monocode_core::session::session_draft_block;
use monocode_core::{
    HarnessAvailability, HarnessId, ModelCatalog, ModelEnv, ModelPrefs, ProjectProviders, Session,
};
use monocode_process::control::{ControlEvents, ControlHost, ControlRequest, control_reply};
use monocode_settings::Kv;
use monocode_store::notes::{Note, NoteUpsert};
use serde_json::{Value, json};

use super::agent_app::{
    AgentAppHost, AppLaunch, AppSessionListing, AppSessionPlacement, DraftResult, SendResult,
    handle_agent_app,
};
use super::orchestrator::{Orchestrator, handle};
use super::peers::OrchestrationPeers;
use crate::history::HistoryPackage;
use crate::projects::backend::{Worktree, Worktrees};
use crate::runtime::Engine;
use crate::runtime::sessions::get_stored_session;
use crate::runtime::util::project_path::same_project_path;
use crate::submit::{Submit, SubmitConfig, SubmitOptions};

/// Delivers control requests from the server's threads to the engine.
pub struct ControlExecutor {
    sender: async_channel::Sender<(String, ControlRequest)>,
}

impl ControlExecutor {
    /// The executor and the receiving end `serve_control_requests` reads.
    pub fn channel() -> (Arc<Self>, async_channel::Receiver<(String, ControlRequest)>) {
        let (sender, receiver) = async_channel::unbounded();
        (Arc::new(Self { sender }), receiver)
    }
}

impl ControlEvents for ControlExecutor {
    fn request(&self, owner: &str, request: ControlRequest) -> Result<(), String> {
        self.sender
            .try_send((owner.to_string(), request))
            .map_err(|_| "MonoCode executor is unavailable".to_string())
    }
}

/// Bind the loopback control server and serve its requests on this app.
pub fn start_control_server(cx: &mut App) -> Result<Arc<ControlHost>, String> {
    let (executor, requests) = ControlExecutor::channel();
    let host = Arc::new(monocode_process::control::init(executor)?);
    serve_control_requests(requests, host.clone(), cx).detach();
    Ok(host)
}

/// Run each request from `requests` and reply through `host`. The server
/// waits up to 35 seconds for each answer.
pub fn serve_control_requests(
    requests: async_channel::Receiver<(String, ControlRequest)>,
    host: Arc<ControlHost>,
    cx: &mut App,
) -> Task<()> {
    cx.spawn(async move |cx| {
        while let Ok((owner, request)) = requests.recv().await {
            let host = host.clone();
            cx.spawn(async move |cx| {
                let id = request.id.clone();
                let response = match run_request(&owner, request, cx).await {
                    Ok(result) => json!({ "ok": true, "result": result }),
                    Err(error) => json!({ "ok": false, "error": error }),
                };
                let reply =
                    cx.background_spawn(async move { control_reply(&host, &owner, id, response) });
                if let Err(error) = reply.await {
                    log::error!("[orchestration] control reply failed: {error}");
                }
            })
            .detach();
        }
    })
}

type SharedAnswer = Shared<LocalBoxFuture<'static, Result<Value, String>>>;

/// `appReceipts`: in-flight and finished app calls by `sourceId:requestId`,
/// so a retried call gets the first call's answer. At most 256 are kept.
#[derive(Default)]
pub struct AppReceipts {
    entries: HashMap<String, (String, SharedAnswer)>,
    order: VecDeque<String>,
}

impl AppReceipts {
    fn get(&self, key: &str) -> Option<(String, SharedAnswer)> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, signature: String, answer: SharedAnswer) {
        if self
            .entries
            .insert(key.clone(), (signature, answer))
            .is_none()
        {
            self.order.push_back(key);
        }
        if self.entries.len() > 256
            && let Some(first) = self.order.pop_front()
        {
            self.entries.remove(&first);
        }
    }

    fn remove_if(&mut self, key: &str, answer: &SharedAnswer) {
        if self
            .entries
            .get(key)
            .is_some_and(|(_, current)| current.ptr_eq(answer))
        {
            self.entries.remove(key);
            self.order.retain(|entry| entry != key);
        }
    }
}

/// What the executor needs, read from the package global for each request.
#[derive(Clone)]
pub struct ExecutorContext {
    pub orchestrator: WeakEntity<Orchestrator>,
    pub peers: Rc<dyn OrchestrationPeers>,
    pub receipts: Rc<RefCell<AppReceipts>>,
    /// The settings store that holds session folders.
    pub kv: Kv,
}

pub(crate) async fn run_request(
    owner: &str,
    request: ControlRequest,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let Some(executor) = cx.update(|cx| super::Orchestration::try_global(cx).map(|o| o.executor()))
    else {
        return Err("MonoCode executor is unavailable".into());
    };
    let input = request.input.as_object().cloned().unwrap_or_default();
    if request.namespace == "control" {
        return handle(
            &executor.orchestrator,
            &request.session_id,
            &request.request_id,
            &request.action,
            &input,
            cx,
        )
        .await;
    }
    if request.namespace != "app" {
        return Err("Unknown CLI namespace".into());
    }
    let source = cx.update(|cx| {
        Engine::sessions(cx)
            .read(cx)
            .get(&request.session_id)
            .cloned()
    });
    let leads_run = executor
        .orchestrator
        .read_with(cx, |orchestrator, _| {
            orchestrator.run(&request.session_id).is_some()
        })
        .unwrap_or(false);
    let Some(source) = source.filter(|source| {
        source.inbox_ask.is_none() && source.orchestration_lead_id.is_none() && !leads_run
    }) else {
        return Err("This session cannot use the MonoCode app CLI".into());
    };
    let key = format!("{}:{}", source.id, request.request_id);
    let signature =
        serde_json::to_string(&json!([request.action, request.input])).unwrap_or_default();
    let previous = executor.receipts.borrow().get(&key);
    if let Some((previous_signature, answer)) = previous {
        if previous_signature != signature {
            return Err("Request ID was already used with different input".into());
        }
        return answer.await;
    }
    let host = EngineAppHost {
        owner: owner.to_string(),
        source_cwd: source.cwd.clone(),
        peers: executor.peers.clone(),
        orchestrator: executor.orchestrator.clone(),
        kv: executor.kv.clone(),
    };
    let answer: SharedAnswer = {
        let (request_id, action) = (request.request_id.clone(), request.action.clone());
        cx.spawn(async move |cx| {
            handle_agent_app(&source, &request_id, &action, &input, &host, cx).await
        })
        .boxed_local()
        .shared()
    };
    executor
        .receipts
        .borrow_mut()
        .insert(key.clone(), signature, answer.clone());
    let result = answer.clone().await;
    if result.is_err() {
        executor.receipts.borrow_mut().remove_if(&key, &answer);
    }
    result
}

fn submit_config(cx: &App) -> Option<SubmitConfig> {
    Submit::try_global(cx).map(|submit| submit.read(cx).config().clone())
}

/// The App.tsx host for one app call: sessions in the caller's project,
/// launches in the caller's window, and the shared notes and worktrees.
pub struct EngineAppHost {
    /// The control owner whose window launches new sessions.
    pub owner: String,
    /// The calling session's project.
    pub source_cwd: String,
    pub peers: Rc<dyn OrchestrationPeers>,
    pub orchestrator: WeakEntity<Orchestrator>,
    pub kv: Kv,
}

fn open_or_stored(id: &str, cx: &mut App) -> Task<Option<Session>> {
    match Engine::sessions(cx).read(cx).get(id).cloned() {
        Some(open) => Task::ready(Some(open)),
        None => get_stored_session(id, cx),
    }
}

fn leads_run(orchestrator: &WeakEntity<Orchestrator>, id: &str, cx: &App) -> bool {
    orchestrator
        .upgrade()
        .is_some_and(|orchestrator| orchestrator.read(cx).run(id).is_some())
}

/// The checks `send` and `draft` share. `Ok(Some(..))` is the earlier answer
/// for a repeated request id.
fn reusable_target<T>(
    target: Option<Session>,
    source_cwd: &str,
    orchestrator: &WeakEntity<Orchestrator>,
    prompt: &str,
    request_id: &str,
    previous_answer: impl FnOnce(&monocode_core::Block) -> Result<T, String>,
    cx: &App,
) -> Result<Result<Session, T>, String> {
    let Some(target) = target.filter(|target| {
        target.orchestration_lead_id.is_none()
            && same_project_path(&target.cwd, source_cwd)
            && !leads_run(orchestrator, &target.id, cx)
    }) else {
        return Err("Session is unavailable in this project".into());
    };
    if let Some(previous) = target
        .blocks
        .iter()
        .find(|block| block.app_request_id.as_deref() == Some(request_id))
    {
        if previous.text != prompt {
            return Err("Request ID was already used with another prompt".into());
        }
        return previous_answer(previous).map(Err);
    }
    if target.is_busy() {
        return Err("Session is busy; try again when it finishes".into());
    }
    if session_draft_block(&target.blocks).is_some() {
        return Err("Session already has a draft; send or remove it first".into());
    }
    Ok(Ok(target))
}

impl AgentAppHost for EngineAppHost {
    fn start(
        &self,
        launch: AppLaunch,
        id: &str,
        placement: Option<AppSessionPlacement>,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let existing = open_or_stored(id, cx);
        let (peers, owner, id) = (self.peers.clone(), self.owner.clone(), id.to_string());
        cx.spawn(async move |cx| {
            let existing = existing.await;
            if let Some(previous) = existing.as_ref().and_then(|session| {
                session
                    .blocks
                    .iter()
                    .find(|block| block.app_request_id.as_deref() == Some(id.as_str()))
            }) {
                if previous.text != launch.prompt
                    || (launch.draft != Some(true) && previous.is_draft())
                {
                    return Err("Request ID was already used for another session launch".into());
                }
                return Ok(());
            }
            if existing.as_ref().is_some_and(|session| {
                session
                    .blocks
                    .iter()
                    .any(|block| block.role == monocode_core::BlockRole::User && !block.is_draft())
            }) {
                return Ok(());
            }
            if existing
                .as_ref()
                .is_some_and(|session| session_draft_block(&session.blocks).is_some())
            {
                return Err("Session ID already has a different draft".into());
            }
            cx.update(|cx| peers.launch_session(&owner, launch, &id, placement, cx))
                .await
        })
    }

    fn sessions(&self, cwd: &str, cx: &mut App) -> Task<Result<Vec<AppSessionListing>, String>> {
        let stored = Engine::writer(cx).list_sessions_by_project(cwd);
        let cwd = cwd.to_string();
        cx.spawn(async move |cx| {
            let stored = stored.await?;
            let mut listing: Vec<AppSessionListing> = Vec::new();
            let mut upsert = |entry: AppSessionListing| match listing
                .iter_mut()
                .find(|current| current.id == entry.id)
            {
                Some(current) => *current = entry,
                None => listing.push(entry),
            };
            for session in stored {
                if session.orchestration_lead_id.is_some() {
                    continue;
                }
                upsert(AppSessionListing {
                    id: session.id,
                    title: session.title,
                    harness: session.harness,
                    model: session.model,
                    busy: false,
                    has_draft: session.draft == Some(true),
                });
            }
            let open = cx.update(|cx| Engine::sessions(cx).read(cx).all().to_vec());
            for session in open {
                if session.orchestration_lead_id.is_some() || !same_project_path(&session.cwd, &cwd)
                {
                    continue;
                }
                upsert(AppSessionListing {
                    has_draft: session_draft_block(&session.blocks).is_some(),
                    busy: session.is_busy(),
                    id: session.id,
                    title: session.title,
                    harness: session.harness,
                    model: session.model,
                });
            }
            Ok(listing)
        })
    }

    fn session(&self, id: &str, cx: &mut App) -> Task<Result<Option<Session>, String>> {
        let target = open_or_stored(id, cx);
        let source_cwd = self.source_cwd.clone();
        cx.spawn(async move |_| {
            Ok(target.await.filter(|target| {
                target.orchestration_lead_id.is_none()
                    && same_project_path(&target.cwd, &source_cwd)
            }))
        })
    }

    fn send(
        &self,
        id: &str,
        prompt: &str,
        request_id: &str,
        cx: &mut App,
    ) -> Task<Result<SendResult, String>> {
        let opening = Engine::sessions(cx).update(cx, |sessions, cx| sessions.ensure_open(id, cx));
        let (source_cwd, orchestrator) = (self.source_cwd.clone(), self.orchestrator.clone());
        let (id, prompt, request_id) = (id.to_string(), prompt.to_string(), request_id.to_string());
        cx.spawn(async move |cx| {
            let target = opening.await;
            let checked = cx.update(|cx| {
                reusable_target(
                    target,
                    &source_cwd,
                    &orchestrator,
                    &prompt,
                    &request_id,
                    |previous| {
                        if previous.is_draft() {
                            Err("Request ID belongs to an unsent draft".to_string())
                        } else {
                            Ok(SendResult {
                                already_submitted: true,
                            })
                        }
                    },
                    cx,
                )
            })?;
            if let Err(previous) = checked {
                return Ok(previous);
            }
            let acceptance = cx.update(|cx| {
                Submit::try_global(cx).map(|submit| {
                    submit.update(cx, |submit, cx| {
                        submit.submit(
                            &id,
                            &prompt,
                            Vec::new(),
                            SubmitOptions {
                                app_request_id: Some(request_id.clone()),
                                ..Default::default()
                            },
                            cx,
                        )
                    })
                })
            });
            let accepted = match acceptance {
                Some(acceptance) => acceptance.resolve().await.map_err(|error| error.message)?,
                None => false,
            };
            if !accepted {
                return Err("Session could not accept the follow-up".into());
            }
            Ok(SendResult {
                already_submitted: false,
            })
        })
    }

    fn draft(
        &self,
        id: &str,
        prompt: &str,
        request_id: &str,
        cx: &mut App,
    ) -> Task<Result<DraftResult, String>> {
        let opening = Engine::sessions(cx).update(cx, |sessions, cx| sessions.ensure_open(id, cx));
        let (source_cwd, orchestrator) = (self.source_cwd.clone(), self.orchestrator.clone());
        let (id, prompt, request_id) = (id.to_string(), prompt.to_string(), request_id.to_string());
        cx.spawn(async move |cx| {
            let target = opening.await;
            let checked = cx.update(|cx| {
                reusable_target(
                    target,
                    &source_cwd,
                    &orchestrator,
                    &prompt,
                    &request_id,
                    |previous| {
                        Ok(DraftResult {
                            already_saved: true,
                            draft: previous.is_draft(),
                        })
                    },
                    cx,
                )
            })?;
            if let Err(previous) = checked {
                return Ok(previous);
            }
            let saved = cx.update(|cx| {
                Submit::try_global(cx).is_some_and(|submit| {
                    submit.update(cx, |submit, cx| {
                        submit.save_draft(&id, &prompt, Vec::new(), Some(request_id.clone()), cx)
                    })
                })
            });
            if !saved {
                return Err("Session could not accept a draft".into());
            }
            Ok(DraftResult {
                already_saved: false,
                draft: true,
            })
        })
    }

    fn worktrees(&self, cwd: &str, cx: &mut App) -> Task<Result<Worktrees, String>> {
        crate::projects::actions::list_worktrees(cwd, cx)
    }

    fn create_worktree(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
        cx: &mut App,
    ) -> Task<Result<Worktree, String>> {
        crate::projects::actions::create_worktree(cwd, branch, base, existing, cx)
    }

    fn notes(&self, cx: &mut App) -> Task<Result<Vec<Note>, String>> {
        match cx.try_global::<HistoryPackage>() {
            Some(history) => {
                let list = history.notes.read(cx).backend().list();
                cx.background_spawn(list)
            }
            None => Task::ready(Err("Notes are not available.".into())),
        }
    }

    fn note(&self, id: &str, cx: &mut App) -> Task<Result<Option<Note>, String>> {
        match cx.try_global::<HistoryPackage>() {
            Some(history) => {
                let notes = history.notes.clone();
                notes.update(cx, |notes, cx| notes.get_note(id, cx))
            }
            None => Task::ready(Err("Notes are not available.".into())),
        }
    }

    fn save_note(&self, note: NoteUpsert, cx: &mut App) -> Task<Result<Note, String>> {
        let Some(notes) = cx
            .try_global::<HistoryPackage>()
            .map(|history| history.notes.clone())
        else {
            return Task::ready(Err("Notes are not available.".into()));
        };
        let upsert = notes.update(cx, |entity, cx| entity.upsert_note(note, cx));
        cx.spawn(async move |cx| {
            let saved = upsert.await?;
            notes.update(cx, |notes, cx| notes.notes_changed(cx));
            Ok(saved)
        })
    }

    fn catalog(&self, cx: &App) -> ModelCatalog {
        submit_config(cx)
            .map(|config| config.catalog.snapshot())
            .unwrap_or_default()
    }

    fn is_harness_available(&self, harness: HarnessId, cx: &App) -> bool {
        submit_config(cx).is_some_and(|config| (config.is_harness_available)(harness))
    }

    fn preferred_model_id(&self, harness: HarnessId, cx: &App) -> String {
        let catalog = self.catalog(cx);
        let prefs = submit_config(cx)
            .map(|config| ModelPrefs::from_local_storage(|key| config.kv.get_item(key)))
            .unwrap_or_default();
        let availability = HarnessAvailability::default();
        let projects = ProjectProviders::default();
        ModelEnv {
            catalog: &catalog,
            prefs: &prefs,
            availability: &availability,
            projects: &projects,
        }
        .preferred_model_id(harness)
    }

    fn kv(&self) -> Kv {
        self.kv.clone()
    }
}
