//! Port of the live half of src/features/connections/model/connections.ts
//! as the `RemoteConnections` entity: the machine list
//! (`useRemoteMachines`), pairing, retry, and disconnect, machine status
//! (`useRemoteMachineOnline`), the `changes.wait` long poll per machine
//! (`watchRemoteChanges`), and each remote project's session list
//! (`useRemoteProjectSessions`). It also keeps the caches RemoteSession.tsx
//! held at module level: host descriptors, model catalogs, recent session
//! snapshots, and usage limit choices.
//!
//! Window events become `RemoteEvent`s. Watches are reference counted like
//! the TypeScript watchers: each `watch_*` call returns a `Subscription`,
//! and dropping the last one for a machine stops its loop.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::pin::pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use gpui::{AsyncApp, Context, EventEmitter, Subscription, Task, WeakEntity};
use monocode_remote::host::protocol::{
    HostDescriptor, HostModelCatalog, HostProject, HostSession, HostSessionSummary, RemoteMachine,
    SessionChanges, SshSetup,
};
use monocode_settings::Kv;
use serde_json::{Value, json};

use super::Clock;
use super::client::{RemoteClient, decode};
use super::connections::{self, cache_sessions, cached_sessions};
use super::remote_projects::{self, RemoteProject};
use super::remote_turns::RemoteChangesDetail;
use super::transport::RemoteFuture;

/// What changed, for views and the other remote entities. Each variant
/// replaces a window event the TypeScript dispatched.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteEvent {
    /// `monocode:remote-machines`: the machine list was read again.
    MachinesChanged,
    /// `monocode:remote-machine-status`: a machine went on or offline.
    StatusChanged { machine_id: String },
    /// `monocode:remote-changes`: a watched machine reported session writes.
    Changes(RemoteChangesDetail),
    /// `monocode:remote-history`: a tab's host session binding changed, or
    /// the session lists should reload.
    HistoryChange,
    /// `monocode:remote-history-updated`: a project's session list was saved.
    HistoryUpdated { project: String },
    /// `monocode:remote-projects-changed`.
    ProjectsChanged,
    /// `monocode:open-connections`: show the connections settings.
    OpenConnections,
    /// `monocode:open-remote-project`: show the open-folder-on-a-machine dialog.
    OpenRemoteProject,
}

/// What a remote session tab chose for a usage limit notice, kept per limit
/// so a later limit shows again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LimitChoice {
    pub limit: String,
    pub dismissed: Option<bool>,
    pub resume_at_reset: Option<bool>,
}

/// `RemoteProjectSessions`: a remote project's host sessions, keeping the
/// last list visible while the machine is unreachable.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteProjectSessions {
    /// `None` when this machine is not connected on this computer.
    pub machine: Option<RemoteMachine>,
    pub sessions: Vec<HostSessionSummary>,
    pub loaded: bool,
}

struct ChangeWatcher {
    count: usize,
    live: bool,
    _task: Task<()>,
}

struct StatusWatcher {
    count: usize,
    _task: Task<()>,
}

struct ProjectWatch {
    count: usize,
    project_id: Option<String>,
    machine: Option<RemoteMachine>,
    sessions: Vec<HostSessionSummary>,
    loaded: bool,
    in_flight: bool,
    again: bool,
    wake: Option<async_channel::Sender<()>>,
    _changes: Option<Subscription>,
    _task: Option<Task<()>>,
}

type Watchers<T> = Rc<RefCell<HashMap<String, T>>>;

/// At most this many host snapshots stay cached for tabs that reopen.
const SNAPSHOT_CACHE: usize = 8;

/// Paired machines and what this desktop knows about them.
pub struct RemoteConnections {
    client: RemoteClient,
    kv: Kv,
    clock: Clock,
    machines: Vec<RemoteMachine>,
    loaded: bool,
    machines_task: Option<Task<()>>,
    online: HashMap<String, bool>,
    change_watchers: Watchers<ChangeWatcher>,
    status_watchers: Watchers<StatusWatcher>,
    project_watches: Watchers<ProjectWatch>,
    descriptors: HashMap<String, HostDescriptor>,
    catalogs: HashMap<String, HostModelCatalog>,
    snapshots: VecDeque<(String, Arc<HostSession>)>,
    limit_choices: HashMap<String, LimitChoice>,
}

impl EventEmitter<RemoteEvent> for RemoteConnections {}

/// `Math.min(cap, base * 2 ** failures)` in milliseconds.
fn backoff(base: u64, failures: u32, cap: u64) -> Duration {
    Duration::from_millis(base.saturating_mul(1 << failures.min(20)).min(cap))
}

impl RemoteConnections {
    pub fn new(client: RemoteClient, kv: Kv, clock: Clock, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            machines: client.cached_machines(),
            loaded: client.machines_loaded(),
            client,
            kv,
            clock,
            machines_task: None,
            online: HashMap::new(),
            change_watchers: Rc::default(),
            status_watchers: Rc::default(),
            project_watches: Rc::default(),
            descriptors: HashMap::new(),
            catalogs: HashMap::new(),
            snapshots: VecDeque::new(),
            limit_choices: HashMap::new(),
        };
        this.refresh_machines(cx);
        this
    }

    pub fn client(&self) -> &RemoteClient {
        &self.client
    }

    pub fn kv(&self) -> &Kv {
        &self.kv
    }

    /// `Date.now()`.
    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }

    // The machine list.

    /// The paired machines, as last read.
    pub fn machines(&self) -> &[RemoteMachine] {
        &self.machines
    }

    /// The list was read at least once, or failed to read.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// The connected machine for an environment.
    pub fn machine_for_environment(&self, environment_id: &str) -> Option<&RemoteMachine> {
        self.machines
            .iter()
            .find(|machine| machine.environment_id == environment_id)
    }

    /// `refreshRemoteMachines`: read the list again. A temporary failure
    /// keeps the last list instead of blanking every remote panel.
    pub fn refresh_machines(&mut self, cx: &mut Context<Self>) {
        let read = self.client.load_machines();
        self.machines_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            this.update(cx, |this, cx| {
                if let Ok(machines) = result {
                    this.machines = machines;
                }
                this.loaded = true;
                this.machines_task = None;
                this.machines_changed(cx);
            })
            .ok();
        }));
    }

    fn machines_changed(&mut self, cx: &mut Context<Self>) {
        self.restart_project_watches(false, cx);
        cx.emit(RemoteEvent::MachinesChanged);
        cx.notify();
    }

    /// `pairMachine`: pair the machine in a `monocode://pair` link.
    pub fn pair(
        &mut self,
        link: String,
        name: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<RemoteMachine, String>> {
        let pair = self.client.pair(link, name);
        cx.spawn(async move |this, cx| {
            let machine = pair.await?;
            this.update(cx, |this, cx| {
                this.machines = this.client.cached_machines();
                this.loaded = true;
                this.machines_changed(cx);
                this.refresh_machines(cx);
            })
            .ok();
            Ok(machine)
        })
    }

    /// `retryMachine`: try every route to the machine again on its next
    /// request.
    pub fn retry(&self, machine_id: &str) -> RemoteFuture<()> {
        self.client.retry(machine_id)
    }

    /// `disconnectMachine`: remove the saved connection from this desktop.
    pub fn disconnect(
        &mut self,
        machine_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let disconnect = self.client.disconnect(machine_id);
        cx.spawn(async move |this, cx| {
            disconnect.await?;
            this.update(cx, |this, cx| {
                this.machines = this.client.cached_machines();
                this.machines_changed(cx);
                this.refresh_machines(cx);
            })
            .ok();
            Ok(())
        })
    }

    /// `remoteRequest`.
    pub fn request(&self, machine_id: &str, method: &str, params: Value) -> RemoteFuture<Value> {
        self.client.request(machine_id, method, params)
    }

    /// SSH setup (`remote_ssh_begin`).
    pub fn ssh_begin(
        &self,
        target: String,
        name: String,
        port: Option<u16>,
        upgrade: bool,
    ) -> RemoteFuture<String> {
        self.client.ssh_begin(target, name, port, upgrade)
    }

    /// `remote_ssh_reconnect`.
    pub fn ssh_reconnect(&self, machine_id: &str, upgrade: bool) -> RemoteFuture<String> {
        self.client.ssh_reconnect(machine_id, upgrade)
    }

    /// `remote_ssh_poll`.
    pub fn ssh_poll(&self, job_id: &str) -> RemoteFuture<SshSetup> {
        self.client.ssh_poll(job_id)
    }

    /// `remote_ssh_answer`.
    pub fn ssh_answer(&self, job_id: &str, prompt_id: &str, answer: String) -> RemoteFuture<()> {
        self.client.ssh_answer(job_id, prompt_id, answer)
    }

    /// `remote_ssh_cancel`.
    pub fn ssh_cancel(&self, job_id: &str) -> RemoteFuture<()> {
        self.client.ssh_cancel(job_id)
    }

    /// `OPEN_CONNECTIONS_EVENT`.
    pub fn open_connections(&mut self, cx: &mut Context<Self>) {
        cx.emit(RemoteEvent::OpenConnections);
    }

    /// `OPEN_REMOTE_PROJECT_EVENT`.
    pub fn open_remote_project(&mut self, cx: &mut Context<Self>) {
        cx.emit(RemoteEvent::OpenRemoteProject);
    }

    // Machine status.

    /// Whether a machine answered its latest request; `None` until the first
    /// check returns.
    pub fn online(&self, machine_id: &str) -> Option<bool> {
        self.online.get(machine_id).copied()
    }

    /// `reportRemoteMachineStatus`: record whether a machine answered its
    /// latest request, for every view that shows its connection state.
    pub fn report_status(&mut self, machine_id: &str, online: bool, cx: &mut Context<Self>) {
        if self.online.get(machine_id) == Some(&online) {
            return;
        }
        self.online.insert(machine_id.to_string(), online);
        cx.emit(RemoteEvent::StatusChanged {
            machine_id: machine_id.to_string(),
        });
        cx.notify();
    }

    /// `watchMachineStatus` (through `useRemoteMachineOnline`): check the
    /// machine every 15 seconds, backing off while it is unreachable.
    pub fn watch_machine_status(
        &mut self,
        machine_id: &str,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let mut watchers = self.status_watchers.borrow_mut();
        if let Some(watcher) = watchers.get_mut(machine_id) {
            watcher.count += 1;
        } else {
            let client = self.client.clone();
            let id = machine_id.to_string();
            let task = cx.spawn(async move |this, cx| {
                let mut failures: u32 = 0;
                loop {
                    let answer = client.request(&id, "environment.describe", json!({})).await;
                    let online = answer.is_ok();
                    failures = if online { 0 } else { (failures + 1).min(4) };
                    if this
                        .update(cx, |this, cx| this.report_status(&id, online, cx))
                        .is_err()
                    {
                        break;
                    }
                    let delay = if failures > 0 {
                        backoff(3_000, failures, 30_000)
                    } else {
                        Duration::from_millis(15_000)
                    };
                    cx.background_executor().timer(delay).await;
                }
            });
            watchers.insert(
                machine_id.to_string(),
                StatusWatcher {
                    count: 1,
                    _task: task,
                },
            );
        }
        drop(watchers);
        release(&self.status_watchers, machine_id, |watcher| {
            &mut watcher.count
        })
    }

    // The change feed.

    /// `remoteChangesLive`: pushed changes for this machine are arriving, so
    /// views can poll rarely. False for hosts before 0.5, which do not
    /// support `changes.wait`.
    pub fn remote_changes_live(&self, machine_id: &str) -> bool {
        self.change_watchers
            .borrow()
            .get(machine_id)
            .is_some_and(|watcher| watcher.live)
    }

    /// `watchRemoteChanges`: hold one `changes.wait` request open per machine
    /// while anything watches it, and emit `RemoteEvent::Changes` for each
    /// batch of session writes.
    pub fn watch_remote_changes(
        &mut self,
        machine_id: &str,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let mut watchers = self.change_watchers.borrow_mut();
        if let Some(watcher) = watchers.get_mut(machine_id) {
            watcher.count += 1;
        } else {
            let task = self.spawn_change_feed(machine_id.to_string(), cx);
            watchers.insert(
                machine_id.to_string(),
                ChangeWatcher {
                    count: 1,
                    live: false,
                    _task: task,
                },
            );
        }
        drop(watchers);
        release(&self.change_watchers, machine_id, |watcher| {
            &mut watcher.count
        })
    }

    fn spawn_change_feed(&self, machine_id: String, cx: &mut Context<Self>) -> Task<()> {
        let client = self.client.clone();
        // Weak: the watcher map owns this task.
        let watchers = Rc::downgrade(&self.change_watchers);
        let key = machine_id.clone();
        let set_live = move |live: bool| {
            if let Some(watchers) = watchers.upgrade()
                && let Some(watcher) = watchers.borrow_mut().get_mut(&key)
            {
                watcher.live = live;
            }
        };
        cx.spawn(async move |this, cx| change_feed(this, client, machine_id, set_live, cx).await)
    }

    /// Dispatch one batch of session writes: project lists reload, then
    /// subscribers hear about it.
    pub fn dispatch_changes(&mut self, detail: RemoteChangesDetail, cx: &mut Context<Self>) {
        let wakes: Vec<async_channel::Sender<()>> = {
            let mut watches = self.project_watches.borrow_mut();
            watches
                .values_mut()
                .filter(|watch| {
                    watch
                        .machine
                        .as_ref()
                        .is_some_and(|machine| machine.id == detail.machine_id)
                        && (detail.reset
                            || detail.sessions.iter().any(|entry| {
                                Some(entry.project_id.as_str()) == watch.project_id.as_deref()
                            }))
                })
                .filter_map(|watch| {
                    if watch.in_flight {
                        watch.again = true;
                        None
                    } else {
                        watch.wake.clone()
                    }
                })
                .collect()
        };
        for wake in wakes {
            let _ = wake.try_send(());
        }
        cx.emit(RemoteEvent::Changes(detail));
    }

    // Tab bindings and remote projects.

    /// `remoteSessionFor`.
    pub fn remote_session_for(&self, shell_id: &str) -> Option<String> {
        connections::remote_session_for(&self.kv, shell_id)
    }

    /// `rememberRemoteSession`: bind a tab to a host session (`None` for a
    /// new session) and announce `REMOTE_HISTORY_CHANGE`.
    pub fn remember_remote_session(
        &mut self,
        shell_id: &str,
        session_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        connections::remember_remote_session(&self.kv, shell_id, session_id);
        self.history_changed(cx);
    }

    /// `refreshRemoteProjectSessions`.
    pub fn refresh_remote_project_sessions(&mut self, cx: &mut Context<Self>) {
        self.history_changed(cx);
    }

    fn history_changed(&mut self, cx: &mut Context<Self>) {
        self.restart_project_watches(true, cx);
        cx.emit(RemoteEvent::HistoryChange);
        cx.notify();
    }

    /// `remotePendingWorktree`.
    pub fn remote_pending_worktree(&self, shell_id: &str) -> Option<String> {
        connections::remote_pending_worktree(&self.kv, shell_id)
    }

    /// `rememberRemotePendingWorktree`.
    pub fn remember_remote_pending_worktree(&self, shell_id: &str, path: Option<&str>) {
        connections::remember_remote_pending_worktree(&self.kv, shell_id, path);
    }

    /// `remoteTabCwd`.
    pub fn remote_tab_cwd(&self, project: &str, shell_id: Option<&str>) -> Option<String> {
        connections::remote_tab_cwd(&self.kv, project, shell_id)
    }

    /// `cachedRemoteSessionSummary`.
    pub fn cached_remote_session_summary(
        &self,
        project: &str,
        session_id: &str,
    ) -> Option<HostSessionSummary> {
        connections::cached_remote_session_summary(&self.kv, project, session_id)
    }

    /// `remoteProjectFor`.
    pub fn remote_project_for(&self, path: &str) -> Option<RemoteProject> {
        remote_projects::remote_project_for(&self.kv, path)
    }

    /// `remoteProjectsOn`.
    pub fn remote_projects_on(&self, environment_id: &str) -> Vec<RemoteProject> {
        remote_projects::remote_projects_on(&self.kv, environment_id)
    }

    /// `rememberRemoteProject`, announcing `REMOTE_PROJECTS_CHANGED`.
    pub fn remember_remote_project(
        &mut self,
        environment_id: &str,
        project: &HostProject,
        cx: &mut Context<Self>,
    ) -> RemoteProject {
        let remote = remote_projects::remember_remote_project(&self.kv, environment_id, project);
        self.restart_project_watches(false, cx);
        cx.emit(RemoteEvent::ProjectsChanged);
        cx.notify();
        remote
    }

    // Project session lists.

    /// `useRemoteProjectSessions`: the current list for a project. Views
    /// call `watch_project_sessions` while they show it.
    pub fn project_sessions(&self, project: &str) -> RemoteProjectSessions {
        if let Some(watch) = self.project_watches.borrow().get(project) {
            return RemoteProjectSessions {
                machine: watch.machine.clone(),
                sessions: watch.sessions.clone(),
                loaded: watch.loaded,
            };
        }
        let remote = self.remote_project_for(project);
        RemoteProjectSessions {
            machine: remote
                .as_ref()
                .and_then(|remote| self.machine_for_environment(&remote.environment_id))
                .cloned(),
            sessions: if remote.is_some() {
                cached_sessions(&self.kv, project)
            } else {
                Vec::new()
            },
            loaded: false,
        }
    }

    /// Keep a project's session list current while the subscription lives:
    /// reload on pushed changes, and poll every 3 seconds (30 with pushed
    /// changes), backing off while the machine is unreachable.
    pub fn watch_project_sessions(
        &mut self,
        project: &str,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let existing = self
            .project_watches
            .borrow_mut()
            .get_mut(project)
            .map(|watch| watch.count += 1)
            .is_some();
        if !existing {
            self.project_watches.borrow_mut().insert(
                project.to_string(),
                ProjectWatch {
                    count: 1,
                    project_id: None,
                    machine: None,
                    sessions: Vec::new(),
                    loaded: false,
                    in_flight: false,
                    again: false,
                    wake: None,
                    _changes: None,
                    _task: None,
                },
            );
            self.restart_project_watch(project, true, cx);
        }
        release(&self.project_watches, project, |watch| &mut watch.count)
    }

    fn restart_project_watches(&mut self, force: bool, cx: &mut Context<Self>) {
        let projects: Vec<String> = self.project_watches.borrow().keys().cloned().collect();
        for project in projects {
            self.restart_project_watch(&project, force, cx);
        }
    }

    /// The effect in `useRemoteProjectSessions`, which re-ran when the
    /// project's host id, its machine, or the refresh counter changed.
    fn restart_project_watch(&mut self, project: &str, force: bool, cx: &mut Context<Self>) {
        let remote = self.remote_project_for(project);
        let machine = remote
            .as_ref()
            .and_then(|remote| self.machine_for_environment(&remote.environment_id))
            .cloned();
        let project_id = remote.as_ref().map(|remote| remote.project_id.clone());
        {
            let watches = self.project_watches.borrow();
            let Some(watch) = watches.get(project) else {
                return;
            };
            let same = watch.project_id == project_id
                && watch.machine.as_ref().map(|machine| &machine.id)
                    == machine.as_ref().map(|machine| &machine.id);
            if same && !force {
                return;
            }
        }
        let sessions = if remote.is_some() {
            cached_sessions(&self.kv, project)
        } else {
            Vec::new()
        };
        let (changes, task, wake) = match (&remote, &machine) {
            (Some(remote), Some(machine)) => {
                let (wake, woken) = async_channel::bounded(1);
                let changes = self.watch_remote_changes(&machine.id, cx);
                let task = self.spawn_project_poll(
                    project.to_string(),
                    remote.project_id.clone(),
                    machine.id.clone(),
                    woken,
                    cx,
                );
                (Some(changes), Some(task), Some(wake))
            }
            _ => (None, None, None),
        };
        // Replace the old loop outside the borrow: dropping its change watch
        // touches another map.
        let old = {
            let mut watches = self.project_watches.borrow_mut();
            let Some(watch) = watches.get_mut(project) else {
                return;
            };
            let old = (watch._changes.take(), watch._task.take());
            watch.project_id = project_id;
            watch.machine = machine;
            watch.sessions = sessions;
            watch.loaded = false;
            watch.in_flight = false;
            watch.again = false;
            watch.wake = wake;
            watch._changes = changes;
            watch._task = task;
            old
        };
        drop(old);
        cx.notify();
    }

    fn spawn_project_poll(
        &self,
        project: String,
        project_id: String,
        machine_id: String,
        woken: async_channel::Receiver<()>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let client = self.client.clone();
        // Weak: the watch map owns this task.
        let watches = Rc::downgrade(&self.project_watches);
        cx.spawn(async move |this, cx| {
            let mut failures: u32 = 0;
            loop {
                update_watch(&watches, &project, |watch| watch.in_flight = true);
                let next: Result<Vec<HostSessionSummary>, String> = client
                    .request_as(
                        &machine_id,
                        "sessions.list",
                        json!({ "projectId": project_id }),
                    )
                    .await;
                let Ok((again, live)) = this.update(cx, |this, cx| {
                    match next {
                        Ok(next) => {
                            failures = 0;
                            cache_sessions(&this.kv, &project, &next);
                            update_watch(&watches, &project, |watch| {
                                watch.sessions = next;
                                watch.loaded = true;
                            });
                            cx.emit(RemoteEvent::HistoryUpdated {
                                project: project.clone(),
                            });
                            cx.notify();
                        }
                        // Keep the cached list and back off while the machine
                        // is unreachable.
                        Err(_) => failures = (failures + 1).min(4),
                    }
                    let again = update_watch(&watches, &project, |watch| {
                        watch.in_flight = false;
                        std::mem::take(&mut watch.again)
                    })
                    .unwrap_or(false);
                    (again, this.remote_changes_live(&machine_id))
                }) else {
                    break;
                };
                if again {
                    continue;
                }
                // Pushed changes refresh the list at once; polling only
                // covers a missed change or an older host.
                let delay = if failures > 0 {
                    backoff(3_000, failures, 30_000)
                } else if live {
                    Duration::from_millis(30_000)
                } else {
                    Duration::from_millis(3_000)
                };
                let timer = cx.background_executor().timer(delay);
                let wake = woken.recv();
                if let futures::future::Either::Right((Err(_), _)) =
                    futures::future::select(pin!(timer), pin!(wake)).await
                {
                    break;
                }
            }
        })
    }

    // Caches RemoteSession.tsx kept at module level.

    /// `cachedDescriptors.get(machineId)`.
    pub fn descriptor(&self, machine_id: &str) -> Option<&HostDescriptor> {
        self.descriptors.get(machine_id)
    }

    pub fn set_descriptor(&mut self, machine_id: &str, descriptor: HostDescriptor) {
        self.descriptors.insert(machine_id.to_string(), descriptor);
    }

    /// `cachedCatalogs.get(catalogKey(machineId, projectId))`.
    pub fn catalog(&self, machine_id: &str, project_id: &str) -> Option<&HostModelCatalog> {
        self.catalogs.get(&catalog_key(machine_id, project_id))
    }

    pub fn set_catalog(&mut self, machine_id: &str, project_id: &str, catalog: HostModelCatalog) {
        self.catalogs
            .insert(catalog_key(machine_id, project_id), catalog);
    }

    /// A recent host snapshot for a tab that opens it again.
    pub fn cached_snapshot(&self, machine_id: &str, session_id: &str) -> Option<Arc<HostSession>> {
        let key = snapshot_key(machine_id, session_id);
        self.snapshots
            .iter()
            .find(|(entry, _)| *entry == key)
            .map(|(_, snapshot)| snapshot.clone())
    }

    /// `rememberSessionSnapshot`: keep the eight most recent.
    pub fn remember_snapshot(
        &mut self,
        machine_id: &str,
        session_id: &str,
        snapshot: Arc<HostSession>,
    ) {
        let key = snapshot_key(machine_id, session_id);
        self.snapshots.retain(|(entry, _)| *entry != key);
        self.snapshots.push_back((key, snapshot));
        if self.snapshots.len() > SNAPSHOT_CACHE {
            self.snapshots.pop_front();
        }
    }

    pub fn forget_snapshot(&mut self, machine_id: &str, session_id: &str) {
        let key = snapshot_key(machine_id, session_id);
        self.snapshots.retain(|(entry, _)| *entry != key);
    }

    /// `preloadRemoteSession`: fetch a host conversation into the snapshot
    /// cache, so its tab opens with the transcript already laid out.
    pub fn preload_remote_session(
        &mut self,
        machine_id: &str,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        if self.cached_snapshot(machine_id, session_id).is_some() {
            return Task::ready(Ok(()));
        }
        let load = self
            .client
            .load_remote_session(machine_id, session_id, None);
        let (machine_id, session_id) = (machine_id.to_string(), session_id.to_string());
        cx.spawn(async move |this, cx| {
            let snapshot = load.await?;
            this.update(cx, |this, _| {
                this.remember_snapshot(&machine_id, &session_id, snapshot)
            })
            .map_err(|error| error.to_string())
        })
    }

    /// `limitChoices.get(key)`.
    pub fn limit_choice(&self, key: &str) -> Option<&LimitChoice> {
        self.limit_choices.get(key)
    }

    pub fn set_limit_choice(&mut self, key: &str, choice: LimitChoice) {
        self.limit_choices.insert(key.to_string(), choice);
    }
}

/// `snapshotKey`.
pub fn snapshot_key(machine_id: &str, session_id: &str) -> String {
    format!("{machine_id}:{session_id}")
}

/// `catalogKey`.
pub fn catalog_key(machine_id: &str, project_id: &str) -> String {
    serde_json::to_string(&[machine_id, project_id]).unwrap_or_default()
}

/// Change one project watch, if it is still there.
fn update_watch<R>(
    watches: &Weak<RefCell<HashMap<String, ProjectWatch>>>,
    project: &str,
    update: impl FnOnce(&mut ProjectWatch) -> R,
) -> Option<R> {
    let watches = watches.upgrade()?;
    let mut map = watches.borrow_mut();
    map.get_mut(project).map(update)
}

/// A subscription that releases one reference to a watcher, and removes it
/// with its loop when none remain.
fn release<T: 'static>(
    watchers: &Watchers<T>,
    key: &str,
    count: impl Fn(&mut T) -> &mut usize + 'static,
) -> Subscription {
    let watchers = Rc::downgrade(watchers);
    let key = key.to_string();
    Subscription::new(move || {
        let Some(watchers) = watchers.upgrade() else {
            return;
        };
        let removed = {
            let mut map = watchers.borrow_mut();
            let Some(watcher) = map.get_mut(&key) else {
                return;
            };
            let count = count(watcher);
            *count = count.saturating_sub(1);
            if *count > 0 {
                return;
            }
            map.remove(&key)
        };
        // Drop outside the borrow: a project watch holds a change watch.
        drop(removed);
    })
}

/// The `changes.wait` loop for one machine.
async fn change_feed(
    this: WeakEntity<RemoteConnections>,
    client: RemoteClient,
    machine_id: String,
    set_live: impl Fn(bool),
    cx: &mut AsyncApp,
) {
    let mut boot: Option<String> = None;
    let mut cursor: i64 = 0;
    let mut failures: u32 = 0;
    loop {
        let mut params = json!({ "after": cursor });
        if let Some(boot) = &boot {
            params["boot"] = json!(boot);
        }
        let answer = match client.request(&machine_id, "changes.wait", params).await {
            Ok(value) => decode::<SessionChanges>(value),
            Err(error) => Err(error),
        };
        match answer {
            Ok(changes) => {
                failures = 0;
                set_live(true);
                // The first answer only establishes the cursor. A later reset
                // means the host restarted, so views reload.
                let reset = changes.reset && boot.is_some();
                if !changes.sessions.is_empty() || reset {
                    let detail = RemoteChangesDetail {
                        machine_id: machine_id.clone(),
                        sessions: changes.sessions,
                        reset,
                    };
                    if this
                        .update(cx, |this, cx| this.dispatch_changes(detail, cx))
                        .is_err()
                    {
                        break;
                    }
                }
                boot = Some(changes.boot);
                cursor = changes.cursor;
            }
            Err(reason) => {
                set_live(false);
                // An older host: keep polling in the views instead.
                if reason.contains("Unsupported host method") {
                    break;
                }
                failures = (failures + 1).min(5);
                cx.background_executor()
                    .timer(backoff(1_000, failures, 30_000))
                    .await;
            }
        }
    }
}
