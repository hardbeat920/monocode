//! Port of App.tsx's remote session sync (lines 2271-2300, 3177-3196, and
//! 10400-10444), `useRemoteTurnUpdates` from
//! src/features/connections/model/remoteTurns.ts, and the action registry
//! in remoteSessionActions.ts, as the `RemoteSessions` entity.
//!
//! - Host snapshots merge into `Sessions` under the tab's own ID
//!   (`onRemoteSnapshot`), and the last one per tab is kept with its host
//!   revision.
//! - Every open remote tab's turn state stays current, including tabs that
//!   are not showing: a pushed change marks a tab busy, and a finished turn
//!   loads its final snapshot once and is announced like a local turn.
//! - `open` gives the `RemoteSession` entity for a tab, which is how other
//!   packages reach a remote tab's submit, approve, and answer actions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{App, AppContext, Context, Entity, Subscription, Task};
use monocode_core::Session;
use monocode_layout::paths::is_remote_project_path;
use monocode_remote::host::protocol::HostSession;

use super::RemoteGlobal;
use super::remote_connections::{RemoteConnections, RemoteEvent};
use super::remote_session::{RemoteSession, RemoteSessionEvent};
use super::remote_session_state::remote_session_state;
use super::remote_turns::{RemoteChangesDetail, RemoteTurnTarget, remote_turn_updates};
use crate::runtime::engine::Engine;

/// The title a tab shows after its draft-only host session was discarded.
pub const NEW_REMOTE_SESSION_TITLE: &str = "New remote session";

/// What a tab in a remote project can show (the outer `RemoteSession`
/// component).
#[derive(Clone)]
pub enum RemoteTab {
    /// The project's machine details are missing.
    MissingProject,
    /// The machine list has not loaded yet.
    Connecting,
    /// The machine is not paired on this computer.
    NotConnected,
    Connected(Entity<RemoteSession>),
}

impl RemoteTab {
    /// The placeholder text for a tab that cannot show its session.
    pub fn message(&self) -> Option<&'static str> {
        match self {
            Self::MissingProject => Some(
                "This project’s machine details are missing. Add the project again from the project rail.",
            ),
            Self::Connecting => Some("Connecting to the machine…"),
            Self::NotConnected => {
                Some("The machine for this project isn’t connected on this computer.")
            }
            Self::Connected(_) => None,
        }
    }

    /// Whether to offer "Manage machines" (`OPEN_CONNECTIONS_EVENT`).
    pub fn offers_manage_machines(&self) -> bool {
        matches!(self, Self::NotConnected)
    }
}

struct OpenTab {
    machine_id: String,
    project_key: String,
    session: Entity<RemoteSession>,
    _snapshots: Subscription,
}

/// Remote tabs and their host sessions.
pub struct RemoteSessions {
    connections: Entity<RemoteConnections>,
    open: HashMap<String, OpenTab>,
    /// `lastRemoteSnapshot`: the last host snapshot merged per tab.
    last_snapshots: HashMap<String, Arc<HostSession>>,
    turn_watches: HashMap<String, Subscription>,
    turn_loads: HashMap<String, Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl RemoteSessions {
    pub fn new(connections: Entity<RemoteConnections>, cx: &mut Context<Self>) -> Self {
        let mut subscriptions =
            vec![
                cx.subscribe(&connections, |this, _, event, cx| match event {
                    RemoteEvent::Changes(detail) => this.turn_changes(detail, cx),
                    RemoteEvent::MachinesChanged
                    | RemoteEvent::HistoryChange
                    | RemoteEvent::ProjectsChanged => this.refresh_turn_watches(cx),
                    _ => {}
                }),
            ];
        if let Some(engine) = Engine::try_global(cx) {
            let sessions = engine.sessions.clone();
            subscriptions.push(cx.observe(&sessions, |this, _, cx| {
                this.prune(cx);
                this.refresh_turn_watches(cx);
            }));
        }
        let mut this = Self {
            connections,
            open: HashMap::new(),
            last_snapshots: HashMap::new(),
            turn_watches: HashMap::new(),
            turn_loads: HashMap::new(),
            _subscriptions: subscriptions,
        };
        this.refresh_turn_watches(cx);
        this
    }

    // Tabs.

    /// The remote session for a tab, created on first use. Call it each
    /// time the tab draws; it also records whether the tab is visible.
    pub fn open(&mut self, shell: &Session, visible: bool, cx: &mut Context<Self>) -> RemoteTab {
        let (project, machine, loaded) = {
            let connections = self.connections.read(cx);
            let project = connections.remote_project_for(&shell.cwd);
            let machine = project
                .as_ref()
                .and_then(|project| connections.machine_for_environment(&project.environment_id))
                .cloned();
            (project, machine, connections.loaded())
        };
        let Some(project) = project else {
            return RemoteTab::MissingProject;
        };
        let Some(machine) = machine else {
            return if loaded {
                RemoteTab::NotConnected
            } else {
                RemoteTab::Connecting
            };
        };
        // The TypeScript keyed the component by machine and tab, so another
        // machine or project starts over.
        if let Some(tab) = self.open.get(&shell.id)
            && tab.machine_id == machine.id
            && tab.project_key == project.key
        {
            let session = tab.session.clone();
            session.update(cx, |session, cx| session.set_visible(visible, cx));
            return RemoteTab::Connected(session);
        }
        let connections = self.connections.clone();
        let shell = shell.clone();
        let machine_id = machine.id.clone();
        let project_key = project.key.clone();
        let shell_id = shell.id.clone();
        let session =
            cx.new(|cx| RemoteSession::new(connections, shell, machine, project, visible, cx));
        let snapshots = cx.subscribe(&session, |this, session, event, cx| {
            let RemoteSessionEvent::Snapshot(snapshot) = event;
            let shell_id = session.read(cx).shell_id().to_string();
            this.on_remote_snapshot(&shell_id, snapshot.clone(), cx);
        });
        self.open.insert(
            shell_id,
            OpenTab {
                machine_id,
                project_key,
                session: session.clone(),
                _snapshots: snapshots,
            },
        );
        RemoteTab::Connected(session)
    }

    /// `remoteSessionActions(shellId)`: the tab's remote session, while it is
    /// open.
    pub fn session(&self, shell_id: &str) -> Option<Entity<RemoteSession>> {
        self.open.get(shell_id).map(|tab| tab.session.clone())
    }

    /// Stop polling a tab's host session. Tabs whose session leaves
    /// `Sessions` close on their own.
    pub fn close(&mut self, shell_id: &str) {
        self.open.remove(shell_id);
    }

    fn prune(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = Engine::try_global(cx) else {
            return;
        };
        let sessions = engine.sessions.read(cx);
        self.open.retain(|shell_id, _| sessions.contains(shell_id));
        self.last_snapshots
            .retain(|shell_id, _| sessions.contains(shell_id));
    }

    /// The last host snapshot merged into this tab.
    pub fn host_snapshot(&self, shell_id: &str) -> Option<Arc<HostSession>> {
        self.last_snapshots.get(shell_id).cloned()
    }

    /// The host revision of the last snapshot merged into this tab.
    pub fn host_revision(&self, shell_id: &str) -> Option<i64> {
        self.last_snapshots
            .get(shell_id)
            .map(|snapshot| snapshot.revision)
    }

    // App.tsx.

    /// `onRemoteSnapshot`: show a host snapshot in the tab's session. `None`
    /// means the tab's draft-only host session was discarded. A host turn
    /// that ends is announced like a local one, whether or not its tab is
    /// showing.
    pub fn on_remote_snapshot(
        &mut self,
        shell_id: &str,
        snapshot: Option<Arc<HostSession>>,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = Engine::try_global(cx) else {
            return;
        };
        let sessions = engine.sessions.clone();
        let Some(snapshot) = snapshot else {
            self.last_snapshots.remove(shell_id);
            sessions.update(cx, |sessions, cx| {
                sessions.update(shell_id, cx, |entry| {
                    entry.title = NEW_REMOTE_SESSION_TITLE.into();
                    entry.blocks = Vec::new();
                    entry.busy = Some(false);
                });
            });
            return;
        };
        if self
            .last_snapshots
            .get(shell_id)
            .is_some_and(|last| Arc::ptr_eq(last, &snapshot))
        {
            return;
        }
        self.last_snapshots
            .insert(shell_id.to_string(), snapshot.clone());
        let Some(shell) = sessions.read(cx).get(shell_id).cloned() else {
            return;
        };
        let finished = shell.is_busy() && !snapshot.session.is_busy();
        let Some(project) = self.connections.read(cx).remote_project_for(&shell.cwd) else {
            return;
        };
        let next = remote_session_state(&shell, &snapshot, &project);
        sessions.update(cx, |sessions, cx| {
            sessions.update(shell_id, cx, |entry| *entry = next);
        });
        if finished {
            RemoteGlobal::peers(cx).announce_finished_later(shell_id, cx);
        }
    }

    /// `markRemoteBusy`: a turn started on the host in a tab that looks idle.
    pub fn mark_remote_busy(&mut self, shell_id: &str, cx: &mut Context<Self>) {
        let Some(engine) = Engine::try_global(cx) else {
            return;
        };
        engine.sessions.clone().update(cx, |sessions, cx| {
            if sessions.get(shell_id).is_some_and(|entry| !entry.is_busy()) {
                sessions.update(shell_id, cx, |entry| entry.busy = Some(true));
            }
        });
    }

    /// The tab already showing a host session, for `onSelectRemoteSession`.
    /// `None` means the app should reserve a new tab and bind it with
    /// `bind_tab`: an apparently blank remote tab may hold composer text or a
    /// create the host has not accepted.
    pub fn tab_for_remote_session(
        &self,
        remote_session_id: &str,
        shell_ids: &[String],
        cx: &App,
    ) -> Option<String> {
        let connections = self.connections.read(cx);
        shell_ids
            .iter()
            .find(|shell_id| {
                connections.remote_session_for(shell_id).as_deref() == Some(remote_session_id)
            })
            .cloned()
    }

    /// `rememberRemoteSession` for a tab the app just created to show a host
    /// session.
    pub fn bind_tab(&mut self, shell_id: &str, remote_session_id: &str, cx: &mut Context<Self>) {
        self.connections.update(cx, |connections, cx| {
            connections.remember_remote_session(shell_id, Some(remote_session_id), cx)
        });
    }

    // Turn updates.

    /// Every open tab that shows a host session on a connected machine.
    pub fn turn_targets(&self, cx: &App) -> Vec<RemoteTurnTarget> {
        let Some(engine) = Engine::try_global(cx) else {
            return Vec::new();
        };
        let connections = self.connections.read(cx);
        engine
            .sessions
            .read(cx)
            .all()
            .iter()
            .filter(|session| is_remote_project_path(&session.cwd))
            .filter_map(|session| {
                let project = connections.remote_project_for(&session.cwd)?;
                let machine = connections.machine_for_environment(&project.environment_id)?;
                let host_session_id = connections.remote_session_for(&session.id)?;
                Some(RemoteTurnTarget {
                    shell_id: session.id.clone(),
                    machine_id: machine.id.clone(),
                    host_session_id,
                    busy: session.is_busy(),
                })
            })
            .collect()
    }

    /// Hold a change watch on each machine that has an open remote tab.
    fn refresh_turn_watches(&mut self, cx: &mut Context<Self>) {
        let machines: HashSet<String> = self
            .turn_targets(cx)
            .into_iter()
            .map(|target| target.machine_id)
            .collect();
        let stale: Vec<String> = self
            .turn_watches
            .keys()
            .filter(|machine| !machines.contains(*machine))
            .cloned()
            .collect();
        for machine in stale {
            self.turn_watches.remove(&machine);
        }
        for machine in machines {
            if self.turn_watches.contains_key(&machine) {
                continue;
            }
            let watch = self.connections.update(cx, |connections, cx| {
                connections.watch_remote_changes(&machine, cx)
            });
            self.turn_watches.insert(machine, watch);
        }
    }

    fn turn_changes(&mut self, detail: &RemoteChangesDetail, cx: &mut Context<Self>) {
        let targets = self.turn_targets(cx);
        let updates = remote_turn_updates(&targets, detail);
        for shell_id in &updates.started {
            self.mark_remote_busy(shell_id, cx);
        }
        let client = self.connections.read(cx).client().clone();
        for target in updates.finished {
            let known = self.last_snapshots.get(&target.shell_id).cloned();
            let load =
                client.load_remote_session(&target.machine_id, &target.host_session_id, known);
            let shell_id = target.shell_id.clone();
            let task = cx.spawn(async move |this, cx| {
                // On failure the tab reloads when it is shown.
                let snapshot = load.await;
                this.update(cx, |this, cx| {
                    this.turn_loads.remove(&shell_id);
                    if let Ok(snapshot) = snapshot {
                        this.on_remote_snapshot(&shell_id, Some(snapshot), cx);
                    }
                })
                .ok();
            });
            self.turn_loads.insert(target.shell_id, task);
        }
    }
}
