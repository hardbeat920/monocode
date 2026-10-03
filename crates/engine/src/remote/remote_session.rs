//! Port of the data hooks in src/features/connections/ui/RemoteSession.tsx
//! (`ConnectedRemoteSession`) as the `RemoteSession` entity: one per open
//! tab in a project on another machine.
//!
//! The host owns the session. This entity polls its snapshot (and reloads at
//! once on pushed changes), dispatches commands with an outbox that survives
//! restarts, shows unconfirmed turns optimistically, applies model and
//! permission changes when the session is idle, and resumes after a usage
//! limit. Views read `session()` and the other accessors and call the action
//! methods; `RemoteSessions` merges each new snapshot into `Sessions`.
//!
//! React effects become `reconcile`, which runs after every state change in
//! the same order the effects ran.

use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Context, Entity, EventEmitter, Subscription, Task};
use monocode_core::block::{TurnIntent, TurnModel};
use monocode_core::harness_event::ApprovalDecision as HarnessDecision;
use monocode_core::models::ModelSetting;
use monocode_core::session::{UsageLimit, WorkspaceMode, usage_limit_resume_due};
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{
    AgentModel, Attachment, Block, BlockRole, Extra, HarnessId, ModelSettings, RuntimeMode, Session,
};
use monocode_remote::host::protocol::{
    ApprovalDecision, CommandReceipt, HostCommand, HostDescriptor, HostModelCatalog, HostSession,
    HostSessionStatus, HostWorktree, REMOTE_PROVIDERS, RemoteAttachment, RemoteMachine,
    RemoteProvider, SendIntent, provider_name, require_host_descriptor,
};
use serde_json::{Value, json};

use super::client::{RemoteClient, decode};
use super::connections::{
    self, PendingScope, clear_pending_remote_command, pending_remote_command,
    pending_remote_followup, save_pending_remote_command,
};
use super::remote_connections::{LimitChoice, RemoteConnections, RemoteEvent, snapshot_key};
use super::remote_models::{
    carry_model_settings, find_remote_model, remote_model_controls, same_model_settings,
};
use super::remote_projects::{RemoteProject, parse_remote_path, remote_path};
use super::remote_turns::RemoteChangesDetail;
use super::transport::RemoteFuture;
use crate::runtime::engine::Engine;
use crate::runtime::in_flight::CONTINUE_PROMPT;

/// `Configuration`: the model, settings, and permission mode a session uses
/// or will use.
#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub harness: RemoteProvider,
    pub model: String,
    pub settings: ModelSettings,
    pub mode: RuntimeMode,
}

/// `same` in the configure effect: the harness is fixed once a session
/// exists, so it is not compared.
fn same_configuration(a: &Configuration, b: &Configuration) -> bool {
    a.model == b.model
        && a.mode == b.mode
        && same_model_settings(Some(&a.settings), Some(&b.settings))
}

/// `OptimisticTurn`: a message shown before the host confirms it.
#[derive(Debug, Clone, PartialEq)]
pub struct OptimisticTurn {
    pub command_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub intent: SendIntent,
    pub draft: bool,
    pub draft_block_id: Option<String>,
    pub plan_block_id: Option<String>,
    pub started_at: i64,
    pub turn_model: TurnModel,
}

/// A first message waiting while its session is created, or a turn that
/// failed to send.
#[derive(Debug, Clone, PartialEq)]
struct Starting {
    turn: OptimisticTurn,
    failed: bool,
}

/// A send the host accepted but no sync has shown yet.
#[derive(Debug, Clone, PartialEq)]
struct UnseenSend {
    session_id: String,
    command_id: String,
    text: String,
    attachments: Vec<Attachment>,
    started_at: i64,
    turn_model: TurnModel,
    draft_block_id: Option<String>,
}

/// The composer options a remote turn honors (`ComposerTurnOptions`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteTurnOptions {
    pub intent: Option<TurnIntent>,
    pub draft_block_id: Option<String>,
}

/// What the notice bar's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// "Retry": send the unconfirmed command again with its original ID.
    RetryPending,
    /// "Try again": send a failed turn again.
    TryAgain,
    /// "Dismiss": clear the error.
    Dismiss,
    /// "Retry": load the host's models again.
    RetryCatalog,
}

impl NoticeAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::RetryPending | Self::RetryCatalog => "Retry",
            Self::TryAgain => "Try again",
            Self::Dismiss => "Dismiss",
        }
    }
}

/// The notice bar above a remote session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteNotice {
    pub text: String,
    pub detail: Option<String>,
    pub action: NoticeAction,
    /// `role="alert"` rather than `status`.
    pub alert: bool,
    /// The button works only while the machine is online, except Dismiss.
    pub enabled: bool,
}

/// What `notice` is made from: the outbox, the failed turn, the last error,
/// and the catalog problem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteSessionStatus {
    /// A command waits for the host to confirm it.
    pub pending: bool,
    /// A command is being dispatched now.
    pub sending: bool,
    /// A turn failed before the host accepted it; `true` when it was a
    /// draft.
    pub failed_draft: Option<bool>,
    /// The last request's error, without an `Error: ` prefix.
    pub error: String,
    /// `catalog.errors[harness] ?? catalogError`.
    pub catalog_problem: String,
}

/// Composer menu actions the host advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemoteFeatures {
    pub attachments: bool,
    pub plan: bool,
    pub draft: bool,
}

/// Emitted for `RemoteSessions`, which merges host snapshots into
/// `Sessions` (App.tsx `onRemoteSnapshot`).
#[derive(Debug, Clone)]
pub enum RemoteSessionEvent {
    /// A host snapshot arrived, or the session was discarded (`None`).
    Snapshot(Option<Arc<HostSession>>),
}

/// The usage limit effect's dependencies: `resumeAtReset`, `resetsAt`,
/// busy, online, the pending command, and the re-check tick.
type LimitEffectKey = (Option<bool>, Option<i64>, bool, bool, Option<String>, u64);

/// One open tab in a remote project.
pub struct RemoteSession {
    connections: Entity<RemoteConnections>,
    client: RemoteClient,
    kv: monocode_settings::Kv,
    /// The tab's local session, which gives its ID and new-session defaults.
    shell: Session,
    machine: RemoteMachine,
    project: RemoteProject,
    visible: bool,
    descriptor: Option<HostDescriptor>,
    online: bool,
    error: String,
    session_id: Option<String>,
    bound_session: Option<String>,
    binding_version: u64,
    deleting_session: Option<String>,
    snapshot: Option<Arc<HostSession>>,
    catalog: Option<HostModelCatalog>,
    catalog_error: String,
    selected_cwd: String,
    draft_workspace_mode: WorkspaceMode,
    draft_worktree_base: String,
    sending: bool,
    preparing: bool,
    unseen_send: Option<UnseenSend>,
    starting: Option<Starting>,
    pending: Option<HostCommand>,
    draft: Configuration,
    changes: Option<Configuration>,
    removing_draft: Option<String>,
    applying: bool,
    applied: Option<Configuration>,
    /// The branch the projects package reports for the execution checkout
    /// (`useProjectBranchesState`), fed in by the view.
    current_branch: Option<String>,
    // Effects.
    poll: Option<Task<()>>,
    poll_epoch: u64,
    in_flight: bool,
    again: bool,
    wake: Option<async_channel::Sender<()>>,
    catalog_task: Option<Task<()>>,
    last_host_id: Option<String>,
    limit_effect: Option<LimitEffectKey>,
    limit_tick: u64,
    limit_timer: Option<Task<()>>,
    reconciling: bool,
    dirty: bool,
    _changes: Subscription,
    _events: Subscription,
}

impl EventEmitter<RemoteSessionEvent> for RemoteSession {}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `temporaryWorktreeBranchName` from source-control/model/worktrees.ts.
fn temporary_worktree_branch_name(id: &str, now: i64) -> String {
    let token: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect::<String>()
        .to_lowercase();
    if token.is_empty() {
        format!("mc/{}", radix36(now))
    } else {
        format!("mc/{token}")
    }
}

/// `Date.now().toString(36)`.
fn radix36(mut value: i64) -> String {
    if value <= 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(std::char::from_digit((value % 36) as u32, 36).unwrap_or('0'));
        value /= 36;
    }
    digits.iter().rev().collect()
}

/// `/^[^:]+:/`: the model id without its harness prefix.
fn strip_model_prefix(id: &str) -> &str {
    match id.find(':') {
        Some(colon) if colon > 0 => &id[colon + 1..],
        _ => id,
    }
}

/// `/^[a-z]+:/`.
fn strip_harness_prefix(id: &str) -> &str {
    match id.find(':') {
        Some(colon) if colon > 0 && id[..colon].bytes().all(|byte| byte.is_ascii_lowercase()) => {
            &id[colon + 1..]
        }
        _ => id,
    }
}

/// `limitKey`.
fn limit_key(limit: &UsageLimit) -> String {
    limit
        .resets_at
        .map_or_else(|| "unknown".to_string(), |resets_at| resets_at.to_string())
}

fn send_intent(intent: Option<TurnIntent>) -> SendIntent {
    match intent {
        Some(TurnIntent::Plan) => SendIntent::Plan,
        Some(TurnIntent::Build) => SendIntent::Build,
        _ => SendIntent::Default,
    }
}

/// `message`: a send, or `/compact` on its own as a compact command.
#[allow(clippy::too_many_arguments)]
fn message(
    session_id: &str,
    text: &str,
    command_id: String,
    attachments: Vec<RemoteAttachment>,
    intent: SendIntent,
    draft_block_id: Option<String>,
    plan_block_id: Option<String>,
) -> HostCommand {
    if text.trim().to_lowercase() == "/compact"
        && attachments.is_empty()
        && draft_block_id.is_none()
        && intent == SendIntent::Default
    {
        return HostCommand::Compact {
            command_id,
            session_id: session_id.to_string(),
        };
    }
    HostCommand::Send {
        command_id,
        session_id: session_id.to_string(),
        text: text.to_string(),
        attachments: Some(attachments),
        intent: Some(intent),
        draft_block_id,
        plan_block_id,
    }
}

/// `{ ...command, sessionId }`.
fn with_session_id(command: &HostCommand, id: &str) -> HostCommand {
    let mut command = command.clone();
    match &mut command {
        HostCommand::Create { .. } => {}
        HostCommand::Configure { session_id, .. }
        | HostCommand::Compact { session_id, .. }
        | HostCommand::Send { session_id, .. }
        | HostCommand::Draft { session_id, .. }
        | HostCommand::RemoveDraft { session_id, .. }
        | HostCommand::Cancel { session_id, .. }
        | HostCommand::Approve { session_id, .. }
        | HostCommand::Answer { session_id, .. } => *session_id = id.to_string(),
    }
    command
}

fn command_session_id(command: &HostCommand) -> Option<&str> {
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

/// `String(reason).replace(/^Error: /, "")`.
fn error_text(reason: &str) -> String {
    reason.strip_prefix("Error: ").unwrap_or(reason).to_string()
}

impl RemoteSession {
    /// A tab bound to `machine` and `project`. `RemoteSessions::open` creates
    /// these; it also forwards their snapshots.
    pub fn new(
        connections: Entity<RemoteConnections>,
        shell: Session,
        machine: RemoteMachine,
        project: RemoteProject,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (client, kv, descriptor, catalog) = {
            let connections = connections.read(cx);
            (
                connections.client().clone(),
                connections.kv().clone(),
                connections.descriptor(&machine.id).cloned(),
                connections
                    .catalog(&machine.id, &project.project_id)
                    .cloned(),
            )
        };
        let session_id = connections::remote_session_for(&kv, &shell.id);
        let snapshot = session_id
            .as_deref()
            .and_then(|id| connections.read(cx).cached_snapshot(&machine.id, id));
        let selected_cwd = connections::remote_pending_worktree(&kv, &shell.id)
            .unwrap_or_else(|| project.cwd.clone());
        let pending = pending_remote_command(
            &kv,
            &project.key,
            &machine.environment_id,
            PendingScope::for_session(session_id.as_deref()),
            Some(&shell.id),
        );
        let draft = Configuration {
            harness: shell.harness,
            model: shell.model.clone(),
            settings: shell.model_settings.clone(),
            mode: shell.runtime_mode,
        };
        let changes = cx.subscribe(&connections, |this, _, event, cx| match event {
            RemoteEvent::HistoryChange => this.binding_changed(cx),
            RemoteEvent::Changes(detail) => this.host_changed(detail, cx),
            _ => {}
        });
        let watch = connections.update(cx, |connections, cx| {
            connections.watch_remote_changes(&machine.id, cx)
        });
        let mut this = Self {
            connections,
            client,
            kv,
            shell,
            visible,
            descriptor,
            online: false,
            error: String::new(),
            bound_session: session_id.clone(),
            session_id,
            binding_version: 0,
            deleting_session: None,
            snapshot,
            catalog,
            catalog_error: String::new(),
            selected_cwd,
            draft_workspace_mode: WorkspaceMode::Current,
            draft_worktree_base: "HEAD".into(),
            sending: false,
            preparing: false,
            unseen_send: None,
            starting: None,
            pending,
            draft,
            changes: None,
            removing_draft: None,
            applying: false,
            applied: None,
            current_branch: None,
            poll: None,
            poll_epoch: 0,
            in_flight: false,
            again: false,
            wake: None,
            catalog_task: None,
            last_host_id: None,
            limit_effect: None,
            limit_tick: 0,
            limit_timer: None,
            reconciling: false,
            dirty: false,
            _changes: changes,
            _events: watch,
            machine,
            project,
        };
        this.binding_changed(cx);
        this.restart_poll(cx);
        if this.descriptor.is_some() {
            this.load_catalog(cx);
        }
        this.changed(cx);
        this
    }

    // Reading.

    pub fn shell_id(&self) -> &str {
        &self.shell.id
    }

    pub fn machine(&self) -> &RemoteMachine {
        &self.machine
    }

    pub fn project(&self) -> &RemoteProject {
        &self.project
    }

    pub fn descriptor(&self) -> Option<&HostDescriptor> {
        self.descriptor.as_ref()
    }

    pub fn online(&self) -> bool {
        self.online
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn snapshot(&self) -> Option<&Arc<HostSession>> {
        self.snapshot.as_ref()
    }

    pub fn catalog(&self) -> Option<&HostModelCatalog> {
        self.catalog.as_ref()
    }

    pub fn pending(&self) -> Option<&HostCommand> {
        self.pending.as_ref()
    }

    pub fn sending(&self) -> bool {
        self.sending
    }

    pub fn draft_configuration(&self) -> &Configuration {
        &self.draft
    }

    /// The host session this tab shows, once a sync has arrived.
    pub fn host_session(&self) -> Option<&Session> {
        self.snapshot
            .as_ref()
            .filter(|snapshot| Some(snapshot.session.id.as_str()) == self.session_id.as_deref())
            .map(|snapshot| &snapshot.session)
    }

    /// `executionCwd`: the host checkout the session runs in.
    pub fn execution_cwd(&self) -> &str {
        self.host_session()
            .map_or(self.selected_cwd.as_str(), |host| host.cwd.as_str())
    }

    /// The execution checkout as a `remote://` path, for the branch state.
    pub fn execution_path(&self) -> String {
        remote_path(&self.machine.environment_id, self.execution_cwd())
    }

    fn active_session_id(&self) -> Option<&str> {
        self.host_session()
            .map(|host| host.id.as_str())
            .or(self.session_id.as_deref())
    }

    fn has_host_block(&self, command_id: &str) -> bool {
        self.host_session()
            .is_some_and(|host| host.blocks.iter().any(|block| block.id == command_id))
    }

    fn unseen_active(&self) -> bool {
        self.unseen_send.as_ref().is_some_and(|unseen| {
            Some(unseen.session_id.as_str()) == self.active_session_id()
                && !self.has_host_block(&unseen.command_id)
        })
    }

    fn starting_active(&self) -> bool {
        self.starting.as_ref().is_some_and(|starting| {
            !starting.failed && !self.has_host_block(&starting.turn.command_id)
        })
    }

    fn pending_send_active(&self) -> bool {
        match &self.pending {
            Some(command @ (HostCommand::Send { .. } | HostCommand::Compact { .. })) => {
                command_session_id(command) == self.active_session_id()
                    && !self.has_host_block(connections::command_id(command))
            }
            _ => false,
        }
    }

    /// A turn runs on the host, or one this tab sent is not confirmed yet.
    pub fn busy(&self) -> bool {
        self.host_session().is_some_and(Session::is_busy)
            || self.unseen_active()
            || (self.starting_active() && !self.starting.as_ref().is_some_and(|s| s.turn.draft))
            || self.pending_send_active()
    }

    fn saved(&self) -> Option<Configuration> {
        self.host_session().map(|host| Configuration {
            harness: host.harness,
            model: host.model.clone(),
            settings: host.model_settings.clone(),
            mode: host.runtime_mode,
        })
    }

    /// The model, settings, and mode the composer shows: unsent changes to a
    /// started session, its saved values, or the new session's draft.
    pub fn configuration(&self) -> Configuration {
        match self.saved() {
            Some(saved) => self.changes.clone().unwrap_or(saved),
            None => self.draft.clone(),
        }
    }

    /// The host's providers, in its order.
    pub fn providers(&self) -> Vec<RemoteProvider> {
        self.descriptor
            .as_ref()
            .map(|descriptor| descriptor.providers.clone())
            .unwrap_or_default()
    }

    /// `remoteFeatures`.
    pub fn features(&self) -> RemoteFeatures {
        let has = |capability: &str| {
            self.descriptor.as_ref().is_some_and(|descriptor| {
                descriptor
                    .capabilities
                    .iter()
                    .any(|entry| entry == capability)
            })
        };
        RemoteFeatures {
            attachments: has("attachments.upload"),
            plan: has("sessions.plan"),
            draft: has("sessions.draft"),
        }
    }

    /// `allowedModelHarnesses`.
    pub fn allowed_model_harnesses(&self) -> Vec<HarnessId> {
        if let Some(host) = self.host_session() {
            return vec![host.harness];
        }
        let providers = self.providers();
        if providers.is_empty() {
            vec![HarnessId::Codex, HarnessId::Claude]
        } else {
            providers
        }
    }

    /// `remoteSessionStarted`.
    pub fn started(&self) -> bool {
        self.session_id.is_some()
    }

    /// `remoteSessionLoading`: an opened host conversation whose transcript
    /// has not arrived yet.
    pub fn loading(&self, cx: &App) -> bool {
        self.session_id.is_some()
            && self.host_session().is_none()
            && self.session(cx).blocks.is_empty()
    }

    fn limit_choice_key(&self) -> Option<String> {
        self.host_session()
            .map(|host| snapshot_key(&self.machine.id, &host.id))
    }

    /// The usage limit notice, unless this computer dismissed it.
    pub fn usage_limit(&self, cx: &App) -> Option<UsageLimit> {
        let host_limit = self.host_session()?.usage_limit?;
        let key = self.limit_choice_key()?;
        let choice = self
            .connections
            .read(cx)
            .limit_choice(&key)
            .filter(|choice| choice.limit == limit_key(&host_limit))
            .cloned();
        if choice
            .as_ref()
            .is_some_and(|choice| choice.dismissed == Some(true))
        {
            return None;
        }
        Some(UsageLimit {
            resume_at_reset: choice.and_then(|choice| choice.resume_at_reset),
            ..host_limit
        })
    }

    /// The tab's local session, as `Sessions` holds it now.
    fn current_shell(&self, cx: &App) -> Session {
        Engine::try_global(cx)
            .and_then(|engine| engine.sessions.read(cx).get(&self.shell.id).cloned())
            .unwrap_or_else(|| self.shell.clone())
    }

    /// The session the pane draws: the host's transcript with this tab's ID,
    /// the remote project path, the composer's configuration, and any turn
    /// the host has not confirmed yet.
    pub fn session(&self, cx: &App) -> Session {
        let host = self.host_session();
        let starting_active = self.starting_active();
        let pending_send_active = self.pending_send_active();
        let unseen_active = self.unseen_active();
        // A draft being sent is replaced by its message at once, as locally.
        let leaving: Vec<&str> = [
            self.removing_draft.as_deref(),
            starting_active
                .then(|| {
                    self.starting
                        .as_ref()
                        .and_then(|s| s.turn.draft_block_id.as_deref())
                })
                .flatten(),
            match (&self.pending, pending_send_active) {
                (Some(HostCommand::Send { draft_block_id, .. }), true) => draft_block_id.as_deref(),
                _ => None,
            },
            unseen_active
                .then(|| {
                    self.unseen_send
                        .as_ref()
                        .and_then(|u| u.draft_block_id.as_deref())
                })
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut blocks: Vec<Block> = host
            .map(|host| {
                host.blocks
                    .iter()
                    .filter(|block| !leaving.contains(&block.id.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let unconfirmed = if unseen_active {
            self.unseen_send.as_ref().map(|unseen| Block {
                attachments: Some(unseen.attachments.clone()),
                started_at: Some(unseen.started_at),
                turn_model: Some(unseen.turn_model.clone()),
                ..Block::new(
                    unseen.command_id.clone(),
                    BlockRole::User,
                    unseen.text.clone(),
                )
            })
        } else if pending_send_active {
            self.pending.as_ref().map(|pending| {
                let text = match pending {
                    HostCommand::Send { text, .. } => text.clone(),
                    _ => "/compact".into(),
                };
                Block::new(connections::command_id(pending), BlockRole::User, text)
            })
        } else if starting_active {
            self.starting.as_ref().map(|starting| Block {
                attachments: Some(starting.turn.attachments.clone()),
                draft: Some(starting.turn.draft),
                started_at: Some(starting.turn.started_at),
                turn_model: Some(starting.turn.turn_model.clone()),
                ..Block::new(
                    starting.turn.command_id.clone(),
                    BlockRole::User,
                    starting.turn.text.clone(),
                )
            })
        } else {
            None
        };
        blocks.extend(unconfirmed);
        let configuration = self.configuration();
        let execution_cwd = self.execution_cwd();
        let mut session = match host {
            Some(host) => host.clone(),
            None => {
                let shell = self.current_shell(cx);
                let mut blank = Session::blank(
                    shell.id.clone(),
                    configuration.harness,
                    configuration.model.clone(),
                    shell.cwd.clone(),
                );
                blank.title = shell.title;
                blank
            }
        };
        session.id = self.shell.id.clone();
        session.cwd = remote_path(&self.machine.environment_id, &self.project.cwd);
        session.worktree_cwd = (execution_cwd != self.project.cwd)
            .then(|| remote_path(&self.machine.environment_id, execution_cwd));
        session.workspace_mode = host.is_none().then_some(self.draft_workspace_mode);
        session.worktree_base = host.is_none().then(|| self.draft_worktree_base.clone());
        session.branch = self
            .current_branch
            .clone()
            .or_else(|| host.and_then(|host| host.branch.clone()));
        session.harness = configuration.harness;
        session.model = configuration.model;
        session.model_settings = configuration.settings;
        session.runtime_mode = configuration.mode;
        session.busy = Some(self.busy());
        session.usage_limit = self.usage_limit(cx);
        session.blocks = blocks;
        session
    }

    /// The notice bar, if anything needs saying.
    pub fn notice(&self) -> Option<RemoteNotice> {
        let detail = (!self.error.is_empty()).then(|| self.error.clone());
        let alert = !self.error.is_empty();
        let notice = |text: String, detail: Option<String>, action: NoticeAction| RemoteNotice {
            text,
            detail,
            action,
            alert,
            enabled: self.online || action == NoticeAction::Dismiss,
        };
        if self.pending.is_some() && !self.sending {
            return Some(notice(
                "Waiting for the host to confirm your request.".into(),
                detail,
                NoticeAction::RetryPending,
            ));
        }
        if let Some(starting) = self.starting.as_ref().filter(|starting| starting.failed) {
            return Some(notice(
                format!(
                    "Couldn’t {} on {}.",
                    if starting.turn.draft {
                        "save the draft"
                    } else {
                        "send the message"
                    },
                    self.machine.name
                ),
                detail,
                NoticeAction::TryAgain,
            ));
        }
        if !self.error.is_empty() {
            return Some(notice(self.error.clone(), None, NoticeAction::Dismiss));
        }
        let catalog_problem = self.catalog_problem();
        if !catalog_problem.is_empty() {
            return Some(notice(
                format!("Couldn’t load models from {}.", self.machine.name),
                Some(catalog_problem),
                NoticeAction::RetryCatalog,
            ));
        }
        None
    }

    /// `catalog.errors[harness] ?? catalogError`: why the host's model list
    /// did not load for the selected provider.
    fn catalog_problem(&self) -> String {
        let harness = self.configuration().harness;
        self.catalog
            .as_ref()
            .and_then(|catalog| catalog.errors.get(&harness).cloned())
            .unwrap_or_else(|| self.catalog_error.clone())
    }

    /// The inputs of `notice`, for views that word the notice themselves.
    pub fn status(&self) -> RemoteSessionStatus {
        RemoteSessionStatus {
            pending: self.pending.is_some(),
            sending: self.sending,
            failed_draft: self
                .starting
                .as_ref()
                .filter(|starting| starting.failed)
                .map(|starting| starting.turn.draft),
            error: self.error.clone(),
            catalog_problem: self.catalog_problem(),
        }
    }

    /// `hostFilePath`: a path a transcript names, as a `remote://` path on
    /// this machine. Relative paths resolve against the execution checkout.
    pub fn host_file_path(&self, path: &str) -> String {
        if parse_remote_path(path).is_some() {
            return path.to_string();
        }
        let bytes = path.as_bytes();
        let windows_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\');
        let absolute = if path.starts_with('/') || path.starts_with("\\\\") || windows_drive {
            path.to_string()
        } else {
            format!(
                "{}/{}",
                self.execution_cwd().trim_end_matches(['\\', '/']),
                path.strip_prefix("./").unwrap_or(path)
            )
        };
        remote_path(&self.machine.environment_id, &absolute)
    }

    // The model source (`ModelSourceContext`).

    /// `modelsFor`: the host's models for a provider.
    pub fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        self.catalog
            .as_ref()
            .and_then(|catalog| catalog.models.get(&harness).cloned())
            .unwrap_or_default()
    }

    /// `resolve`: a model with the settings controls to show for it. The
    /// saved model's effort stays visible while the catalog loads, fails, or
    /// no longer lists it.
    pub fn resolve_model(&self, harness: HarnessId, id: &str) -> AgentModel {
        let host = self.host_session();
        let saved_model = host.map(|host| host.model.as_str());
        let empty = ModelSettings::new();
        let saved_settings = if Some(id) == saved_model {
            host.map_or(&empty, |host| &host.model_settings)
        } else {
            &empty
        };
        let controls = remote_model_controls(
            self.catalog.as_ref(),
            harness,
            id,
            saved_settings,
            saved_model,
        );
        let mut model = controls.model.unwrap_or_else(|| {
            let bare = strip_harness_prefix(id);
            let name = if id.is_empty() {
                "Loading models…"
            } else {
                bare
            };
            AgentModel::new(id, harness, name).with_native_id(bare)
        });
        model.settings = Some(controls.settings);
        model
    }

    /// `find`: a model any of the host's providers lists.
    pub fn find_model(&self, id: &str) -> Option<AgentModel> {
        self.providers()
            .into_iter()
            .flat_map(|harness| self.models_for(harness))
            .find(|model| model.id == id)
    }

    /// `available`: the host runs this provider, and a started session keeps
    /// its own.
    pub fn model_available(&self, harness: HarnessId) -> bool {
        self.providers().contains(&harness)
            && self
                .host_session()
                .is_none_or(|host| host.harness == harness)
    }

    /// `probed`.
    pub fn models_probed(&self) -> bool {
        self.descriptor.is_some()
    }

    // Inputs from the view.

    /// Whether the tab is showing. Hidden tabs poll less often.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            self.restart_poll(cx);
        }
    }

    /// The branch of the execution checkout, from the projects package.
    pub fn set_current_branch(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        if self.current_branch != branch {
            self.current_branch = branch;
            cx.notify();
        }
    }

    // Effects.

    /// After each state change: the React effects, in their order.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.reconciling {
            self.dirty = true;
            return;
        }
        self.reconciling = true;
        for _ in 0..8 {
            self.dirty = false;
            self.reconcile(cx);
            if !self.dirty {
                break;
            }
        }
        self.reconciling = false;
        cx.notify();
    }

    fn reconcile(&mut self, cx: &mut Context<Self>) {
        // A started session no longer needs its pending worktree choice.
        let host_id = self.host_session().map(|host| host.id.clone());
        if host_id != self.last_host_id {
            if host_id.is_some() {
                connections::remember_remote_pending_worktree(&self.kv, &self.shell.id, None);
            }
            self.last_host_id = host_id;
        }
        // An accepted turn stays on screen until a sync shows the host's
        // copy, so the transcript never drops it for a moment in between.
        if let Some(starting) = &self.starting
            && !starting.failed
            && self.has_host_block(&starting.turn.command_id)
        {
            self.starting = None;
        }
        // A draft being removed leaves the transcript at once and returns if
        // the host turns the removal down.
        if let Some(removing) = &self.removing_draft
            && !self
                .host_session()
                .is_some_and(|host| host.blocks.iter().any(|block| block.id == *removing))
        {
            self.removing_draft = None;
        }
        if let Some(unseen) = &self.unseen_send
            && self.host_session().is_some_and(|host| {
                host.id == unseen.session_id
                    && host
                        .blocks
                        .iter()
                        .any(|block| block.id == unseen.command_id)
            })
        {
            self.unseen_send = None;
        }
        self.sync_draft_model();
        self.apply_configuration(cx);
        self.limit_effect(cx);
    }

    /// A new session starts with the tab's model when the host offers it,
    /// and otherwise with the host's first model. Settings follow the host's
    /// entry: the same id can differ between machines.
    fn sync_draft_model(&mut self) {
        let providers = self.providers();
        if !self.online || self.session_id.is_some() || providers.is_empty() {
            return;
        }
        let Some(catalog) = &self.catalog else {
            return;
        };
        let harness = if providers.contains(&self.draft.harness) {
            self.draft.harness
        } else {
            providers[0]
        };
        let models = catalog
            .models
            .get(&harness)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Some(model) = find_remote_model(models, &self.draft.model).or(models.first()) else {
            return;
        };
        let settings = carry_model_settings(
            model.settings.as_deref().unwrap_or_default(),
            &self.draft.settings,
        );
        if self.draft.harness == harness
            && self.draft.model == model.id
            && same_model_settings(Some(&settings), Some(&self.draft.settings))
        {
            return;
        }
        self.draft = Configuration {
            harness,
            model: model.id.clone(),
            settings,
            mode: self.draft.mode,
        };
    }

    /// Model, effort, and permission changes apply directly, as locally. A
    /// running turn keeps its settings; the change is sent once it finishes.
    fn apply_configuration(&mut self, cx: &mut Context<Self>) {
        let (Some(changes), Some(saved), Some(host_id)) = (
            self.changes.clone(),
            self.saved(),
            self.host_session().map(|host| host.id.clone()),
        ) else {
            return;
        };
        if same_configuration(&changes, &saved) {
            self.applied = None;
            self.changes = None;
            return;
        }
        if self
            .applied
            .as_ref()
            .is_some_and(|applied| same_configuration(&changes, applied))
        {
            return;
        }
        if self.busy() || !self.online || self.pending.is_some() || self.applying {
            return;
        }
        self.applying = true;
        let run = self.run(
            HostCommand::Configure {
                command_id: new_id(),
                session_id: host_id,
                model: changes.model.clone(),
                model_settings: changes.settings.clone(),
                runtime_mode: changes.mode,
            },
            None,
            None,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let receipt = run.await;
            this.update(cx, |this, cx| {
                if receipt.is_some() {
                    this.applied = Some(changes);
                }
                this.applying = false;
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// An armed usage limit notice resumes the session once its limit has
    /// reset, as a local session does.
    fn limit_effect(&mut self, cx: &mut Context<Self>) {
        let usage_limit = self.usage_limit(cx);
        let key = (
            usage_limit.and_then(|limit| limit.resume_at_reset),
            usage_limit.and_then(|limit| limit.resets_at),
            self.busy(),
            self.online,
            self.pending
                .as_ref()
                .map(|pending| connections::command_id(pending).to_string()),
            self.limit_tick,
        );
        if self.limit_effect.as_ref() == Some(&key) {
            return;
        }
        self.limit_effect = Some(key);
        self.limit_timer = None;
        let Some(limit) = usage_limit.filter(|limit| limit.resume_at_reset == Some(true)) else {
            return;
        };
        let Some(resets_at) = limit.resets_at else {
            return;
        };
        let now = self.connections.read(cx).now();
        if usage_limit_resume_due(&self.session(cx), now) && self.online && self.pending.is_none() {
            self.resume_after_limit(cx);
            return;
        }
        // Re-check every minute at most: timers drift while the machine sleeps.
        let delay = (resets_at - now).clamp(1_000, 60_000) as u64;
        self.limit_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            this.update(cx, |this, cx| {
                this.limit_tick += 1;
                this.changed(cx);
            })
            .ok();
        }));
    }

    /// `REMOTE_HISTORY_CHANGE`: the tab may now show another host session.
    fn binding_changed(&mut self, cx: &mut Context<Self>) {
        let next = connections::remote_session_for(&self.kv, &self.shell.id);
        if next != self.bound_session {
            self.binding_version += 1;
            self.bound_session = next.clone();
            self.starting = None;
            self.unseen_send = None;
            self.changes = None;
            self.applied = None;
            self.error.clear();
            self.removing_draft = None;
            self.preparing = false;
            self.snapshot = next.as_deref().and_then(|id| {
                self.connections
                    .read(cx)
                    .cached_snapshot(&self.machine.id, id)
            });
        }
        self.pending = self.pending_for(next.as_deref());
        if self.session_id != next {
            self.session_id = next;
            self.restart_poll(cx);
        }
        self.changed(cx);
    }

    fn pending_for(&self, session_id: Option<&str>) -> Option<HostCommand> {
        pending_remote_command(
            &self.kv,
            &self.project.key,
            &self.machine.environment_id,
            PendingScope::for_session(session_id),
            Some(&self.shell.id),
        )
    }

    /// The host reports each write to this session, so the transcript
    /// updates as soon as it changes instead of on the next poll.
    fn host_changed(&mut self, detail: &RemoteChangesDetail, _cx: &mut Context<Self>) {
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        if detail.machine_id != self.machine.id
            || (!detail.reset && !detail.sessions.iter().any(|entry| entry.id == session_id))
        {
            return;
        }
        if self.in_flight {
            self.again = true;
        } else if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
    }

    /// The polling effect, which re-ran when the session, the refresh
    /// counter, or visibility changed.
    fn restart_poll(&mut self, cx: &mut Context<Self>) {
        self.poll_epoch += 1;
        let epoch = self.poll_epoch;
        let version = self.binding_version;
        let session_id = self.session_id.clone();
        let machine = self.machine.clone();
        let project_id = self.project.project_id.clone();
        let visible = self.visible;
        let client = self.client.clone();
        let (wake, woken) = async_channel::bounded(1);
        self.in_flight = false;
        self.again = false;
        self.wake = Some(wake);
        self.poll = Some(cx.spawn(async move |this, cx| {
            // Every request carries the expected host identity; describe
            // again only after a failure, when the host may have been replaced.
            let mut described = false;
            let mut failed: u32 = 0;
            let stale = move |this: &RemoteSession| {
                epoch != this.poll_epoch
                    || version != this.binding_version
                    || (session_id.is_some() && this.deleting_session == session_id)
            };
            loop {
                let Ok(known) = this.update(cx, |this, _| {
                    this.in_flight = true;
                    this.snapshot
                        .clone()
                        .filter(|snapshot| Some(&snapshot.session.id) == this.session_id.as_ref())
                }) else {
                    return;
                };
                let session_id = this
                    .read_with(cx, |this, _| this.session_id.clone())
                    .ok()
                    .flatten();
                let mut outcome: Result<Option<Arc<HostSession>>, String> = Ok(None);
                if !described {
                    let providers: Vec<&str> = REMOTE_PROVIDERS.iter().map(|p| provider_name(*p)).collect();
                    let answer = client
                        .request(&machine.id, "environment.describe", json!({ "supportedProviders": providers }))
                        .await
                        .and_then(|value| require_host_descriptor(&value))
                        .and_then(|host| {
                            if host.environment_id == machine.environment_id {
                                Ok(host)
                            } else {
                                Err("Host identity changed. Reconnect this machine before continuing.".into())
                            }
                        });
                    match answer {
                        Ok(host) => {
                            let Ok(fresh) = this.update(cx, |this, cx| {
                                if stale(this) {
                                    return false;
                                }
                                this.described(host, cx);
                                true
                            }) else {
                                return;
                            };
                            if !fresh {
                                return;
                            }
                            described = true;
                        }
                        Err(error) => outcome = Err(error),
                    }
                }
                if outcome.is_ok()
                    && let Some(session_id) = &session_id
                {
                    outcome = client
                        .load_remote_session(&machine.id, session_id, known)
                        .await
                        .and_then(|next| {
                            if next.project_id == project_id {
                                Ok(Some(next))
                            } else {
                                Err("This session belongs to a different host project".into())
                            }
                        });
                }
                let Ok(Some((again, live, active))) = this.update(cx, |this, cx| {
                    if stale(this) {
                        return None;
                    }
                    let mut active = false;
                    match outcome {
                        Ok(next) => {
                            this.online = true;
                            let machine_id = this.machine.id.clone();
                            this.connections
                                .update(cx, |connections, cx| connections.report_status(&machine_id, true, cx));
                            // A catalog request that failed while offline is
                            // retried on recovery.
                            if failed > 0 {
                                this.load_catalog(cx);
                            }
                            failed = 0;
                            if let (Some(next), Some(id)) = (&next, &this.session_id) {
                                let (machine_id, id, next) = (machine_id.clone(), id.clone(), next.clone());
                                this.connections.update(cx, |connections, _| {
                                    connections.remember_snapshot(&machine_id, &id, next)
                                });
                            }
                            active = next.as_ref().is_some_and(|next| next.session.is_busy());
                            this.snapshot = next.clone();
                            // `RemoteSessions` skips a snapshot it already merged.
                            if let Some(next) = next {
                                cx.emit(RemoteSessionEvent::Snapshot(Some(next)));
                            }
                        }
                        Err(_) => {
                            this.online = false;
                            let machine_id = this.machine.id.clone();
                            this.connections
                                .update(cx, |connections, cx| connections.report_status(&machine_id, false, cx));
                            described = false;
                            failed += 1;
                        }
                    }
                    this.in_flight = false;
                    this.changed(cx);
                    let again = std::mem::take(&mut this.again);
                    let live = this.connections.read(cx).remote_changes_live(&this.machine.id);
                    Some((again, live, active))
                }) else {
                    return;
                };
                if again {
                    continue;
                }
                // With pushed changes, polling only covers a missed change.
                let delay = if failed > 0 {
                    (750 * (1u64 << failed.min(4))).min(10_000)
                } else if active {
                    if live { 5_000 } else { 750 }
                } else if visible {
                    if live { 15_000 } else { 3_000 }
                } else if live {
                    30_000
                } else {
                    10_000
                };
                let timer = cx.background_executor().timer(Duration::from_millis(delay));
                if let futures::future::Either::Right((Err(_), _)) =
                    futures::future::select(pin!(timer), pin!(woken.recv())).await
                {
                    return;
                }
            }
        }));
    }

    fn described(&mut self, host: HostDescriptor, cx: &mut Context<Self>) {
        let environment_changed = self
            .descriptor
            .as_ref()
            .is_none_or(|old| old.environment_id != host.environment_id);
        let machine_id = self.machine.id.clone();
        let cached = host.clone();
        self.connections.update(cx, |connections, _| {
            connections.set_descriptor(&machine_id, cached)
        });
        self.descriptor = Some(host);
        if environment_changed {
            self.load_catalog(cx);
        }
        self.changed(cx);
    }

    /// The catalog effect: `models.list` for this project.
    fn load_catalog(&mut self, cx: &mut Context<Self>) {
        if self.descriptor.is_none() {
            return;
        }
        let request = self.client.request(
            &self.machine.id,
            "models.list",
            json!({ "projectId": self.project.project_id }),
        );
        self.catalog_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await.and_then(decode::<HostModelCatalog>);
            this.update(cx, |this, cx| {
                match result {
                    Ok(catalog) => {
                        let (machine_id, project_id) =
                            (this.machine.id.clone(), this.project.project_id.clone());
                        let cached = catalog.clone();
                        this.connections.update(cx, |connections, _| {
                            connections.set_catalog(&machine_id, &project_id, cached)
                        });
                        this.catalog = Some(catalog);
                        this.catalog_error.clear();
                    }
                    Err(reason) => this.catalog_error = reason,
                }
                this.changed(cx);
            })
            .ok();
        }));
    }

    /// `refresh` in the model source: the host re-probes when a provider CLI
    /// changes or its catalog ages, so each picker opening asks again.
    pub fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
        self.load_catalog(cx);
    }

    // Commands.

    fn selected_turn_model(&self) -> TurnModel {
        let configuration = self.configuration();
        let models = self.models_for(configuration.harness);
        let name = find_remote_model(&models, &configuration.model)
            .map(|model| model.name.clone())
            .unwrap_or_else(|| strip_model_prefix(&configuration.model).to_string());
        TurnModel {
            harness: configuration.harness,
            id: configuration.model,
            name,
            extra: Extra::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn optimistic_turn(
        &self,
        text: &str,
        attachments: Vec<Attachment>,
        intent: SendIntent,
        draft: bool,
        draft_block_id: Option<String>,
        plan_block_id: Option<String>,
        cx: &App,
    ) -> OptimisticTurn {
        OptimisticTurn {
            command_id: new_id(),
            text: text.to_string(),
            attachments,
            intent,
            draft,
            draft_block_id,
            plan_block_id,
            started_at: self.connections.read(cx).now(),
            turn_model: self.selected_turn_model(),
        }
    }

    /// `run`: dispatch a command. The outbox entry is written first and keeps
    /// the command's ID across disconnects and restarts; an ambiguous answer
    /// is retried explicitly instead of sending a new prompt.
    fn run(
        &mut self,
        command: HostCommand,
        optimistic: Option<&OptimisticTurn>,
        followup: Option<HostCommand>,
        cx: &mut Context<Self>,
    ) -> Task<Option<CommandReceipt>> {
        if self.sending {
            return Task::ready(None);
        }
        let version = self.binding_version;
        self.sending = true;
        self.error.clear();
        if let HostCommand::Send {
            command_id,
            session_id,
            text,
            draft_block_id,
            ..
        } = &command
        {
            self.note_unseen(
                command_id,
                session_id,
                text,
                draft_block_id.clone(),
                optimistic,
                cx,
            );
        } else if let HostCommand::Compact {
            command_id,
            session_id,
        } = &command
        {
            self.note_unseen(command_id, session_id, "/compact", None, optimistic, cx);
        }
        let attachments = optimistic
            .map(|turn| turn.attachments.clone())
            .unwrap_or_default();
        save_pending_remote_command(
            &self.kv,
            &self.project.key,
            &self.machine.environment_id,
            &command,
            Some(&self.shell.id),
            followup.as_ref(),
        );
        self.pending = Some(command.clone());
        cx.notify();
        let params = serde_json::to_value(&command).unwrap_or(Value::Null);
        let dispatch = self
            .client
            .request(&self.machine.id, "commands.dispatch", params);
        cx.spawn(async move |this, cx| {
            let answer = dispatch.await.and_then(decode::<CommandReceipt>);
            this.update(cx, |this, cx| {
                let receipt = this.dispatched(&command, answer, version, &attachments, cx);
                this.sending = false;
                this.changed(cx);
                receipt
            })
            .ok()
            .flatten()
        })
    }

    fn note_unseen(
        &mut self,
        command_id: &str,
        session_id: &str,
        text: &str,
        draft_block_id: Option<String>,
        optimistic: Option<&OptimisticTurn>,
        cx: &App,
    ) {
        if self
            .unseen_send
            .as_ref()
            .is_some_and(|unseen| unseen.command_id == command_id)
        {
            return;
        }
        self.unseen_send = Some(UnseenSend {
            session_id: session_id.to_string(),
            command_id: command_id.to_string(),
            text: text.to_string(),
            attachments: optimistic
                .map(|turn| turn.attachments.clone())
                .unwrap_or_default(),
            started_at: optimistic
                .map_or_else(|| self.connections.read(cx).now(), |turn| turn.started_at),
            turn_model: optimistic.map_or_else(
                || self.selected_turn_model(),
                |turn| turn.turn_model.clone(),
            ),
            draft_block_id,
        });
    }

    /// The end of `run`, once the host answered or the request failed.
    fn dispatched(
        &mut self,
        command: &HostCommand,
        answer: Result<CommandReceipt, String>,
        version: u64,
        attachments: &[Attachment],
        cx: &mut Context<Self>,
    ) -> Option<CommandReceipt> {
        let (key, environment) = (
            self.project.key.clone(),
            self.machine.environment_id.clone(),
        );
        let id = connections::command_id(command).to_string();
        match answer {
            Ok(receipt) => {
                if matches!(command, HostCommand::Create { .. }) {
                    if let Some(next) = pending_remote_followup(&self.kv, &key, &environment, &id)
                        .filter(|next| !matches!(next, HostCommand::Create { .. }))
                    {
                        save_pending_remote_command(
                            &self.kv,
                            &key,
                            &environment,
                            &with_session_id(&next, &receipt.session_id),
                            Some(&self.shell.id),
                            None,
                        );
                    }
                    if version == self.binding_version {
                        self.open_session(&receipt.session_id, cx);
                    }
                }
                clear_pending_remote_command(&self.kv, &key, &environment, &id);
                if version != self.binding_version {
                    return Some(receipt);
                }
                if let HostCommand::Draft {
                    command_id,
                    session_id,
                    text,
                    ..
                } = command
                {
                    // Accepted drafts are actionable before the next snapshot.
                    self.show_draft(command_id, session_id, text, attachments, cx);
                    self.starting = None;
                }
                let scope_id = match command {
                    HostCommand::Create { .. } => Some(receipt.session_id.clone()),
                    _ => self.session_id.clone(),
                };
                self.pending = self.pending_for(scope_id.as_deref());
                self.restart_poll(cx);
                Some(receipt)
            }
            Err(reason) => {
                if version != self.binding_version {
                    return None;
                }
                if reason.contains("Host rejected request:") {
                    if matches!(
                        command,
                        HostCommand::Send { .. } | HostCommand::Compact { .. }
                    ) && self
                        .unseen_send
                        .as_ref()
                        .is_some_and(|unseen| unseen.command_id == id)
                    {
                        self.unseen_send = None;
                    }
                    clear_pending_remote_command(&self.kv, &key, &environment, &id);
                    let session_id = self.session_id.clone();
                    self.pending = self.pending_for(session_id.as_deref());
                }
                self.error = error_text(&reason);
                None
            }
        }
    }

    /// The local copy of an accepted draft.
    fn show_draft(
        &mut self,
        command_id: &str,
        session_id: &str,
        text: &str,
        attachments: &[Attachment],
        cx: &mut Context<Self>,
    ) {
        let configuration = self.configuration();
        let now = self.connections.read(cx).now();
        let current = self
            .snapshot
            .clone()
            .filter(|current| current.session.id == session_id);
        let mut next = match &current {
            Some(current) => (**current).clone(),
            None => HostSession {
                session: self.current_shell(cx),
                project_id: self.project.project_id.clone(),
                revision: 0,
                run_id: None,
                status: HostSessionStatus::Idle,
                created_at: None,
                updated_at: now,
                archived: None,
                pinned: None,
                auto_worktree_branch: None,
                block_revisions: None,
                extra: Extra::new(),
            },
        };
        next.status = HostSessionStatus::Idle;
        let session = &mut next.session;
        session.id = session_id.to_string();
        session.cwd = self.selected_cwd.clone();
        session.harness = configuration.harness;
        session.model = configuration.model;
        session.model_settings = configuration.settings;
        session.runtime_mode = configuration.mode;
        session.busy = Some(false);
        let mut blocks: Vec<Block> = if current.is_some() {
            session
                .blocks
                .iter()
                .filter(|block| !block.is_draft() && block.id != command_id)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        blocks.push(Block {
            draft: Some(true),
            attachments: Some(attachments.to_vec()),
            ..Block::new(command_id, BlockRole::User, text)
        });
        session.blocks = blocks;
        self.snapshot = Some(Arc::new(next));
    }

    fn open_session(&mut self, id: &str, cx: &mut Context<Self>) {
        self.bound_session = Some(id.to_string());
        let shell_id = self.shell.id.clone();
        let id_owned = id.to_string();
        self.connections.update(cx, |connections, cx| {
            connections.remember_remote_session(&shell_id, Some(&id_owned), cx)
        });
        if self.session_id.as_deref() != Some(id) {
            self.session_id = Some(id.to_string());
            self.restart_poll(cx);
        }
    }

    /// A conversation that was only a draft goes with it, as a local one
    /// does, and the tab starts over as a new conversation.
    fn discard_session(&mut self, id: String, cx: &mut Context<Self>) {
        let version = self.binding_version;
        self.deleting_session = Some(id.clone());
        let delete = self.client.request(
            &self.machine.id,
            "sessions.delete",
            json!({ "projectId": self.project.project_id, "sessionId": id }),
        );
        cx.spawn(async move |this, cx| {
            let result = delete.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        let machine_id = this.machine.id.clone();
                        this.connections.update(cx, |connections, _| {
                            connections.forget_snapshot(&machine_id, &id)
                        });
                        if version != this.binding_version {
                            return;
                        }
                        this.snapshot = None;
                        let shell_id = this.shell.id.clone();
                        this.connections.update(cx, |connections, cx| {
                            connections.remember_remote_session(&shell_id, None, cx)
                        });
                        cx.emit(RemoteSessionEvent::Snapshot(None));
                    }
                    Err(reason) => {
                        if version != this.binding_version {
                            return;
                        }
                        this.deleting_session = None;
                        this.restart_poll(cx);
                        this.removing_draft = None;
                        this.error = error_text(&reason);
                    }
                }
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `dispatchTurn`: upload the turn's files, then send it or save it as a
    /// draft.
    fn dispatch_turn(
        &mut self,
        id: String,
        turn: OptimisticTurn,
        uploaded: Option<Vec<RemoteAttachment>>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<CommandReceipt>, String>> {
        let version = self.binding_version;
        let upload: Option<RemoteFuture<Vec<RemoteAttachment>>> =
            match (&turn.draft_block_id, uploaded) {
                (Some(_), _) => None,
                (None, Some(uploaded)) => Some(Box::pin(async move { Ok(uploaded) })),
                (None, None) => Some(
                    self.client
                        .upload_attachments(&self.machine.id, turn.attachments.clone()),
                ),
            };
        cx.spawn(async move |this, cx| {
            let refs = match upload {
                Some(upload) => upload.await?,
                None => Vec::new(),
            };
            let Ok(Some(run)) = this.update(cx, |this, cx| {
                if version != this.binding_version {
                    return None;
                }
                let command = if turn.draft {
                    HostCommand::Draft {
                        command_id: turn.command_id.clone(),
                        session_id: id,
                        text: turn.text.clone(),
                        attachments: Some(refs),
                    }
                } else {
                    message(
                        &id,
                        &turn.text,
                        turn.command_id.clone(),
                        refs,
                        turn.intent,
                        turn.draft_block_id.clone(),
                        turn.plan_block_id.clone(),
                    )
                };
                Some(this.run(command, Some(&turn), None, cx))
            }) else {
                return Ok(None);
            };
            Ok(run.await)
        })
    }

    /// `startSession`: create the session on the host, then send the first
    /// message. The message is saved with the create, so a retry after an
    /// uncertain create sends the original text.
    fn start_session(&mut self, turn: OptimisticTurn, cx: &mut Context<Self>) {
        let version = self.binding_version;
        let upload = self
            .client
            .upload_attachments(&self.machine.id, turn.attachments.clone());
        cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                let uploaded = upload.await?;
                let Ok(Some((worktree, base, cwd, client, machine_id, project_id))) =
                    this.update(cx, |this, _| {
                        (version == this.binding_version).then(|| {
                            (
                                this.draft_workspace_mode == WorkspaceMode::Worktree,
                                this.draft_worktree_base.clone(),
                                this.selected_cwd.clone(),
                                this.client.clone(),
                                this.machine.id.clone(),
                                this.project.project_id.clone(),
                            )
                        })
                    })
                else {
                    return Ok(());
                };
                let mut worktree_cwd = cwd.clone();
                let mut auto_worktree_branch = None;
                if worktree {
                    let now = this
                        .read_with(cx, |this, cx| this.connections.read(cx).now())
                        .unwrap_or(0);
                    let tree = client
                        .request(
                            &machine_id,
                            "git.worktreeCreate",
                            json!({
                                "projectId": project_id,
                                "cwd": cwd,
                                "branch": temporary_worktree_branch_name(&new_id(), now),
                                "base": base,
                                "existing": false,
                            }),
                        )
                        .await
                        .and_then(decode::<HostWorktree>);
                    let tree = match tree {
                        Ok(tree) => tree,
                        Err(reason) => {
                            this.update(cx, |this, cx| {
                                if version == this.binding_version {
                                    this.error = reason;
                                    this.fail_starting(&turn);
                                    this.changed(cx);
                                }
                            })
                            .ok();
                            return Ok(());
                        }
                    };
                    let Ok(true) = this.update(cx, |this, cx| {
                        if version != this.binding_version {
                            return false;
                        }
                        connections::remember_remote_pending_worktree(
                            &this.kv,
                            &this.shell.id,
                            Some(&tree.path),
                        );
                        this.selected_cwd = tree.path.clone();
                        this.draft_workspace_mode = WorkspaceMode::Current;
                        this.changed(cx);
                        true
                    }) else {
                        return Ok(());
                    };
                    worktree_cwd = tree.path;
                    auto_worktree_branch = tree.branch;
                }
                let Ok(create) = this.update(cx, |this, cx| {
                    let followup = if turn.draft {
                        HostCommand::Draft {
                            command_id: turn.command_id.clone(),
                            session_id: String::new(),
                            text: turn.text.clone(),
                            attachments: Some(uploaded.clone()),
                        }
                    } else {
                        message(
                            "",
                            &turn.text,
                            turn.command_id.clone(),
                            uploaded.clone(),
                            turn.intent,
                            turn.draft_block_id.clone(),
                            turn.plan_block_id.clone(),
                        )
                    };
                    let create = HostCommand::Create {
                        command_id: new_id(),
                        project_id: this.project.project_id.clone(),
                        worktree_cwd: (worktree_cwd != this.project.cwd)
                            .then(|| worktree_cwd.clone()),
                        auto_worktree_branch: auto_worktree_branch.clone(),
                        harness: this.draft.harness,
                        model: this.draft.model.clone(),
                        model_settings: Some(this.draft.settings.clone()),
                        runtime_mode: this.draft.mode,
                    };
                    (
                        this.run(create, Some(&turn), Some(followup.clone()), cx),
                        followup,
                    )
                }) else {
                    return Ok(());
                };
                let (run, followup) = create;
                let receipt = run.await;
                let Ok(Some(send)) = this.update(cx, |this, cx| {
                    if version != this.binding_version {
                        return None;
                    }
                    let Some(receipt) = receipt else {
                        this.fail_starting(&turn);
                        this.changed(cx);
                        return None;
                    };
                    Some(this.run(
                        with_session_id(&followup, &receipt.session_id),
                        Some(&turn),
                        None,
                        cx,
                    ))
                }) else {
                    return Ok(());
                };
                let sent = send.await;
                this.update(cx, |this, cx| {
                    if version == this.binding_version && sent.is_none() {
                        this.fail_starting(&turn);
                        this.changed(cx);
                    }
                })
                .ok();
                Ok(())
            }
            .await;
            this.update(cx, |this, cx| {
                if let Err(reason) = result
                    && version == this.binding_version
                {
                    this.error = reason;
                    this.fail_starting(&turn);
                }
                this.preparing = false;
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    fn fail_starting(&mut self, turn: &OptimisticTurn) {
        self.starting = Some(Starting {
            turn: turn.clone(),
            failed: true,
        });
    }

    /// Send a turn that is already in `starting` to an existing session.
    fn send_turn(&mut self, session_id: String, turn: OptimisticTurn, cx: &mut Context<Self>) {
        let version = self.binding_version;
        let dispatch = self.dispatch_turn(session_id, turn.clone(), None, cx);
        cx.spawn(async move |this, cx| {
            let result = dispatch.await;
            this.update(cx, |this, cx| {
                if version == this.binding_version {
                    match result {
                        Ok(Some(_)) => {}
                        Ok(None) => this.fail_starting(&turn),
                        Err(reason) => {
                            this.error = reason;
                            this.fail_starting(&turn);
                        }
                    }
                }
                this.preparing = false;
                this.changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `submit`: send a message, or save it as a draft. `false` when the
    /// session cannot take it now.
    pub fn submit(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        options: &RemoteTurnOptions,
        cx: &mut Context<Self>,
    ) -> bool {
        self.submit_turn(text, attachments, options, false, None, cx)
    }

    fn submit_turn(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        options: &RemoteTurnOptions,
        as_draft: bool,
        plan_block_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.online
            || self.sending
            || self.preparing
            || self.pending.is_some()
            || self.busy()
            || (text.trim().is_empty() && attachments.is_empty())
        {
            return false;
        }
        let turn = self.optimistic_turn(
            text,
            attachments,
            send_intent(options.intent),
            as_draft,
            options.draft_block_id.clone(),
            plan_block_id,
            cx,
        );
        self.preparing = true;
        self.starting = Some(Starting {
            turn: turn.clone(),
            failed: false,
        });
        let Some(host_id) = self.host_session().map(|host| host.id.clone()) else {
            if self.session_id.is_some() || self.draft.model.is_empty() {
                self.preparing = false;
                self.starting = None;
                self.changed(cx);
                return false;
            }
            self.start_session(turn, cx);
            self.changed(cx);
            return true;
        };
        if self.changes.is_some() {
            self.preparing = false;
            self.starting = None;
            self.changed(cx);
            return false;
        }
        self.send_turn(host_id, turn, cx);
        self.changed(cx);
        true
    }

    /// `saveDraft`.
    pub fn save_draft(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.submit_turn(
            text,
            attachments,
            &RemoteTurnOptions::default(),
            true,
            None,
            cx,
        )
    }

    /// `retryPending`: send the unconfirmed command again with its ID; after
    /// a create, send the saved first message too.
    pub fn retry_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        if self.sending {
            return;
        }
        let version = self.binding_version;
        if let Some(starting) = &mut self.starting {
            starting.failed = false;
        }
        let run = self.run(pending.clone(), None, None, cx);
        cx.spawn(async move |this, cx| {
            let receipt = run.await;
            let Some(receipt) = receipt else {
                return;
            };
            if !matches!(pending, HostCommand::Create { .. }) {
                return;
            }
            let next = this
                .update(cx, |this, cx| {
                    if version != this.binding_version {
                        return None;
                    }
                    let next = this.pending_for(Some(&receipt.session_id))?;
                    Some(this.run(next, None, None, cx))
                })
                .ok()
                .flatten();
            if let Some(next) = next {
                next.await;
            }
        })
        .detach();
        self.changed(cx);
    }

    /// "Try again" on a turn that failed to send.
    pub fn try_again(&mut self, cx: &mut Context<Self>) {
        let Some(starting) = self.starting.clone() else {
            return;
        };
        let turn = starting.turn;
        self.starting = Some(Starting {
            turn: turn.clone(),
            failed: false,
        });
        self.preparing = true;
        match self.session_id.clone() {
            None => self.start_session(turn, cx),
            Some(session_id) => self.send_turn(session_id, turn, cx),
        }
        self.changed(cx);
    }

    /// Run the notice bar's action.
    pub fn run_notice_action(&mut self, action: NoticeAction, cx: &mut Context<Self>) {
        match action {
            NoticeAction::RetryPending => self.retry_pending(cx),
            NoticeAction::TryAgain => self.try_again(cx),
            NoticeAction::Dismiss => {
                self.error.clear();
                self.changed(cx);
            }
            NoticeAction::RetryCatalog => self.refresh_catalog(cx),
        }
    }

    /// `selectWorktree`: use another host checkout for a session that has
    /// not started.
    pub fn select_worktree(&mut self, path: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let parsed = parse_remote_path(path)
            .filter(|parsed| parsed.environment_id == self.machine.environment_id)
            .ok_or_else(|| "Choose a worktree on this machine".to_string())?;
        if self.session_id.is_some() {
            return Err(
                "This session’s worktree is fixed. Start a new session to use another.".into(),
            );
        }
        if parsed.host_path == self.execution_cwd() {
            return Ok(());
        }
        connections::remember_remote_pending_worktree(
            &self.kv,
            &self.shell.id,
            Some(&parsed.host_path),
        );
        self.selected_cwd = parsed.host_path;
        self.changed(cx);
        Ok(())
    }

    /// `buildPlan`: build an approved plan with the session's current model.
    pub fn build_plan(
        &mut self,
        block_id: &str,
        target: Option<&monocode_core::block::PlanBuildTarget>,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self
            .host_session()
            .and_then(|host| {
                host.blocks
                    .iter()
                    .find(|block| block.id == block_id && block.role == BlockRole::Plan)
            })
            .cloned()
        else {
            return;
        };
        if block.text.trim().is_empty() || block.streaming == Some(true) || self.busy() {
            return;
        }
        let configuration = self.configuration();
        if let Some(target) = target
            && (target.harness != configuration.harness
                || target.model != configuration.model
                || !same_model_settings(
                    Some(&target.model_settings),
                    Some(&configuration.settings),
                ))
        {
            self.error =
                "Select that model in the composer before building this remote plan.".into();
            self.changed(cx);
            return;
        }
        self.submit_turn(
            &format!("Build the approved plan:\n\n{}", block.text),
            Vec::new(),
            &RemoteTurnOptions {
                intent: Some(TurnIntent::Build),
                draft_block_id: None,
            },
            false,
            Some(block_id.to_string()),
            cx,
        );
    }

    /// `stopTurn`.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(host_id) = self
            .host_session()
            .filter(|host| host.is_busy())
            .map(|host| host.id.clone())
        else {
            return;
        };
        let Some(run_id) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.run_id.clone())
        else {
            return;
        };
        self.run(
            HostCommand::Cancel {
                command_id: new_id(),
                session_id: host_id,
                run_id,
            },
            None,
            None,
            cx,
        )
        .detach();
    }

    fn running(&self) -> Option<(String, String)> {
        let host_id = self.host_session()?.id.clone();
        let run_id = self.snapshot.as_ref()?.run_id.clone()?;
        Some((host_id, run_id))
    }

    /// `approve`.
    pub fn approve(&mut self, request_id: i64, decision: HarnessDecision, cx: &mut Context<Self>) {
        let Some((session_id, run_id)) = self.running() else {
            return;
        };
        let decision = match decision {
            HarnessDecision::Allow => ApprovalDecision::Allow,
            HarnessDecision::Deny => ApprovalDecision::Deny,
        };
        self.run(
            HostCommand::Approve {
                command_id: new_id(),
                session_id,
                run_id,
                request_id,
                decision,
            },
            None,
            None,
            cx,
        )
        .detach();
    }

    /// `answer`.
    pub fn answer(&mut self, request_id: i64, reply: UserQuestionReply, cx: &mut Context<Self>) {
        let Some((session_id, run_id)) = self.running() else {
            return;
        };
        self.run(
            HostCommand::Answer {
                command_id: new_id(),
                session_id,
                run_id,
                request_id,
                reply,
            },
            None,
            None,
            cx,
        )
        .detach();
    }

    /// `compact`: `/compact` through the host provider's compaction.
    pub fn compact(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(host_id) = self.host_session().map(|host| host.id.clone()) else {
            return false;
        };
        if self.busy() || self.pending.is_some() || self.changes.is_some() || !self.online {
            return false;
        }
        let command = message(
            &host_id,
            "/compact",
            new_id(),
            Vec::new(),
            SendIntent::Default,
            None,
            None,
        );
        self.run(command, None, None, cx).detach();
        true
    }

    /// `onRemoveDraft`. A session that holds only this draft is deleted.
    pub fn remove_draft(&mut self, draft_block_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(host) = self.host_session().cloned() else {
            return false;
        };
        if self.busy() || self.pending.is_some() || !self.online || self.removing_draft.is_some() {
            return false;
        }
        self.removing_draft = Some(draft_block_id.to_string());
        if host.blocks.iter().all(|block| block.id == draft_block_id) {
            self.discard_session(host.id, cx);
        } else {
            let run = self.run(
                HostCommand::RemoveDraft {
                    command_id: new_id(),
                    session_id: host.id,
                    draft_block_id: draft_block_id.to_string(),
                },
                None,
                None,
                cx,
            );
            cx.spawn(async move |this, cx| {
                if run.await.is_none() {
                    this.update(cx, |this, cx| {
                        this.removing_draft = None;
                        this.changed(cx);
                    })
                    .ok();
                }
            })
            .detach();
        }
        self.changed(cx);
        true
    }

    // Composer changes.

    fn update_configuration(
        &mut self,
        update: impl FnOnce(Configuration) -> Configuration,
        cx: &mut Context<Self>,
    ) {
        match self.saved() {
            Some(saved) => {
                let current = self.changes.clone().unwrap_or(saved);
                self.changes = Some(update(current));
            }
            None => self.draft = update(self.draft.clone()),
        }
        self.changed(cx);
    }

    /// `onModelChange`: keep the settings the new model supports.
    pub fn set_model(&mut self, harness: HarnessId, model: &str, cx: &mut Context<Self>) {
        let settings: Vec<ModelSetting> = self
            .resolve_model(harness, model)
            .settings
            .unwrap_or_default();
        let model = model.to_string();
        self.update_configuration(
            |current| Configuration {
                settings: carry_model_settings(&settings, &current.settings),
                harness,
                model,
                ..current
            },
            cx,
        );
    }

    /// `onModelSettingsChange`.
    pub fn set_model_settings(&mut self, settings: ModelSettings, cx: &mut Context<Self>) {
        self.update_configuration(
            |current| Configuration {
                settings,
                ..current
            },
            cx,
        );
    }

    /// `onRuntimeModeChange`.
    pub fn set_runtime_mode(&mut self, mode: RuntimeMode, cx: &mut Context<Self>) {
        self.update_configuration(|current| Configuration { mode, ..current }, cx);
    }

    /// `onWorkspaceModeChange`: a new worktree or the current checkout for
    /// the first message.
    pub fn set_workspace_mode(
        &mut self,
        mode: WorkspaceMode,
        base: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.draft_workspace_mode = mode;
        if let Some(base) = base.filter(|base| !base.is_empty()) {
            self.draft_worktree_base = base;
        }
        self.changed(cx);
    }

    /// `onWorktreeBaseChange`.
    pub fn set_worktree_base(&mut self, base: String, cx: &mut Context<Self>) {
        self.draft_worktree_base = base;
        self.changed(cx);
    }

    /// `onBranchChange`: the branch picker switched branches on the host.
    pub fn branch_changed(&mut self, cx: &mut Context<Self>) {
        Engine::hooks(cx).workspace.notify_git_changed(cx);
    }

    // Usage limits.

    fn choose_limit(
        &mut self,
        dismissed: Option<bool>,
        resume_at_reset: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let (Some(key), Some(limit)) = (
            self.limit_choice_key(),
            self.host_session().and_then(|host| host.usage_limit),
        ) else {
            return;
        };
        let choice = LimitChoice {
            limit: limit_key(&limit),
            dismissed,
            resume_at_reset,
        };
        self.connections.update(cx, |connections, _| {
            connections.set_limit_choice(&key, choice)
        });
        self.changed(cx);
    }

    /// `onUsageLimitResume`: continue now.
    pub fn resume_after_limit(&mut self, cx: &mut Context<Self>) {
        if self.usage_limit(cx).is_none() {
            return;
        }
        if self.submit(
            CONTINUE_PROMPT,
            Vec::new(),
            &RemoteTurnOptions::default(),
            cx,
        ) {
            self.choose_limit(Some(true), None, cx);
        }
    }

    /// `onUsageLimitResumeAtReset`.
    pub fn set_resume_at_reset(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.choose_limit(None, Some(enabled), cx);
    }

    /// `onUsageLimitDismiss`.
    pub fn dismiss_usage_limit(&mut self, cx: &mut Context<Self>) {
        self.choose_limit(Some(true), None, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_match_the_typescript() {
        assert_eq!(
            temporary_worktree_branch_name("AB-12cd-34ef", 0),
            "mc/ab12cd34"
        );
        assert_eq!(temporary_worktree_branch_name("---", 36), "mc/10");
        assert_eq!(strip_model_prefix("claude:opus"), "opus");
        assert_eq!(strip_model_prefix(":opus"), ":opus");
        assert_eq!(strip_harness_prefix("Claude:opus"), "Claude:opus");
        assert_eq!(
            message(
                "s",
                " /Compact ",
                "c".into(),
                Vec::new(),
                SendIntent::Default,
                None,
                None
            ),
            HostCommand::Compact {
                command_id: "c".into(),
                session_id: "s".into()
            }
        );
        assert!(matches!(
            message(
                "s",
                "/compact",
                "c".into(),
                Vec::new(),
                SendIntent::Plan,
                None,
                None
            ),
            HostCommand::Send { .. }
        ));
        assert_eq!(
            limit_key(&UsageLimit {
                resets_at: None,
                resume_at_reset: None
            }),
            "unknown"
        );
        assert_eq!(error_text("Error: nope"), "nope");
    }
}
