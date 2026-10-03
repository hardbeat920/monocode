//! Tests for the `Inbox` entity: the useInboxUnseen.test.ts cases, with the
//! notification preferences behind `InboxHooks`, and the App.tsx inbox
//! actions.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext, Entity, Task, TestAppContext};
use monocode_core::Extra;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::session::{LinkedWorkItem, Session};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::backend::fake::FakeBackend;
use super::ci_repair::CiRepairRequest;
use super::ci_repair::build_ci_repair_request;
use super::ci_repair::tests::failed;
use super::ci_repair_tracking::{CiRepairOutcome, CiRepairPhase};
use super::client::InboxClient;
use super::github_tasks::inbox_item_key;
use super::github_tasks::test_items::{item, with};
use super::hooks::{CiRepairSettle, InboxHooks};
use super::inbox::Inbox;
use super::inbox_notifications::InboxNotificationSubject;
use super::inbox_seen::InboxSeenEntry;
use super::inbox_self_activity::InboxSelfActivityTarget;
use super::rail::RecentProject;
use super::time::date_parse;
use super::types::{InboxItem, InboxKind, InboxProvider, WorkItemKind};
use crate::runtime::Engine;
use crate::runtime::session_store::SessionSummary;
use crate::runtime::testing::{FakeBackend as StoreFake, init_test_engine};

/// Notification preferences, sounds, and navigation that tests control.
#[derive(Default)]
pub(crate) struct TestHooks {
    pub muted: RefCell<HashSet<String>>,
    pub disabled: RefCell<HashMap<String, HashSet<&'static str>>>,
    pub cues: RefCell<Vec<String>>,
    pub appeared: RefCell<Vec<usize>>,
    pub accept_submit: RefCell<bool>,
    pub submits: RefCell<Vec<(String, String)>>,
    pub settles: RefCell<Vec<CiRepairSettle>>,
    pub opened_tabs: RefCell<Vec<(String, String)>>,
    pub selected: RefCell<Vec<String>>,
    pub left: RefCell<usize>,
    pub sidebars: RefCell<Vec<Option<String>>>,
    pub linked_changes: RefCell<Vec<(String, Option<LinkedWorkItem>, bool)>>,
}

impl InboxHooks for TestHooks {
    fn notification_project_id(&self, item: &InboxItem) -> String {
        if item.project_path.is_empty() {
            format!("repository:github.com/{}", item.repo.to_lowercase())
        } else {
            format!("local:{}", item.project_path)
        }
    }

    fn allows_notification_indicator(&self, subject: &InboxNotificationSubject, _cx: &App) -> bool {
        !self.muted.borrow().contains(&subject.project_id)
            && !self
                .disabled
                .borrow()
                .get(&subject.project_id)
                .is_some_and(|categories| categories.contains(subject.category.as_str()))
    }

    fn play_inbox_cue(&self, subject: &InboxNotificationSubject, _cx: &mut App) -> bool {
        self.cues.borrow_mut().push(subject.project_id.clone());
        true
    }

    fn inbox_appeared(&self, items: &[InboxItem], _cx: &mut App) {
        self.appeared.borrow_mut().push(items.len());
    }

    fn leave_inbox(&self, _cx: &mut App) {
        *self.left.borrow_mut() += 1;
    }

    fn show_sessions_sidebar(&self, cwd: Option<&str>, _cx: &mut App) {
        self.sidebars.borrow_mut().push(cwd.map(str::to_string));
    }

    fn open_session_tab(&self, session_id: &str, cwd: &str, _cx: &mut App) {
        self.opened_tabs
            .borrow_mut()
            .push((session_id.to_string(), cwd.to_string()));
    }

    fn select_session(&self, session_id: &str, _cx: &mut App) -> Task<()> {
        self.selected.borrow_mut().push(session_id.to_string());
        Task::ready(())
    }

    fn submit_ci_repair(
        &self,
        session_id: &str,
        request: &CiRepairRequest,
        settle: CiRepairSettle,
        _cx: &mut App,
    ) -> bool {
        self.submits
            .borrow_mut()
            .push((session_id.to_string(), request.text.clone()));
        self.settles.borrow_mut().push(settle);
        *self.accept_submit.borrow()
    }

    fn linked_work_item_changed(
        &self,
        session_id: &str,
        linked: Option<&LinkedWorkItem>,
        refresh: bool,
        _cx: &mut App,
    ) {
        self.linked_changes
            .borrow_mut()
            .push((session_id.to_string(), linked.cloned(), refresh));
    }
}

pub(crate) struct Setup {
    pub inbox: Entity<Inbox>,
    pub backend: Arc<FakeBackend>,
    pub store: Arc<StoreFake>,
    pub hooks: Rc<TestHooks>,
    pub client: InboxClient,
}

pub(crate) fn setup(
    cx: &mut TestAppContext,
    handler: impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static,
) -> Setup {
    let store = init_test_engine(cx);
    let backend = FakeBackend::new(handler);
    let client = InboxClient::new(
        backend.clone(),
        monocode_settings::Kv::in_memory(),
        cx.executor(),
    );
    let hooks = Rc::new(TestHooks::default());
    let inbox_hooks: Rc<dyn InboxHooks> = hooks.clone();
    let inbox_client = client.clone();
    let inbox = cx.new(|cx| {
        let mut inbox = Inbox::new(inbox_client, cx);
        inbox.set_hooks(inbox_hooks);
        inbox
    });
    Setup {
        inbox,
        backend,
        store,
        hooks,
        client,
    }
}

fn status_off(command: &str) -> Option<Result<Value, String>> {
    command
        .ends_with("_status")
        .then(|| Ok(json!({ "connected": false })))
}

fn remote_pr(repo: &str, updated_at: &str) -> Value {
    json!({
        "kind": "pr",
        "repo": repo,
        "number": 42,
        "title": "Update sidebar activity",
        "url": format!("https://github.com/{repo}/pull/42"),
        "state": "open",
        "updatedAt": updated_at,
        "labels": [],
        "assignees": [],
        "draft": false,
    })
}

/// GitHub lists `prs` for the checkout whose repository they name.
#[derive(Clone, Default)]
struct Remote {
    prs: Arc<Mutex<Vec<Value>>>,
    exact: Arc<Mutex<Option<Value>>>,
}

impl Remote {
    fn handler(&self) -> impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static {
        let prs = self.prs.clone();
        let exact = self.exact.clone();
        move |command, args| match command {
            "git_github_repositories" => {
                let cwd = args["cwd"].as_str().unwrap_or_default();
                Ok(json!([format!(
                    "acme/{}",
                    cwd.rsplit('/').next().unwrap_or_default()
                )]))
            }
            "git_github_work_items" => Ok(if args["kind"] == "pr" {
                Value::Array(
                    prs.lock()
                        .iter()
                        .filter(|pr| pr["repo"] == args["repo"])
                        .cloned()
                        .collect(),
                )
            } else {
                json!([])
            }),
            "git_github_work_item" => exact.lock().clone().ok_or_else(|| "not found".to_string()),
            "git_github_work_item_thread" => Ok(json!({
                "comments": [{
                    "id": "c1",
                    "kind": "comment",
                    "author": "maya",
                    "body": "Please rebase",
                    "createdAt": "2026-09-13T11:00:00Z",
                }],
                "commits": [],
                "truncated": false,
            })),
            _ => {
                status_off(command).unwrap_or_else(|| Err(format!("Unexpected command: {command}")))
            }
        }
    }
}

fn linked_pr() -> LinkedWorkItem {
    LinkedWorkItem {
        kind: WorkItemKind::Pr,
        repo: "acme/app".into(),
        number: 42,
        url: "https://github.com/acme/app/pull/42".into(),
        extra: Extra::new(),
    }
}

fn linked_session() -> SessionSummary {
    let mut summary = SessionSummary::new("linked-session", "/tmp/app", HarnessId::Codex);
    summary.model = "gpt-5".into();
    summary.runtime_mode = RuntimeMode::Supervised;
    summary.title = "codex · Update sidebar activity".into();
    summary.created_at = date_parse("2026-09-13T10:00:00Z").unwrap();
    summary.updated_at = date_parse("2026-09-13T10:00:00Z").unwrap();
    summary.linked_work_item = Some(linked_pr());
    summary
}

const KEY: &str = "github:acme/app:pr:42";

/// Run every task, then read a settled future.
fn done<T>(cx: &mut TestAppContext, future: impl std::future::Future<Output = T>) -> T {
    cx.run_until_parked();
    futures::FutureExt::now_or_never(future).expect("the task settled")
}

fn mount(s: &Setup, cx: &mut TestAppContext, recents: Vec<RecentProject>) {
    s.inbox.update(cx, |inbox, cx| {
        inbox.set_activity_inputs(recents, "/tmp/app".into(), vec![linked_session()], cx)
    });
    cx.run_until_parked();
}

fn unseen(s: &Setup, cx: &mut TestAppContext) -> bool {
    s.inbox.read_with(cx, |inbox, _| inbox.unseen())
}

fn has_indicator(s: &Setup, cx: &mut TestAppContext) -> bool {
    s.inbox.read_with(cx, |inbox, _| {
        inbox.linked_session_update_ids().contains("linked-session")
    })
}

fn has_update(s: &Setup, cx: &mut TestAppContext) -> bool {
    s.inbox.read_with(cx, |inbox, _| {
        inbox.linked_session_update("linked-session").is_some()
    })
}

fn preferences_changed(s: &Setup, cx: &mut TestAppContext) {
    s.inbox
        .update(cx, |inbox, cx| inbox.notification_preferences_changed(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn updates_inbox_and_linked_session_indicators_on_category_changes_without_consuming_unread_activity(
    cx: &mut TestAppContext,
) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    let entry = InboxSeenEntry::new(KEY, "2026-09-13T12:00:00Z");
    s.client
        .seed_inbox_seen_if_needed(&[InboxSeenEntry::new(KEY, "2026-09-12T12:00:00Z")]);
    mount(&s, cx, vec![]);
    assert!(unseen(&s, cx));
    assert!(has_indicator(&s, cx));

    s.hooks
        .disabled
        .borrow_mut()
        .insert("local:/tmp/app".into(), HashSet::from(["pullRequests"]));
    preferences_changed(&s, cx);
    assert!(!unseen(&s, cx));
    assert!(!has_indicator(&s, cx));
    assert!(has_update(&s, cx));
    assert!(s.client.is_inbox_entry_unseen(&entry));

    s.hooks.disabled.borrow_mut().clear();
    s.hooks.muted.borrow_mut().insert("local:/tmp/app".into());
    preferences_changed(&s, cx);
    assert!(!unseen(&s, cx));
    assert!(!has_indicator(&s, cx));

    // The mute expires.
    s.hooks.muted.borrow_mut().clear();
    preferences_changed(&s, cx);
    assert!(unseen(&s, cx));
    assert!(has_indicator(&s, cx));
    assert!(s.client.is_inbox_entry_unseen(&entry));
}

#[gpui::test]
fn updates_the_dot_immediately_on_mute_resume_and_read(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    let entry = InboxSeenEntry::new(KEY, "2026-09-13T12:00:00Z");
    s.client
        .seed_inbox_seen_if_needed(&[InboxSeenEntry::new(KEY, "2026-09-12T12:00:00Z")]);
    mount(&s, cx, vec![]);
    assert!(unseen(&s, cx));
    s.hooks.muted.borrow_mut().insert("local:/tmp/app".into());
    preferences_changed(&s, cx);
    assert!(!unseen(&s, cx));
    assert!(s.client.is_inbox_entry_unseen(&entry));
    s.hooks.muted.borrow_mut().clear();
    preferences_changed(&s, cx);
    assert!(unseen(&s, cx));
    s.client.mark_inbox_item_seen(&entry);
    cx.run_until_parked();
    assert!(!unseen(&s, cx));
}

#[gpui::test]
fn badges_only_unmuted_projects_while_muted_activity_stays_unread(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    remote
        .prs
        .lock()
        .push(remote_pr("acme/other", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    let muted_entry = InboxSeenEntry::new(KEY, "2026-09-13T12:00:00Z");
    let other_entry = InboxSeenEntry::new("github:acme/other:pr:42", "2026-09-13T12:00:00Z");
    s.client.seed_inbox_seen_if_needed(&[
        InboxSeenEntry::new(KEY, "2026-09-12T12:00:00Z"),
        InboxSeenEntry::new("github:acme/other:pr:42", "2026-09-12T12:00:00Z"),
    ]);
    s.hooks.muted.borrow_mut().insert("local:/tmp/app".into());
    mount(
        &s,
        cx,
        vec![RecentProject {
            path: "/tmp/other".into(),
            opened_at: 1,
        }],
    );
    assert!(unseen(&s, cx));
    s.client.mark_inbox_item_seen(&other_entry);
    cx.run_until_parked();
    assert!(!unseen(&s, cx));
    assert!(s.client.is_inbox_entry_unseen(&muted_entry));
    assert!(!has_indicator(&s, cx));
    assert!(has_update(&s, cx));
}

#[gpui::test]
fn reuses_the_inbox_list_for_linked_session_updates(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    mount(&s, cx, vec![]);
    assert!(has_indicator(&s, cx));
    // One list: the issues and the pull requests of the one repository.
    assert_eq!(s.backend.count("git_github_work_items"), 2);
    assert_eq!(s.backend.count("git_github_work_item"), 0);
}

#[gpui::test]
fn falls_back_to_an_exact_lookup_only_when_the_inbox_omits_the_item(cx: &mut TestAppContext) {
    let remote = Remote::default();
    *remote.exact.lock() = Some(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    mount(&s, cx, vec![]);
    assert!(has_indicator(&s, cx));
    assert_eq!(s.backend.count("git_github_work_items"), 2);
    assert_eq!(
        s.backend.calls_to("git_github_work_item"),
        [json!({ "cwd": "/tmp/app", "repo": "acme/app", "kind": "pr", "number": 42 })]
    );
}

#[gpui::test]
fn clears_a_linked_session_update_as_soon_as_its_remote_snapshot_is_read(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    mount(&s, cx, vec![]);
    assert!(has_indicator(&s, cx));
    s.inbox.update(cx, |inbox, cx| {
        inbox.mark_linked_session_update_seen(
            "linked-session",
            date_parse("2026-09-13T12:00:00Z").unwrap(),
            cx,
        )
    });
    assert!(!has_indicator(&s, cx));
}

#[gpui::test]
fn acknowledges_an_app_authored_revision_without_a_cue_or_linked_session_notification(
    cx: &mut TestAppContext,
) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T11:00:00Z"));
    let s = setup(cx, remote.handler());
    s.client.mark_linked_session_update_seen(
        "linked-session",
        date_parse("2026-09-13T11:00:00Z").unwrap(),
    );
    mount(&s, cx, vec![]);
    assert!(!unseen(&s, cx));
    assert!(!has_indicator(&s, cx));

    *remote.prs.lock() = vec![remote_pr("acme/app", "2026-09-13T12:05:00Z")];
    s.client
        .record_inbox_self_activity(InboxSelfActivityTarget::work_item(
            InboxProvider::Github,
            InboxKind::Pr,
            "acme/app",
            42,
        ));
    cx.run_until_parked();

    let entry = InboxSeenEntry::new(KEY, "2026-09-13T12:05:00Z");
    assert_eq!(s.backend.count("git_github_work_items"), 4);
    assert!(s.hooks.cues.borrow().is_empty());
    assert!(!s.client.is_inbox_entry_unseen(&entry));
    assert!(!unseen(&s, cx));
    assert!(!has_indicator(&s, cx));
}

#[gpui::test]
fn rings_once_for_new_activity_and_polls_every_thirty_seconds(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T11:00:00Z"));
    let s = setup(cx, remote.handler());
    mount(&s, cx, vec![]);
    assert_eq!(*s.hooks.appeared.borrow(), [0]);
    *remote.prs.lock() = vec![remote_pr("acme/app", "2026-09-13T12:00:00Z")];
    cx.executor().advance_clock(super::inbox::POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(s.backend.count("git_github_work_items"), 4);
    assert_eq!(*s.hooks.cues.borrow(), ["local:/tmp/app"]);
    assert!(unseen(&s, cx));
    assert!(has_indicator(&s, cx));
}

#[gpui::test]
fn reveals_the_linked_activity_card_for_an_updated_session(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    let mut open = Session::blank("linked-session", HarnessId::Codex, "gpt-5", "/tmp/app");
    open.linked_work_item = Some(linked_pr());
    cx.update(|cx| Engine::sessions(cx).update(cx, |sessions, cx| sessions.upsert(open, cx)));
    mount(&s, cx, vec![]);
    s.inbox.update(cx, |inbox, cx| {
        inbox.reveal_linked_session_update("linked-session", cx)
    });
    cx.run_until_parked();
    let card = cx.update(|cx| {
        Engine::sessions(cx)
            .read(cx)
            .get("linked-session")
            .and_then(|session| session.linked_work_item_update_card.clone())
    });
    let card = card.expect("a card");
    assert_eq!(
        card.status,
        monocode_core::inbox::LinkedWorkItemUpdateStatus::Ready
    );
    assert_eq!(card.counts.comments, 1);
    assert_eq!(card.entries[0].text, "Please rebase");
    s.inbox.update(cx, |inbox, cx| {
        inbox.dismiss_linked_work_item_update_card("linked-session", cx)
    });
    assert!(cx.update(|cx| {
        Engine::sessions(cx)
            .read(cx)
            .get("linked-session")
            .is_some_and(|session| session.linked_work_item_update_card.is_none())
    }));
}

// App.tsx actions.

fn github_item() -> InboxItem {
    with(item(42, "2026-09-13T12:00:00Z"), |row| {
        row.kind = InboxKind::Pr;
        row.repo = "acme/app".into();
        row.title = "Update sidebar activity".into();
        row.url = "https://github.com/acme/app/pull/42".into();
        row.project_path = "/tmp/app".into();
    })
}

fn session(cx: &mut TestAppContext, id: &str) -> Option<Session> {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned())
}

fn session_ids(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| Engine::sessions(cx).read(cx).ids())
}

#[gpui::test]
fn starting_an_item_opens_a_seeded_chat_in_its_project(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let task = s.inbox.update(cx, |inbox, cx| {
        inbox.start_inbox_item(github_item(), None, cx)
    });
    cx.run_until_parked();
    let id = done(cx, task).unwrap();
    let started = session(cx, &id).unwrap();
    assert_eq!(started.title, "#42 Update sidebar activity");
    assert_eq!(started.cwd, "/tmp/app");
    assert_eq!(started.linked_work_item, Some(linked_pr()));
    let card = started.inbox_card.unwrap();
    assert_eq!(card.identifier, "#42");
    assert!(card.prompt.starts_with("Work on this GitHub pull request:"));
    assert_eq!(
        *s.hooks.opened_tabs.borrow(),
        [(id, "/tmp/app".to_string())]
    );
    assert_eq!(*s.hooks.left.borrow(), 1);
}

#[gpui::test]
fn starting_a_linear_item_loads_its_description_first(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| match command {
        "linear_issue_details" => Ok(json!({ "body": "Steps to reproduce", "author": "Ada" })),
        _ => Err(format!("Unexpected command: {command}")),
    });
    let linear = with(item(9, "2026-09-13T12:00:00Z"), |row| {
        row.provider = InboxProvider::Linear;
        row.kind = InboxKind::Linear;
        row.id = Some("lin-9".into());
        row.identifier = Some("ENG-9".into());
        row.title = "Fix auth".into();
        row.project_path = String::new();
    });
    let task = s
        .inbox
        .update(cx, |inbox, cx| inbox.start_inbox_item(linear, None, cx));
    cx.run_until_parked();
    let id = done(cx, task).unwrap();
    let started = session(cx, &id).unwrap();
    assert_eq!(started.title, "ENG-9 Fix auth");
    assert_eq!(started.cwd, "~");
    assert!(started.linked_work_item.is_none());
    assert!(
        started
            .inbox_card
            .unwrap()
            .prompt
            .contains("Steps to reproduce")
    );
}

#[gpui::test]
fn ask_opens_one_temporary_conversation_per_item(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| match command {
        "gitlab_work_item_details" => Ok(json!({ "body": "MR description", "author": "Ada" })),
        _ => Err(format!("Unexpected command: {command}")),
    });
    let gitlab = with(github_item(), |row| {
        row.provider = InboxProvider::Gitlab;
        row.url = "https://gitlab.example.com/acme/app/-/merge_requests/42".into();
    });
    let first = s
        .inbox
        .update(cx, |inbox, cx| inbox.ask_inbox_item(gitlab.clone(), cx));
    let second = s
        .inbox
        .update(cx, |inbox, cx| inbox.ask_inbox_item(gitlab.clone(), cx));
    cx.run_until_parked();
    let id = done(cx, first).unwrap();
    assert_eq!(done(cx, second).unwrap(), id);
    let again = s
        .inbox
        .update(cx, |inbox, cx| inbox.ask_inbox_item(gitlab.clone(), cx));
    cx.run_until_parked();
    assert_eq!(done(cx, again).unwrap(), id);
    assert_eq!(session_ids(cx).len(), 1);
    let ask = session(cx, &id).unwrap();
    assert_eq!(ask.title, "Ask · Update sidebar activity");
    let context = ask.inbox_ask.unwrap();
    assert_eq!(
        context.key,
        "gitlab:gitlab.example.com:/acme/app/-/merge_requests/42"
    );
    assert_eq!(context.description.as_deref(), Some("MR description"));
    assert_eq!(s.backend.count("gitlab_work_item_details"), 1);
}

#[gpui::test]
fn restarting_ask_replaces_the_conversation_with_a_blank_one(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let ask = s
        .inbox
        .update(cx, |inbox, cx| inbox.ask_inbox_item(github_item(), cx));
    cx.run_until_parked();
    let id = done(cx, ask).unwrap();
    s.inbox
        .update(cx, |inbox, cx| inbox.set_ask_session(Some(id.clone()), cx));
    let restart = s
        .inbox
        .update(cx, |inbox, cx| inbox.restart_inbox_ask(github_item(), cx));
    cx.run_until_parked();
    let fresh = done(cx, restart).unwrap();
    assert_ne!(fresh, id);
    assert_eq!(session_ids(cx), std::slice::from_ref(&fresh));
    let restarted = session(cx, &fresh).unwrap();
    assert_eq!(restarted.title, "Ask · Update sidebar activity");
    assert!(restarted.inbox_ask.is_some());
    assert_eq!(
        s.inbox
            .read_with(cx, |inbox, _| inbox.ask_session().map(str::to_string)),
        Some(fresh)
    );
    assert!(s.store.commands().contains(&"session_delete".to_string()));
}

fn repair_request() -> CiRepairRequest {
    build_ci_repair_request("acme/app", 42, "abc", &[failed("tests", None, None)])
}

#[gpui::test]
fn repairing_checks_needs_a_local_project(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let no_project = with(github_item(), |row| row.project_path = String::new());
    let task = s.inbox.update(cx, |inbox, cx| {
        inbox.repair_checks(no_project, repair_request(), None, cx)
    });
    assert_eq!(
        done(cx, task),
        Err("Choose a local project for this PR first.".into())
    );
}

#[gpui::test]
fn repairing_checks_starts_a_tracked_chat_and_settles_it(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    *s.hooks.accept_submit.borrow_mut() = true;
    let task = s.inbox.update(cx, |inbox, cx| {
        inbox.repair_checks(github_item(), repair_request(), None, cx)
    });
    cx.run_until_parked();
    done(cx, task).unwrap();
    let ids = session_ids(cx);
    assert_eq!(ids.len(), 1);
    let chat = session(cx, &ids[0]).unwrap();
    assert_eq!(chat.title, "Fix CI #42: Update sidebar activity");
    assert_eq!(chat.linked_work_item, Some(linked_pr()));
    assert_eq!(
        *s.hooks.submits.borrow(),
        [(
            ids[0].clone(),
            "Fix 1 failed CI check for acme/app PR #42.".to_string()
        )]
    );
    assert_eq!(*s.hooks.selected.borrow(), [ids[0].clone()]);
    let repairs = s.inbox.update(cx, |inbox, _| inbox.ci_repairs());
    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0].phase, CiRepairPhase::Running);
    assert_eq!(repairs[0].session_id, ids[0]);

    let settle = s.hooks.settles.borrow_mut().pop().unwrap();
    cx.update(|cx| settle(CiRepairOutcome::Completed, cx));
    cx.run_until_parked();
    let repairs = s.inbox.update(cx, |inbox, _| inbox.ci_repairs());
    assert_eq!(repairs[0].phase, CiRepairPhase::Completed);
}

#[gpui::test]
fn repairing_checks_refuses_busy_chats_and_forgets_rejected_repairs(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let mut busy = Session::blank("busy", HarnessId::Claude, "", "/tmp/app");
    busy.busy = Some(true);
    let other = Session::blank("other-project", HarnessId::Claude, "", "/tmp/elsewhere");
    let idle = Session::blank("idle", HarnessId::Claude, "", "/tmp/app");
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.upsert(busy, cx);
            sessions.upsert(other, cx);
            sessions.upsert(idle, cx);
        })
    });
    let run = |s: &Setup, cx: &mut TestAppContext, id: &str| {
        let task = s.inbox.update(cx, |inbox, cx| {
            inbox.repair_checks(github_item(), repair_request(), Some(id.to_string()), cx)
        });
        cx.run_until_parked();
        done(cx, task)
    };
    assert_eq!(
        run(&s, cx, "busy"),
        Err("This chat is busy. Choose another chat or start a new one.".into())
    );
    assert_eq!(
        run(&s, cx, "other-project"),
        Err("Choose a chat from this project.".into())
    );
    let rejected = run(&s, cx, "idle").unwrap_err();
    assert!(rejected.contains("Could not start this fix"));
    assert!(s.inbox.update(cx, |inbox, _| inbox.ci_repairs()).is_empty());
}

#[gpui::test]
fn linking_a_session_saves_and_rolls_back_on_failure(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let open = Session::blank("chat", HarnessId::Claude, "", "/tmp/app");
    cx.update(|cx| Engine::sessions(cx).update(cx, |sessions, cx| sessions.upsert(open, cx)));
    s.inbox.update(cx, |inbox, cx| {
        inbox.set_session_linked_work_item("chat", Some(linked_pr()), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        session(cx, "chat").unwrap().linked_work_item,
        Some(linked_pr())
    );
    assert_eq!(
        s.store.calls("session_set_linked_work_item")[0]["linkedWorkItem"]["number"],
        json!(42)
    );

    s.store.set_failing("session_set_linked_work_item", true);
    s.inbox.update(cx, |inbox, cx| {
        inbox.set_session_linked_work_item("chat", None, cx)
    });
    assert_eq!(session(cx, "chat").unwrap().linked_work_item, None);
    cx.run_until_parked();
    assert_eq!(
        session(cx, "chat").unwrap().linked_work_item,
        Some(linked_pr())
    );
    let changes = s.hooks.linked_changes.borrow();
    assert_eq!(
        changes.last().unwrap(),
        &("chat".to_string(), Some(linked_pr()), true)
    );
}

#[gpui::test]
fn linked_panels_open_beside_their_session_and_close(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let open = Session::blank("chat", HarnessId::Claude, "", "/tmp/app");
    cx.update(|cx| Engine::sessions(cx).update(cx, |sessions, cx| sessions.upsert(open, cx)));
    s.inbox.update(cx, |inbox, cx| {
        inbox.open_linked_work_item(linked_pr(), "chat", cx)
    });
    cx.run_until_parked();
    s.inbox.read_with(cx, |inbox, _| {
        assert_eq!(inbox.linked_panels().len(), 1);
        assert_eq!(inbox.linked_panels()[0].cwd, "/tmp/app");
        assert!(
            inbox
                .active_linked_panel("other", &["chat".into()])
                .is_some()
        );
        assert!(inbox.active_linked_panel("other", &[]).is_none());
    });
    s.inbox.update(cx, |inbox, cx| {
        inbox.retain_linked_panels(&HashSet::new(), cx)
    });
    assert!(
        s.inbox
            .read_with(cx, |inbox, _| inbox.linked_panels().is_empty())
    );
    s.inbox.update(cx, |inbox, cx| {
        inbox.open_linked_work_item(linked_pr(), "chat", cx)
    });
    s.inbox.update(cx, |inbox, cx| {
        inbox.close_linked_work_item_panel("chat", cx)
    });
    cx.run_until_parked();
    // The close superseded the pending open.
    assert!(
        s.inbox
            .read_with(cx, |inbox, _| inbox.linked_panels().is_empty())
    );
}

#[gpui::test]
fn related_sessions_prefer_the_open_copy_and_sort_newest_first(cx: &mut TestAppContext) {
    let s = setup(cx, |command, _| {
        Err(format!("Unexpected command: {command}"))
    });
    let mut stored = linked_session();
    stored.updated_at = 5;
    let mut older = linked_session();
    older.id = "older".into();
    older.updated_at = 1;
    s.inbox.update(cx, |inbox, cx| {
        inbox.set_stored_linked_sessions(vec![older, stored], cx)
    });
    let mut open = Session::blank("linked-session", HarnessId::Claude, "", "/tmp/app");
    open.title = "Live title".into();
    open.linked_work_item = Some(linked_pr());
    let mut ask = Session::blank("ask", HarnessId::Claude, "", "/tmp/app");
    ask.linked_work_item = Some(linked_pr());
    ask.inbox_ask = Some(monocode_core::inbox::InboxAskContext {
        key: "k".into(),
        title: "t".into(),
        url: "u".into(),
        provider: InboxProvider::Github,
        description: None,
        extra: Extra::new(),
    });
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.upsert(open, cx);
            sessions.upsert(ask, cx);
        })
    });
    let rows = s
        .inbox
        .read_with(cx, |inbox, cx| inbox.inbox_related_sessions(&[], cx));
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, ["linked-session", "older"]);
    assert_eq!(rows[0].title, "Live title");
    assert_eq!(rows[0].updated_at, 5);
}

#[gpui::test]
fn marking_items_seen_updates_the_badge(cx: &mut TestAppContext) {
    let remote = Remote::default();
    remote
        .prs
        .lock()
        .push(remote_pr("acme/app", "2026-09-13T12:00:00Z"));
    let s = setup(cx, remote.handler());
    s.client
        .seed_inbox_seen_if_needed(&[InboxSeenEntry::new(KEY, "2026-09-12T12:00:00Z")]);
    mount(&s, cx, vec![]);
    let listed = with(github_item(), |row| {
        row.updated_at = "2026-09-13T12:00:00Z".into()
    });
    assert_eq!(inbox_item_key(&listed), KEY);
    assert!(
        s.inbox
            .read_with(cx, |inbox, _| inbox.is_item_unseen(&listed))
    );
    s.inbox
        .update(cx, |inbox, cx| inbox.mark_item_seen(&listed, cx));
    assert!(!unseen(&s, cx));
}
