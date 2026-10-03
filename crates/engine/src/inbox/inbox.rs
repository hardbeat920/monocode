//! The `Inbox` entity: the app-wide inbox state the views observe.
//!
//! It ports `useInboxActivity` from src/features/inbox/hooks/useInboxUnseen.ts
//! (the background poll behind the Inbox badge and the linked-session
//! updates) and the inbox actions in src/app/App.tsx: starting a session from
//! an item, the temporary Ask conversation and its restart, CI repairs,
//! linking a session to a work item, the linked work item panels, the
//! linked-activity card, and the related-session lists.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::{LocalBoxFuture, Shared};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Task};
use monocode_core::block::BlockRole;
use monocode_core::inbox::{InboxAskContext, LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus};
use monocode_core::session::{LinkedWorkItem, Session};

use super::ci_repair::CiRepairRequest;
use super::ci_repair_sessions::{ci_repair_sessions, session_unavailable_for_repair};
use super::ci_repair_tracking::{CiRepairOutcome, CiRepairTracker, TrackedCiRepair};
use super::client::{InboxClient, InboxSignal, SignalSubscription};
use super::github_tasks::{inbox_composer_card, inbox_item_key, inbox_list_cache_key};
use super::hooks::{InboxHooks, NoopInboxHooks};
use super::inbox_ask::inbox_ask_key;
use super::inbox_filters::{
    apply_inbox_filters, inbox_fetch_state, load_inbox_filters, prune_inbox_filters,
};
use super::inbox_notifications::{
    InboxNotificationSubject, InboxNotificationTracker, inbox_notification_subject,
};
use super::inbox_seen::{InboxSeenEntry, RememberedInboxItem};
use super::jira::load_hidden_jira_project_ids;
use super::linear::load_hidden_linear_team_ids;
use super::linked_session_updates::{
    LinkedSessionUpdate, LinkedWorkItemTarget, linked_session_updates, linked_work_item_targets,
    linked_work_item_update_key,
};
use super::linked_work_item_activity::{
    complete_linked_work_item_update_card, fail_linked_work_item_update_card,
    pending_linked_work_item_update_card,
};
use super::rail::{RecentProject, inbox_projects_for_rail};
use super::session_work_item::linked_work_item_from_inbox_item;
use super::time::date_parse;
use super::types::{GithubWorkItem, InboxItem, InboxProvider, InboxQuery, work_kind};
use crate::runtime::Engine;
use crate::runtime::session_history::summary_from_session;
use crate::runtime::session_store::SessionSummary;
use crate::runtime::util::concurrent::for_each_concurrent;
use crate::runtime::util::project_path::same_project_path;

/// `POLL_MS`.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);
/// `FALLBACK_REFRESH_MS`.
const FALLBACK_REFRESH_MS: i64 = 60_000;
/// `MAX_CONCURRENT_LOOKUPS`.
const MAX_CONCURRENT_LOOKUPS: usize = 3;

/// A cloneable foreground result, shared by everyone who asks for the same
/// thing while it runs.
pub type LocalPending<T> = Shared<LocalBoxFuture<'static, Result<T, String>>>;

/// What the views hear besides `cx.notify()`.
#[derive(Debug, Clone, PartialEq)]
pub enum InboxEvent {
    /// The open Ask conversation got a fresh session (`setInboxAskPortal`).
    AskSessionReplaced { from: String, to: String },
}

/// `LinkedWorkItemPanelState`: a linked issue or PR beside its session.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedWorkItemPanel {
    pub item: LinkedWorkItem,
    pub session_id: String,
    pub cwd: String,
    pub ci_repair: Option<CiRepairRequest>,
}

/// The background poll of `useInboxActivity`.
#[derive(Default)]
struct Activity {
    recents: Vec<RecentProject>,
    cwd: String,
    sessions: Vec<SessionSummary>,
    projects: Vec<String>,
    effect_key: Option<String>,
    generation: u64,
    pulling: bool,
    pull_again: bool,
    poll: Option<Task<()>>,
    pull_task: Option<Task<()>>,
    tracker: InboxNotificationTracker,
    entries: Vec<(InboxSeenEntry, InboxNotificationSubject)>,
    fallback_fetched_at: HashMap<String, i64>,
    unseen: bool,
    /// Linked GitHub items by `linkedWorkItemUpdateKey`, as the poll last
    /// saw them.
    work_items: HashMap<String, InboxItem>,
    updates: Vec<LinkedSessionUpdate>,
    update_ids: HashSet<String>,
}

/// The app-wide inbox state.
pub struct Inbox {
    client: InboxClient,
    hooks: Rc<dyn InboxHooks>,
    ci_repairs: CiRepairTracker,
    activity: Activity,
    linked_panels: Vec<LinkedWorkItemPanel>,
    linked_panel_request: u64,
    opening_asks: HashMap<String, LocalPending<String>>,
    ask_session: Option<String>,
    linked_activity_fetches: HashMap<String, i64>,
    stored_linked_sessions: Vec<SessionSummary>,
    _signals: SignalSubscription,
    _signal_task: Task<()>,
}

impl EventEmitter<InboxEvent> for Inbox {}

struct GlobalInbox(Entity<Inbox>);

impl Global for GlobalInbox {}

/// `.then(ok, err)` on a foreground task, shared by every caller.
fn local_pending<T: Clone + 'static>(task: Task<Result<T, String>>) -> LocalPending<T> {
    task.boxed_local().shared()
}

impl Inbox {
    /// Create the entity and install it as the app's inbox.
    pub fn init(client: InboxClient, cx: &mut App) -> Entity<Inbox> {
        let inbox = cx.new(|cx| Inbox::new(client, cx));
        cx.set_global(GlobalInbox(inbox.clone()));
        inbox
    }

    pub fn global(cx: &App) -> Entity<Inbox> {
        cx.global::<GlobalInbox>().0.clone()
    }

    pub fn try_global(cx: &App) -> Option<Entity<Inbox>> {
        cx.try_global::<GlobalInbox>()
            .map(|global| global.0.clone())
    }

    pub fn new(client: InboxClient, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = async_channel::unbounded::<InboxSignal>();
        let signals = client.subscribe(move |signal| {
            let _ = sender.try_send(signal);
        });
        let signal_task = cx.spawn(async move |this, cx| {
            while let Ok(signal) = receiver.recv().await {
                if this
                    .update(cx, |this, cx| this.handle_signal(signal, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            ci_repairs: CiRepairTracker::new(client.kv().clone()),
            client,
            hooks: Rc::new(NoopInboxHooks),
            activity: Activity::default(),
            linked_panels: Vec::new(),
            linked_panel_request: 0,
            opening_asks: HashMap::new(),
            ask_session: None,
            linked_activity_fetches: HashMap::new(),
            stored_linked_sessions: Vec::new(),
            _signals: signals,
            _signal_task: signal_task,
        }
    }

    /// Fill in the hooks the rest of the app provides.
    pub fn set_hooks(&mut self, hooks: Rc<dyn InboxHooks>) {
        self.hooks = hooks;
    }

    /// Use another CI repair history (tests use a fixed clock).
    pub fn set_ci_repair_tracker(&mut self, tracker: CiRepairTracker) {
        self.ci_repairs = tracker;
    }

    pub fn client(&self) -> &InboxClient {
        &self.client
    }

    pub fn hooks(&self) -> Rc<dyn InboxHooks> {
        self.hooks.clone()
    }

    fn handle_signal(&mut self, signal: InboxSignal, cx: &mut Context<Self>) {
        match signal {
            InboxSignal::Seen => self.apply_unseen(cx),
            InboxSignal::SelfActivity | InboxSignal::JiraChange => {
                if !self.activity.projects.is_empty() && self.activity.poll.is_some() {
                    self.pull(true, cx);
                }
            }
            InboxSignal::LinkedSessionSeen => self.recompute_linked_updates(cx),
            InboxSignal::CiRepairs => cx.notify(),
            InboxSignal::LinearChange
            | InboxSignal::GitlabChange
            | InboxSignal::AzureDevOpsChange => {}
        }
        cx.notify();
    }

    // The background poll (`useInboxActivity`).

    /// The Inbox badge: an unread item in a project whose notifications show
    /// badges.
    pub fn unseen(&self) -> bool {
        self.activity.unseen
    }

    /// Linked sessions whose GitHub item changed since they last looked, in
    /// session order (`linkedSessionUpdates`).
    pub fn linked_session_updates(&self) -> &[LinkedSessionUpdate] {
        &self.activity.updates
    }

    /// `linkedSessionUpdates.get(sessionId)`.
    pub fn linked_session_update(&self, session_id: &str) -> Option<&LinkedSessionUpdate> {
        self.activity
            .updates
            .iter()
            .find(|update| update.session_id == session_id)
    }

    /// The updates that show an indicator (`linkedSessionUpdateIds`).
    pub fn linked_session_update_ids(&self) -> &HashSet<String> {
        &self.activity.update_ids
    }

    /// The rail projects the poll covers.
    pub fn activity_projects(&self) -> &[String] {
        &self.activity.projects
    }

    /// The poll's inputs: the recent projects, the sidebar folder, and the
    /// sidebar's session history (`useInboxActivity(recents, cwd, sessions)`).
    /// A change of projects or linked targets restarts the poll.
    pub fn set_activity_inputs(
        &mut self,
        recents: Vec<RecentProject>,
        cwd: String,
        sessions: Vec<SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        let target_key = linked_work_item_targets(&sessions)
            .iter()
            .map(|target| target.key.clone())
            .collect::<Vec<_>>()
            .join("\0");
        let recents_key = recents
            .iter()
            .map(|project| format!("{}\u{1}{}", project.path, project.opened_at))
            .collect::<Vec<_>>()
            .join("\0");
        let effect_key = format!("{cwd}\u{2}{recents_key}\u{2}{target_key}");
        self.activity.recents = recents;
        self.activity.cwd = cwd;
        self.activity.sessions = sessions;
        self.recompute_linked_updates(cx);
        if self.activity.effect_key.as_deref() != Some(effect_key.as_str()) {
            self.activity.effect_key = Some(effect_key);
            self.restart_activity(cx);
        }
    }

    /// The window became visible again (`visibilitychange`).
    pub fn window_became_visible(&mut self, cx: &mut Context<Self>) {
        if !self.activity.projects.is_empty() && self.activity.poll.is_some() {
            self.pull(true, cx);
        }
    }

    /// Notification preferences changed: re-check the badge and the
    /// linked-session indicators without touching read state.
    pub fn notification_preferences_changed(&mut self, cx: &mut Context<Self>) {
        self.apply_unseen(cx);
        self.recompute_linked_updates(cx);
    }

    /// Run a poll now (`pull(true)`).
    pub fn refresh_activity(&mut self, cx: &mut Context<Self>) {
        if !self.activity.projects.is_empty() {
            self.pull(true, cx);
        }
    }

    fn restart_activity(&mut self, cx: &mut Context<Self>) {
        let activity = &mut self.activity;
        activity.generation += 1;
        activity.pulling = false;
        activity.pull_again = false;
        activity.poll = None;
        activity.pull_task = None;
        let now = self.client.now();
        activity.projects = inbox_projects_for_rail(&activity.recents, &activity.cwd, now)
            .into_iter()
            .map(|project| project.path)
            .collect();
        if activity.projects.is_empty() {
            activity.entries.clear();
            self.apply_unseen(cx);
            return;
        }
        self.pull(false, cx);
        // Keep polling while hidden: inbox automations ride this refresh.
        let generation = self.activity.generation;
        self.activity.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let alive = this
                    .update(cx, |this, cx| {
                        if this.activity.generation == generation {
                            this.pull(true, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
    }

    fn subject(&self, item: &InboxItem) -> InboxNotificationSubject {
        inbox_notification_subject(
            self.hooks.notification_project_id(item),
            item.kind,
            &item.updated_at,
        )
    }

    fn apply_unseen(&mut self, cx: &mut Context<Self>) {
        let entries: Vec<InboxSeenEntry> = self
            .activity
            .entries
            .iter()
            .filter(|(_, subject)| self.hooks.allows_notification_indicator(subject, cx))
            .map(|(entry, _)| entry.clone())
            .collect();
        let unseen = self.client.inbox_has_unseen_items(&entries);
        if unseen != self.activity.unseen {
            self.activity.unseen = unseen;
            cx.notify();
        }
    }

    fn recompute_linked_updates(&mut self, cx: &mut Context<Self>) {
        let github: HashMap<String, GithubWorkItem> = self
            .activity
            .work_items
            .iter()
            .filter_map(|(key, item)| item.to_github().map(|github| (key.clone(), github)))
            .collect();
        let client = self.client.clone();
        let updates = linked_session_updates(&self.activity.sessions, &github, &|id| {
            client.linked_session_seen_at(id)
        });
        let ids: HashSet<String> = updates
            .iter()
            .filter(|update| {
                let key = linked_work_item_update_key(
                    &update.item.repo,
                    update.item.kind,
                    update.item.number,
                );
                let item = self
                    .activity
                    .work_items
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| {
                        InboxItem::from_github(&update.item, "", InboxProvider::Github)
                    });
                let mut item = item;
                item.provider = InboxProvider::Github;
                self.hooks
                    .allows_notification_indicator(&self.subject(&item), cx)
            })
            .map(|update| update.session_id.clone())
            .collect();
        if updates != self.activity.updates || ids != self.activity.update_ids {
            self.activity.updates = updates;
            self.activity.update_ids = ids;
            cx.notify();
        }
    }

    fn merge_snapshots(&mut self, snapshots: Vec<(String, InboxItem)>, cx: &mut Context<Self>) {
        let mut changed = false;
        for (key, item) in snapshots {
            if self
                .activity
                .work_items
                .get(&key)
                .is_some_and(|current| current.updated_at == item.updated_at)
            {
                continue;
            }
            self.activity.work_items.insert(key, item);
            changed = true;
        }
        if changed {
            self.recompute_linked_updates(cx);
        }
    }

    fn pull(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.activity.pulling {
            self.activity.pull_again |= force;
            return;
        }
        self.activity.pulling = true;
        let generation = self.activity.generation;
        let projects = self.activity.projects.clone();
        let kv = self.client.kv();
        let filters = prune_inbox_filters(&load_inbox_filters(kv), &projects);
        let query = InboxQuery {
            assigned_to_me: filters.assigned_to_me,
            state: inbox_fetch_state(&filters),
            search: String::new(),
            linear_hidden_team_ids: Some(load_hidden_linear_team_ids(kv)),
            jira_hidden_project_ids: Some(load_hidden_jira_project_ids(kv)),
        };
        let listed = self.client.list_inbox_items(&projects, &query, force);
        let client = self.client.clone();
        self.activity.pull_task = Some(cx.spawn(async move |this, cx| {
            let result = listed.await;
            let missing = this
                .update(cx, |this, cx| match result {
                    Ok(listed) if this.activity.generation == generation => {
                        let scope = inbox_list_cache_key(&projects, &query);
                        this.apply_listed(listed, &scope, &filters, cx)
                    }
                    _ => None,
                })
                .ok()
                .flatten();
            if let Some((cwd, missing)) = missing {
                let found = fetch_fallback_updates(&client, &cwd, &missing).await;
                let _ = this.update(cx, |this, cx| {
                    if this.activity.generation == generation && !found.is_empty() {
                        this.merge_snapshots(found, cx);
                    }
                });
            }
            let _ = this.update(cx, |this, cx| {
                if this.activity.generation != generation {
                    return;
                }
                this.activity.pulling = false;
                if this.activity.pull_again {
                    this.activity.pull_again = false;
                    this.pull(true, cx);
                }
            });
        }));
    }

    /// The synchronous part of a successful poll. Returns the linked targets
    /// the list omitted, for an exact lookup.
    fn apply_listed(
        &mut self,
        listed: super::types::InboxListResult,
        scope: &str,
        filters: &super::inbox_filters::InboxFilters,
        cx: &mut Context<Self>,
    ) -> Option<(String, Vec<LinkedWorkItemTarget>)> {
        let client = self.client.clone();
        let hooks = self.hooks.clone();
        let now = client.now();
        let visible = apply_inbox_filters(&listed.items, filters, "", now, None);
        hooks.remember_notification_projects(&listed.items, cx);
        let project_id = |item: &InboxItem| hooks.notification_project_id(item);
        let observed = self.activity.tracker.observe(
            &listed.items,
            scope,
            &listed.errors.providers(),
            &project_id,
        );
        // Called on every successful poll so retained automation claims can
        // be retried even when nothing newly appeared.
        hooks.inbox_appeared(&observed.appeared, cx);
        let self_authored: Vec<&InboxItem> = observed
            .changed
            .iter()
            .filter(|item| client.consume_inbox_self_activity(item))
            .collect();
        let self_keys: HashSet<String> = self_authored
            .iter()
            .map(|item| inbox_item_key(item))
            .collect();
        let visible_keys: HashSet<String> = visible.iter().map(inbox_item_key).collect();
        // Every change is observed even when its cue is suppressed. At most
        // one eligible project chimes; muted projects cannot take that slot.
        for item in &observed.changed {
            let key = inbox_item_key(item);
            if self_keys.contains(&key) || !visible_keys.contains(&key) {
                continue;
            }
            if hooks.play_inbox_cue(&self.subject(item), cx) {
                break;
            }
        }
        self.activity.entries = visible
            .iter()
            .map(|item| {
                (
                    InboxSeenEntry::new(inbox_item_key(item), item.updated_at.clone()),
                    self.subject(item),
                )
            })
            .collect();
        client.remember_inbox_items(
            &listed
                .items
                .iter()
                .map(|item| RememberedInboxItem {
                    key: inbox_item_key(item),
                    updated_at: item.updated_at.clone(),
                    project_path: item.project_path.clone(),
                })
                .collect::<Vec<_>>(),
        );
        let entries: Vec<InboxSeenEntry> = self
            .activity
            .entries
            .iter()
            .map(|(entry, _)| entry.clone())
            .collect();
        client.seed_inbox_seen_if_needed(&entries);
        let self_entries: Vec<InboxSeenEntry> = entries
            .iter()
            .filter(|entry| self_keys.contains(&entry.key))
            .cloned()
            .collect();
        if !self_entries.is_empty() {
            client.mark_inbox_items_seen(&self_entries);
        }
        for item in &self_authored {
            let Some(kind) =
                work_kind(item.kind).filter(|_| item.provider == InboxProvider::Github)
            else {
                continue;
            };
            let Some(updated_at) = date_parse(&item.updated_at) else {
                continue;
            };
            let key = linked_work_item_update_key(&item.repo, kind, item.number);
            for session in &self.activity.sessions {
                if session.linked_work_item.as_ref().is_some_and(|linked| {
                    linked_work_item_update_key(&linked.repo, linked.kind, linked.number) == key
                }) {
                    client.mark_linked_session_update_seen(&session.id, updated_at);
                }
            }
        }
        self.apply_unseen(cx);

        let targets = linked_work_item_targets(&self.activity.sessions);
        let target_keys: HashSet<&str> = targets.iter().map(|target| target.key.as_str()).collect();
        let mut listed_keys: HashSet<String> = HashSet::new();
        let mut snapshots: Vec<(String, InboxItem)> = Vec::new();
        for item in &listed.items {
            let Some(kind) =
                work_kind(item.kind).filter(|_| item.provider == InboxProvider::Github)
            else {
                continue;
            };
            let key = linked_work_item_update_key(&item.repo, kind, item.number);
            if !target_keys.contains(key.as_str()) {
                continue;
            }
            listed_keys.insert(key.clone());
            if date_parse(&item.updated_at).is_some() {
                snapshots.push((key, item.clone()));
            }
        }
        if !snapshots.is_empty() {
            self.merge_snapshots(snapshots, cx);
        }
        let missing: Vec<LinkedWorkItemTarget> = targets
            .into_iter()
            .filter(|target| {
                if listed_keys.contains(&target.key) {
                    return false;
                }
                let last = self
                    .activity
                    .fallback_fetched_at
                    .get(&target.key)
                    .copied()
                    .unwrap_or(0);
                if now - last < FALLBACK_REFRESH_MS {
                    return false;
                }
                self.activity
                    .fallback_fetched_at
                    .insert(target.key.clone(), now);
                true
            })
            .collect();
        cx.notify();
        (!missing.is_empty()).then(|| (self.activity.cwd.clone(), missing))
    }

    // Read state.

    /// `markInboxItemSeen` for one opened card.
    pub fn mark_item_seen(&mut self, item: &InboxItem, cx: &mut Context<Self>) {
        self.client.mark_inbox_item_seen(&InboxSeenEntry::new(
            inbox_item_key(item),
            item.updated_at.clone(),
        ));
        self.apply_unseen(cx);
    }

    /// `markInboxItemsSeen`. Returns false when the read state could not be
    /// saved.
    pub fn mark_items_seen(&mut self, items: &[InboxItem], cx: &mut Context<Self>) -> bool {
        let entries: Vec<InboxSeenEntry> = items
            .iter()
            .map(|item| InboxSeenEntry::new(inbox_item_key(item), item.updated_at.clone()))
            .collect();
        let saved = self.client.mark_inbox_items_seen(&entries);
        self.apply_unseen(cx);
        saved
    }

    /// `isInboxEntryUnseen` for a card.
    pub fn is_item_unseen(&self, item: &InboxItem) -> bool {
        self.client.is_inbox_entry_unseen(&InboxSeenEntry::new(
            inbox_item_key(item),
            item.updated_at.clone(),
        ))
    }

    /// `markLinkedSessionUpdateSeen`: the session read this remote snapshot.
    pub fn mark_linked_session_update_seen(
        &mut self,
        session_id: &str,
        remote_updated_at: i64,
        cx: &mut Context<Self>,
    ) {
        self.client
            .mark_linked_session_update_seen(session_id, remote_updated_at);
        self.recompute_linked_updates(cx);
    }

    // CI repairs.

    /// `getCiRepairs`.
    pub fn ci_repairs(&mut self) -> Vec<TrackedCiRepair> {
        self.ci_repairs.get_ci_repairs().to_vec()
    }

    /// `rebaseCiRepairs`: a project folder moved.
    pub fn rebase_ci_repairs(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.ci_repairs.rebase_ci_repairs(from, to);
        cx.notify();
    }

    /// The `storage` event for CI repair keys written by another process.
    pub fn ci_repair_storage_changed(&mut self, key: Option<&str>, cx: &mut Context<Self>) {
        if self.ci_repairs.handle_storage_event(key) {
            cx.notify();
        }
    }

    /// `ciRepairSessions`: the chats a repair may run in.
    pub fn repair_sessions(&self, history: &[SessionSummary], cx: &App) -> Vec<SessionSummary> {
        ci_repair_sessions(history, Engine::sessions(cx).read(cx).all())
    }

    /// `onRepairChecks`: start a repair in `session_id`, or in a new chat for
    /// the PR's project, then open that chat.
    pub fn repair_checks(
        &mut self,
        item: InboxItem,
        request: CiRepairRequest,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let cwd = item.project_path.clone();
        if cwd.is_empty() {
            return Task::ready(Err("Choose a local project for this PR first.".into()));
        }
        let hooks = self.hooks.clone();
        cx.spawn(async move |this, cx| {
            let session = match &session_id {
                Some(id) => {
                    let opening = cx.update(|cx| {
                        Engine::sessions(cx).update(cx, |sessions, cx| sessions.ensure_open(id, cx))
                    });
                    opening.await
                }
                None => None,
            };
            if session_id.is_some()
                && session.as_ref().is_none_or(|session| {
                    session.inbox_ask.is_some()
                        || session.orchestration_lead_id.is_some()
                        || !same_project_path(&session.cwd, &cwd)
                })
            {
                return Err("Choose a chat from this project.".into());
            }
            if session.as_ref().is_some_and(session_unavailable_for_repair) {
                return Err("This chat is busy. Choose another chat or start a new one.".into());
            }
            let repair_session_id = match session {
                Some(session) => session.id,
                None => cx.update(|cx| {
                    let mut session = hooks.new_default_session(&cwd, true, cx);
                    session.title = format!("Fix CI #{}: {}", item.number, item.title);
                    session.linked_work_item = linked_work_item_from_inbox_item(&item);
                    let id = session.id.clone();
                    Engine::sessions(cx).update(cx, |sessions, cx| {
                        sessions.insert(session, cx);
                    });
                    id
                }),
            };
            let entity = this.upgrade().ok_or("The inbox closed")?;
            cx.update(|cx| {
                track_ci_repair(&entity, &cwd, &request, &repair_session_id, &hooks, cx)
            })?;
            let select = cx.update(|cx| {
                hooks.leave_inbox(cx);
                hooks.show_sessions_sidebar(Some(&cwd), cx);
                hooks.select_session(&repair_session_id, cx)
            });
            select.await;
            Ok(())
        })
    }

    // Sessions from items.

    /// `onStartInboxItem`: a new chat seeded with the item, in its project.
    pub fn start_inbox_item(
        &mut self,
        item: InboxItem,
        body: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, String>> {
        let description = self.client.inbox_tracker_description(&item, body);
        let hooks = self.hooks.clone();
        cx.spawn(async move |_, cx| {
            let description = description.await?;
            Ok(cx.update(|cx| {
                hooks.leave_inbox(cx);
                let cwd = if item.project_path.is_empty() {
                    hooks.start_cwd(cx)
                } else {
                    item.project_path.clone()
                };
                hooks.show_sessions_sidebar(Some(&cwd), cx);
                let mut session = hooks.new_default_session(&cwd, true, cx);
                session.title = format!("{} {}", item.item_ref(), item.title);
                session.inbox_card = Some(inbox_composer_card(&item, description.as_deref()));
                if let Some(linked) = linked_work_item_from_inbox_item(&item) {
                    session.linked_work_item = Some(linked);
                }
                let id = session.id.clone();
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.insert(session, cx);
                });
                hooks.open_session_tab(&id, &cwd, cx);
                id
            }))
        })
    }

    /// The open Ask conversation's session, if any (`inboxAskPortal`).
    pub fn ask_session(&self) -> Option<&str> {
        self.ask_session.as_deref()
    }

    /// The Ask conversation the Inbox shows now.
    pub fn set_ask_session(&mut self, session_id: Option<String>, cx: &mut Context<Self>) {
        if self.ask_session != session_id {
            self.ask_session = session_id;
            cx.notify();
        }
    }

    /// `onAskInboxItem`: the item's temporary conversation, created once.
    pub fn ask_inbox_item(
        &mut self,
        item: InboxItem,
        cx: &mut Context<Self>,
    ) -> LocalPending<String> {
        let key = inbox_ask_key(&item);
        if let Some(pending) = self.opening_asks.get(&key) {
            return pending.clone();
        }
        let client = self.client.clone();
        let hooks = self.hooks.clone();
        let ask_key = key.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = open_ask_session(&client, &hooks, &item, &ask_key, cx).await;
            let _ = this.update(cx, |this, _| {
                this.opening_asks.remove(&ask_key);
            });
            result
        });
        let pending = local_pending(task);
        self.opening_asks.insert(key, pending.clone());
        pending
    }

    /// `onRestartInboxAsk`: stop and delete the conversation, then start a
    /// blank one for the same item.
    pub fn restart_inbox_ask(
        &mut self,
        item: InboxItem,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, String>> {
        let opening = self.ask_inbox_item(item, cx);
        let hooks = self.hooks.clone();
        cx.spawn(async move |this, cx| {
            let id = opening.await?;
            let sessions = cx.update(|cx| Engine::sessions(cx));
            let (current, stop) = sessions.update(cx, |sessions, cx| {
                let current = sessions.get(&id).cloned();
                sessions.begin_removal(&id);
                (current, sessions.stop_for_removal(&id, cx))
            });
            let result: Result<String, String> = async {
                let stopped = stop.await.or(current).ok_or("The conversation is gone")?;
                let forgets = cx.update(|cx| {
                    let engine = Engine::hooks(cx);
                    engine
                        .harness
                        .session_child_harnesses(&stopped)
                        .into_iter()
                        .map(|harness| engine.harness.forget_session(harness, &id, cx))
                        .collect::<Vec<_>>()
                });
                futures::future::join_all(forgets).await;
                let image_paths: Vec<String> = stopped
                    .blocks
                    .iter()
                    .filter(|block| block.role == BlockRole::Image)
                    .filter_map(|block| block.image.as_ref().map(|image| image.path.clone()))
                    .collect();
                let delete = cx.update(|cx| Engine::writer(cx).delete_session(&id, image_paths));
                delete.await?;
                let fresh = cx.update(|cx| {
                    let mut fresh = hooks.new_session_like(&stopped, cx);
                    fresh.title = stopped.title.clone();
                    fresh.inbox_ask = stopped.inbox_ask.clone();
                    fresh
                });
                let fresh_id = fresh.id.clone();
                sessions.update(cx, |sessions, cx| {
                    let mut fresh = Some(fresh);
                    sessions.update_all(
                        cx,
                        |session| if session.id == id { fresh.take() } else { None },
                    );
                });
                let _ = this.update(cx, |this, cx| {
                    if this.ask_session.as_deref() == Some(id.as_str()) {
                        this.ask_session = Some(fresh_id.clone());
                        cx.emit(InboxEvent::AskSessionReplaced {
                            from: id.clone(),
                            to: fresh_id.clone(),
                        });
                        cx.notify();
                    }
                });
                Ok(fresh_id)
            }
            .await;
            sessions.update(cx, |sessions, _| sessions.end_removal(&id));
            result
        })
    }

    /// `onInboxCardDismiss`.
    pub fn dismiss_inbox_card(&mut self, session_id: &str, cx: &mut Context<Self>) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            if sessions
                .get(session_id)
                .is_some_and(|session| session.inbox_card.is_some())
            {
                sessions.update(session_id, cx, |session| session.inbox_card = None);
            }
        });
    }

    // Linked work items.

    /// `setLinkedWorkItemUpdateCard`: change the card only when it differs.
    fn set_linked_update_card(
        session_id: &str,
        cx: &mut App,
        update: impl FnOnce(Option<&LinkedWorkItemUpdateCard>) -> Option<LinkedWorkItemUpdateCard>,
    ) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            let Some(current) = sessions.get(session_id) else {
                return;
            };
            let next = update(current.linked_work_item_update_card.as_ref());
            if next != current.linked_work_item_update_card {
                sessions.update(session_id, cx, |session| {
                    session.linked_work_item_update_card = next
                });
            }
        });
    }

    /// `onLinkedWorkItemUpdateCardDismiss`.
    pub fn dismiss_linked_work_item_update_card(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) {
        Self::set_linked_update_card(session_id, cx, |_| None);
    }

    /// `revealLinkedSessionUpdate`: load what changed on the linked item
    /// into the session's card, detached from navigation.
    pub fn reveal_linked_session_update(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(update) = self.linked_session_update(session_id).cloned() else {
            return;
        };
        let Some(session) = Engine::sessions(cx).read(cx).get(session_id).cloned() else {
            return;
        };
        let Some(linked) = session.linked_work_item.clone() else {
            return;
        };
        if session
            .linked_work_item_update_card
            .as_ref()
            .is_some_and(|card| {
                card.updated_at == update.updated_at
                    && card.status != LinkedWorkItemUpdateStatus::Error
            })
        {
            return;
        }
        if self.linked_activity_fetches.get(session_id) == Some(&update.updated_at) {
            return;
        }
        let pending = pending_linked_work_item_update_card(&update);
        self.linked_activity_fetches
            .insert(session_id.to_string(), update.updated_at);
        // A stale or failed card should not stay up while fresh details load.
        Self::set_linked_update_card(session_id, cx, |current| {
            current
                .filter(|card| {
                    card.updated_at == update.updated_at
                        && card.status == LinkedWorkItemUpdateStatus::Ready
                })
                .cloned()
        });
        let thread = self.client.github_work_item_thread(
            &session.cwd,
            &linked.repo,
            linked.kind,
            linked.number,
            true,
        );
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            let result = thread.await;
            let _ = this.update(cx, |this, cx| {
                if this.linked_activity_fetches.get(&id) != Some(&pending.updated_at) {
                    return;
                }
                this.linked_activity_fetches.remove(&id);
                if this
                    .linked_session_update(&id)
                    .map(|update| update.updated_at)
                    != Some(pending.updated_at)
                {
                    return;
                }
                let card = match &result {
                    Ok(thread) => complete_linked_work_item_update_card(&pending, thread),
                    Err(_) => fail_linked_work_item_update_card(&pending),
                };
                Self::set_linked_update_card(&id, cx, |_| Some(card));
            });
        })
        .detach();
    }

    /// Sessions linked to work items that the store knows, outside the open
    /// project history (`storedLinkedSessions`).
    pub fn stored_linked_sessions(&self) -> &[SessionSummary] {
        &self.stored_linked_sessions
    }

    pub fn set_stored_linked_sessions(
        &mut self,
        rows: Vec<SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        self.stored_linked_sessions = rows;
        cx.notify();
    }

    /// `listLinkedSessions` when the Inbox opens.
    pub fn load_stored_linked_sessions(&mut self, cx: &mut Context<Self>) {
        let list = Engine::writer(cx).list_linked_sessions();
        cx.spawn(async move |this, cx| {
            // Already-loaded and live sessions still give a useful fallback.
            if let Ok(rows) = list.await {
                let _ = this.update(cx, |this, cx| this.set_stored_linked_sessions(rows, cx));
            }
        })
        .detach();
    }

    /// `inboxRelatedSessions`: every session linked to a work item, the open
    /// copy winning, newest first.
    pub fn inbox_related_sessions(
        &self,
        history: &[SessionSummary],
        cx: &App,
    ) -> Vec<SessionSummary> {
        let mut order: Vec<String> = Vec::new();
        let mut by_id: HashMap<String, SessionSummary> = HashMap::new();
        fn put(
            order: &mut Vec<String>,
            by_id: &mut HashMap<String, SessionSummary>,
            summary: SessionSummary,
        ) {
            if !by_id.contains_key(&summary.id) {
                order.push(summary.id.clone());
            }
            by_id.insert(summary.id.clone(), summary);
        }
        for session in &self.stored_linked_sessions {
            put(&mut order, &mut by_id, session.clone());
        }
        for session in history
            .iter()
            .filter(|session| session.linked_work_item.is_some())
        {
            put(&mut order, &mut by_id, session.clone());
        }
        for session in Engine::sessions(cx).read(cx).all() {
            if session.inbox_ask.is_some() || session.linked_work_item.is_none() {
                continue;
            }
            let summary = summary_from_session(session, None);
            let next = match by_id.get(&session.id) {
                Some(current) => SessionSummary {
                    harness: summary.harness,
                    model: summary.model.clone(),
                    runtime_mode: summary.runtime_mode,
                    title: summary.title.clone(),
                    cwd: summary.cwd.clone(),
                    linked_work_item: summary.linked_work_item.clone(),
                    ..current.clone()
                },
                None => summary,
            };
            put(&mut order, &mut by_id, next);
        }
        let mut rows: Vec<SessionSummary> = order
            .into_iter()
            .filter_map(|id| by_id.remove(&id))
            .collect();
        rows.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        rows
    }

    /// `onSetHistorySessionLinkedWorkItem`: link a session to a work item, or
    /// unlink it, saving in the background and rolling back on failure.
    pub fn set_session_linked_work_item(
        &mut self,
        session_id: &str,
        linked: Option<LinkedWorkItem>,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        let previous = sessions
            .read(cx)
            .get(session_id)
            .and_then(|session| session.linked_work_item.clone())
            .or_else(|| {
                self.activity
                    .sessions
                    .iter()
                    .chain(self.stored_linked_sessions.iter())
                    .find(|session| session.id == session_id)
                    .and_then(|session| session.linked_work_item.clone())
            });
        sessions.update(cx, |sessions, cx| {
            sessions.invalidate_loaded(session_id);
            sessions.update(session_id, cx, |session| {
                session.linked_work_item = linked.clone()
            });
        });
        self.patch_linked_summaries(session_id, linked.as_ref());
        self.hooks
            .linked_work_item_changed(session_id, linked.as_ref(), false, cx);
        if let Some(index) = self
            .linked_panels
            .iter()
            .position(|panel| panel.session_id == session_id)
        {
            self.linked_panels.remove(index);
        }
        cx.notify();
        let save = Engine::writer(cx).set_session_linked_work_item(session_id, linked.as_ref());
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            if save.await.is_ok() {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    if sessions
                        .get(&id)
                        .is_some_and(|session| session.linked_work_item == linked)
                    {
                        sessions.update(&id, cx, |session| {
                            session.linked_work_item = previous.clone()
                        });
                    }
                });
                for session in &mut this.activity.sessions {
                    if session.id == id && session.linked_work_item == linked {
                        session.linked_work_item = previous.clone();
                    }
                }
                match &previous {
                    Some(previous) => {
                        for session in &mut this.stored_linked_sessions {
                            if session.id == id {
                                session.linked_work_item = Some(previous.clone());
                            }
                        }
                    }
                    None => this
                        .stored_linked_sessions
                        .retain(|session| session.id != id),
                }
                this.hooks
                    .linked_work_item_changed(&id, previous.as_ref(), true, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn patch_linked_summaries(&mut self, session_id: &str, linked: Option<&LinkedWorkItem>) {
        for session in &mut self.activity.sessions {
            if session.id == session_id {
                session.linked_work_item = linked.cloned();
            }
        }
        match linked {
            Some(linked) => {
                for session in &mut self.stored_linked_sessions {
                    if session.id == session_id {
                        session.linked_work_item = Some(linked.clone());
                    }
                }
            }
            None => self
                .stored_linked_sessions
                .retain(|session| session.id != session_id),
        }
    }

    /// Every linked work item panel, oldest first.
    pub fn linked_panels(&self) -> &[LinkedWorkItemPanel] {
        &self.linked_panels
    }

    /// `activeLinkedWorkItemPanel`: the focused session's panel, else the
    /// newest panel for a session in the active tab.
    pub fn active_linked_panel(
        &self,
        focused_id: &str,
        tab_session_ids: &[String],
    ) -> Option<&LinkedWorkItemPanel> {
        self.linked_panels
            .iter()
            .find(|panel| panel.session_id == focused_id)
            .or_else(|| {
                self.linked_panels
                    .iter()
                    .rev()
                    .find(|panel| tab_session_ids.contains(&panel.session_id))
            })
    }

    /// Drop panels whose session left every open tab.
    pub fn retain_linked_panels(
        &mut self,
        open_session_ids: &HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        let before = self.linked_panels.len();
        self.linked_panels
            .retain(|panel| open_session_ids.contains(&panel.session_id));
        if self.linked_panels.len() != before {
            cx.notify();
        }
    }

    /// `closeLinkedWorkItemPanel`.
    pub fn close_linked_work_item_panel(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.linked_panel_request += 1;
        self.linked_panels
            .retain(|panel| panel.session_id != session_id);
        cx.notify();
    }

    fn session_cwd(&self, session_id: &str, cx: &App) -> Option<String> {
        Engine::sessions(cx)
            .read(cx)
            .get(session_id)
            .map(|session| session.cwd.clone())
            .or_else(|| {
                self.activity
                    .sessions
                    .iter()
                    .chain(self.stored_linked_sessions.iter())
                    .find(|session| session.id == session_id)
                    .map(|session| session.cwd.clone())
            })
    }

    /// `onOpenLinkedWorkItem`: open the session, then show the linked item
    /// beside it.
    pub fn open_linked_work_item(
        &mut self,
        item: LinkedWorkItem,
        session_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.linked_panel_request += 1;
        let request = self.linked_panel_request;
        self.hooks.leave_inbox(cx);
        let cwd = self
            .session_cwd(session_id, cx)
            .unwrap_or_else(|| self.hooks.sidebar_cwd(cx));
        let select = self.hooks.select_session(session_id, cx);
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            select.await;
            let _ = this.update(cx, |this, cx| {
                if this.linked_panel_request != request
                    || !Engine::sessions(cx).read(cx).contains(&id)
                {
                    return;
                }
                // Reinsert so this panel wins when the tab holds several
                // sessions with remembered panels.
                this.linked_panels.retain(|panel| panel.session_id != id);
                this.linked_panels.push(LinkedWorkItemPanel {
                    item,
                    session_id: id,
                    cwd,
                    ci_repair: None,
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// `onOpenInboxSession`.
    pub fn open_inbox_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.hooks.leave_inbox(cx);
        let cwd = self.session_cwd(session_id, cx);
        self.hooks.show_sessions_sidebar(cwd.as_deref(), cx);
        self.hooks.select_session(session_id, cx).detach();
    }
}

/// The body of `onAskInboxItem`: reuse the item's open conversation or
/// create one in the item's project.
async fn open_ask_session(
    client: &InboxClient,
    hooks: &Rc<dyn InboxHooks>,
    item: &InboxItem,
    key: &str,
    cx: &mut gpui::AsyncApp,
) -> Result<String, String> {
    let existing = cx.update(|cx| {
        Engine::sessions(cx)
            .read(cx)
            .all()
            .iter()
            .find(|session| session.inbox_ask.as_ref().is_some_and(|ask| ask.key == key))
            .map(|session| session.id.clone())
    });
    if let Some(id) = existing {
        return Ok(id);
    }
    let candidate = if item.project_path.is_empty() {
        cx.update(|cx| hooks.sidebar_cwd(cx))
    } else {
        item.project_path.clone()
    };
    let cwd = if !candidate.is_empty() && candidate != "~" {
        candidate
    } else {
        let task = cx.update(|cx| hooks.default_cwd(cx));
        task.await?
    };
    let description = match (item.provider, work_kind(item.kind)) {
        (InboxProvider::Linear | InboxProvider::Jira, _) => {
            client.inbox_tracker_description(item, None).await?
        }
        (InboxProvider::Gitlab, Some(kind)) => Some(
            match client.peek_gitlab_work_item_details(&item.repo, kind, item.number) {
                Some(details) => details.body,
                None => {
                    client
                        .gitlab_work_item_details(&item.repo, kind, item.number)
                        .await?
                        .body
                }
            },
        ),
        (InboxProvider::AzureDevops, Some(kind)) => Some(
            match client.peek_azure_dev_ops_work_item_details(&item.repo, kind, item.number) {
                Some(details) => details.body,
                None => {
                    client
                        .azure_dev_ops_work_item_details(&item.repo, kind, item.number)
                        .await?
                        .body
                }
            },
        ),
        _ => None,
    };
    Ok(cx.update(|cx| {
        let mut session: Session = hooks.new_default_session(&cwd, false, cx);
        session.title = format!("Ask · {}", item.title);
        session.inbox_ask = Some(InboxAskContext {
            key: key.to_string(),
            title: item.title.clone(),
            url: item.url.clone(),
            provider: item.provider,
            description,
            extra: Default::default(),
        });
        let id = session.id.clone();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
        id
    }))
}

/// `trackCiRepair` against the inbox's tracker: record the repair, submit
/// its turn through the hook, and settle it when the turn ends.
fn track_ci_repair(
    inbox: &Entity<Inbox>,
    cwd: &str,
    request: &CiRepairRequest,
    session_id: &str,
    hooks: &Rc<dyn InboxHooks>,
    cx: &mut App,
) -> Result<(), String> {
    let repair = inbox.update(cx, |inbox, cx| {
        let repair =
            inbox
                .ci_repairs
                .begin(cwd, request, session_id, uuid::Uuid::new_v4().to_string());
        cx.notify();
        repair
    });
    let weak = inbox.downgrade();
    let settled = repair.clone();
    let settle = Box::new(move |outcome: CiRepairOutcome, cx: &mut App| {
        let weak = weak.clone();
        // The turn may end inside another update; settle afterwards.
        cx.defer(move |cx| {
            let _ = weak.update(cx, |inbox, cx| {
                inbox.ci_repairs.settle(&settled, outcome);
                cx.notify();
            });
        });
    });
    if hooks.submit_ci_repair(session_id, request, settle, cx) {
        return Ok(());
    }
    inbox.update(cx, |inbox, cx| {
        inbox.ci_repairs.discard(&repair);
        cx.notify();
    });
    Err("Could not start this fix. Choose another chat and try again.".into())
}

/// `fetchFallbackUpdates`: exact lookups for linked items the list omitted,
/// three at a time.
async fn fetch_fallback_updates(
    client: &InboxClient,
    cwd: &str,
    targets: &[LinkedWorkItemTarget],
) -> Vec<(String, InboxItem)> {
    let results = std::cell::RefCell::new(Vec::new());
    for_each_concurrent(
        targets,
        MAX_CONCURRENT_LOOKUPS,
        |target, _| {
            let lookup = client.github_work_item(
                cwd,
                &target.item.repo,
                target.item.kind,
                target.item.number,
                true,
            );
            let key = target.key.clone();
            let results = &results;
            async move {
                // The next shared poll retries missing items.
                if let Ok(item) = lookup.await
                    && date_parse(&item.updated_at).is_some()
                {
                    results.borrow_mut().push((
                        key,
                        InboxItem::from_github(&item, "", InboxProvider::Github),
                    ));
                }
            }
        },
        || true,
    )
    .await;
    results.into_inner()
}
