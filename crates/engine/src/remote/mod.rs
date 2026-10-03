//! Engine package `remote`: the desktop side of MonoCode Connect sessions.
//!
//! Ported from src/features/connections/model (except protocol.ts, which
//! lives in `monocode_remote::host::protocol`), the data hooks in
//! src/features/connections/ui/RemoteSession.tsx, App.tsx's remote session
//! sync, and the remote routing in src/platform/tauri/fs.ts.
//!
//! - `RemoteConnections` (entity): paired machines, pairing, retry, and
//!   disconnect, machine status, the `changes.wait` long poll, remote
//!   projects, and their session lists.
//! - `RemoteSession` (entity, one per open remote tab): the host session's
//!   transcript, commands, and composer state.
//! - `RemoteSessions` (entity): merges host snapshots into `Sessions`, keeps
//!   every remote tab's turn state current, and announces finished turns.
//! - `RemoteClient`: thread-safe requests, session sync, uploads, and
//!   `invoke_workspace`, which runs file and Git commands for `remote://`
//!   paths on their machine.
//! - `peers`: what this package fills in for the runtime and the other
//!   packages (`RemoteHooks`, submit's `SubmitRemoteHooks`, the workspace
//!   file backend, approvals).
//!
//! Start with `RemoteGlobal::init_native(kv, data_dir, cx)` after
//! `Engine::init`.

pub mod client;
pub mod connections;
pub mod peers;
pub mod remote_attachment_previews;
pub mod remote_attachments;
pub mod remote_commands;
pub mod remote_connections;
pub mod remote_models;
pub mod remote_projects;
pub mod remote_session;
pub mod remote_session_state;
pub mod remote_sessions;
pub mod remote_turns;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod transport;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{App, AppContext, Entity, Global};
use monocode_core::Session;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::user_question::UserQuestionReply;
use monocode_settings::Kv;
use serde_json::{Map, Value};

pub use client::RemoteClient;
pub use peers::{DefaultRemotePeers, DesktopRemoteHooks, RemotePeers};
pub use remote_commands::{HOST_COMMANDS, REMOTE_PATH_PREFIX};
pub use remote_connections::{LimitChoice, RemoteConnections, RemoteEvent, RemoteProjectSessions};
pub use remote_projects::RemoteProject;
pub use remote_session::{
    Configuration, NoticeAction, RemoteFeatures, RemoteNotice, RemoteSession, RemoteSessionEvent,
    RemoteSessionStatus, RemoteTurnOptions,
};
pub use remote_sessions::{RemoteSessions, RemoteTab};
pub use remote_turns::RemoteChangesDetail;
pub use transport::{NativeTransport, RemoteFuture, RemoteTransport};

use crate::runtime::engine::Engine;

/// Epoch milliseconds. Tests pass a clock they move with the executor's.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// What `RemoteGlobal::init` needs.
pub struct RemoteConfig {
    pub transport: Arc<dyn RemoteTransport>,
    pub kv: Kv,
    pub clock: Clock,
}

/// The remote entities, as a GPUI global.
pub struct RemoteGlobal {
    pub connections: Entity<RemoteConnections>,
    pub sessions: Entity<RemoteSessions>,
    pub client: RemoteClient,
    pub kv: Kv,
    peers: Rc<dyn RemotePeers>,
}

impl Global for RemoteGlobal {}

impl RemoteGlobal {
    /// Create the entities, install the global, and fill in the runtime's
    /// `RemoteHooks`. `Engine::init` must have run.
    pub fn init(config: RemoteConfig, cx: &mut App) {
        let client = RemoteClient::new(config.transport);
        let connections = {
            let (client, kv, clock) = (client.clone(), config.kv.clone(), config.clock);
            cx.new(|cx| RemoteConnections::new(client, kv, clock, cx))
        };
        let sessions = {
            let connections = connections.clone();
            cx.new(|cx| RemoteSessions::new(connections, cx))
        };
        if Engine::try_global(cx).is_some() {
            Engine::set_hooks(cx, |hooks| hooks.remote = Rc::new(DesktopRemoteHooks));
        }
        cx.set_global(RemoteGlobal {
            connections,
            sessions,
            client,
            kv: config.kv,
            peers: Rc::new(DefaultRemotePeers),
        });
    }

    /// `init` with the saved connections in `<data_dir>/remote-machines.json`.
    pub fn init_native(kv: Kv, data_dir: PathBuf, cx: &mut App) {
        Self::init(
            RemoteConfig {
                transport: Arc::new(NativeTransport::new(data_dir)),
                kv,
                clock: Arc::new(now_ms),
            },
            cx,
        );
    }

    pub fn global(cx: &App) -> &RemoteGlobal {
        cx.global::<RemoteGlobal>()
    }

    pub fn try_global(cx: &App) -> Option<&RemoteGlobal> {
        cx.try_global::<RemoteGlobal>()
    }

    /// The `RemoteConnections` entity.
    pub fn connections(cx: &App) -> Entity<RemoteConnections> {
        Self::global(cx).connections.clone()
    }

    /// The `RemoteSessions` entity.
    pub fn sessions(cx: &App) -> Entity<RemoteSessions> {
        Self::global(cx).sessions.clone()
    }

    /// The calls this package makes into others.
    pub fn peers(cx: &App) -> Rc<dyn RemotePeers> {
        Self::try_global(cx)
            .map(|remote| remote.peers.clone())
            .unwrap_or_else(|| Rc::new(DefaultRemotePeers))
    }

    /// Replace the calls this package makes into others.
    pub fn set_peers(cx: &mut App, peers: Rc<dyn RemotePeers>) {
        cx.global_mut::<RemoteGlobal>().peers = peers;
    }

    // What the other packages ask about remote tabs.

    /// `remoteProjectFor(session.cwd)`: the session runs on a remote host.
    pub fn is_remote_session(session: &Session, cx: &App) -> bool {
        Self::is_remote_project(&session.cwd, cx)
    }

    /// `remoteProjectFor(cwd)`.
    pub fn is_remote_project(cwd: &str, cx: &App) -> bool {
        Self::try_global(cx)
            .is_some_and(|remote| remote_projects::remote_project_for(&remote.kv, cwd).is_some())
    }

    /// `remoteSessionFor`: the host session a tab shows.
    pub fn remote_session_for(shell_id: &str, cx: &App) -> Option<String> {
        Self::try_global(cx)
            .and_then(|remote| connections::remote_session_for(&remote.kv, shell_id))
    }

    /// `rememberRemoteSession`, announcing `REMOTE_HISTORY_CHANGE`.
    pub fn remember_remote_session(shell_id: &str, session_id: Option<&str>, cx: &mut App) {
        let Some(connections) = Self::try_global(cx).map(|remote| remote.connections.clone())
        else {
            return;
        };
        connections.update(cx, |connections, cx| {
            connections.remember_remote_session(shell_id, session_id, cx)
        });
    }

    /// `remotePendingWorktree`.
    pub fn remote_pending_worktree(shell_id: &str, cx: &App) -> Option<String> {
        Self::try_global(cx)
            .and_then(|remote| connections::remote_pending_worktree(&remote.kv, shell_id))
    }

    /// `rememberRemotePendingWorktree`.
    pub fn remember_remote_pending_worktree(shell_id: &str, path: Option<&str>, cx: &App) {
        if let Some(remote) = Self::try_global(cx) {
            connections::remember_remote_pending_worktree(&remote.kv, shell_id, path);
        }
    }

    /// A remote tab's pane is closing (`WorkspaceDelegate::
    /// remember_remote_session`): forget its host session and pending
    /// worktree, and stop polling for it.
    pub fn forget_tab(shell_id: &str, cx: &mut App) {
        let Some(sessions) = Self::try_global(cx).map(|remote| remote.sessions.clone()) else {
            return;
        };
        Self::remember_remote_pending_worktree(shell_id, None, cx);
        Self::remember_remote_session(shell_id, None, cx);
        sessions.update(cx, |sessions, _| sessions.close(shell_id));
    }

    /// `cachedRemoteSessionSummary` for a remote tab: its host session's
    /// title and provider, from the project's last session list.
    pub fn cached_summary(
        session: &Session,
        cx: &App,
    ) -> Option<monocode_remote::host::protocol::HostSessionSummary> {
        let remote = Self::try_global(cx)?;
        let host_id = connections::remote_session_for(&remote.kv, &session.id)?;
        connections::cached_remote_session_summary(&remote.kv, &session.cwd, &host_id)
    }

    /// `remoteTabCwd`.
    pub fn remote_tab_cwd(project: &str, shell_id: Option<&str>, cx: &App) -> Option<String> {
        Self::try_global(cx)
            .and_then(|remote| connections::remote_tab_cwd(&remote.kv, project, shell_id))
    }

    fn open_session(shell_id: &str, cx: &App) -> Option<Entity<RemoteSession>> {
        Self::try_global(cx)?.sessions.read(cx).session(shell_id)
    }

    /// `remoteSessionActions(sessionId)?.approve`.
    pub fn approve(shell_id: &str, request_id: i64, decision: ApprovalDecision, cx: &mut App) {
        if let Some(session) = Self::open_session(shell_id, cx) {
            session.update(cx, |session, cx| session.approve(request_id, decision, cx));
        }
    }

    /// `remoteSessionActions(sessionId)?.answer`.
    pub fn answer(shell_id: &str, request_id: i64, reply: &UserQuestionReply, cx: &mut App) {
        if let Some(session) = Self::open_session(shell_id, cx) {
            let reply = reply.clone();
            session.update(cx, |session, cx| session.answer(request_id, reply, cx));
        }
    }

    /// `remoteSessionActions(sessionId)?.submit`.
    pub fn submit(
        shell_id: &str,
        text: &str,
        attachments: &[monocode_core::Attachment],
        options: &RemoteTurnOptions,
        cx: &mut App,
    ) -> bool {
        let Some(session) = Self::open_session(shell_id, cx) else {
            return false;
        };
        session.update(cx, |session, cx| {
            session.submit(text, attachments.to_vec(), options, cx)
        })
    }

    /// `remoteSessionActions(sessionId)?.saveDraft`.
    pub fn save_draft(
        shell_id: &str,
        text: &str,
        attachments: &[monocode_core::Attachment],
        cx: &mut App,
    ) -> bool {
        let Some(session) = Self::open_session(shell_id, cx) else {
            return false;
        };
        session.update(cx, |session, cx| {
            session.save_draft(text, attachments.to_vec(), cx)
        })
    }

    /// `remoteSessionActions(sessionId)?.compact`.
    pub fn compact(shell_id: &str, cx: &mut App) -> bool {
        let Some(session) = Self::open_session(shell_id, cx) else {
            return false;
        };
        session.update(cx, |session, cx| session.compact(cx))
    }

    /// `remoteSessionActions(sessionId)?.stop`.
    pub fn stop(shell_id: &str, cx: &mut App) {
        if let Some(session) = Self::open_session(shell_id, cx) {
            session.update(cx, |session, cx| session.stop(cx));
        }
    }

    /// `buildRemotePlan`.
    pub fn build_plan(
        shell_id: &str,
        block_id: &str,
        target: Option<&monocode_core::block::PlanBuildTarget>,
        cx: &mut App,
    ) {
        if let Some(session) = Self::open_session(shell_id, cx) {
            session.update(cx, |session, cx| session.build_plan(block_id, target, cx));
        }
    }
}

/// `invokeWorkspace` for the engine: `Some` with the host's answer when the
/// command's paths live on a connected machine, `None` when the caller runs
/// the command on this computer. With no remote package installed, a remote
/// path fails with the TypeScript's "connect this project's machine" error.
pub fn invoke_workspace(
    command: &str,
    args: &Map<String, Value>,
    cx: &App,
) -> Option<RemoteFuture<Value>> {
    match RemoteGlobal::try_global(cx) {
        Some(remote) => remote.client.invoke_workspace(command, args),
        None => remote_commands::is_remote_workspace_call(args).then(|| {
            let error = remote_commands::NO_RUNNER.to_string();
            Box::pin(async move { Err(error) }) as RemoteFuture<Value>
        }),
    }
}
