//! In-memory implementations of the data traits and synthetic inbox data,
//! for the tests and the gallery. Nothing here talks to a provider.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{AnyView, App, Subscription, Task, Window};

use crate::data::*;

/// Change listeners that fire on the next effect cycle, the way an
/// entity's observers do.
type ListenerFn = Rc<dyn Fn(&mut App)>;

#[derive(Clone, Default)]
pub struct Listeners {
    next: Rc<RefCell<u64>>,
    entries: Rc<RefCell<Vec<(u64, ListenerFn)>>>,
}

impl Listeners {
    pub fn subscribe(&self, listener: Listener) -> Subscription {
        let id = {
            let mut next = self.next.borrow_mut();
            *next += 1;
            *next
        };
        self.entries.borrow_mut().push((id, Rc::from(listener)));
        let entries = self.entries.clone();
        Subscription::new(move || entries.borrow_mut().retain(|(entry, _)| *entry != id))
    }

    pub fn notify(&self, cx: &mut App) {
        let entries = self.entries.clone();
        cx.defer(move |cx| {
            let listeners: Vec<ListenerFn> =
                entries.borrow().iter().map(|(_, f)| f.clone()).collect();
            for listener in listeners {
                listener(cx);
            }
        });
    }
}

fn ready<T: 'static>(value: Result<T, String>) -> DataTask<T> {
    Task::ready(value)
}

/// A task that finishes when the test sends on the returned channel.
pub fn held<T: 'static>(cx: &mut App) -> (DataTask<T>, oneshot::Sender<Result<T, String>>) {
    let (tx, rx) = oneshot::channel();
    let task = cx.spawn(async move |_| rx.await.unwrap_or_else(|_| Err("cancelled".into())));
    (task, tx)
}

// Detail data.

/// An item's detail data held in memory.
#[derive(Clone)]
pub struct FakeDetail {
    pub state: Rc<RefCell<InboxDetailState>>,
    pub calls: Rc<RefCell<Vec<String>>>,
    pub listeners: Listeners,
    /// What `run_pr_action` answers with.
    pub action_result: Rc<RefCell<Option<Result<InboxItem, String>>>>,
    pub item: Rc<RefCell<InboxItem>>,
}

impl FakeDetail {
    pub fn new(item: InboxItem, state: InboxDetailState) -> Self {
        Self {
            state: Rc::new(RefCell::new(state)),
            calls: Rc::default(),
            listeners: Listeners::default(),
            action_result: Rc::default(),
            item: Rc::new(RefCell::new(item)),
        }
    }

    pub fn update(&self, f: impl FnOnce(&mut InboxDetailState), cx: &mut App) {
        f(&mut self.state.borrow_mut());
        self.listeners.notify(cx);
    }
}

impl InboxDetailData for FakeDetail {
    fn subscribe(&self, listener: Listener, _: &mut App) -> Subscription {
        self.listeners.subscribe(listener)
    }

    fn state(&self, _: &App) -> InboxDetailState {
        self.state.borrow().clone()
    }

    fn set_item(&self, item: InboxItem, _: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("set_item #{}", item.number));
        *self.item.borrow_mut() = item;
    }

    fn set_revision(&self, revision: u64, _: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("set_revision {revision}"));
    }

    fn show_diff(&self, full_file: bool, _: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("show_diff {full_file}"));
    }

    fn set_reply_to(&self, reply_to: Option<InboxReplyTarget>, cx: &mut App) {
        self.state.borrow_mut().reply_to = reply_to;
        self.listeners.notify(cx);
    }

    fn post_comment(&self, body: String, cx: &mut App) -> DataTask<()> {
        self.calls.borrow_mut().push(format!("post_comment {body}"));
        self.state.borrow_mut().reply_to = None;
        self.listeners.notify(cx);
        ready(Ok(()))
    }

    fn run_pr_action(&self, action: GithubPrAction, cx: &mut App) -> DataTask<InboxItem> {
        self.calls
            .borrow_mut()
            .push(format!("run_pr_action {action:?}"));
        let result = self
            .action_result
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Ok(self.item.borrow().clone()));
        if let Err(error) = &result {
            self.state.borrow_mut().action_error = Some(error.clone());
        }
        self.listeners.notify(cx);
        ready(result)
    }
}

// Checks data.

/// One pull request's checks held in memory.
#[derive(Clone)]
pub struct FakeChecks {
    pub state: Rc<RefCell<PrChecksState>>,
    pub params: Rc<RefCell<PrChecksParams>>,
    pub refreshes: Rc<RefCell<usize>>,
    pub listeners: Listeners,
}

impl FakeChecks {
    pub fn new(state: PrChecksState) -> Self {
        Self {
            state: Rc::new(RefCell::new(state)),
            params: Rc::default(),
            refreshes: Rc::default(),
            listeners: Listeners::default(),
        }
    }

    pub fn set(&self, state: PrChecksState, cx: &mut App) {
        *self.state.borrow_mut() = state;
        self.listeners.notify(cx);
    }
}

impl PrChecksData for FakeChecks {
    fn subscribe(&self, listener: Listener, _: &mut App) -> Subscription {
        self.listeners.subscribe(listener)
    }

    fn state(&self, _: &App) -> PrChecksState {
        self.state.borrow().clone()
    }

    fn set_params(&self, params: PrChecksParams, _: &mut App) {
        *self.params.borrow_mut() = params;
    }

    fn refresh(&self, _: &mut App) {
        *self.refreshes.borrow_mut() += 1;
    }
}

// List data.

/// The Inbox page list held in memory. Search matches titles.
#[derive(Clone)]
pub struct FakeList {
    pub state: Rc<RefCell<InboxListState>>,
    pub items: Rc<RefCell<Vec<ListedItem>>>,
    pub calls: Rc<RefCell<Vec<String>>>,
    pub listeners: Listeners,
    /// `mark_source_read` fails while set, like a full storage.
    pub fail_writes: Rc<RefCell<bool>>,
}

impl FakeList {
    pub fn new(state: InboxListState, items: Vec<ListedItem>) -> Self {
        Self {
            state: Rc::new(RefCell::new(state)),
            items: Rc::new(RefCell::new(items)),
            calls: Rc::default(),
            listeners: Listeners::default(),
            fail_writes: Rc::default(),
        }
    }

    fn refresh_unseen(&self) {
        let source = self.state.borrow().source;
        let unseen = self
            .items
            .borrow()
            .iter()
            .any(|listed| listed.item.provider == source && listed.unseen);
        self.state.borrow_mut().source_has_unseen = unseen;
    }
}

impl InboxListData for FakeList {
    fn subscribe(&self, listener: Listener, _: &mut App) -> Subscription {
        self.listeners.subscribe(listener)
    }

    fn state(&self, _: &App) -> InboxListState {
        self.refresh_unseen();
        let mut state = self.state.borrow().clone();
        state.item_count = self.items.borrow().len();
        state
    }

    fn visible_items(&self, search: &str, _: &App) -> Vec<ListedItem> {
        let source = self.state.borrow().source;
        let needle = search.trim().to_lowercase();
        self.items
            .borrow()
            .iter()
            .filter(|listed| listed.item.provider == source)
            .filter(|listed| {
                needle.is_empty() || listed.item.title.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    fn set_source(&self, source: InboxSource, cx: &mut App) {
        self.state.borrow_mut().source = source;
        self.listeners.notify(cx);
    }

    fn set_filters(&self, filters: InboxFilters, cx: &mut App) {
        let active = filters != InboxFilters::default();
        let mut state = self.state.borrow_mut();
        state.filters = filters;
        state.filters_active = active;
        drop(state);
        self.listeners.notify(cx);
    }

    fn set_hidden_linear_team_ids(&self, ids: Vec<String>, cx: &mut App) {
        self.state.borrow_mut().hidden_linear_team_ids = ids;
        self.listeners.notify(cx);
    }

    fn set_hidden_jira_project_ids(&self, ids: Vec<String>, cx: &mut App) {
        self.state.borrow_mut().hidden_jira_project_ids = ids;
        self.listeners.notify(cx);
    }

    fn refresh(&self, cx: &mut App) {
        self.calls.borrow_mut().push("refresh".into());
        self.listeners.notify(cx);
    }

    fn mark_source_read(&self, cx: &mut App) {
        if *self.fail_writes.borrow() {
            self.state.borrow_mut().read_status_error =
                Some("Could not save read status. Please try again.".into());
        } else {
            let source = self.state.borrow().source;
            for listed in self.items.borrow_mut().iter_mut() {
                if listed.item.provider == source {
                    listed.unseen = false;
                }
            }
            self.state.borrow_mut().read_status_error = None;
        }
        self.listeners.notify(cx);
    }

    fn mark_item_seen(&self, item: &ListedItem, cx: &mut App) {
        for listed in self.items.borrow_mut().iter_mut() {
            if listed.key == item.key {
                listed.unseen = false;
            }
        }
        self.listeners.notify(cx);
    }

    fn update_item(&self, item: InboxItem, cx: &mut App) {
        for listed in self.items.borrow_mut().iter_mut() {
            if listed.item.number == item.number && listed.item.repo == item.repo {
                listed.item = item.clone();
            }
        }
        self.listeners.notify(cx);
    }
}

// Services.

/// Everything the fake services answer, keyed for the tests.
#[derive(Default)]
pub struct FakeServicesState {
    pub details: HashMap<i64, FakeDetail>,
    pub checks: HashMap<i64, FakeChecks>,
    pub check_details: HashMap<String, Result<GithubCheckDetails, String>>,
    /// Check detail requests wait for the test while set.
    pub hold_check_details: bool,
    pub held_check_details: Vec<oneshot::Sender<Result<GithubCheckDetails, String>>>,
    pub files: HashMap<String, String>,
    pub repairs: Vec<TrackedCiRepair>,
    pub repair_sessions: Vec<RelatedSession>,
    pub work_items: HashMap<i64, InboxItem>,
    pub calls: Vec<String>,
    pub repair_starts: Vec<(CiRepairStart, Option<String>)>,
    pub opened_urls: Vec<String>,
    pub copied: Vec<String>,
}

/// [`InboxServices`] over [`FakeServicesState`].
#[derive(Clone)]
pub struct FakeServices {
    pub state: Rc<RefCell<FakeServicesState>>,
    pub now: i64,
    pub repair_listeners: Listeners,
}

impl FakeServices {
    pub fn new(now: i64) -> Rc<Self> {
        Rc::new(Self {
            state: Rc::default(),
            now,
            repair_listeners: Listeners::default(),
        })
    }

    pub fn detail(&self, number: i64) -> Option<FakeDetail> {
        self.state.borrow().details.get(&number).cloned()
    }

    pub fn checks(&self, number: i64) -> Option<FakeChecks> {
        self.state.borrow().checks.get(&number).cloned()
    }

    pub fn set_repairs(&self, repairs: Vec<TrackedCiRepair>, cx: &mut App) {
        self.state.borrow_mut().repairs = repairs;
        self.repair_listeners.notify(cx);
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.borrow().calls.clone()
    }
}

impl InboxServices for FakeServices {
    fn now_ms(&self) -> i64 {
        self.now
    }

    fn open_detail(&self, item: &InboxItem, _: &mut App) -> Rc<dyn InboxDetailData> {
        let mut state = self.state.borrow_mut();
        let detail = state
            .details
            .entry(item.number)
            .or_insert_with(|| {
                FakeDetail::new(
                    item.clone(),
                    InboxDetailState {
                        details: Loadable::loading(),
                        thread: Loadable::loading(),
                        ..Default::default()
                    },
                )
            })
            .clone();
        state.calls.push(format!("open_detail #{}", item.number));
        Rc::new(detail)
    }

    fn open_pr_checks(&self, params: PrChecksParams, _: &mut App) -> Rc<dyn PrChecksData> {
        let mut state = self.state.borrow_mut();
        state
            .calls
            .push(format!("open_pr_checks #{}", params.number));
        let checks = state
            .checks
            .entry(params.number)
            .or_insert_with(|| {
                FakeChecks::new(PrChecksState {
                    loading: true,
                    ..Default::default()
                })
            })
            .clone();
        *checks.params.borrow_mut() = params;
        Rc::new(checks)
    }

    fn peek_github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        _: &App,
    ) -> Option<InboxItem> {
        let _ = cwd;
        self.state.borrow().work_items.get(&target.number).cloned()
    }

    fn github_work_item(
        &self,
        cwd: &str,
        target: &LinkedWorkItem,
        _: &mut App,
    ) -> DataTask<InboxItem> {
        let mut state = self.state.borrow_mut();
        state
            .calls
            .push(format!("github_work_item {cwd} #{}", target.number));
        ready(
            state
                .work_items
                .get(&target.number)
                .cloned()
                .ok_or_else(|| "Not found".to_string()),
        )
    }

    fn fetch_check_details(
        &self,
        cwd: &str,
        repo: &str,
        job_id: &str,
        cx: &mut App,
    ) -> DataTask<GithubCheckDetails> {
        let hold = {
            let mut state = self.state.borrow_mut();
            state
                .calls
                .push(format!("fetch_check_details {cwd} {repo} {job_id}"));
            state.hold_check_details
        };
        if hold {
            let (task, tx) = held(cx);
            self.state.borrow_mut().held_check_details.push(tx);
            return task;
        }
        ready(
            self.state
                .borrow()
                .check_details
                .get(job_id)
                .cloned()
                .unwrap_or_else(|| {
                    Ok(GithubCheckDetails {
                        steps: Vec::new(),
                        annotations: Vec::new(),
                        notice: None,
                    })
                }),
        )
    }

    fn commit_file_text(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
        _: &mut App,
    ) -> DataTask<Option<String>> {
        let mut state = self.state.borrow_mut();
        state
            .calls
            .push(format!("commit_file_text {cwd} {sha} {relative}"));
        ready(Ok(state.files.get(relative).cloned()))
    }

    fn ci_repairs(&self, _: &App) -> Vec<TrackedCiRepair> {
        self.state.borrow().repairs.clone()
    }

    fn subscribe_ci_repairs(&self, listener: Listener, _: &mut App) -> Subscription {
        self.repair_listeners.subscribe(listener)
    }

    fn start_item(&self, item: InboxItem, body: Option<String>, _: &mut App) -> DataTask<()> {
        self.state.borrow_mut().calls.push(format!(
            "start_item #{} {} {}",
            item.number,
            item.project_path,
            body.unwrap_or_default()
        ));
        ready(Ok(()))
    }

    fn repair_checks(
        &self,
        item: &InboxItem,
        start: CiRepairStart,
        session_id: Option<String>,
        _: &mut App,
    ) -> DataTask<()> {
        let mut state = self.state.borrow_mut();
        state.calls.push(format!("repair_checks #{}", item.number));
        state.repair_starts.push((start, session_id));
        ready(Ok(()))
    }

    fn repair_sessions(&self, _: &InboxItem, _: &App) -> Vec<RelatedSession> {
        self.state.borrow().repair_sessions.clone()
    }

    fn ask(&self, item: &InboxItem, _: &mut App) -> DataTask<String> {
        self.state
            .borrow_mut()
            .calls
            .push(format!("ask #{}", item.number));
        ready(Ok(format!("ask-{}", item.number)))
    }

    fn ask_restart(&self, item: &InboxItem, _: &mut App) -> DataTask<String> {
        self.state
            .borrow_mut()
            .calls
            .push(format!("ask_restart #{}", item.number));
        ready(Ok(format!("ask-{}-2", item.number)))
    }

    fn ask_pane(&self, _: &str, _: &mut Window, _: &mut App) -> Option<AnyView> {
        None
    }

    fn ask_unmounted(&self, _: &mut App) {}

    fn open_url(&self, url: &str, _: &mut App) {
        self.state.borrow_mut().opened_urls.push(url.to_string());
    }

    fn copy_text(&self, text: &str, _: &mut App) -> bool {
        self.state.borrow_mut().copied.push(text.to_string());
        true
    }

    fn fetch_media(&self, url: &str, _: &mut App) -> DataTask<InboxMedia> {
        let _ = url;
        ready(Err("unsupported".into()))
    }
}

// Synthetic data.

/// 2026-09-30 15:00 UTC, the gallery's "now".
pub const NOW: i64 = 1_790_780_400_000;

fn label(name: &str, color: &str) -> GithubLabel {
    GithubLabel {
        name: name.into(),
        color: color.into(),
    }
}

fn person(login: &str) -> GithubAssignee {
    GithubAssignee {
        login: login.into(),
        avatar_url: None,
    }
}

fn mark(color: u32) -> ProjectMark {
    ProjectMark {
        logo_path: None,
        mascot_name: None,
        mascot_color: Some(monocode_ui::color::hex(color)),
    }
}

/// The gallery's projects.
pub fn sample_projects() -> Vec<InboxProjectOption> {
    vec![
        InboxProjectOption {
            path: "/Users/dev/monocode".into(),
            name: "monocode".into(),
            mark: mark(0x60a5fa),
        },
        InboxProjectOption {
            path: "/Users/dev/relay".into(),
            name: "relay".into(),
            mark: mark(0xf472b6),
        },
    ]
}

/// The pull request with failing checks.
pub fn sample_pr() -> InboxItem {
    let mut item = InboxItem::github(
        InboxKind::Pr,
        "monocode/monocode",
        412,
        "Stream tool output into the transcript while commands run",
    );
    item.created_at = Some("2026-09-27T10:12:00Z".into());
    item.updated_at = "2026-09-30T14:20:00Z".into();
    item.project_path = "/Users/dev/monocode".into();
    item.labels = vec![
        label("transcript", "7c3aed"),
        label("performance", "0e8a16"),
    ];
    item.assignees = vec![person("hannah-k")];
    item
}

fn sample_issue(number: i64, title: &str, updated: &str) -> InboxItem {
    let mut item = InboxItem::github(InboxKind::Issue, "monocode/monocode", number, title);
    item.updated_at = updated.into();
    item.project_path = "/Users/dev/monocode".into();
    item
}

fn listed(key: &str, item: InboxItem, unseen: bool, related: usize, color: u32) -> ListedItem {
    ListedItem {
        key: key.into(),
        item,
        unseen,
        related_sessions: (0..related)
            .map(|index| RelatedSession {
                id: format!("session-{index}"),
                title: if index == 0 {
                    "Fix streaming flicker in tool cards".into()
                } else {
                    "Review transcript PR".into()
                },
                archived: index == 1,
            })
            .collect(),
        project_mark: mark(color),
    }
}

/// The gallery's GitHub tab, plus a Linear issue and a GitLab merge
/// request.
pub fn sample_items() -> Vec<ListedItem> {
    let mut draft = InboxItem::github(
        InboxKind::Pr,
        "monocode/relay",
        88,
        "Pin TLS fingerprints for remote hosts",
    );
    draft.draft = true;
    draft.updated_at = "2026-09-30T09:05:00Z".into();
    draft.project_path = "/Users/dev/relay".into();
    draft.labels = vec![label("security", "d73a4a")];

    let mut merged = InboxItem::github(
        InboxKind::Pr,
        "monocode/monocode",
        405,
        "Move the settings store to Kv",
    );
    merged.state = "merged".into();
    merged.updated_at = "2026-09-29T18:40:00Z".into();
    merged.project_path = "/Users/dev/monocode".into();

    let mut completed = sample_issue(
        398,
        "Sidebar loses its scroll position after archiving",
        "2026-09-28T11:00:00Z",
    );
    completed.state = "closed".into();
    completed.state_reason = Some("completed".into());
    completed.labels = vec![label("bug", "d73a4a")];

    let mut bug = sample_issue(
        417,
        "Composer drops pasted images on Windows",
        "2026-09-30T13:45:00Z",
    );
    bug.labels = vec![label("bug", "d73a4a"), label("windows", "1d76db")];
    bug.assignees = vec![person("sam-o")];

    let mut linear = InboxItem::github(
        InboxKind::Linear,
        "",
        231,
        "Onboarding checklist for new hosts",
    );
    linear.provider = InboxProvider::Linear;
    linear.id = Some("lin-231".into());
    linear.identifier = Some("ENG-231".into());
    linear.team_name = Some("Engineering".into());
    linear.state = "In Progress".into();
    linear.state_type = Some("started".into());
    linear.updated_at = "2026-09-30T12:00:00Z".into();
    linear.url = "https://linear.app/monocode/issue/ENG-231".into();

    let mut gitlab = InboxItem::github(
        InboxKind::Pr,
        "platform/api",
        57,
        "Rate limit webhook retries",
    );
    gitlab.provider = InboxProvider::Gitlab;
    gitlab.attention_reason = Some("review_requested".into());
    gitlab.updated_at = "2026-09-30T08:30:00Z".into();
    gitlab.url = "https://gitlab.example.com/platform/api/-/merge_requests/57".into();

    vec![
        listed(
            "github:monocode/monocode:pr:412",
            sample_pr(),
            true,
            2,
            0x60a5fa,
        ),
        listed("github:monocode/monocode:issue:417", bug, true, 0, 0x60a5fa),
        listed("github:monocode/relay:pr:88", draft, false, 0, 0xf472b6),
        listed(
            "github:monocode/monocode:pr:405",
            merged,
            false,
            1,
            0x60a5fa,
        ),
        listed(
            "github:monocode/monocode:issue:398",
            completed,
            false,
            0,
            0x60a5fa,
        ),
        listed("linear:lin-231", linear, true, 0, 0x60a5fa),
        listed("gitlab:platform/api:pr:57", gitlab, false, 0, 0x60a5fa),
    ]
}

/// The list state for the gallery's GitHub tab.
pub fn sample_list_state() -> InboxListState {
    InboxListState {
        cwd: "/Users/dev/monocode".into(),
        projects: sample_projects(),
        source: InboxProvider::Github,
        visible_sources: vec![
            InboxProvider::Github,
            InboxProvider::Linear,
            InboxProvider::Gitlab,
        ],
        connectable_sources: vec![InboxProvider::Jira, InboxProvider::AzureDevops],
        ..Default::default()
    }
}

/// The failing pull request's description, thread, and diff.
pub fn sample_pr_detail() -> InboxDetailState {
    let body = "Tool output now streams into the transcript while a command runs, instead of \
appearing all at once when it exits.\n\n## Changes\n\n- Buffer stdout per tool call and flush \
every 80 ms\n- Keep the scroll anchored while new lines arrive\n- Collapse output past 400 lines \
behind **Show all**\n\n```rust\nlet chunk = reader.read_until(b'\\n', &mut line)?;\nsender.send(ToolOutput { id, chunk }).await?;\n```\n\nCloses #398.";
    InboxDetailState {
        details: Loadable::ready(WorkItemDetails {
            body: body.into(),
            author: "hannah-k".into(),
            author_avatar_url: None,
            base_ref_name: Some("main".into()),
            head_ref_name: Some("hk/stream-tool-output".into()),
            review_decision: Some("CHANGES_REQUESTED".into()),
        }),
        thread: Loadable::ready(WorkItemThread {
            comments: vec![
                WorkItemComment {
                    id: "c1".into(),
                    kind: "review".into(),
                    author: "sam-o".into(),
                    body: "The flush interval reads well. Two failing jobs look related to the \
new snapshot tests, can you take a look?"
                        .into(),
                    created_at: "2026-09-30T11:02:00Z".into(),
                    url: "https://github.com/monocode/monocode/pull/412#pullrequestreview-1".into(),
                    state: "CHANGES_REQUESTED".into(),
                    ..Default::default()
                },
                WorkItemComment {
                    id: "c2".into(),
                    kind: "review_comment".into(),
                    author: "sam-o".into(),
                    body: "This drops the last partial line when the process exits.".into(),
                    created_at: "2026-09-30T11:05:00Z".into(),
                    url: "https://github.com/monocode/monocode/pull/412#discussion_r2".into(),
                    path: "crates/harness/src/stream.rs".into(),
                    line: Some(88),
                    thread_id: "t-2".into(),
                    replies: vec![WorkItemComment {
                        id: "c3".into(),
                        kind: "review_comment".into(),
                        author: "hannah-k".into(),
                        body: "Good catch, flushing on EOF now.".into(),
                        created_at: "2026-09-30T13:30:00Z".into(),
                        url: "https://github.com/monocode/monocode/pull/412#discussion_r3".into(),
                        thread_id: "t-2".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            review_decision: "CHANGES_REQUESTED".into(),
            base_ref_name: "main".into(),
            head_ref_name: "hk/stream-tool-output".into(),
            ..Default::default()
        }),
        diff: Loadable::ready(sample_diff()),
        ..Default::default()
    }
}

/// A two-file patch.
pub fn sample_diff() -> PrDiff {
    let patch = "diff --git a/crates/harness/src/stream.rs b/crates/harness/src/stream.rs
index 3f1c2aa..8d9e0b1 100644
--- a/crates/harness/src/stream.rs
+++ b/crates/harness/src/stream.rs
@@ -80,10 +80,14 @@ impl ToolStream {
     pub async fn pump(&mut self) -> Result<()> {
         let mut line = Vec::new();
         loop {
-            let read = self.reader.read_to_end(&mut line).await?;
-            self.flush(&line).await?;
-            break;
+            let read = self.reader.read_until(b'\\n', &mut line).await?;
+            if read == 0 {
+                self.flush(&line).await?;
+                break;
+            }
+            if self.last_flush.elapsed() >= FLUSH_EVERY {
+                self.flush(&line).await?;
+            }
         }
         Ok(())
     }
diff --git a/crates/view-transcript/src/tool_card.rs b/crates/view-transcript/src/tool_card.rs
index 11aa22b..33cc44d 100644
--- a/crates/view-transcript/src/tool_card.rs
+++ b/crates/view-transcript/src/tool_card.rs
@@ -12,6 +12,7 @@ pub struct ToolCard {
     output: String,
     collapsed: bool,
+    anchored: bool,
 }
";
    PrDiff {
        additions: 8,
        deletions: 3,
        files: vec![
            PrFile {
                path: "crates/harness/src/stream.rs".into(),
                additions: 7,
                deletions: 3,
            },
            PrFile {
                path: "crates/view-transcript/src/tool_card.rs".into(),
                additions: 1,
                deletions: 0,
            },
        ],
        patch: patch.into(),
        truncated: false,
    }
}

fn check(
    name: &str,
    workflow: &str,
    state: GithubPrCheckState,
    job: Option<u32>,
    secs: Option<i64>,
) -> GithubPrCheck {
    let started = "2026-09-30T14:00:00Z";
    GithubPrCheck {
        name: name.into(),
        workflow: workflow.into(),
        state,
        url: job
            .map(|job| format!("https://github.com/monocode/monocode/actions/runs/9131/job/{job}")),
        started_at: secs.map(|_| started.into()),
        completed_at: secs.map(|secs| {
            let end = crate::model::date_parse(started).unwrap_or(0) + secs * 1000;
            chrono::DateTime::from_timestamp_millis(end)
                .map(|date| date.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default()
        }),
    }
}

/// The failing pull request's head commit.
pub const SAMPLE_HEAD: &str = "4c1f0d9e2b7a6c5d8e9f0a1b2c3d4e5f6a7b8c9d";

/// Checks with two failures, one running, passes, and a skip.
pub fn sample_checks() -> GithubPrChecks {
    GithubPrChecks {
        head_oid: SAMPLE_HEAD.into(),
        checks: vec![
            check(
                "Unit tests / macOS",
                "CI",
                GithubPrCheckState::Fail,
                Some(101),
                Some(412),
            ),
            check(
                "Snapshot tests",
                "CI",
                GithubPrCheckState::Fail,
                Some(102),
                Some(95),
            ),
            check(
                "Unit tests / Windows",
                "CI",
                GithubPrCheckState::Pending,
                Some(103),
                None,
            ),
            check(
                "Clippy",
                "Lint",
                GithubPrCheckState::Pass,
                Some(104),
                Some(64),
            ),
            check(
                "rustfmt",
                "Lint",
                GithubPrCheckState::Pass,
                Some(105),
                Some(12),
            ),
            check(
                "Unit tests / Linux",
                "CI",
                GithubPrCheckState::Pass,
                Some(106),
                Some(388),
            ),
            check(
                "Docs preview",
                "Pages",
                GithubPrCheckState::Skipping,
                None,
                None,
            ),
        ],
    }
}

/// The macOS job's steps and annotations.
pub fn sample_check_details() -> GithubCheckDetails {
    GithubCheckDetails {
        steps: vec![
            GithubCheckStep {
                name: "Set up job".into(),
                state: GithubPrCheckState::Pass,
                started_at: Some("2026-09-30T14:00:00Z".into()),
                completed_at: Some("2026-09-30T14:00:03Z".into()),
            },
            GithubCheckStep {
                name: "Build".into(),
                state: GithubPrCheckState::Pass,
                started_at: Some("2026-09-30T14:00:03Z".into()),
                completed_at: Some("2026-09-30T14:04:41Z".into()),
            },
            GithubCheckStep {
                name: "Run tests".into(),
                state: GithubPrCheckState::Fail,
                started_at: Some("2026-09-30T14:04:41Z".into()),
                completed_at: Some("2026-09-30T14:06:52Z".into()),
            },
        ],
        annotations: vec![GithubCheckAnnotation {
            path: "crates/harness/src/stream.rs".into(),
            line: 87,
            message: "assertion `left == right` failed\n  left: 2\n right: 3".into(),
            level: "failure".into(),
        }],
        notice: None,
    }
}

/// The source the annotation points into.
pub fn sample_stream_source() -> String {
    let mut lines: Vec<String> = (1..=85).map(|n| format!("// line {n}")).collect();
    lines.push("            let read = self.reader.read_until(b'\\n', &mut line).await?;".into());
    lines.push("            if read == 0 {".into());
    lines.push("                self.flush(&line).await?;".into());
    lines.push("                break;".into());
    lines.join("\n")
}

/// A running CI repair for the snapshot job.
pub fn sample_repair() -> TrackedCiRepair {
    TrackedCiRepair {
        repo: "monocode/monocode".into(),
        number: 412,
        head_oid: SAMPLE_HEAD.into(),
        checks: vec![CiRepairCheck {
            name: "Snapshot tests".into(),
            workflow: "CI".into(),
            url: Some("https://github.com/monocode/monocode/actions/runs/9131/job/102".into()),
        }],
        id: "repair-1".into(),
        cwd: "/Users/dev/monocode".into(),
        session_id: "repair-chat".into(),
        started_at: NOW - 600_000,
        sequence: None,
        phase: CiRepairPhase::Running,
    }
}

/// Fake services loaded with the gallery's data.
pub fn sample_services() -> Rc<FakeServices> {
    let services = FakeServices::new(NOW);
    {
        let mut state = services.state.borrow_mut();
        state
            .details
            .insert(412, FakeDetail::new(sample_pr(), sample_pr_detail()));
        state.checks.insert(
            412,
            FakeChecks::new(PrChecksState {
                checks: Some(sample_checks()),
                ..Default::default()
            }),
        );
        state
            .check_details
            .insert("101".into(), Ok(sample_check_details()));
        state.files.insert(
            "crates/harness/src/stream.rs".into(),
            sample_stream_source(),
        );
        state.repair_sessions = vec![
            RelatedSession {
                id: "repair-chat".into(),
                title: "Fix snapshot tests".into(),
                archived: false,
            },
            RelatedSession {
                id: "session-0".into(),
                title: "Fix streaming flicker in tool cards".into(),
                archived: false,
            },
        ];
        state.work_items.insert(412, sample_pr());
    }
    services
}
