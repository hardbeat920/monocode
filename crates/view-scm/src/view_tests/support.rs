//! Test doubles: a scriptable [`GitBackend`] that records every call (the
//! TypeScript tests' `vi.mock` of platform/tauri/fs), and the app setup.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gpui::{App, AppContext as _, Task, TestAppContext};
use monocode_core::appearance::ChangesView;
use monocode_engine::projects::GitStatuses;
use monocode_engine::projects::testing::FakeBackend;
use monocode_ui::AppearanceSettings;

use crate::git::{
    GitBackend, GitChangedFile, GitDiffIndex, GitFileDiff, GitFileDiffKind, GitHistory,
    GitHubWorkItem, GitPr, GitRangeContext, Worktree, Worktrees,
};
use crate::hooks::{CommitMessageRequest, ScmHooks};
use crate::scm::Scm;

#[derive(Default)]
struct FakeState {
    calls: Vec<(String, Vec<String>)>,
    failing: HashMap<String, String>,
    range: Option<GitRangeContext>,
    pr_url: String,
    pr: Option<GitPr>,
    pr_action: Option<GitHubWorkItem>,
    created: Option<Worktree>,
}

/// A [`GitBackend`] that records calls and answers from scripted values.
#[derive(Default)]
pub struct FakeGit {
    state: Mutex<FakeState>,
}

impl FakeGit {
    fn record(&self, name: &str, args: &[&str]) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        state.calls.push((
            name.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
        ));
        match state.failing.get(name) {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// The arguments of every call to `name`.
    pub fn calls(&self, name: &str) -> Vec<Vec<String>> {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, args)| args.clone())
            .collect()
    }

    pub fn fail(&self, name: &str, error: Option<&str>) {
        let mut state = self.state.lock().unwrap();
        match error {
            Some(error) => state.failing.insert(name.into(), error.into()),
            None => state.failing.remove(name),
        };
    }

    pub fn set_range(&self, range: GitRangeContext) {
        self.state.lock().unwrap().range = Some(range);
    }

    pub fn set_pr_url(&self, url: &str) {
        self.state.lock().unwrap().pr_url = url.into();
    }

    pub fn set_pr_action(&self, item: GitHubWorkItem) {
        self.state.lock().unwrap().pr_action = Some(item);
    }
}

impl GitBackend for FakeGit {
    fn git_diff_files(&self, cwd: &str) -> Result<GitDiffIndex, String> {
        self.record("git_diff_files", &[cwd])?;
        Ok(GitDiffIndex::default())
    }

    fn git_file_diff(
        &self,
        cwd: &str,
        relative: &str,
        kind: GitFileDiffKind,
    ) -> Result<GitFileDiff, String> {
        self.record("git_file_diff", &[cwd, relative, kind.as_str()])?;
        Ok(GitFileDiff {
            path: format!("{cwd}/{relative}"),
            relative: relative.into(),
            status: "modified".into(),
            original: String::new(),
            current: String::new(),
            binary: false,
            too_large: false,
        })
    }

    fn git_history(&self, cwd: &str) -> Result<GitHistory, String> {
        self.record("git_history", &[cwd])?;
        Ok(GitHistory::default())
    }

    fn git_commit_files(&self, cwd: &str, sha: &str) -> Result<Vec<GitChangedFile>, String> {
        self.record("git_commit_files", &[cwd, sha])?;
        Ok(Vec::new())
    }

    fn git_commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        relative: &str,
    ) -> Result<GitFileDiff, String> {
        self.record("git_commit_file_diff", &[cwd, sha, relative])?;
        self.git_file_diff(cwd, relative, GitFileDiffKind::Unstaged)
    }

    fn git_stage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.record("git_stage_file", &[cwd, relative])
    }

    fn git_stage_contents(&self, cwd: &str, relative: &str, contents: &str) -> Result<(), String> {
        self.record("git_stage_contents", &[cwd, relative, contents])
    }

    fn git_unstage_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.record("git_unstage_file", &[cwd, relative])
    }

    fn git_discard_file(&self, cwd: &str, relative: &str) -> Result<(), String> {
        self.record("git_discard_file", &[cwd, relative])
    }

    fn git_stage_all(&self, cwd: &str) -> Result<(), String> {
        self.record("git_stage_all", &[cwd])
    }

    fn git_unstage_all(&self, cwd: &str) -> Result<(), String> {
        self.record("git_unstage_all", &[cwd])
    }

    fn git_discard_all(&self, cwd: &str) -> Result<(), String> {
        self.record("git_discard_all", &[cwd])
    }

    fn git_commit(&self, cwd: &str, message: &str, amend: bool) -> Result<(), String> {
        self.record(
            "git_commit",
            &[cwd, message, if amend { "amend" } else { "" }],
        )
    }

    fn git_head_message(&self, cwd: &str) -> Result<String, String> {
        self.record("git_head_message", &[cwd])?;
        Ok(String::new())
    }

    fn git_push(&self, cwd: &str) -> Result<(), String> {
        self.record("git_push", &[cwd])
    }

    fn git_pull(&self, cwd: &str) -> Result<(), String> {
        self.record("git_pull", &[cwd])
    }

    fn git_sync(&self, cwd: &str) -> Result<(), String> {
        self.record("git_sync", &[cwd])
    }

    fn git_range_context(&self, cwd: &str) -> Result<GitRangeContext, String> {
        self.record("git_range_context", &[cwd])?;
        self.state
            .lock()
            .unwrap()
            .range
            .clone()
            .ok_or_else(|| "no range".to_string())
    }

    fn git_pr_status(&self, cwd: &str) -> Result<Option<GitPr>, String> {
        self.record("git_pr_status", &[cwd])?;
        Ok(self.state.lock().unwrap().pr.clone())
    }

    fn git_pr_create(
        &self,
        cwd: &str,
        title: &str,
        body: &str,
        base: &str,
        head: &str,
    ) -> Result<String, String> {
        self.record("git_pr_create", &[cwd, title, body, base, head])?;
        Ok(self.state.lock().unwrap().pr_url.clone())
    }

    fn git_checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        force: bool,
    ) -> Result<String, String> {
        self.record(
            "git_checkout",
            &[
                cwd,
                name,
                remote.unwrap_or("null"),
                if force { "force" } else { "" },
            ],
        )?;
        Ok(name.into())
    }

    fn git_create_branch(&self, cwd: &str, name: &str, force: bool) -> Result<String, String> {
        self.record(
            "git_create_branch",
            &[cwd, name, if force { "force" } else { "" }],
        )?;
        Ok(name.into())
    }

    fn git_stash(&self, cwd: &str, message: Option<&str>) -> Result<(), String> {
        self.record("git_stash", &[cwd, message.unwrap_or("")])
    }

    fn git_worktree_create(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
    ) -> Result<Worktree, String> {
        self.record(
            "git_worktree_create",
            &[cwd, branch, base, if existing { "existing" } else { "" }],
        )?;
        Ok(self
            .state
            .lock()
            .unwrap()
            .created
            .clone()
            .unwrap_or_else(|| Worktree::new(format!("{cwd}-worktrees/{branch}"), Some(branch))))
    }

    fn git_worktree_check_remove(&self, cwd: &str, path: &str, force: bool) -> Result<(), String> {
        self.record(
            "git_worktree_check_remove",
            &[cwd, path, if force { "force" } else { "" }],
        )
    }

    fn git_github_pr_action(
        &self,
        cwd: &str,
        repo: &str,
        number: i64,
        action: &str,
    ) -> Result<GitHubWorkItem, String> {
        let number = number.to_string();
        self.record("git_github_pr_action", &[cwd, repo, &number, action])?;
        self.state
            .lock()
            .unwrap()
            .pr_action
            .clone()
            .ok_or_else(|| "no result".to_string())
    }

    fn reveal_path(&self, path: &str) -> Result<(), String> {
        self.record("reveal_path", &[path])
    }
}

pub struct Setup {
    pub scm: Scm,
    pub git: Arc<FakeGit>,
    pub backend: Arc<FakeBackend>,
}

/// Install the theme and widgets, and make an [`Scm`] over the fakes.
pub fn setup(cx: &mut TestAppContext, hooks: ScmHooks) -> Setup {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
    });
    let git = Arc::new(FakeGit::default());
    let backend = FakeBackend::new();
    let scm = cx.update(|cx| {
        let projects: Arc<dyn monocode_engine::projects::ProjectsBackend> = backend.clone();
        let statuses = cx.new(|_| GitStatuses::new(projects, Arc::new(|| 0)));
        Scm::new(git.clone(), statuses, hooks, ChangesView::List, cx)
    });
    Setup { scm, git, backend }
}

/// Hooks that answer every confirm with yes and record alerts, opened URLs,
/// and watched-file invalidations.
#[derive(Clone, Default)]
pub struct Recorded {
    pub alerts: Rc<RefCell<Vec<String>>>,
    pub urls: Rc<RefCell<Vec<String>>>,
    pub files_changed: Rc<RefCell<usize>>,
    /// The paths of each watched-file invalidation, `None` for all files.
    pub changed_paths: Rc<RefCell<Vec<Option<Vec<String>>>>>,
    pub pr_content_calls: Rc<RefCell<usize>>,
}

impl Recorded {
    pub fn hooks(&self) -> ScmHooks {
        let alerts = self.alerts.clone();
        let urls = self.urls.clone();
        let files = self.files_changed.clone();
        let changed = self.changed_paths.clone();
        let pr_calls = self.pr_content_calls.clone();
        ScmHooks {
            confirm: Some(Rc::new(|_, _, _| Task::ready(true))),
            alert: Some(Rc::new(move |message, _, _| {
                alerts.borrow_mut().push(message)
            })),
            open_url: Some(Rc::new(move |url, _| {
                urls.borrow_mut().push(url.to_string())
            })),
            files_changed: Some(Rc::new(move |paths, _| {
                *files.borrow_mut() += 1;
                changed.borrow_mut().push(paths.map(|paths| paths.to_vec()));
            })),
            generate_pr_content: Some(Rc::new(move |_, _| {
                *pr_calls.borrow_mut() += 1;
                Task::ready(Ok(None))
            })),
            ..Default::default()
        }
    }
}

/// Sets a flag when dropped, to see that a cancelled task stopped.
pub struct DropFlag(pub Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// What [`pending_generator`] returns: the generator, the sender that
/// finishes its first call, the cancelled flag, and the call count.
pub type PendingGenerator = (
    crate::hooks::GenerateCommitMessage,
    Rc<RefCell<Option<futures::channel::oneshot::Sender<String>>>>,
    Arc<AtomicBool>,
    Rc<RefCell<usize>>,
);

/// A commit message generator whose first call waits on the returned
/// sender, with a flag that turns on when that call is cancelled. Later
/// calls answer `next` at once.
pub fn pending_generator(next: &'static str) -> PendingGenerator {
    let (tx, rx) = futures::channel::oneshot::channel::<String>();
    let rx = Rc::new(RefCell::new(Some(rx)));
    let aborted = Arc::new(AtomicBool::new(false));
    let calls = Rc::new(RefCell::new(0usize));
    let generator = {
        let (aborted, calls) = (aborted.clone(), calls.clone());
        Rc::new(
            move |_: CommitMessageRequest, cx: &mut App| -> Task<Result<String, String>> {
                *calls.borrow_mut() += 1;
                match rx.borrow_mut().take() {
                    Some(rx) => {
                        let guard = DropFlag(aborted.clone());
                        cx.foreground_executor().spawn(async move {
                            let _guard = guard;
                            rx.await.map_err(|_| "cancelled".to_string())
                        })
                    }
                    None => Task::ready(Ok(next.to_string())),
                }
            },
        ) as crate::hooks::GenerateCommitMessage
    };
    (generator, Rc::new(RefCell::new(Some(tx))), aborted, calls)
}

pub fn index() -> GitDiffIndex {
    GitDiffIndex {
        branch: Some("feature/pull".into()),
        head: Some("abc123".into()),
        files: Vec::new(),
        additions: 0,
        deletions: 0,
        remote: None,
        upstream: None,
        default_branch: Some("main".into()),
        ahead: 0,
        behind: 0,
        ahead_of_default: 0,
        head_pushed: true,
    }
}

pub fn tree() -> Worktree {
    Worktree {
        path: "/repo-worktrees/feature".into(),
        branch: Some("feature".into()),
        head: "abc".into(),
        is_main: false,
        locked: false,
        prunable: false,
        missing: false,
        dirty: Some(true),
        unpushed: Some(2),
        session_ids: Vec::new(),
    }
}

pub fn main_tree(path: &str) -> Worktree {
    Worktree {
        path: path.into(),
        branch: Some("main".into()),
        is_main: true,
        ..tree()
    }
}

pub fn worktrees(list: Vec<Worktree>) -> Worktrees {
    Worktrees {
        worktrees: list,
        default_root: "/repo-worktrees".into(),
    }
}
