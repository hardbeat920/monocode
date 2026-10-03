//! The package global and the wiring App.tsx did with effects and refs:
//! `orchestrator.bind`, `orchestrator.sync()` on every sessions change,
//! `attachOrchestrationWorkers` on every runs change, the runtime and submit
//! hooks, and the orchestration card actions (App.tsx 9300-9411).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, Entity, Global, Subscription, Task, WeakEntity};
use monocode_core::block::TurnIntent;
use monocode_core::orchestration::{
    OrchestrationProposal, OrchestrationProposalStatus, OrchestrationSettings,
    with_orchestration_proposal,
};
use monocode_core::{Extra, HarnessEvent, Session};
use monocode_process::control::ControlHost;
use monocode_process::harness::HarnessHost;
use monocode_settings::Kv;
use monocode_store::session_store::SessionStore;

use super::catalog::discover_orchestration_settings;
use super::engine_host::EngineHost;
use super::executor::{AppReceipts, ExecutorContext};
use super::host::OrchestrationStorage;
use super::orchestrator::{self, Orchestrator, hydrate, start_approved};
use super::peers::{NoPeers, OrchestrationPeers};
use super::plan::{
    complete_orchestration_proposal, orchestration_planning_prompt, orchestration_repair_prompt,
    validate_settings,
};
use super::storage::NativeStorage;
use crate::runtime::{Engine, OrchestrationHooks};
use crate::submit::hooks::SubmitOrchestrationHooks;
use crate::submit::{Submit, SubmitOptions};

/// What the package needs from the app.
pub struct OrchestrationConfig {
    pub storage: Rc<dyn OrchestrationStorage>,
    /// The control server, for worker grants and turn release.
    pub control: Option<Arc<ControlHost>>,
    /// The harness process host, for `harness_kill`.
    pub harness_host: Option<HarnessHost>,
    /// The owner id control grants are issued to.
    pub owner: String,
    /// The settings store (session folders).
    pub kv: Kv,
    pub peers: Rc<dyn OrchestrationPeers>,
}

impl OrchestrationConfig {
    /// Storage over `monocode.db` and the control server.
    pub fn native(
        store: Arc<SessionStore>,
        control: Arc<ControlHost>,
        harness_host: Option<HarnessHost>,
        owner: impl Into<String>,
        kv: Kv,
    ) -> Self {
        let owner = owner.into();
        Self {
            storage: Rc::new(NativeStorage {
                store,
                control: control.clone(),
                owner: owner.clone(),
            }),
            control: Some(control),
            harness_host,
            owner,
            kv,
            peers: Rc::new(NoPeers),
        }
    }
}

/// The package global.
pub struct Orchestration {
    orchestrator: Entity<Orchestrator>,
    executor: ExecutorContext,
    confirming: Rc<RefCell<HashSet<String>>>,
    _subscriptions: Vec<Subscription>,
}

impl Global for Orchestration {}

impl Orchestration {
    /// Create the orchestrator, bind it to the engine, and install the
    /// runtime and submit hooks. Call after `Engine::init` and `Submit::init`.
    pub fn init(config: OrchestrationConfig, cx: &mut App) -> Entity<Orchestrator> {
        let orchestrator = cx.new(|_| Orchestrator::new(config.storage.clone()));
        let host = Rc::new(EngineHost {
            control: config.control.clone(),
            harness_host: config.harness_host.clone(),
            owner: config.owner.clone(),
            peers: config.peers.clone(),
        });
        orchestrator.update(cx, |orchestrator, _| orchestrator.bind(Rc::new(host)));
        let weak = orchestrator.downgrade();
        Engine::set_hooks(cx, |hooks| {
            hooks.orchestration = Rc::new(RuntimeHooks(weak.clone()));
        });
        if let Some(submit) = Submit::try_global(cx) {
            let hooks = Rc::new(SubmitHooks {
                orchestrator: weak.clone(),
                peers: config.peers.clone(),
            });
            submit.update(cx, |submit, _| {
                submit.set_peers(|peers| peers.orchestration = hooks);
            });
        }
        let sessions = Engine::sessions(cx);
        let sync = {
            let weak = weak.clone();
            cx.observe(&sessions, move |_, cx| {
                if let Some(orchestrator) = weak.upgrade() {
                    orchestrator.update(cx, |orchestrator, cx| orchestrator.sync(cx));
                }
            })
        };
        let attach = cx.observe(&orchestrator, |orchestrator, cx| {
            attach_workers(&orchestrator, cx);
        });
        cx.set_global(Orchestration {
            orchestrator: orchestrator.clone(),
            executor: ExecutorContext {
                orchestrator: weak,
                peers: config.peers,
                receipts: Rc::new(RefCell::new(AppReceipts::default())),
                kv: config.kv,
            },
            confirming: Rc::default(),
            _subscriptions: vec![sync, attach],
        });
        orchestrator
    }

    pub fn global(cx: &App) -> &Orchestration {
        cx.global::<Orchestration>()
    }

    pub fn try_global(cx: &App) -> Option<&Orchestration> {
        cx.try_global::<Orchestration>()
    }

    /// The app's orchestrator.
    pub fn orchestrator(cx: &App) -> Entity<Orchestrator> {
        Self::global(cx).orchestrator.clone()
    }

    pub(crate) fn executor(&self) -> ExecutorContext {
        self.executor.clone()
    }

    /// `orchestrator.deleteSession`: drain control writes, then run `remove`.
    pub fn delete_session<F>(
        id: &str,
        remove: impl FnOnce() -> F + 'static,
        cx: &mut App,
    ) -> Task<Result<(), String>>
    where
        F: std::future::Future<Output = Result<(), String>> + 'static,
    {
        let weak = Self::orchestrator(cx).downgrade();
        let id = id.to_string();
        cx.spawn(async move |cx| orchestrator::delete_session(&weak, &id, remove, cx).await)
    }

    /// `orchestrator.hydrate` for a lead the user opened.
    pub fn hydrate(id: &str, cx: &mut App) -> Task<Result<(), String>> {
        let weak = Self::orchestrator(cx).downgrade();
        let id = id.to_string();
        cx.spawn(async move |cx| hydrate(&weak, &id, cx).await)
    }

    /// Resume a paused run, or start an ordinary one.
    pub fn resume(
        lead_id: &str,
        harnesses: Vec<monocode_core::HarnessId>,
        max_workers: i64,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let weak = Self::orchestrator(cx).downgrade();
        let lead_id = lead_id.to_string();
        cx.spawn(async move |cx| {
            orchestrator::start(&weak, &lead_id, &harnesses, max_workers, None, cx).await
        })
    }

    /// `orchestrator.stopRun`.
    pub fn stop_run(lead_id: &str, cx: &mut App) -> Task<Result<(), String>> {
        let weak = Self::orchestrator(cx).downgrade();
        let lead_id = lead_id.to_string();
        cx.spawn(async move |cx| orchestrator::stop_run(&weak, &lead_id, cx).await)
    }
}

/// The `attachOrchestrationWorkers` effect: mark worker sessions with their
/// lead whenever runs change.
fn attach_workers(orchestrator: &Entity<Orchestrator>, cx: &mut App) {
    let mut parents: HashMap<String, String> = HashMap::new();
    for run in orchestrator.read(cx).snapshot() {
        for task in &run.tasks {
            parents.insert(task.session_id.clone(), run.lead_id.clone());
        }
    }
    if parents.is_empty() {
        return;
    }
    Engine::sessions(cx).update(cx, |sessions, cx| {
        sessions.update_all(cx, |session| {
            let parent = parents.get(&session.id)?;
            (session.orchestration_lead_id.as_ref() != Some(parent)).then(|| Session {
                orchestration_lead_id: Some(parent.clone()),
                ..session.clone()
            })
        });
    });
}

/// The runtime's `OrchestrationHooks`.
struct RuntimeHooks(WeakEntity<Orchestrator>);

impl OrchestrationHooks for RuntimeHooks {
    fn stop_for_session(&self, session_id: &str, cx: &mut App) -> Task<()> {
        let Some(entity) = self.0.upgrade() else {
            return Task::ready(());
        };
        match orchestrator::stop_for_session(&entity, session_id, cx) {
            Some(stopping) => cx.spawn(async move |_| {
                if let Err(error) = stopping.await {
                    log::error!("[orchestration] stop failed: {error}");
                }
            }),
            None => Task::ready(()),
        }
    }

    fn running_lead_ids(&self, cx: &App) -> HashSet<String> {
        self.0
            .upgrade()
            .map(|entity| entity.read(cx).running_lead_ids())
            .unwrap_or_default()
    }
}

/// Submit's orchestration hooks.
struct SubmitHooks {
    orchestrator: WeakEntity<Orchestrator>,
    peers: Rc<dyn OrchestrationPeers>,
}

impl SubmitOrchestrationHooks for SubmitHooks {
    fn submission_error(&self, session_id: &str, managed: bool, cx: &App) -> Option<String> {
        self.orchestrator
            .upgrade()?
            .read(cx)
            .submission_error(session_id, managed, cx)
    }

    fn led_run_status(&self, session_id: &str, cx: &App) -> Option<String> {
        let run = self.orchestrator.upgrade()?.read(cx).run(session_id)?;
        Some(run.status.as_str().to_string())
    }

    fn run_status_for_session(&self, session_id: &str, cx: &App) -> Option<String> {
        let run = self
            .orchestrator
            .upgrade()?
            .read(cx)
            .for_session(session_id)?;
        Some(run.status.as_str().to_string())
    }

    fn observe(&self, session_id: &str, event: &HarnessEvent, cx: &mut App) {
        if let Some(entity) = self.orchestrator.upgrade() {
            entity.update(cx, |orchestrator, cx| {
                orchestrator.observe(session_id, event, cx)
            });
        }
    }

    fn prompt(&self, session_id: &str, text: String, cx: &App) -> String {
        match self.orchestrator.upgrade() {
            Some(entity) => entity.read(cx).prompt(session_id, &text),
            None => text,
        }
    }

    fn stop_for_session(&self, session_id: &str, cx: &mut App) -> Option<Task<()>> {
        let entity = self.orchestrator.upgrade()?;
        let stopping = orchestrator::stop_for_session(&entity, session_id, cx)?;
        Some(cx.spawn(async move |_| {
            if let Err(error) = stopping.await {
                log::error!("[orchestration] stop failed: {error}");
            }
        }))
    }

    fn discover_settings(&self, cx: &mut App) -> Task<OrchestrationSettings> {
        let probe = self.peers.probe_availability(cx);
        let config = Submit::try_global(cx).map(|submit| submit.read(cx).config().clone());
        cx.spawn(async move |_| {
            let Some(config) = config else {
                return empty_settings();
            };
            let registry = config.registry.clone();
            let catalog = config.catalog.clone();
            let live = config.catalog.clone();
            let available = config.is_harness_available.clone();
            let discovered = discover_orchestration_settings(
                || probe,
                |id| available(id),
                |ids| async move {
                    registry
                        .refresh_harness_catalogs(ids, false, |id| live.has_live_catalog(id))
                        .await
                },
                || catalog.snapshot(),
            )
            .await;
            // TODO(port): discoverOrchestrationSettings threw here and the
            // turn failed; submit's hook has no error path, so an empty
            // catalog fails the proposal's validation instead.
            discovered.unwrap_or_else(|error| {
                log::warn!("[orchestration] {error}");
                empty_settings()
            })
        })
    }

    fn planning_prompt(&self, prompt: &str, settings: &OrchestrationSettings, cwd: &str) -> String {
        orchestration_planning_prompt(prompt, settings, cwd)
    }

    fn repair_prompt(&self, proposal: &OrchestrationProposal) -> String {
        orchestration_repair_prompt(proposal)
    }

    fn complete_proposal(
        &self,
        draft: &OrchestrationProposal,
        response: &str,
        error: Option<&str>,
    ) -> OrchestrationProposal {
        complete_orchestration_proposal(draft, response, error)
    }
}

fn empty_settings() -> OrchestrationSettings {
    OrchestrationSettings {
        choices: Vec::new(),
        max_workers: 2,
        extra: Extra::new(),
    }
}

/// `updateOrchestrationCard`: put a proposal on the lead's plan block.
fn update_card(
    lead_id: &str,
    block_id: &str,
    proposal: &OrchestrationProposal,
    cx: &mut App,
) -> Option<Session> {
    let sessions = Engine::sessions(cx);
    sessions.update(cx, |sessions, cx| {
        sessions.update(lead_id, cx, |session| {
            *session = with_orchestration_proposal(session, block_id, proposal);
        });
    });
    sessions.read(cx).get(lead_id).cloned()
}

fn card_proposal(
    lead_id: &str,
    block_id: &str,
    cx: &App,
) -> Option<(Session, OrchestrationProposal)> {
    let session = Engine::sessions(cx).read(cx).get(lead_id).cloned()?;
    let proposal = session
        .blocks
        .iter()
        .find(|block| block.id == block_id)?
        .orchestration
        .clone()?;
    Some((session, proposal))
}

/// `orchestrationActions.update`: keep the discovered catalog authoritative
/// while allowing task and parallelism edits.
pub fn update_orchestration_card(
    lead_id: &str,
    block_id: &str,
    edited: &OrchestrationProposal,
    cx: &mut App,
) -> Result<(), String> {
    let Some((session, proposal)) = card_proposal(lead_id, block_id, cx) else {
        return Ok(());
    };
    let confirming = Orchestration::try_global(cx)
        .is_some_and(|package| package.confirming.borrow().contains(lead_id));
    if session.is_busy() || proposal.status != OrchestrationProposalStatus::Ready || confirming {
        return Ok(());
    }
    let mut settings = proposal.settings.clone();
    settings.max_workers = edited.settings.max_workers;
    let settings = validate_settings(&settings)?;
    update_card(
        lead_id,
        block_id,
        &OrchestrationProposal {
            settings,
            tasks: edited.tasks.clone(),
            ..proposal
        },
        cx,
    );
    Ok(())
}

/// `orchestrationActions.confirm`: save the edited card, then start exactly
/// its assignments.
pub fn confirm_orchestration_card(
    lead_id: &str,
    block_id: &str,
    cx: &mut App,
) -> Task<Result<(), String>> {
    let Some(package) = Orchestration::try_global(cx) else {
        return Task::ready(Err("Orchestration is not available.".into()));
    };
    let confirming = package.confirming.clone();
    if !confirming.borrow_mut().insert(lead_id.to_string()) {
        return Task::ready(Ok(()));
    }
    let weak = package.orchestrator.downgrade();
    let (lead_id, block_id) = (lead_id.to_string(), block_id.to_string());
    cx.spawn(async move |cx| {
        let mut proposal: Option<OrchestrationProposal> = None;
        let result: Result<(), String> = async {
            hydrate(&weak, &lead_id, cx).await?;
            let found = cx.update(|cx| card_proposal(&lead_id, &block_id, cx));
            proposal = found.as_ref().map(|(_, proposal)| proposal.clone());
            let Some((session, ready)) = found.filter(|(session, proposal)| {
                !session.is_busy() && proposal.status == OrchestrationProposalStatus::Ready
            }) else {
                return Err("Wait for the proposal to finish before confirming.".into());
            };
            if session.harness != ready.author.harness || session.model != ready.author.model {
                return Err("The lead model has changed. Switch back to the model shown on this card, or generate a new proposal.".into());
            }
            let starting = cx.update(|cx| {
                update_card(
                    &lead_id,
                    &block_id,
                    &OrchestrationProposal {
                        status: OrchestrationProposalStatus::Starting,
                        ..ready.clone()
                    },
                    cx,
                )
            });
            // Save the edited card before anything can execute.
            if let Some(starting) = starting {
                cx.update(|cx| Engine::writer(cx).upsert_session(&starting))
                    .await?;
            }
            start_approved(&weak, &lead_id, &block_id, &ready, cx).await?;
            cx.update(|cx| {
                update_card(
                    &lead_id,
                    &block_id,
                    &OrchestrationProposal {
                        status: OrchestrationProposalStatus::Approved,
                        ..ready
                    },
                    cx,
                )
            });
            Ok(())
        }
        .await;
        // TODO(port): this also turns a card that was still planning into a
        // ready one when confirm is refused, as the TypeScript did.
        if result.is_err()
            && let Some(proposal) = proposal
        {
            cx.update(|cx| {
                update_card(
                    &lead_id,
                    &block_id,
                    &OrchestrationProposal {
                        status: OrchestrationProposalStatus::Ready,
                        ..proposal
                    },
                    cx,
                )
            });
        }
        confirming.borrow_mut().remove(&lead_id);
        result
    })
}

/// `orchestrationActions.retry`: ask the lead for the card again.
pub fn retry_orchestration_card(lead_id: &str, block_id: &str, cx: &mut App) {
    let Some((session, proposal)) = card_proposal(lead_id, block_id, cx) else {
        return;
    };
    if session.is_busy() {
        return;
    }
    let Some(submit) = Submit::try_global(cx) else {
        return;
    };
    let request = proposal.request.clone();
    submit.update(cx, |submit, cx| {
        submit.on_submit(
            lead_id,
            &request,
            Vec::new(),
            SubmitOptions {
                intent: Some(TurnIntent::Orchestrate),
                orchestration_retry: Some(proposal),
                ..Default::default()
            },
            cx,
        );
    });
}
