//! Entity tests for the projects package: the `Projects` mirror, git status
//! loading, and the App.tsx flows in `actions`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use gpui::{Entity, Subscription, TestAppContext};
use monocode_core::appearance::SidebarTabId;
use monocode_core::block::{Block, BlockRole};
use monocode_core::session::{MessageQueueStatus, QueuedMessage, WorkspaceMode, new_session};
use monocode_core::{HarnessId, Session};
use monocode_git::fs::{GitBranches, GitChangedFile, GitDiffIndex, GitDiffStats};
use monocode_layout::layout::{WorkspaceTab, new_file_tab, new_tab};
use monocode_layout::paths::project_key;
use monocode_layout::tab_groups::KEY_VERSION_KEY;
use monocode_settings::Kv;
use serde_json::json;

use super::actions::{
    apply_project_location_change, on_branch_change, on_cwd_change, on_place_session_in_folder,
    on_remove_project, on_remove_worktree, on_workspace_mode_change, on_worktree_base_change,
    on_worktree_change, open_projects,
};
use super::backend::{ProjectLocation, Worktree, WorktreeRemoval, Worktrees};
use super::git_status::{DIFF_STATS_RESUME_TTL_MS, GIT_POLL, WatchKind};
use super::hooks::SessionFolderTarget;
use super::project_chat_background::ProjectChatBackgroundSettings;
use super::project_groups::ProjectGroup;
use super::testing::{FakeBackend, TestHooks};
use super::{ModelInputs, Projects, ProjectsConfig, ProjectsEvent, ProjectsGlobal, recents};
use crate::runtime::engine::Engine;
use crate::runtime::sessions::Sessions;
use crate::runtime::testing::{FakeBackend as StoreFake, init_test_engine};

struct Setup {
    kv: Kv,
    backend: Arc<FakeBackend>,
    store: Arc<StoreFake>,
    hooks: Rc<TestHooks>,
    clock: Arc<AtomicI64>,
    projects: Entity<Projects>,
    sessions: Entity<Sessions>,
}

impl Setup {
    fn advance(&self, cx: &mut TestAppContext, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
        cx.executor()
            .advance_clock(Duration::from_millis(ms as u64));
        cx.run_until_parked();
    }

    fn insert(&self, cx: &mut TestAppContext, session: Session) {
        self.sessions
            .update(cx, |sessions, cx| sessions.insert(session, cx));
    }

    fn session(&self, cx: &mut TestAppContext, id: &str) -> Option<Session> {
        self.sessions
            .read_with(cx, |sessions, _| sessions.get(id).cloned())
    }

    fn ids(&self, cx: &mut TestAppContext) -> Vec<String> {
        self.sessions.read_with(cx, |sessions, _| sessions.ids())
    }
}

fn init(cx: &mut TestAppContext) -> Setup {
    let store = init_test_engine(cx);
    let kv = Kv::in_memory();
    kv.set_item(KEY_VERSION_KEY, "2");
    let backend = FakeBackend::new();
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let time = clock.clone();
    cx.update(|cx| {
        ProjectsGlobal::init(
            ProjectsConfig {
                kv: kv.clone(),
                backend: backend.clone(),
                clock: Arc::new(move || time.load(Ordering::SeqCst)),
            },
            cx,
        )
    });
    let hooks = TestHooks::new();
    cx.update(|cx| ProjectsGlobal::set_hooks(cx, hooks.clone()));
    let projects = cx.update(|cx| ProjectsGlobal::projects(cx));
    let sessions = cx.update(|cx| Engine::sessions(cx));
    Setup {
        kv,
        backend,
        store,
        hooks,
        clock,
        projects,
        sessions,
    }
}

fn session(id: &str, cwd: &str, harness: HarnessId) -> Session {
    let inputs = ModelInputs::default();
    new_session(&inputs.env(), id, harness, cwd, None, None, None)
}

fn chat(id: &str, cwd: &str) -> Session {
    let mut session = session(id, cwd, HarnessId::Codex);
    session.blocks = vec![Block::new(format!("{id}-user"), BlockRole::User, id)];
    session
}

fn tab_for(id: &str) -> WorkspaceTab {
    WorkspaceTab {
        id: format!("tab-{id}"),
        ..new_tab(id)
    }
}

fn record_events(
    projects: &Entity<Projects>,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<Vec<ProjectsEvent>>>, Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(projects, move |_, event: &ProjectsEvent, _| {
            seen.borrow_mut().push(*event)
        })
    });
    (events, subscription)
}

// The Projects mirror.

#[gpui::test]
fn remembering_a_project_restores_it_from_the_archive_and_records_its_folder(
    cx: &mut TestAppContext,
) {
    let setup = init(cx);
    let (events, _subscription) = record_events(&setup.projects, cx);
    recents::remember_project(&setup.kv, "/work/app");
    recents::archive_project(&setup.kv, "/work/app");
    cx.run_until_parked();
    events.borrow_mut().clear();
    setup.backend.push_location(Some(ProjectLocation {
        path: "/work/app".into(),
        identity: "unix:1:2".into(),
    }));

    setup.projects.update(cx, |projects, cx| {
        projects.remember_project("/work/app/", cx)
    });
    cx.run_until_parked();

    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.recents()[0].path, "/work/app");
        assert!(projects.archived().is_empty());
    });
    let seen = events.borrow().clone();
    assert!(seen.contains(&ProjectsEvent::ArchivedChanged));
    assert!(seen.contains(&ProjectsEvent::PathsChanged));
    assert_eq!(
        super::project_location::stored_identity(&setup.kv, "/work/app").as_deref(),
        Some("unix:1:2")
    );
}

#[gpui::test]
fn a_write_from_another_package_reloads_the_mirror(cx: &mut TestAppContext) {
    let setup = init(cx);
    let (events, _subscription) = record_events(&setup.projects, cx);
    setup
        .kv
        .set_item(recents::KEY, r#"[{"path":"/elsewhere/app","openedAt":5}]"#);
    cx.run_until_parked();
    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.recents()[0].path, "/elsewhere/app");
    });
    assert!(events.borrow().contains(&ProjectsEvent::PathsChanged));
}

#[gpui::test]
fn appearance_and_background_changes_announce_themselves(cx: &mut TestAppContext) {
    let setup = init(cx);
    let (events, _subscription) = record_events(&setup.projects, cx);
    setup.projects.update(cx, |projects, cx| {
        projects.save_label("/work/app", "App", cx);
        projects.save_chat_background_settings(
            "/work/app",
            &ProjectChatBackgroundSettings {
                path: "/bg.png".into(),
                empty_opacity: 0.2,
                session_opacity: 0.3,
                scope: monocode_core::appearance::ChatBackgroundScope::All,
                effect: monocode_core::appearance::NewThreadBackgroundEffect::None,
            },
            true,
            cx,
        );
    });
    let seen = events.borrow().clone();
    assert!(seen.contains(&ProjectsEvent::LabelsChanged));
    assert!(seen.contains(&ProjectsEvent::ChatBackgroundChanged {
        image_changed: true
    }));
    setup.projects.update(cx, |projects, _| {
        assert_eq!(
            projects.labels().get("/work/app").map(String::as_str),
            Some("App")
        );
    });
}

#[gpui::test]
fn groups_and_pins_round_trip_through_the_entity(cx: &mut TestAppContext) {
    let setup = init(cx);
    let group = setup
        .projects
        .update(cx, |projects, cx| projects.create_group(cx));
    setup.projects.update(cx, |projects, cx| {
        projects.set_group_assignment("/work/app", Some(&group.id), cx);
        projects.toggle_pin("/work/app", cx);
        projects.update_group(
            &group.id,
            |group| ProjectGroup {
                collapsed: true,
                ..group
            },
            cx,
        );
    });
    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.groups()[0].name, "New group");
        assert!(projects.groups()[0].collapsed);
        assert_eq!(
            projects.group_id_for_path("/work/app"),
            Some(group.id.clone())
        );
        assert_eq!(projects.pinned(), ["/work/app"]);
    });
}

#[gpui::test]
async fn one_folder_lookup_runs_per_project(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.push_location(Some(ProjectLocation {
        path: "/work/renamed".into(),
        identity: "unix:1:2".into(),
    }));
    let (first, second) = setup.projects.update(cx, |projects, cx| {
        (
            projects.synchronize_location("/work/app", cx),
            projects.synchronize_location("/work/app/", cx),
        )
    });
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert!(first.moved);
    assert_eq!(first, second);
    let lookups = setup
        .backend
        .location_calls()
        .iter()
        .filter(|(path, _)| path == "/work/app")
        .count();
    assert_eq!(lookups, 1);
}

// projectData.test.ts

#[gpui::test]
fn rebase_moves_path_keyed_project_settings_to_the_renamed_folder(cx: &mut TestAppContext) {
    let setup = init(cx);
    let from = "/work/monocode";
    let to = "/work/monocode-personal";
    let old_key = project_key(from);
    let new_key = project_key(to);
    setup.projects.update(cx, |projects, cx| {
        projects.save_groups(&[ProjectGroup::new("personal", "Personal", false)], cx);
        projects.set_group_assignment(from, Some("personal"), cx);
        projects.save_label(&old_key, "My MonoCode", cx);
        projects.save_sidebar_tab(from, SidebarTabId::Changes, cx);
        projects.save_chat_background_settings(
            &old_key,
            &ProjectChatBackgroundSettings {
                path: "/images/background.png".into(),
                empty_opacity: 0.2,
                session_opacity: 0.1,
                scope: monocode_core::appearance::ChatBackgroundScope::All,
                effect: monocode_core::appearance::NewThreadBackgroundEffect::Dither,
            },
            false,
            cx,
        );
    });

    cx.update(|cx| super::project_data::rebase_project_data(from, to, cx));

    setup.projects.update(cx, |projects, _| {
        let labels = projects.labels();
        assert_eq!(labels.len(), 1);
        assert_eq!(
            labels.get(&new_key).map(String::as_str),
            Some("My MonoCode")
        );
        assert_eq!(projects.group_id_for_path(to).as_deref(), Some("personal"));
        assert!(projects.chat_background_settings(&old_key).is_none());
        let moved = projects.chat_background_settings(&new_key).unwrap();
        assert_eq!(moved.path, "/images/background.png");
        assert_eq!(
            moved.effect,
            monocode_core::appearance::NewThreadBackgroundEffect::Dither
        );
        assert_eq!(projects.sidebar_tab(to), SidebarTabId::Changes);
        assert_eq!(projects.sidebar_tab(from), SidebarTabId::Sessions);
    });
    assert!(
        setup
            .hooks
            .calls()
            .contains(&format!("rebase_session_folder_settings({from}, {to})"))
    );
}

#[gpui::test]
async fn removing_project_data_deletes_chats_images_and_settings(cx: &mut TestAppContext) {
    let setup = init(cx);
    let key = project_key("/work/app");
    setup.store.insert_session(&chat("s1", "/work/app"));
    setup.store.insert_session(&chat("s2", "/work/other"));
    setup.projects.update(cx, |projects, cx| {
        projects.save_groups(&[ProjectGroup::new("g", "G", false)], cx);
        projects.set_group_assignment("/work/app", Some("g"), cx);
        projects.save_label(&key, "App", cx);
        projects.save_sidebar_tab("/work/app", SidebarTabId::Files, cx);
    });
    let mut providers = monocode_core::project_providers::ProjectProviders::default();
    providers.set_project_provider_hidden("/work/app", HarnessId::Claude, true);
    setup.kv.set_item(
        monocode_core::project_providers::PROJECT_PROVIDER_SETTINGS_KEY,
        &providers.to_json(),
    );

    let count = cx.update(|cx| super::project_data::project_session_count("/work/app", cx));
    cx.run_until_parked();
    assert_eq!(count.await, 1);

    cx.update(|cx| super::project_data::remove_project_data("/work/app/", cx))
        .detach();
    cx.run_until_parked();

    assert_eq!(setup.store.calls("session_delete").len(), 1);
    assert_eq!(setup.store.calls("session_delete")[0]["sessionId"], "s1");
    assert_eq!(
        setup.backend.calls("remove_project_logo"),
        [json!({ "project": key })]
    );
    assert_eq!(
        setup.backend.calls("remove_project_chat_background"),
        [json!({ "project": key })]
    );
    setup.projects.update(cx, |projects, _| {
        assert!(projects.labels().is_empty());
        assert_eq!(projects.group_id_for_path("/work/app"), None);
        assert_eq!(projects.sidebar_tab("/work/app"), SidebarTabId::Sessions);
    });
    assert_eq!(
        setup
            .kv
            .get_item(monocode_core::project_providers::PROJECT_PROVIDER_SETTINGS_KEY)
            .as_deref(),
        Some("{}")
    );
}

// Git status.

fn changed(path: &str, relative: &str) -> GitChangedFile {
    GitChangedFile {
        path: path.into(),
        relative: relative.into(),
        status: "modified".into(),
        additions: 1,
        deletions: 0,
        staged: false,
        unstaged: true,
    }
}

fn index(files: Vec<GitChangedFile>) -> GitDiffIndex {
    GitDiffIndex {
        additions: files.len() as i64,
        files,
        ..GitDiffIndex::default()
    }
}

#[gpui::test]
fn file_statuses_load_on_watch_and_follow_directory_changes(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.set_diff_index(
        "/repo",
        Ok(index(vec![changed("/repo/src/a.ts", "src/a.ts")])),
    );
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let watch = status.update(cx, |status, cx| status.watch(WatchKind::FileStatuses, cx));
    cx.run_until_parked();
    status.read_with(cx, |status, _| {
        assert_eq!(
            status
                .file_statuses()
                .dirs
                .get("/repo/src")
                .map(String::as_str),
            Some("modified")
        );
    });
    assert_eq!(setup.backend.count("git_diff_index"), 1);

    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    git.update(cx, |git, cx| git.dirs_changed(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_index"), 2);

    drop(watch);
    git.update(cx, |git, cx| git.dirs_changed(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_index"), 2);
}

#[gpui::test]
fn branches_settle_even_when_the_folder_is_not_a_repo(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup
        .backend
        .set_branches("/plain", Err("not a git repository".into()));
    let status = cx.update(|cx| ProjectsGlobal::git_status("/plain", cx));
    let _watch = status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx));
    status.read_with(cx, |status, _| assert!(!status.branches_state().settled));
    cx.run_until_parked();
    status.read_with(cx, |status, _| {
        assert!(status.branches_state().settled);
        assert_eq!(status.branches(), None);
    });

    let notified = Rc::new(RefCell::new(0));
    let count = notified.clone();
    let _observe = cx.update(|cx| cx.observe(&status, move |_, _| *count.borrow_mut() += 1));
    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    git.update(cx, |git, cx| git.window_focused(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_branches"), 2);
    assert_eq!(*notified.borrow(), 0);

    git.update(cx, |git, cx| {
        git.seed_branches(
            "/plain",
            GitBranches {
                current: Some("main".into()),
                ..GitBranches::default()
            },
            cx,
        )
    });
    status.read_with(cx, |status, _| {
        assert_eq!(
            status
                .branches()
                .and_then(|branches| branches.current.as_deref()),
            Some("main")
        );
    });
}

#[gpui::test]
fn diff_stats_reload_on_focus_only_after_the_resume_ttl(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.set_diff_stats(
        "/repo",
        Ok(GitDiffStats {
            files: 2,
            additions: 5,
            deletions: 1,
        }),
    );
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    let _watch = status.update(cx, |status, cx| status.watch(WatchKind::DiffStats, cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_stats"), 1);
    status.read_with(cx, |status, _| {
        assert_eq!(status.diff_stats().map(|stats| stats.files), Some(2))
    });

    git.update(cx, |git, cx| git.window_focused(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_stats"), 1);

    setup
        .clock
        .fetch_add(DIFF_STATS_RESUME_TTL_MS, Ordering::SeqCst);
    git.update(cx, |git, cx| git.window_focused(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_stats"), 2);

    // A git change reloads even while hidden.
    git.update(cx, |git, cx| git.set_hidden(true, cx));
    git.update(cx, |git, cx| git.git_changed(cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_stats"), 3);
}

#[gpui::test]
fn applied_stats_win_over_a_load_already_running(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.set_diff_stats(
        "/repo",
        Ok(GitDiffStats {
            files: 9,
            additions: 9,
            deletions: 9,
        }),
    );
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    let _watch = status.update(cx, |status, cx| status.watch(WatchKind::DiffStats, cx));
    let fresher = GitDiffStats {
        files: 1,
        additions: 2,
        deletions: 3,
    };
    git.update(cx, |git, cx| {
        git.apply_diff_stats("/repo", fresher.clone(), cx)
    });
    cx.run_until_parked();
    status.read_with(cx, |status, _| {
        assert_eq!(status.diff_stats(), Some(&fresher))
    });
}

#[gpui::test]
async fn worktrees_keep_the_last_list_beside_an_error_and_follow_git_changes(
    cx: &mut TestAppContext,
) {
    let setup = init(cx);
    let listed = Worktrees {
        worktrees: vec![Worktree::new("/repo", Some("main"))],
        default_root: "/repo-worktrees".into(),
    };
    setup.backend.set_worktrees("/repo", Ok(listed.clone()));
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    let _watch = status.update(cx, |status, cx| status.watch(WatchKind::Worktrees, cx));
    cx.run_until_parked();
    status.read_with(cx, |status, _| {
        assert_eq!(status.worktrees().data.as_ref(), Some(&listed))
    });

    setup
        .backend
        .set_worktrees("/repo", Err("git failed".into()));
    let loaded = status.update(cx, |status, cx| status.refresh_worktrees(cx));
    cx.run_until_parked();
    assert!(!loaded.await);
    status.read_with(cx, |status, _| {
        assert_eq!(status.worktrees().data.as_ref(), Some(&listed));
        assert_eq!(status.worktrees().error.as_deref(), Some("git failed"));
    });

    // A git change during a read gets one follow-up read.
    setup.backend.set_worktrees("/repo", Ok(listed.clone()));
    setup.backend.clear_calls();
    let refresh = status.update(cx, |status, cx| status.refresh_worktrees(cx));
    git.update(cx, |git, cx| git.git_changed(cx));
    cx.run_until_parked();
    assert!(refresh.await);
    assert_eq!(setup.backend.count("git_worktrees"), 2);
}

#[gpui::test]
fn the_diff_index_polls_while_watched_and_visible(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup
        .backend
        .set_diff_index("/repo", Ok(index(vec![changed("/repo/a.ts", "a.ts")])));
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let git = cx.update(|cx| ProjectsGlobal::git(cx));
    let watch = status.update(cx, |status, cx| status.watch(WatchKind::Index, cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_index"), 1);
    status.read_with(cx, |status, _| {
        assert_eq!(status.index().map(|index| index.files.len()), Some(1));
        assert_eq!(status.diff_stats().map(|stats| stats.files), Some(1));
    });

    setup.advance(cx, GIT_POLL.as_millis() as i64);
    assert_eq!(setup.backend.count("git_diff_index"), 2);

    git.update(cx, |git, cx| git.set_hidden(true, cx));
    setup.advance(cx, GIT_POLL.as_millis() as i64);
    assert_eq!(setup.backend.count("git_diff_index"), 2);

    git.update(cx, |git, cx| git.set_hidden(false, cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_diff_index"), 3);

    drop(watch);
    setup.advance(cx, GIT_POLL.as_millis() as i64 * 2);
    assert_eq!(setup.backend.count("git_diff_index"), 3);
}

#[gpui::test]
fn an_index_change_announces_a_git_change_to_other_watchers(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup
        .backend
        .set_diff_index("/repo", Ok(index(vec![changed("/repo/a.ts", "a.ts")])));
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let _index = status.update(cx, |status, cx| status.watch(WatchKind::Index, cx));
    let _branches = status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx));
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_branches"), 1);

    setup.backend.set_diff_index(
        "/repo",
        Ok(index(vec![
            changed("/repo/a.ts", "a.ts"),
            changed("/repo/b.ts", "b.ts"),
        ])),
    );
    status.update(cx, |status, cx| status.reload_index(cx));
    cx.run_until_parked();
    // The changed index announced a git change; branches reloaded once, and
    // the index reload it caused found nothing new.
    assert_eq!(setup.backend.count("git_branches"), 2);
    assert_eq!(setup.backend.count("git_diff_index"), 3);
}

#[gpui::test]
fn git_changes_in_one_cycle_reload_once(cx: &mut TestAppContext) {
    let setup = init(cx);
    let status = cx.update(|cx| ProjectsGlobal::git_status("/repo", cx));
    let _watch = status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        super::notify_git_changed(cx);
        super::notify_git_changed(cx);
    });
    cx.run_until_parked();
    assert_eq!(setup.backend.count("git_branches"), 2);
}

// Folder, branch, and working copy.

#[gpui::test]
fn moving_a_conversation_to_another_folder_opens_a_new_tab(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, chat("s1", "/work/alpha"));
    cx.update(|cx| on_cwd_change("s1", "/work/beta/", cx));
    cx.run_until_parked();

    let ids = setup.ids(cx);
    assert_eq!(ids.len(), 2);
    let created = setup.session(cx, &ids[1]).unwrap();
    assert_eq!(created.cwd, "/work/beta");
    assert_eq!(created.harness, HarnessId::Codex);
    assert_eq!(setup.session(cx, "s1").unwrap().cwd, "/work/alpha");
    assert_eq!(setup.hooks.project_cwd(), "/work/beta");
    assert_eq!(setup.hooks.tabs()[0].focused_id, created.id);
    assert_eq!(setup.hooks.active_tab_id(), setup.hooks.tabs()[0].id);
    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.recents()[0].path, "/work/beta")
    });
}

#[gpui::test]
fn a_blank_session_moves_in_place_and_drops_its_working_copy(cx: &mut TestAppContext) {
    let setup = init(cx);
    let mut blank = session("s1", "/work/alpha", HarnessId::Codex);
    blank.branch = Some("feature".into());
    blank.worktree_cwd = Some("/work/alpha-worktrees/feature".into());
    blank.workspace_mode = Some(WorkspaceMode::Worktree);
    blank.worktree_base = Some("main".into());
    setup.insert(cx, blank);

    cx.update(|cx| on_cwd_change("s1", "/work/beta", cx));
    cx.run_until_parked();

    let moved = setup.session(cx, "s1").unwrap();
    assert_eq!(moved.cwd, "/work/beta");
    assert_eq!(moved.branch, None);
    assert_eq!(moved.worktree_cwd, None);
    assert_eq!(moved.workspace_mode, None);
    assert_eq!(moved.worktree_base, None);
    assert_eq!(setup.ids(cx), ["s1"]);
    // The previous project's pending changes are kept.
    assert_eq!(
        setup.store.calls("session_checkpoint_keep")[0]["cwd"],
        "/work/alpha"
    );
    assert!(
        setup
            .hooks
            .calls()
            .contains(&"session_project_changed(s1, /work/beta)".to_string())
    );
}

#[gpui::test]
fn a_branch_change_drops_the_provider_thread_and_saves(cx: &mut TestAppContext) {
    let setup = init(cx);
    let mut current = chat("s1", "/repo");
    current.branch = Some("old".into());
    current.provider_session_id = Some("thread".into());
    setup.insert(cx, current);
    cx.update(|cx| on_branch_change("s1", cx));
    cx.run_until_parked();
    let next = setup.session(cx, "s1").unwrap();
    assert_eq!(next.branch, None);
    assert_eq!(next.provider_session_id, None);
    // Opening the chat saved its user turn; the branch change saves again.
    let saves = setup.store.calls("session_upsert");
    let last = &saves.last().unwrap()["session"];
    assert!(last.get("branch").is_none_or(serde_json::Value::is_null));
    assert!(
        last.get("providerSessionId")
            .is_none_or(serde_json::Value::is_null)
    );
}

#[gpui::test]
fn workspace_mode_changes_only_before_the_working_copy_exists(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, session("blank", "/repo", HarnessId::Codex));
    setup.insert(cx, chat("chat", "/repo"));

    cx.update(|cx| on_workspace_mode_change("blank", WorkspaceMode::Worktree, None, cx));
    assert_eq!(setup.session(cx, "blank").unwrap().workspace_mode, None);

    cx.update(|cx| on_workspace_mode_change("blank", WorkspaceMode::Worktree, Some("main"), cx));
    let blank = setup.session(cx, "blank").unwrap();
    assert_eq!(blank.workspace_mode, Some(WorkspaceMode::Worktree));
    assert_eq!(blank.worktree_base.as_deref(), Some("main"));

    cx.update(|cx| on_worktree_base_change("blank", "develop", cx));
    assert_eq!(
        setup.session(cx, "blank").unwrap().worktree_base.as_deref(),
        Some("develop")
    );

    cx.update(|cx| on_workspace_mode_change("chat", WorkspaceMode::Worktree, Some("main"), cx));
    assert_eq!(setup.session(cx, "chat").unwrap().workspace_mode, None);

    cx.update(|cx| on_workspace_mode_change("blank", WorkspaceMode::Current, None, cx));
    let blank = setup.session(cx, "blank").unwrap();
    assert_eq!(blank.workspace_mode, None);
    assert_eq!(blank.worktree_base, None);
}

fn feature_tree() -> Worktree {
    Worktree::new("/repo-worktrees/feature", Some("feature"))
}

#[gpui::test]
async fn a_blank_session_selects_a_worktree_in_place(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.set_worktrees(
        "/repo",
        Ok(Worktrees {
            worktrees: vec![Worktree::new("/repo", Some("main")), feature_tree()],
            default_root: "/repo-worktrees".into(),
        }),
    );
    let mut blank = session("s1", "/repo", HarnessId::Codex);
    blank.composer_seed = Some("draft".into());
    setup.insert(cx, blank);

    let change = cx.update(|cx| on_worktree_change("s1", feature_tree(), cx));
    cx.run_until_parked();
    change.await.unwrap();

    let moved = setup.session(cx, "s1").unwrap();
    assert_eq!(
        moved.worktree_cwd.as_deref(),
        Some("/repo-worktrees/feature")
    );
    assert_eq!(moved.branch.as_deref(), Some("feature"));
    assert_eq!(moved.composer_seed.as_deref(), Some("draft"));
    assert!(
        setup
            .hooks
            .calls()
            .contains(&"refresh_history(/repo)".to_string())
    );
    let switching = setup.sessions.read_with(cx, |sessions, _| {
        sessions.switching_worktree("s1").is_some()
    });
    assert!(!switching);
}

#[gpui::test]
async fn a_conversation_selecting_a_worktree_opens_a_new_session(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.backend.set_worktrees(
        "/repo",
        Ok(Worktrees {
            worktrees: vec![feature_tree()],
            default_root: "/repo-worktrees".into(),
        }),
    );
    setup.insert(cx, chat("s1", "/repo"));
    let change = cx.update(|cx| on_worktree_change("s1", feature_tree(), cx));
    cx.run_until_parked();
    change.await.unwrap();
    let ids = setup.ids(cx);
    assert_eq!(ids.len(), 2);
    let created = setup.session(cx, &ids[1]).unwrap();
    assert_eq!(
        created.worktree_cwd.as_deref(),
        Some("/repo-worktrees/feature")
    );
    assert_eq!(setup.session(cx, "s1").unwrap().worktree_cwd, None);
    assert_eq!(setup.hooks.tabs()[0].focused_id, created.id);
}

#[gpui::test]
async fn worktree_changes_refuse_busy_queued_and_orchestrated_sessions(cx: &mut TestAppContext) {
    let setup = init(cx);
    let mut busy = chat("busy", "/repo");
    busy.busy = Some(true);
    setup.insert(cx, busy);
    let mut queued = chat("queued", "/repo");
    queued.queued_messages = Some(vec![QueuedMessage {
        id: "q".into(),
        text: "later".into(),
        attachments: Vec::new(),
        note_card: None,
        handoff_card: None,
        intent: None,
    }]);
    setup.insert(cx, queued);
    setup.insert(cx, chat("lead", "/repo"));
    setup
        .hooks
        .state
        .borrow_mut()
        .running_orchestration
        .push("lead".into());
    setup.insert(cx, session("blank", "/repo", HarnessId::Codex));

    for (id, error) in [
        (
            "busy",
            "Wait for this session to finish before changing working copies.",
        ),
        (
            "missing",
            "Wait for this session to finish before changing working copies.",
        ),
        (
            "queued",
            "Clear queued messages before changing working copies.",
        ),
        (
            "lead",
            "Stop this orchestration run before changing working copies.",
        ),
        (
            "blank",
            "This worktree is no longer available. Refresh the picker.",
        ),
    ] {
        let change = cx.update(|cx| on_worktree_change(id, feature_tree(), cx));
        cx.run_until_parked();
        assert_eq!(change.await, Err(error.to_string()), "{id}");
    }
}

// Worktree deletion.

fn listed_feature(setup: &Setup, session_ids: &[&str]) {
    setup.backend.set_worktrees(
        "/repo",
        Ok(Worktrees {
            worktrees: vec![
                Worktree::new("/repo", Some("main")),
                Worktree {
                    session_ids: session_ids.iter().map(|id| id.to_string()).collect(),
                    ..feature_tree()
                },
            ],
            default_root: "/repo-worktrees".into(),
        }),
    );
}

#[gpui::test]
async fn a_worktree_with_sessions_needs_keep_sessions(cx: &mut TestAppContext) {
    let setup = init(cx);
    listed_feature(&setup, &["saved"]);
    let removal =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", false, false, cx));
    cx.run_until_parked();
    assert_eq!(
        removal.await,
        Err("Move or delete the sessions using this worktree first.".into())
    );
    assert_eq!(setup.backend.count("git_worktree_remove"), 0);
}

#[gpui::test]
async fn open_files_in_the_worktree_block_deletion(cx: &mut TestAppContext) {
    let setup = init(cx);
    listed_feature(&setup, &[]);
    setup.hooks.state.borrow_mut().open_files = vec![new_file_tab(
        "/repo-worktrees/feature/a.ts",
        "/repo-worktrees/feature",
        false,
        None,
        None,
    )];
    let removal =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", false, true, cx));
    cx.run_until_parked();
    assert_eq!(
        removal.await,
        Err("Close the files and terminals open in this worktree first.".into())
    );
}

#[gpui::test]
async fn kept_sessions_detach_when_their_worktree_is_deleted(cx: &mut TestAppContext) {
    let setup = init(cx);
    listed_feature(&setup, &["saved"]);
    setup.backend.set_removal(WorktreeRemoval {
        session_ids: vec!["saved".into()],
        project_cwd: "/repo".into(),
    });
    let mut open = chat("open", "/repo");
    open.worktree_cwd = Some("/repo-worktrees/feature".into());
    open.branch = Some("feature".into());
    open.provider_session_id = Some("thread".into());
    setup.insert(cx, open);
    setup.hooks.state.borrow_mut().project_cwd = "/repo-worktrees/feature".into();
    setup.hooks.state.borrow_mut().summaries = vec![{
        let mut row =
            crate::runtime::session_store::SessionSummary::new("saved", "/repo", HarnessId::Codex);
        row.worktree_cwd = Some("/repo-worktrees/feature".into());
        row
    }];

    let removal =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", true, true, cx));
    cx.run_until_parked();
    removal.await.unwrap();

    assert_eq!(
        setup.backend.calls("git_worktree_remove"),
        [
            json!({ "cwd": "/repo", "path": "/repo-worktrees/feature", "force": true, "keepSessions": true })
        ]
    );
    let kept = setup.session(cx, "open").unwrap();
    assert_eq!(kept.worktree_removed, Some(true));
    assert_eq!(kept.branch, None);
    assert_eq!(kept.provider_session_id, None);
    assert_eq!(kept.queue_status, Some(MessageQueueStatus::Paused));
    assert_eq!(setup.hooks.project_cwd(), "/repo");
    let row = setup.hooks.state.borrow().summaries[0].clone();
    assert_eq!(row.worktree_removed, Some(true));
    let removing = setup
        .sessions
        .read_with(cx, |sessions, _| sessions.is_removing("open"));
    assert!(!removing);
    // A second deletion of the same path can start again.
    let again =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", true, true, cx));
    cx.run_until_parked();
    assert!(again.await.is_ok());
}

#[gpui::test]
async fn a_second_deletion_of_the_same_worktree_is_refused(cx: &mut TestAppContext) {
    let setup = init(cx);
    listed_feature(&setup, &[]);
    let first =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", false, false, cx));
    let second =
        cx.update(|cx| on_remove_worktree("/repo", "/repo-worktrees/feature", false, false, cx));
    cx.run_until_parked();
    assert_eq!(
        second.await,
        Err("This worktree is already being deleted.".into())
    );
    assert!(first.await.is_ok());
    assert_eq!(setup.backend.count("git_worktree_remove"), 1);
}

// Opening and removing projects.

#[gpui::test]
fn opening_folders_creates_tabs_and_remembers_every_project(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, chat("b1", "/beta"));
    {
        let mut state = setup.hooks.state.borrow_mut();
        state.tabs = vec![tab_for("b1")];
        state.active_tab_id = "tab-b1".into();
    }
    cx.update(|cx| open_projects(&["/one".into(), "/beta".into(), "/two".into()], cx));
    cx.run_until_parked();

    assert_eq!(setup.ids(cx).len(), 3);
    let tabs = setup.hooks.tabs();
    assert_eq!(tabs.len(), 3);
    assert_eq!(setup.hooks.state.borrow().pages_closed, 1);
    assert_eq!(setup.hooks.project_cwd(), "/two");
    assert_eq!(setup.hooks.active_tab_id(), tabs[2].id);
    setup.projects.read_with(cx, |projects, _| {
        let paths: Vec<&str> = projects
            .recents()
            .iter()
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(paths, ["/two", "/beta", "/one"]);
    });

    // A dismissed picker changes nothing.
    cx.update(|cx| open_projects(&[], cx));
    assert_eq!(setup.hooks.state.borrow().pages_closed, 1);
}

#[gpui::test]
fn archiving_a_project_closes_its_tabs_and_saves_idle_chats(cx: &mut TestAppContext) {
    let setup = init(cx);
    recents::remember_project(&setup.kv, "/other");
    setup.insert(cx, chat("a1", "/alpha"));
    let mut busy = chat("a2", "/alpha");
    busy.busy = Some(true);
    setup.insert(cx, busy);
    setup.insert(cx, chat("o1", "/other"));
    {
        let mut state = setup.hooks.state.borrow_mut();
        state.tabs = vec![tab_for("a1"), tab_for("a2"), tab_for("o1")];
        state.active_tab_id = "tab-a1".into();
        state.project_cwd = "/alpha".into();
    }
    setup
        .projects
        .update(cx, |projects, cx| projects.remember_project("/alpha", cx));

    cx.update(|cx| on_remove_project("/alpha", false, cx));
    cx.run_until_parked();

    assert_eq!(setup.ids(cx), ["a2", "o1"]);
    let tabs = setup.hooks.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, "tab-o1");
    assert_eq!(setup.hooks.active_tab_id(), "tab-o1");
    assert_eq!(setup.hooks.project_cwd(), "/other");
    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.archived()[0].path, "/alpha");
        assert!(projects.recents().iter().all(|item| item.path != "/alpha"));
    });
    let saved: Vec<String> = setup
        .store
        .calls("session_upsert")
        .iter()
        .map(|call| {
            call["session"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert!(saved.contains(&"a1".to_string()));
}

#[gpui::test]
fn deleting_the_last_project_leaves_a_home_chat(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, chat("a1", "/alpha"));
    {
        let mut state = setup.hooks.state.borrow_mut();
        state.tabs = vec![tab_for("a1")];
        state.active_tab_id = "tab-a1".into();
        state.project_cwd = "/alpha".into();
    }
    cx.update(|cx| on_remove_project("/alpha", true, cx));
    cx.run_until_parked();

    let ids = setup.ids(cx);
    assert_eq!(ids.len(), 1);
    assert_eq!(setup.session(cx, &ids[0]).unwrap().cwd, "~");
    assert_eq!(setup.hooks.tabs()[0].focused_id, ids[0]);
    assert_eq!(setup.hooks.project_cwd(), "~");
    assert_eq!(setup.hooks.state.borrow().composer_focused, Some(true));
    assert!(
        setup
            .hooks
            .calls()
            .contains(&"project_sidebar_tab_removed(/alpha)".to_string())
    );
    assert_eq!(setup.backend.count("remove_project_logo"), 1);
}

#[gpui::test]
fn a_remote_session_files_its_remote_id(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, chat("shell", "remote://env/home/me/app"));
    setup
        .hooks
        .state
        .borrow_mut()
        .remote_sessions
        .insert("shell".into(), "remote-1".into());
    let target = SessionFolderTarget::New {
        name: "Active".into(),
    };
    cx.update(|cx| on_place_session_in_folder("shell", &target, cx));
    assert_eq!(
        setup.hooks.calls(),
        [format!(
            "place_session_in_folder(remote://env/home/me/app, remote-1, {target:?})"
        )]
    );
}

#[gpui::test]
async fn a_renamed_folder_carries_its_sessions_settings_and_rail_entry(cx: &mut TestAppContext) {
    let setup = init(cx);
    setup.insert(cx, chat("s1", "/work/monocode"));
    setup.insert(cx, chat("s2", "/work/other"));
    setup.projects.update(cx, |projects, cx| {
        projects.remember_project("/work/monocode", cx);
        projects.toggle_pin("/work/monocode", cx);
    });
    setup.hooks.state.borrow_mut().project_cwd = "/work/monocode".into();

    let change = cx.update(|cx| {
        apply_project_location_change("/work/monocode", "/work/monocode-personal", cx)
    });
    cx.run_until_parked();
    change.await.unwrap();

    assert_eq!(
        setup.store.calls("session_rebase_project"),
        [json!({ "fromCwd": "/work/monocode", "toCwd": "/work/monocode-personal" })]
    );
    assert_eq!(
        setup.session(cx, "s1").unwrap().cwd,
        "/work/monocode-personal"
    );
    assert_eq!(setup.session(cx, "s2").unwrap().cwd, "/work/other");
    assert_eq!(setup.hooks.project_cwd(), "/work/monocode-personal");
    setup.projects.read_with(cx, |projects, _| {
        assert_eq!(projects.recents()[0].path, "/work/monocode-personal");
        assert_eq!(projects.pinned(), ["/work/monocode-personal"]);
    });
    let calls = setup.hooks.calls();
    for expected in [
        "rebase_ci_repairs(/work/monocode, /work/monocode-personal)",
        "rebase_loaded_project(/work/monocode, /work/monocode-personal)",
        "project_location_changed(/work/monocode, /work/monocode-personal)",
        "rebase_session_folder_settings(/work/monocode, /work/monocode-personal)",
        "project_sidebar_tab_moved(/work/monocode, /work/monocode-personal)",
    ] {
        assert!(calls.contains(&expected.to_string()), "{expected}");
    }
}
