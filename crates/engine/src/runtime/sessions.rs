//! The `Sessions` entity: every open session, harness event batching, and
//! the save policy. Port of the session state in App.tsx (lines 877-1410,
//! 1466-1546, 1696-2109, 3674-3853): `flushHarnessEvents`,
//! `enqueueHarnessEvent`, `applyApprovalEvent`, `persistSession` and the
//! debounced save effect, the in-flight and workspace snapshot effects,
//! idle detach, `loadStoredSession`, `ensureOpenSession`, and the history
//! prefetch.
//!
//! Views observe the entity (`cx.observe`) for any change, or subscribe to
//! `SessionsEvent` for specific ones.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AsyncApp, Context, EventEmitter, Task, WeakEntity};
use monocode_core::{HarnessEvent, HarnessId, Session};

use super::engine::Engine;
use super::harness_flush::{FlushKind, ScheduledFlush, schedule_harness_flush};
use super::in_flight::{in_flight_refs, in_flight_snapshot_key, should_write_in_flight_snapshot};
use super::reducer::{Reducer, apply_harness_events, last_user_block_id};
use super::session_cache::SessionCache;
use super::session_store::{
    SessionSummary, backfill_claude_shell_commands, claude_shell_placeholder_ids,
    persist_fingerprint, record_to_session, should_persist_session,
};

/// The debounce on saves after the last session change.
pub const PERSIST_DEBOUNCE: Duration = Duration::from_millis(650);
/// The debounce on workspace snapshot saves.
pub const WORKSPACE_SNAPSHOT_DEBOUNCE: Duration = Duration::from_millis(250);
/// `SESSION_DETACH_DELAY_MS`: idle detach waits until a switch has painted,
/// and a burst of switches pays for it once.
pub const SESSION_DETACH_DELAY: Duration = Duration::from_millis(250);

/// What changed in `Sessions`, for subscribers that need more than "something".
#[derive(Debug, Clone, PartialEq)]
pub enum SessionsEvent {
    /// Harness events applied to these sessions.
    EventsApplied { session_ids: Vec<String> },
    /// A save finished. History merges the row into its project
    /// (`mergeProjectHistorySummary`) when it shows that project.
    Persisted(Box<SessionSummary>),
    /// These sessions left memory: idle detach or removal.
    Closed { session_ids: Vec<String> },
    /// A stored session could not open. History should refresh its listing.
    LoadFailed { session_id: String },
    /// The set of busy sessions (with the leads of busy workers) changed.
    BusyChanged,
}

type SharedLoad = Shared<Task<Option<Session>>>;

/// Every open session, in tab-independent order.
pub struct Sessions {
    list: Vec<Session>,
    reducer: Reducer,
    queued: HashMap<String, Vec<HarnessEvent>>,
    flush: Option<ScheduledFlush>,
    turn_gen: HashMap<String, u64>,
    busy_ids: HashSet<String>,

    last_persisted: HashMap<String, String>,
    last_bound_provider: HashMap<String, String>,
    last_persisted_user_block: HashMap<String, String>,
    pending_persist: Vec<String>,
    persist_timer: Option<Task<()>>,
    removing: HashSet<String>,
    switching_worktrees: HashMap<String, String>,
    skip_forget: HashSet<String>,

    loaded_cache: SessionCache,
    loads: HashMap<String, (u64, SharedLoad)>,
    next_load: u64,
    load_epochs: HashMap<String, u64>,
    opening: HashSet<String>,
    active_prefetch: Option<Task<()>>,

    in_flight_sync_key: Option<String>,
    saw_in_flight: bool,
    workspace_autosave: bool,
    workspace_sync_key: Option<String>,
    workspace_timer: Option<Task<()>>,
    detach_timer: Option<Task<()>>,
}

impl EventEmitter<SessionsEvent> for Sessions {}

fn is_approval_event(event: &HarnessEvent) -> bool {
    matches!(
        event,
        HarnessEvent::ApprovalRequested { .. }
            | HarnessEvent::ApprovalResolved { .. }
            | HarnessEvent::QuestionAsked { .. }
            | HarnessEvent::QuestionResolved { .. }
    )
}

impl Sessions {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            list: Vec::new(),
            reducer: apply_harness_events,
            queued: HashMap::new(),
            flush: None,
            turn_gen: HashMap::new(),
            busy_ids: HashSet::new(),
            last_persisted: HashMap::new(),
            last_bound_provider: HashMap::new(),
            last_persisted_user_block: HashMap::new(),
            pending_persist: Vec::new(),
            persist_timer: None,
            removing: HashSet::new(),
            switching_worktrees: HashMap::new(),
            skip_forget: HashSet::new(),
            loaded_cache: SessionCache::new(),
            loads: HashMap::new(),
            next_load: 0,
            load_epochs: HashMap::new(),
            opening: HashSet::new(),
            active_prefetch: None,
            in_flight_sync_key: None,
            saw_in_flight: false,
            workspace_autosave: true,
            workspace_sync_key: None,
            workspace_timer: None,
            detach_timer: None,
        }
    }

    /// Swap the transcript reducer, for tests.
    pub fn set_reducer(&mut self, reducer: Reducer) {
        self.reducer = reducer;
    }

    // Reading.

    /// Every open session.
    pub fn all(&self) -> &[Session] {
        &self.list
    }

    pub fn get(&self, session_id: &str) -> Option<&Session> {
        self.list.iter().find(|session| session.id == session_id)
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.get(session_id).is_some()
    }

    pub fn ids(&self) -> Vec<String> {
        self.list.iter().map(|session| session.id.clone()).collect()
    }

    /// Busy sessions plus the leads of busy workers (`busySessionIds`).
    pub fn busy_session_ids(&self) -> &HashSet<String> {
        &self.busy_ids
    }

    /// Events waiting for the next flush.
    pub fn queued_events(&self, session_id: &str) -> &[HarnessEvent] {
        self.queued
            .get(session_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The scheduled flush's cadence, while one is pending.
    pub fn scheduled_flush(&self) -> Option<FlushKind> {
        self.flush.as_ref().map(|flush| flush.kind)
    }

    // Changing.

    /// Change one session. `false` when it is not open.
    pub fn update(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Session),
    ) -> bool {
        let Some(session) = self
            .list
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            return false;
        };
        update(session);
        self.changed(&[session_id.to_string()], cx);
        true
    }

    /// Replace sessions through `map`, which returns `None` to keep one as it
    /// is (`setSessions(prev => prev.map(...))`).
    pub fn update_all(
        &mut self,
        cx: &mut Context<Self>,
        mut map: impl FnMut(&Session) -> Option<Session>,
    ) {
        let mut changed = Vec::new();
        for session in &mut self.list {
            if let Some(next) = map(session) {
                changed.push(next.id.clone());
                *session = next;
            }
        }
        if !changed.is_empty() {
            self.changed(&changed, cx);
        }
    }

    /// Add a session, or replace the open one with the same id.
    pub fn upsert(&mut self, session: Session, cx: &mut Context<Self>) {
        let id = session.id.clone();
        match self.list.iter_mut().find(|open| open.id == id) {
            Some(open) => *open = session,
            None => self.list.push(session),
        }
        self.changed(&[id], cx);
    }

    /// Add a session unless one with its id is open.
    pub fn insert(&mut self, session: Session, cx: &mut Context<Self>) -> bool {
        if self.contains(&session.id) {
            return false;
        }
        self.upsert(session, cx);
        true
    }

    /// Replace the whole list.
    pub fn set_all(&mut self, sessions: Vec<Session>, cx: &mut Context<Self>) {
        let closed: Vec<String> = self
            .list
            .iter()
            .filter(|open| !sessions.iter().any(|next| next.id == open.id))
            .map(|open| open.id.clone())
            .collect();
        self.list = sessions;
        let ids = self.ids();
        self.closed(&closed, cx);
        self.changed(&ids, cx);
    }

    /// Remove a session from memory. The stored row stays.
    pub fn remove(&mut self, session_id: &str, cx: &mut Context<Self>) -> Option<Session> {
        let index = self
            .list
            .iter()
            .position(|session| session.id == session_id)?;
        let removed = self.list.remove(index);
        self.closed(&[session_id.to_string()], cx);
        self.changed(&[], cx);
        Some(removed)
    }

    /// Keep only the sessions `keep` accepts.
    pub fn retain(&mut self, cx: &mut Context<Self>, mut keep: impl FnMut(&Session) -> bool) {
        let mut closed = Vec::new();
        self.list.retain(|session| {
            let kept = keep(session);
            if !kept {
                closed.push(session.id.clone());
            }
            kept
        });
        if !closed.is_empty() {
            self.closed(&closed, cx);
            self.changed(&[], cx);
        }
    }

    /// `replaceBlankPaneWithSession`, the session half: drop the blank pane's
    /// session and open `session` in its place. The workspace package swaps
    /// the pane.
    pub fn replace_blank(&mut self, blank_id: &str, session: Session, cx: &mut Context<Self>) {
        self.last_persisted.remove(blank_id);
        if let Some(blank) = self.get(blank_id) {
            let harness = blank.harness;
            let hooks = Engine::hooks(cx);
            hooks.harness.forget_session(harness, blank_id, cx).detach();
        }
        let had_blank = self.contains(blank_id);
        self.list.retain(|entry| entry.id != blank_id);
        let id = session.id.clone();
        if !self.contains(&id) {
            self.list.push(session);
        }
        if had_blank {
            self.closed(&[blank_id.to_string()], cx);
        }
        self.changed(&[id], cx);
    }

    // Harness events.

    /// `enqueueHarnessEvent`: approval and question events apply at once so
    /// a prompt never waits on output; everything else batches and flushes
    /// once per frame for a visible session, or on the background cadence.
    pub fn enqueue_event(&mut self, session_id: &str, event: HarnessEvent, cx: &mut Context<Self>) {
        if is_approval_event(&event) {
            self.apply_approval_event(session_id, event, cx);
            return;
        }
        self.queued
            .entry(session_id.to_string())
            .or_default()
            .push(event);
        let hooks = Engine::hooks(cx);
        let hidden = hooks.workspace.window_hidden(cx);
        let foreground = !hidden && hooks.workspace.is_foreground(session_id, cx);
        // A visible stream must not wait for a background-only timer.
        if foreground && self.scheduled_flush() == Some(FlushKind::Timeout) {
            self.flush = None;
        }
        if self.flush.is_none() {
            self.flush = Some(schedule_harness_flush(
                cx,
                foreground,
                hidden,
                |this: &mut Self, cx| this.flush(cx),
            ));
        }
    }

    /// `applyApprovalEvent`: apply this session's queued events and the
    /// approval now.
    fn apply_approval_event(
        &mut self,
        session_id: &str,
        event: HarnessEvent,
        cx: &mut Context<Self>,
    ) {
        let mut events = self.queued.remove(session_id).unwrap_or_default();
        events.push(event);
        let mut batches = HashMap::new();
        batches.insert(session_id.to_string(), events);
        self.apply_batches(batches, cx);
    }

    /// `flushHarnessEvents`: apply every queued batch now. Call it before
    /// reading a session that must be current, such as on window focus or
    /// when a tab becomes active.
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.flush = None;
        if self.queued.is_empty() {
            return;
        }
        let batches = std::mem::take(&mut self.queued);
        self.apply_batches(batches, cx);
    }

    fn apply_batches(
        &mut self,
        batches: HashMap<String, Vec<HarnessEvent>>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = Vec::new();
        for session in &mut self.list {
            let Some(events) = batches.get(&session.id) else {
                continue;
            };
            if (self.reducer)(session, events) {
                changed.push(session.id.clone());
            }
        }
        if changed.is_empty() {
            return;
        }
        let hooks = Engine::hooks(cx);
        hooks.attention.sync_dock_badge(&self.list, cx);
        self.changed(&changed, cx);
        cx.emit(SessionsEvent::EventsApplied {
            session_ids: changed.clone(),
        });
        cx.defer(move |cx| {
            hooks.submit.sessions_flushed(&changed, cx);
            for id in &changed {
                if let Some(events) = batches.get(id) {
                    hooks.remote.session_events(id, events, cx);
                }
            }
        });
    }

    // Turns.

    /// The turn generation for a session. A turn's callbacks compare it
    /// with the value they started under and stop when it moved.
    pub fn turn_gen(&self, session_id: &str) -> u64 {
        self.turn_gen.get(session_id).copied().unwrap_or(0)
    }

    /// Start a new generation and return it.
    pub fn bump_turn_gen(&mut self, session_id: &str) -> u64 {
        let next = self.turn_gen(session_id) + 1;
        self.turn_gen.insert(session_id.to_string(), next);
        next
    }

    /// `stopSessionForRemoval`: stop the orchestrator's work for the session
    /// and cancel its live turn, flushing before and after. Resolves to the
    /// session as it stands afterwards.
    pub fn stop_for_removal(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Option<Session>> {
        let hooks = Engine::hooks(cx);
        let stop = hooks.orchestration.stop_for_session(session_id, cx);
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            stop.await;
            let cancels = this
                .update(cx, |this, cx| {
                    let open = this.get(&id)?.clone();
                    if !open.is_busy() {
                        return Some(Err(open));
                    }
                    this.bump_turn_gen(&id);
                    this.flush(cx);
                    let cancels: Vec<Task<()>> = hooks
                        .harness
                        .session_child_harnesses(&open)
                        .into_iter()
                        .map(|harness| hooks.harness.cancel_turn(harness, &id, cx))
                        .collect();
                    Some(Ok(cancels))
                })
                .ok()
                .flatten()?;
            match cancels {
                Err(idle) => Some(idle),
                Ok(cancels) => {
                    futures::future::join_all(cancels).await;
                    this.update(cx, |this, cx| {
                        this.flush(cx);
                        this.get(&id).cloned()
                    })
                    .ok()
                    .flatten()
                }
            }
        })
    }

    // Saving.

    /// Mark sessions restored from a window transfer or a resumed workspace
    /// as already saved (`importedSessionsApplied`).
    pub fn adopt_restored(&mut self, sessions: &[Session]) {
        for session in sessions {
            self.last_persisted
                .insert(session.id.clone(), persist_fingerprint(session));
            if let Some(user_id) = last_user_block_id(session) {
                self.last_persisted_user_block
                    .insert(session.id.clone(), user_id.to_string());
            }
            if let Some(provider) = session.provider_session_id.as_ref() {
                self.last_bound_provider
                    .insert(session.id.clone(), provider.clone());
            }
        }
    }

    /// `persistSession`: save one open session now unless it is unchanged
    /// since its last save.
    pub fn persist(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.get(session_id).cloned() {
            self.persist_session(session, cx);
        }
    }

    /// `persistSession` for a session value, which may already be leaving
    /// memory.
    pub fn persist_session(&mut self, session: Session, cx: &mut Context<Self>) {
        if !should_persist_session(&session)
            || self.removing.contains(&session.id)
            || self.switching_worktrees.contains_key(&session.id)
        {
            return;
        }
        let fingerprint = persist_fingerprint(&session);
        // Leaving a session flushes it. An unchanged one would still rewrite
        // and re-diff its whole transcript under the store lock.
        if self.last_persisted.get(&session.id) == Some(&fingerprint) {
            return;
        }
        let upsert = Engine::writer(cx).upsert_session(&session);
        let id = session.id;
        cx.spawn(async move |this, cx| {
            let Ok(Some(summary)) = upsert.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.last_persisted.insert(id, fingerprint);
                cx.emit(SessionsEvent::Persisted(Box::new(summary)));
            })
            .ok();
        })
        .detach();
    }

    /// Forget the saved fingerprint, so the next save writes even when
    /// nothing changed (`lastPersisted.delete`).
    pub fn forget_persisted(&mut self, session_id: &str) {
        self.last_persisted.remove(session_id);
    }

    /// Clear the queued save and saved-turn bookkeeping for a discarded chat.
    pub fn clear_save_state(&mut self, session_id: &str) {
        self.pending_persist.retain(|pending| pending != session_id);
        self.last_persisted.remove(session_id);
        self.last_persisted_user_block.remove(session_id);
    }

    /// The fingerprint of the last save that finished for this session.
    pub fn last_persisted(&self, session_id: &str) -> Option<&str> {
        self.last_persisted.get(session_id).map(String::as_str)
    }

    /// Sessions waiting for the debounced save.
    pub fn pending_persist(&self) -> &[String] {
        &self.pending_persist
    }

    /// `removingSessionIds`: while removal runs, the session is not saved.
    pub fn begin_removal(&mut self, session_id: &str) {
        self.removing.insert(session_id.to_string());
    }

    pub fn end_removal(&mut self, session_id: &str) {
        self.removing.remove(session_id);
    }

    pub fn is_removing(&self, session_id: &str) -> bool {
        self.removing.contains(session_id)
    }

    /// `switchingWorktrees`: while a session moves to another working copy,
    /// it is not saved.
    pub fn begin_worktree_switch(&mut self, session_id: &str, path: &str) {
        self.switching_worktrees
            .insert(session_id.to_string(), path.to_string());
    }

    pub fn end_worktree_switch(&mut self, session_id: &str) {
        self.switching_worktrees.remove(session_id);
    }

    pub fn switching_worktree(&self, session_id: &str) -> Option<&str> {
        self.switching_worktrees.get(session_id).map(String::as_str)
    }

    /// `skipForgetSessionIds`: idle detach leaves these sessions alone.
    pub fn set_skip_forget(&mut self, session_id: &str, skip: bool) {
        if skip {
            self.skip_forget.insert(session_id.to_string());
        } else {
            self.skip_forget.remove(session_id);
        }
    }

    /// Whether this window autosaves the workspace snapshot. A window opened
    /// by a window transfer does not.
    pub fn set_workspace_autosave(&mut self, enabled: bool) {
        self.workspace_autosave = enabled;
    }

    /// The bookkeeping every change runs: the effects App.tsx ran on each
    /// `sessions` render.
    fn changed(&mut self, ids: &[String], cx: &mut Context<Self>) {
        cx.notify();
        self.observe_for_persist(ids, cx);
        self.sync_in_flight_snapshot(cx);
        self.schedule_workspace_snapshot(cx);
        self.schedule_detach(cx);
        self.update_busy_ids(cx);
    }

    fn closed(&mut self, ids: &[String], cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        for id in ids {
            self.pending_persist.retain(|pending| pending != id);
            self.queued.remove(id);
        }
        let live: HashSet<String> = self.ids().into_iter().collect();
        Engine::hooks(cx).side_threads.sessions_closed(&live, cx);
        cx.emit(SessionsEvent::Closed {
            session_ids: ids.to_vec(),
        });
    }

    fn update_busy_ids(&mut self, cx: &mut Context<Self>) {
        let mut ids = HashSet::new();
        for session in &self.list {
            if session.is_busy() {
                ids.insert(session.id.clone());
                if let Some(lead) = session.orchestration_lead_id.as_ref() {
                    ids.insert(lead.clone());
                }
            }
        }
        if ids != self.busy_ids {
            self.busy_ids = ids;
            cx.emit(SessionsEvent::BusyChanged);
        }
    }

    /// The save effect: a new provider thread or a new user turn saves at
    /// once; idle, parked, and never-saved sessions save after the debounce.
    fn observe_for_persist(&mut self, ids: &[String], cx: &mut Context<Self>) {
        let hooks = Engine::hooks(cx);
        let visible: HashSet<String> = hooks.workspace.tab_session_ids(cx).into_iter().collect();
        for id in ids {
            if self.removing.contains(id) || self.switching_worktrees.contains_key(id) {
                continue;
            }
            let Some(session) = self.get(id) else {
                continue;
            };
            let parked = !visible.contains(id);
            let provider = session
                .provider_session_id
                .clone()
                .filter(|provider| !provider.is_empty());
            let newly_bound = provider
                .as_ref()
                .is_some_and(|provider| self.last_bound_provider.get(id) != Some(provider));
            let last_user = last_user_block_id(session).map(str::to_string);
            let new_user_turn = last_user
                .as_ref()
                .is_some_and(|user| self.last_persisted_user_block.get(id) != Some(user));
            let persistable = should_persist_session(session);
            let busy = session.is_busy();
            if newly_bound && let Some(provider) = provider {
                self.last_bound_provider.insert(id.clone(), provider);
            }
            if new_user_turn && let Some(user) = last_user {
                self.last_persisted_user_block.insert(id.clone(), user);
            }
            if (newly_bound || new_user_turn) && persistable {
                self.persist(id, cx);
            }
            if persistable
                && (!busy
                    || parked
                    || newly_bound
                    || new_user_turn
                    || !self.last_persisted.contains_key(id))
                && !self.pending_persist.contains(id)
            {
                self.pending_persist.push(id.clone());
            }
        }
        // Every change restarts the debounce, as the effect cleanup did.
        self.persist_timer = None;
        if self.pending_persist.is_empty() {
            return;
        }
        let timer = cx.background_executor().timer(PERSIST_DEBOUNCE);
        self.persist_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| this.persist_pending(cx)).ok();
        }));
    }

    /// Save every session waiting for the debounce now.
    pub fn persist_pending(&mut self, cx: &mut Context<Self>) {
        self.persist_timer = None;
        let dirty = std::mem::take(&mut self.pending_persist);
        for id in dirty {
            if self.removing.contains(&id) || self.switching_worktrees.contains_key(&id) {
                continue;
            }
            self.persist(&id, cx);
        }
    }

    /// The in-flight snapshot effect: keep the stored quit snapshot equal to
    /// the chats a quit would cut short. The workspace package calls this
    /// when tabs change too.
    pub fn sync_in_flight_snapshot(&mut self, cx: &mut Context<Self>) {
        let tab_ids = Engine::hooks(cx).workspace.tab_session_ids(cx);
        let refs = in_flight_refs(&self.list, &tab_ids);
        if !refs.is_empty() {
            self.saw_in_flight = true;
        }
        let key = in_flight_snapshot_key(&refs);
        if !should_write_in_flight_snapshot(
            &key,
            &refs,
            self.in_flight_sync_key.as_deref(),
            self.saw_in_flight,
        ) {
            return;
        }
        self.in_flight_sync_key = Some(key);
        Engine::writer(cx).replace_in_flight_sessions(refs).detach();
    }

    /// The workspace snapshot effect: save the layout 250 ms after it
    /// changes. The workspace package calls this when tabs change too.
    pub fn schedule_workspace_snapshot(&mut self, cx: &mut Context<Self>) {
        // TODO(port): the effect cleanup cleared the pending save on every
        // re-run, and a re-run with an unchanged key returned without
        // scheduling again, so a save could be dropped. Ported as written.
        self.workspace_timer = None;
        if !self.workspace_autosave {
            return;
        }
        let hooks = Engine::hooks(cx);
        let Some(snapshot) = hooks.workspace.collect_snapshot(&self.list, cx) else {
            return;
        };
        let key = hooks.workspace.snapshot_key(&snapshot);
        if self.workspace_sync_key.as_ref() == Some(&key) {
            return;
        }
        self.workspace_sync_key = Some(key);
        let timer = cx.background_executor().timer(WORKSPACE_SNAPSHOT_DEBOUNCE);
        self.workspace_timer = Some(cx.spawn(async move |_, cx| {
            timer.await;
            let save = cx.update(|cx| Engine::writer(cx).save_workspace_snapshot(snapshot));
            let _ = save.await;
        }));
    }

    /// Start the idle detach timer unless one is pending. The workspace
    /// package calls this when tabs change too.
    pub fn schedule_detach(&mut self, cx: &mut Context<Self>) {
        if self.detach_timer.is_some() {
            return;
        }
        let timer = cx.background_executor().timer(SESSION_DETACH_DELAY);
        self.detach_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| this.detach_idle_sessions(cx))
                .ok();
        }));
    }

    /// `detachIdleSessions`: hidden idle sessions save, move to the load
    /// cache, and drop their harness child. Inbox Asks, workers of a visible
    /// or running lead, unseen finished chats (with live agents on), and
    /// sessions still opening stay.
    pub fn detach_idle_sessions(&mut self, cx: &mut Context<Self>) {
        self.detach_timer = None;
        let hooks = Engine::hooks(cx);
        let mut visible: HashSet<String> =
            hooks.workspace.tab_session_ids(cx).into_iter().collect();
        for session in &self.list {
            if session.inbox_ask.is_some() {
                visible.insert(session.id.clone());
            }
        }
        let running_leads = hooks.orchestration.running_lead_ids(cx);
        let workers: Vec<String> = self
            .list
            .iter()
            .filter(|session| {
                session
                    .orchestration_lead_id
                    .as_ref()
                    .is_some_and(|lead| visible.contains(lead) || running_leads.contains(lead))
            })
            .map(|session| session.id.clone())
            .collect();
        visible.extend(workers);
        for id in &visible {
            self.opening.remove(id);
            self.loaded_cache.remove(id);
        }
        let keep_unseen = hooks.attention.live_agents_enabled(cx);
        let unseen = hooks.attention.unseen_finished_ids(cx);
        let stays = |this: &Self, session: &Session| {
            visible.contains(&session.id)
                || session.is_busy()
                || this.opening.contains(&session.id)
                || (keep_unseen && unseen.contains(&session.id))
        };
        let idle: Vec<Session> = self
            .list
            .iter()
            .filter(|session| !stays(self, session))
            .cloned()
            .collect();
        if idle.is_empty() {
            return;
        }
        for session in &idle {
            if self.skip_forget.contains(&session.id) {
                continue;
            }
            if should_persist_session(session) {
                self.loaded_cache.remember(session.clone());
            }
            self.persist_session(session.clone(), cx);
            for harness in hooks.harness.session_child_harnesses(session) {
                hooks
                    .harness
                    .forget_session(harness, &session.id, cx)
                    .detach();
            }
        }
        let skip = self.skip_forget.clone();
        let mut closed = Vec::new();
        let mut kept = Vec::with_capacity(self.list.len());
        for session in std::mem::take(&mut self.list) {
            if stays(self, &session) || skip.contains(&session.id) {
                kept.push(session);
            } else {
                closed.push(session.id);
            }
        }
        self.list = kept;
        if !closed.is_empty() {
            self.closed(&closed, cx);
            self.changed(&[], cx);
        }
    }

    // Loading.

    /// The closed-session cache.
    pub fn loaded_cache(&self) -> &SessionCache {
        &self.loaded_cache
    }

    /// `rememberLoadedSession` into the closed-session cache.
    pub fn remember_loaded(&mut self, session: Session) {
        self.loaded_cache.remember(session);
    }

    /// `invalidateLoadedSession`: forget a cached or in-progress load, so the
    /// next open reads the store again.
    pub fn invalidate_loaded(&mut self, session_id: &str) {
        self.opening.remove(session_id);
        self.loaded_cache.remove(session_id);
        self.loads.remove(session_id);
        *self.load_epochs.entry(session_id.to_string()).or_insert(0) += 1;
    }

    /// Reject in-flight reads that may still contain deleted ownership.
    pub(crate) fn invalidate_pending_loads(&mut self) {
        let pending: Vec<String> = self.loads.keys().cloned().collect();
        for id in pending {
            self.invalidate_loaded(&id);
        }
    }

    /// `loadStoredSession`: the cached copy, the load already running, or a
    /// new read from the store. Resolves to `None` when the session is
    /// missing, being removed, or was invalidated meanwhile.
    pub fn load_stored(&mut self, session_id: &str, cx: &mut Context<Self>) -> SharedLoad {
        if let Some(cached) = self.loaded_cache.take(session_id) {
            // The cache owns closed sessions only. Hand this one to live
            // state instead of keeping a stale duplicate.
            return Task::ready(Some(cached)).shared();
        }
        if let Some((_, pending)) = self.loads.get(session_id) {
            return pending.clone();
        }
        let epoch = self.load_epochs.get(session_id).copied().unwrap_or(0);
        let load_id = self.next_load;
        self.next_load += 1;
        let read = get_stored_session(session_id, cx);
        let id = session_id.to_string();
        let loading = cx
            .spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let loaded = read.await;
                this.update(cx, |this, _| {
                    if this
                        .loads
                        .get(&id)
                        .is_some_and(|(pending, _)| *pending == load_id)
                    {
                        this.loads.remove(&id);
                    }
                    let current = this.load_epochs.get(&id).copied().unwrap_or(0) == epoch;
                    loaded.filter(|_| current && !this.removing.contains(&id))
                })
                .ok()
                .flatten()
            })
            .shared();
        self.loads
            .insert(session_id.to_string(), (load_id, loading.clone()));
        loading
    }

    /// `ensureOpenSession`: the open session, or the stored one opened now.
    pub fn ensure_open(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Option<Session>> {
        if let Some(open) = self.get(session_id) {
            return Task::ready(Some(open.clone()));
        }
        self.opening.insert(session_id.to_string());
        let load = self.load_stored(session_id, cx);
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            let restored = load.await;
            this.update(cx, |this, cx| this.open_restored(&id, restored, cx))
                .ok()
                .flatten()
        })
    }

    fn open_restored(
        &mut self,
        id: &str,
        restored: Option<Session>,
        cx: &mut Context<Self>,
    ) -> Option<Session> {
        let Some(restored) = restored.filter(|_| !self.removing.contains(id)) else {
            self.opening.remove(id);
            cx.emit(SessionsEvent::LoadFailed {
                session_id: id.to_string(),
            });
            return None;
        };
        self.loaded_cache.remove(id);
        if let Some(appeared) = self.get(id) {
            return Some(appeared.clone());
        }
        let hooks = Engine::hooks(cx);
        if restored.worktree_removed != Some(true)
            && restored
                .provider_session_id
                .as_ref()
                .is_some_and(|p| !p.is_empty())
            && hooks.harness.is_live_harness(restored.harness)
        {
            hooks.harness.bind_session(&restored, cx);
        }
        self.last_persisted
            .insert(restored.id.clone(), persist_fingerprint(&restored));
        self.list.push(restored.clone());
        self.changed(std::slice::from_ref(&restored.id), cx);
        Some(restored)
    }

    /// `onPrefetchHistorySession`: load a stored session into the cache
    /// while the pointer rests on its card. One prefetch at a time.
    pub fn prefetch(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if self.removing.contains(session_id)
            || self.contains(session_id)
            || self.loaded_cache.contains(session_id)
            || self.loads.contains_key(session_id)
            || self.active_prefetch.is_some()
        {
            return;
        }
        let loading = self.load_stored(session_id, cx);
        let id = session_id.to_string();
        self.active_prefetch = Some(cx.spawn(async move |this, cx| {
            let loaded = loading.await;
            this.update(cx, |this, _| {
                if let Some(loaded) = loaded
                    && !this.removing.contains(&id)
                    && !this.contains(&id)
                {
                    this.loaded_cache.remember(loaded);
                }
                this.active_prefetch = None;
            })
            .ok();
        }));
    }

    // Boot.

    /// The boot effect: probe harness availability, refresh the catalogs of
    /// the harnesses already open, then re-resolve those sessions' models.
    pub fn refresh_models(&mut self, cx: &mut Context<Self>) {
        let hooks = Engine::hooks(cx);
        hooks.harness.probe_availability(cx);
        // Only the harnesses already in this window. Probing every installed
        // CLI at boot left unused agents running in the background.
        let mut harnesses: Vec<HarnessId> = Vec::new();
        for session in &self.list {
            if !harnesses.contains(&session.harness) {
                harnesses.push(session.harness);
            }
        }
        let refresh = hooks.harness.refresh_catalogs(harnesses, cx);
        cx.spawn(async move |this, cx| {
            refresh.await;
            this.update(cx, |this, cx| {
                let hooks = Engine::hooks(cx);
                let updates: Vec<(String, String, monocode_core::ModelSettings)> = this
                    .list
                    .iter()
                    .filter(|session| hooks.harness.is_live_harness(session.harness))
                    .filter_map(|session| {
                        let (model, settings) = hooks.harness.resolve_model(session, cx)?;
                        (model != session.model || settings != session.model_settings)
                            .then(|| (session.id.clone(), model, settings))
                    })
                    .collect();
                for (id, model, settings) in updates {
                    this.update(&id, cx, |session| {
                        session.model = model;
                        session.model_settings = settings;
                    });
                }
            })
            .ok();
        })
        .detach();
    }

    /// `bindResumedSessions`: attach each restored provider thread.
    pub fn bind_resumed(&self, cx: &mut App) {
        let hooks = Engine::hooks(cx);
        bind_resumed_sessions(&self.list, &hooks, cx);
    }
}

/// `bindResumedSessions` from appLifecycle.ts.
pub fn bind_resumed_sessions(
    sessions: &[Session],
    hooks: &super::hooks::EngineHooks,
    cx: &mut App,
) {
    for session in sessions {
        if session.worktree_removed == Some(true)
            || session
                .provider_session_id
                .as_ref()
                .is_none_or(|id| id.is_empty())
            || !hooks.harness.is_live_harness(session.harness)
        {
            continue;
        }
        hooks.harness.bind_session(session, cx);
    }
}

/// `getSession`: a stored session with its load-time repairs. Old Claude
/// rows get their Bash commands back from Claude's transcript, and the
/// provider hooks repair Cursor subagents and OMP interjections. Repairs
/// that change the transcript are saved before the session is shown.
pub fn get_stored_session(session_id: &str, cx: &App) -> Task<Option<Session>> {
    let writer = Engine::writer(cx);
    let record = writer.get_record(session_id);
    cx.spawn(async move |cx| {
        let record = record.await.ok().flatten()?;
        let mut session = record_to_session(record);
        if session.harness == HarnessId::Claude
            && let Some(provider) = session
                .provider_session_id
                .clone()
                .filter(|p| !p.is_empty())
        {
            let tool_ids = claude_shell_placeholder_ids(&session);
            if !tool_ids.is_empty() {
                let commands = writer
                    .claude_shell_commands(
                        &provider,
                        session.provider_account_id.as_deref(),
                        tool_ids,
                    )
                    .await;
                // A missing or unreadable Claude transcript must not block the session.
                if let Ok(commands) = commands
                    && let Some(blocks) = backfill_claude_shell_commands(&session.blocks, &commands)
                {
                    session.blocks = blocks;
                    let _ = writer.upsert_session(&session).await;
                }
            }
        }
        let recover = cx.update(|cx| {
            Engine::hooks(cx)
                .harness
                .recover_loaded_session(session, cx)
        });
        let recovered = recover.await;
        if recovered.persist {
            // Persist before exposing the restored session to a new live turn.
            let _ = writer.upsert_session(&recovered.session).await;
        }
        Some(recovered.session)
    })
}
