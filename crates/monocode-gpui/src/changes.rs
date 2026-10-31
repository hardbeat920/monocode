//! GitChangesPanel.tsx in the inset layout: the Changes header with branch
//! actions, the commit box, sync and pull-request actions, and the staged
//! and unstaged files as a list or a tree. Every Git call runs off-thread,
//! one mutation at a time per checkout.
use crate::tooltip::Tip;
use crate::{
    file_icons::file_type_icon,
    git_status::{GitChanged, ProjectGit},
    input::{self, Input},
    popover::{
        Align, Anchor, AnchorSlot, Menu, MenuAction, MenuEvent, MenuItem, MenuStyle, menu_popover,
    },
    right_panel::*,
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, Hsla, PromptLevel, Role,
    SharedString, Subscription, Task, Window, div, prelude::*, px, rgb, uniform_list,
};
use monocode_core::{
    agent::{AccessMode, Settings},
    provider::{self, Event, Kind, Request},
    scm::{self, ChangedFile, DiffIndex, Pr},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};
const ROW: f32 = 28.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChangesView {
    #[default]
    List,
    Tree,
}
pub(crate) enum ChangesEvent {
    View(ChangesView),
    /// A file's review, or all of one side's when `path` is None.
    OpenDiff {
        cwd: String,
        path: Option<String>,
        staged: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Kind2 {
    Staged,
    Unstaged,
}
impl Kind2 {
    fn key(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
enum FileAction {
    Stage,
    Unstage,
    Discard,
}

/// One 28px line of the scrolling list.
#[derive(Clone)]
enum Line {
    Section(Kind2),
    File {
        kind: Kind2,
        file: usize,
        depth: Option<usize>,
    },
    Dir {
        kind: Kind2,
        path: String,
        name: String,
        depth: usize,
        status: Option<String>,
        open: bool,
    },
}

#[derive(Default)]
struct ChangeDir {
    name: String,
    path: String,
    dirs: Vec<ChangeDir>,
    files: Vec<usize>,
    status: Option<String>,
}
/// buildChangeTree / sortChangeDir: folders first, names compared as
/// `localeCompare` roughly does, and a status shared by every descendant.
fn build_tree(files: &[ChangedFile], members: &[usize]) -> ChangeDir {
    let mut root = ChangeDir::default();
    for &ix in members {
        let segments: Vec<&str> = files[ix].relative.split('/').collect();
        let mut node = &mut root;
        for segment in &segments[..segments.len() - 1] {
            let path = if node.path.is_empty() {
                segment.to_string()
            } else {
                format!("{}/{segment}", node.path)
            };
            let pos = match node.dirs.iter().position(|d| d.path == path) {
                Some(pos) => pos,
                None => {
                    node.dirs.push(ChangeDir {
                        name: segment.to_string(),
                        path,
                        ..Default::default()
                    });
                    node.dirs.len() - 1
                }
            };
            node = &mut node.dirs[pos];
        }
        node.files.push(ix);
    }
    sort_dir(&mut root, files);
    root
}
fn locale_key(s: &str) -> (String, String) {
    (s.to_lowercase(), s.to_owned())
}
fn sort_dir(dir: &mut ChangeDir, files: &[ChangedFile]) -> Option<String> {
    dir.dirs.sort_by_key(|d| locale_key(&d.name));
    dir.files
        .sort_by_key(|&ix| locale_key(basename(&files[ix].relative)));
    let mut status: Option<String> = None;
    let mut mixed = false;
    let mut merge = |next: Option<String>| match (next, &status) {
        (None, _) => mixed = true,
        (Some(n), None) => status = Some(n),
        (Some(n), Some(s)) if &n != s => mixed = true,
        _ => {}
    };
    for child in &mut dir.dirs {
        merge(sort_dir(child, files));
    }
    for &ix in &dir.files {
        merge(Some(files[ix].status.clone()));
    }
    dir.status = if mixed { None } else { status };
    dir.status.clone()
}
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn dirname(relative: &str) -> &str {
    match relative.rfind('/') {
        Some(i) if i > 0 => &relative[..i],
        _ => "",
    }
}
fn status_letter(status: &str) -> &'static str {
    match status {
        "untracked" => "U",
        "added" => "A",
        "deleted" => "D",
        _ => "M",
    }
}
pub(crate) fn status_color(status: &str) -> Hsla {
    rgb(match status {
        "untracked" => SKY_400,
        "added" => DIFF_ADD,
        "deleted" => DIFF_DEL,
        _ => AMBER_400,
    })
    .into()
}
fn sync_status_label(index: &DiffIndex) -> String {
    let plural = |n: i64| if n == 1 { "" } else { "s" };
    if index.ahead > 0 && index.behind > 0 {
        format!(
            "Diverged from {}",
            index.upstream.as_deref().unwrap_or("upstream")
        )
    } else if index.ahead > 0 {
        format!("{} unpushed commit{}", index.ahead, plural(index.ahead))
    } else if index.behind > 0 {
        format!("{} incoming commit{}", index.behind, plural(index.behind))
    } else {
        "No files".into()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    Branch,
    Commit,
}
struct OpenMenu {
    kind: MenuKind,
    menu: Entity<Menu>,
    _events: Subscription,
}

pub(crate) struct Changes {
    git: Entity<ProjectGit>,
    cwd: String,
    enabled: bool,
    pub(crate) text_provider: Kind,
    message: Entity<Input>,
    amend: Option<(Option<String>, Option<String>)>,
    busy: Option<String>,
    status: Option<String>,
    status_clear: Option<Task<()>>,
    pr: Option<Pr>,
    pr_branch: Option<String>,
    pr_request: u64,
    generate_cancel: Option<Arc<AtomicBool>>,
    staged_open: bool,
    changes_open: bool,
    view: ChangesView,
    collapsed: HashSet<String>,
    selected: Option<(String, Kind2)>,
    /// The row under the pointer, which shows its actions. Kept as state
    /// rather than a group-hover `display`, so layout and paint always agree.
    hovered: Option<usize>,
    lines: Vec<Line>,
    menu: Option<OpenMenu>,
    branch_anchor: AnchorSlot,
    commit_anchor: AnchorSlot,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<ChangesEvent> for Changes {}

impl Changes {
    pub(crate) fn new(git: Entity<ProjectGit>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(Input::commit_message);
        let subscriptions = vec![
            cx.observe(&git, |this, _, cx| this.index_changed(cx)),
            cx.subscribe(&git, |this, _, _: &GitChanged, cx| this.index_changed(cx)),
            cx.subscribe(&message, |_, _, _: &input::Changed, cx| cx.notify()),
        ];
        let mut this = Self {
            git,
            cwd: String::new(),
            enabled: false,
            text_provider: Kind::Claude,
            message,
            amend: None,
            busy: None,
            status: None,
            status_clear: None,
            pr: None,
            pr_branch: None,
            pr_request: 0,
            generate_cancel: None,
            staged_open: true,
            changes_open: true,
            view: ChangesView::List,
            collapsed: HashSet::new(),
            selected: None,
            hovered: None,
            lines: Vec::new(),
            menu: None,
            branch_anchor: AnchorSlot::default(),
            commit_anchor: AnchorSlot::default(),
            _subscriptions: subscriptions,
        };
        this.rebuild(cx);
        this
    }
    pub(crate) fn set_view(&mut self, view: ChangesView, cx: &mut Context<Self>) {
        self.view = view;
        self.rebuild(cx);
    }
    pub(crate) fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            let request = self.pr_request;
            self.index_changed(cx);
            if self.pr_request == request {
                self.load_pr(cx);
            }
        }
    }
    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            self.load_pr(cx);
        }
    }
    fn index(&self, cx: &App) -> Option<DiffIndex> {
        self.git.read(cx).index.clone()
    }
    fn index_changed(&mut self, cx: &mut Context<Self>) {
        let git = self.git.read(cx);
        let project = git.project().to_owned();
        if project != self.cwd {
            // Checkouts don't share drafts or in-flight generation.
            self.cwd = project;
            self.cancel_generate();
            self.amend = None;
            self.pr = None;
            self.pr_branch = None;
            self.selected = None;
            self.message
                .update(cx, |m, cx| m.set_text(String::new(), cx));
        }
        let index = self.git.read(cx).index.clone();
        if let Some((branch, head)) = &self.amend
            && (index.as_ref().map(|i| &i.branch) != Some(branch)
                || index.as_ref().map(|i| &i.head) != Some(head))
        {
            self.amend = None;
            self.message
                .update(cx, |m, cx| m.set_text(String::new(), cx));
        }
        let branch = index.as_ref().and_then(|i| i.branch.clone());
        if self.enabled && branch != self.pr_branch {
            self.pr_branch = branch;
            self.load_pr(cx);
        }
        self.rebuild(cx);
    }
    fn load_pr(&mut self, cx: &mut Context<Self>) {
        self.pr_request += 1;
        let request = self.pr_request;
        let branch = self
            .git
            .read(cx)
            .index
            .as_ref()
            .and_then(|i| i.branch.clone());
        if self.cwd.is_empty() || self.cwd == "~" || branch.is_none() {
            self.pr = None;
            return;
        }
        let path = PathBuf::from(&self.cwd);
        let work = cx
            .background_executor()
            .spawn(async move { scm::pr_status(&path) });
        cx.spawn(async move |this, cx| {
            let pr = work.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                if this.pr_request == request {
                    this.pr = pr;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn staged_unstaged(index: Option<&DiffIndex>) -> (Vec<usize>, Vec<usize>) {
        let files = index.map_or(&[][..], |i| &i.files[..]);
        (
            (0..files.len()).filter(|&i| files[i].staged).collect(),
            (0..files.len()).filter(|&i| files[i].unstaged).collect(),
        )
    }
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let index = self.git.read(cx).index.clone();
        let (staged, unstaged) = Self::staged_unstaged(index.as_ref());
        let files = index.as_ref().map_or(&[][..], |i| &i.files[..]);
        let mut lines = Vec::new();
        for (kind, members, open) in [
            (Kind2::Staged, &staged, self.staged_open),
            (Kind2::Unstaged, &unstaged, self.changes_open),
        ] {
            if members.is_empty() {
                continue;
            }
            lines.push(Line::Section(kind));
            if !open {
                continue;
            }
            match self.view {
                ChangesView::List => lines.extend(members.iter().map(|&file| Line::File {
                    kind,
                    file,
                    depth: None,
                })),
                ChangesView::Tree => {
                    let tree = build_tree(files, members);
                    self.push_dir(&tree, kind, 0, &mut lines);
                }
            }
        }
        self.lines = lines;
        self.hovered = None;
        cx.notify();
    }
    fn push_dir(&self, dir: &ChangeDir, kind: Kind2, depth: usize, lines: &mut Vec<Line>) {
        for child in &dir.dirs {
            let open = !self
                .collapsed
                .contains(&format!("{}:{}", kind.key(), child.path));
            lines.push(Line::Dir {
                kind,
                path: child.path.clone(),
                name: child.name.clone(),
                depth,
                status: child.status.clone(),
                open,
            });
            if open {
                self.push_dir(child, kind, depth + 1, lines);
            }
        }
        lines.extend(dir.files.iter().map(|&file| Line::File {
            kind,
            file,
            depth: Some(depth),
        }));
    }

    fn set_status(&mut self, text: &str, cx: &mut Context<Self>) {
        self.status = Some(text.into());
        self.status_clear = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |this, cx| {
                this.status = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn mutated(&mut self, cx: &mut Context<Self>) {
        self.git.update(cx, |git, cx| git.changed(cx));
    }
    /// `window.alert`.
    fn fail(error: String, window: &mut Window, cx: &mut App) {
        drop(window.prompt(PromptLevel::Critical, &error, None, &["OK"], cx));
    }
    /// confirmNative: Tauri's `ask` with the OK label it was given.
    fn confirm(
        message: String,
        ok: &'static str,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Task<bool> {
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &[
                gpui::PromptButton::ok(ok),
                gpui::PromptButton::cancel("Cancel"),
            ],
            cx,
        );
        cx.background_executor()
            .spawn(async move { matches!(answer.await, Ok(0)) })
    }
    /// Runs `work` against the checkout with `busy` held, then rereads.
    fn run_git(
        &mut self,
        busy: String,
        work: impl FnOnce(PathBuf) -> Result<(), String> + Send + 'static,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(busy);
        cx.notify();
        let path = PathBuf::from(&self.cwd);
        let task = cx.background_executor().spawn(async move { work(path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.mutated(cx);
                match result {
                    Ok(()) => then(this, window, cx),
                    Err(error) => Self::fail(error, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn run_file(
        &mut self,
        file: ChangedFile,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        let relative = file.relative.clone();
        let start = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.run_git(
                relative.clone(),
                move |path| match action {
                    FileAction::Stage => scm::stage_file(&path, &relative),
                    FileAction::Unstage => scm::unstage_file(&path, &relative),
                    FileAction::Discard => scm::discard_file(&path, &relative),
                },
                |_, _, _| {},
                window,
                cx,
            );
        };
        if action != FileAction::Discard {
            start(self, window, cx);
            return;
        }
        let name = basename(&file.relative).to_owned();
        let untracked = file.status == "untracked";
        let ask = Self::confirm(
            if untracked {
                format!("Delete untracked file {name}?")
            } else {
                format!("Discard changes in {name}? This cannot be undone.")
            },
            if untracked { "Delete" } else { "Discard" },
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if ask.await {
                let _ = this.update_in(cx, start);
            }
        })
        .detach();
    }
    fn run_all(&mut self, action: FileAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let key = match action {
            FileAction::Stage => "stage",
            FileAction::Unstage => "unstage",
            FileAction::Discard => "discard",
        };
        let start = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.run_git(
                key.into(),
                move |path| match action {
                    FileAction::Stage => scm::stage_all(&path),
                    FileAction::Unstage => scm::unstage_all(&path),
                    FileAction::Discard => scm::discard_all(&path),
                },
                |_, _, _| {},
                window,
                cx,
            );
        };
        if action != FileAction::Discard {
            start(self, window, cx);
            return;
        }
        let index = self.index(cx);
        let (_, unstaged) = Self::staged_unstaged(index.as_ref());
        let Some(index) = index.filter(|_| !unstaged.is_empty()) else {
            return;
        };
        let only = &index.files[unstaged[0]];
        let n = unstaged.len();
        let untracked_only = n == 1 && only.status == "untracked";
        let message = if untracked_only {
            format!("Delete untracked file {}?", basename(&only.relative))
        } else if n == 1 {
            format!(
                "Discard changes in {}? This cannot be undone.",
                basename(&only.relative)
            )
        } else {
            format!("Discard all unstaged changes in {n} files? This cannot be undone.")
        };
        let ask = Self::confirm(
            message,
            if untracked_only { "Delete" } else { "Discard" },
            window,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if ask.await {
                let _ = this.update_in(cx, start);
            }
        })
        .detach();
    }
    fn run_folder(
        &mut self,
        relative: String,
        stage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = format!("{}:{relative}", if stage { "stage" } else { "unstage" });
        self.run_git(
            key,
            move |path| {
                if stage {
                    scm::stage_file(&path, &relative)
                } else {
                    scm::unstage_file(&path, &relative)
                }
            },
            |_, _, _| {},
            window,
            cx,
        );
    }

    fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.index(cx);
        if !index.is_some_and(|i| i.remote.is_some() && i.upstream.is_some()) {
            return;
        }
        self.status = None;
        self.run_git(
            "pull".into(),
            |path| scm::pull(&path),
            |this, _, cx| this.set_status("Pull complete", cx),
            window,
            cx,
        );
    }
    fn cancel_generate(&mut self) {
        if let Some(cancel) = self.generate_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
            if self.busy.as_deref() == Some("generate") {
                self.busy = None;
            }
        }
    }
    /// pickTextHarness: the conversation's provider when installed, else
    /// Claude Code, then Codex.
    fn text_harness(&self) -> Option<(Kind, PathBuf)> {
        pick_text_harness(self.text_provider)
    }
    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_files = self.index(cx).is_some_and(|i| !i.files.is_empty());
        if !has_files || self.busy.is_some() || self.generate_cancel.is_some() {
            return;
        }
        let Some((kind, binary)) = self.text_harness() else {
            Self::fail(
                "No provider CLI is installed to write a commit message.".into(),
                window,
                cx,
            );
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.generate_cancel = Some(cancel.clone());
        self.busy = Some("generate".into());
        cx.notify();
        let cwd = PathBuf::from(&self.cwd);
        let flag = cancel.clone();
        let work = cx
            .background_executor()
            .spawn(async move { generate_commit_message(kind, binary, cwd, flag) });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                this.generate_cancel = None;
                this.busy = None;
                match result {
                    Ok(message) => this.message.update(cx, |m, cx| m.set_text(message, cx)),
                    Err(error) => Self::fail(error, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.amend.take().is_some() {
            cx.notify();
            return;
        }
        let path = PathBuf::from(&self.cwd);
        let work = cx
            .background_executor()
            .spawn(async move { scm::head_message(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(head) => {
                    if this.message.read(cx).content.trim().is_empty() {
                        this.message.update(cx, |m, cx| m.set_text(head, cx));
                    }
                    let index = this.index(cx);
                    this.amend = Some((
                        index.as_ref().and_then(|i| i.branch.clone()),
                        index.and_then(|i| i.head),
                    ));
                    cx.notify();
                }
                Err(error) => Self::fail(error, window, cx),
            });
        })
        .detach();
    }

    fn can_commit(&self, cx: &App) -> bool {
        let index = self.git.read(cx).index.as_ref();
        let staged = index.is_some_and(|i| i.files.iter().any(|f| f.staged));
        (staged || self.amend.is_some())
            && !self.message.read(cx).content.trim().is_empty()
            && self.busy.is_none()
    }
    fn on_default(index: Option<&DiffIndex>) -> bool {
        index.is_some_and(|i| i.branch.is_some() && i.branch == i.default_branch)
    }
    fn commit(&mut self, push: bool, create_pr: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_commit(cx) {
            return;
        }
        let index = self.index(cx);
        let mut questions = Vec::new();
        if (push || create_pr)
            && Self::on_default(index.as_ref())
            && let Some(branch) = index.as_ref().and_then(|i| i.branch.clone())
        {
            questions.push((
                if create_pr {
                    format!("Create a pull request from default branch \"{branch}\"?")
                } else {
                    format!("Push to default branch \"{branch}\"?")
                },
                "OK",
            ));
        }
        if self.amend.is_some() && index.as_ref().is_some_and(|i| i.head_pushed) {
            questions.push((
                "Amend a commit that is already pushed? MonoCode cannot push the result. You will need a force push from the terminal.".into(),
                "Amend",
            ));
        }
        self.menu = None;
        cx.spawn_in(window, async move |this, cx| {
            for (question, ok) in questions {
                let Ok(ask) =
                    this.update_in(cx, |_, window, cx| Self::confirm(question, ok, window, cx))
                else {
                    return;
                };
                if !ask.await {
                    return;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.commit_now(push, create_pr, window, cx)
            });
        })
        .detach();
    }
    fn commit_now(
        &mut self,
        push: bool,
        create_pr: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = self.message.read(cx).content.to_string();
        let amend = self.amend.is_some();
        let pr = create_pr.then(|| self.text_harness());
        self.run_git(
            if create_pr { "pr" } else { "commit" }.into(),
            move |path| {
                scm::commit(&path, &message, amend)?;
                if push || create_pr {
                    scm::push(&path)?;
                }
                Ok(())
            },
            move |this, window, cx| {
                this.amend = None;
                this.message
                    .update(cx, |m, cx| m.set_text(String::new(), cx));
                if let Some(harness) = pr {
                    this.open_created_pr(harness, window, cx);
                }
            },
            window,
            cx,
        );
    }
    /// generatePrContent, `gh pr create`, then the browser.
    fn open_created_pr(
        &mut self,
        harness: Option<(Kind, PathBuf)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = Some("pr".into());
        cx.notify();
        let cwd = PathBuf::from(&self.cwd);
        let work = cx
            .background_executor()
            .spawn(async move { create_pr_blocking(harness, &cwd) });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.mutated(cx);
                this.load_pr(cx);
                match result {
                    Ok(url) => cx.open_url(url.trim()),
                    Err(error) => Self::fail(error, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_git(
            "sync".into(),
            |path| scm::sync(&path),
            |this, _, cx| this.load_pr(cx),
            window,
            cx,
        );
    }
    fn create_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.index(cx);
        let ahead = index.as_ref().is_some_and(|i| i.ahead > 0);
        let question = Self::on_default(index.as_ref()).then(|| {
            format!(
                "Create a pull request from default branch \"{}\"?",
                index
                    .as_ref()
                    .and_then(|i| i.branch.clone())
                    .unwrap_or_default()
            )
        });
        let harness = self.text_harness();
        cx.spawn_in(window, async move |this, cx| {
            if let Some(question) = question {
                let Ok(ask) = this.update_in(cx, |_, window, cx| {
                    Self::confirm(question, "OK", window, cx)
                }) else {
                    return;
                };
                if !ask.await {
                    return;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if ahead {
                    this.run_git(
                        "pr".into(),
                        |path| scm::push(&path),
                        move |this, window, cx| this.open_created_pr(harness, window, cx),
                        window,
                        cx,
                    );
                } else {
                    this.open_created_pr(harness, window, cx);
                }
            });
        })
        .detach();
    }

    fn open_menu(&mut self, kind: MenuKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.as_ref().is_some_and(|m| m.kind == kind) {
            self.menu = None;
            cx.notify();
            return;
        }
        self.menu = None;
        let index = self.index(cx);
        let items: Vec<MenuItem> = match kind {
            MenuKind::Branch => {
                let can_pull = index
                    .as_ref()
                    .is_some_and(|i| i.remote.is_some() && i.upstream.is_some());
                let pulling = self.busy.as_deref() == Some("pull");
                vec![
                    MenuAction::new("pull", if pulling { "Pulling…" } else { "Pull" })
                        .icon(if pulling { "loader" } else { "refresh-cw" })
                        .disabled(self.busy.is_some() || !can_pull)
                        .into(),
                ]
            }
            MenuKind::Commit => {
                let (can_push, can_push_pr) = self.push_options(cx);
                vec![
                    MenuAction::new("push", "Commit & Push")
                        .disabled(!can_push)
                        .into(),
                    MenuAction::new("push-pr", "Commit, Push & Create PR")
                        .disabled(!can_push_pr)
                        .into(),
                    MenuItem::Separator,
                    MenuAction::new("amend", "Amend Last Commit")
                        .checked(self.amend.is_some())
                        .into(),
                ]
            }
        };
        let (label, width) = match kind {
            MenuKind::Branch => ("Branch actions", 144.),
            MenuKind::Commit => ("Commit options", 192.),
        };
        let menu = cx.new(|cx| {
            Menu::new(label, items, window, cx)
                .width(width)
                .style(MenuStyle::Git)
        });
        let events = cx.subscribe_in(
            &menu,
            window,
            move |this, _, event: &MenuEvent, window, cx| {
                this.menu = None;
                if let MenuEvent::Picked(id, _) = event {
                    match id.as_ref() {
                        "pull" => this.pull(window, cx),
                        "push" => this.commit(true, false, window, cx),
                        "push-pr" => this.commit(true, true, window, cx),
                        "amend" => this.toggle_amend(window, cx),
                        _ => {}
                    }
                }
                cx.notify();
            },
        );
        self.menu = Some(OpenMenu {
            kind,
            menu,
            _events: events,
        });
        cx.notify();
    }
    fn push_options(&self, cx: &App) -> (bool, bool) {
        let index = self.git.read(cx).index.as_ref();
        let has_remote = index.is_some_and(|i| i.remote.is_some());
        let diverged = index.is_some_and(|i| i.ahead > 0 && i.behind > 0);
        let head_pushed = index.is_some_and(|i| i.head_pushed);
        let can_push = self.can_commit(cx)
            && has_remote
            && !diverged
            && (self.amend.is_none() || !head_pushed);
        let open_pr = self.pr.as_ref().is_some_and(|p| p.state == "open");
        (can_push, can_push && !open_pr && !Self::on_default(index))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.git.read(cx).index.clone();
        let busy = self.busy.is_some();
        let menu_open = self
            .menu
            .as_ref()
            .is_some_and(|m| m.kind == MenuKind::Branch);
        let branch = index.as_ref().and_then(|i| i.branch.clone());
        let index = index.unwrap_or_default();
        pane_header("changes-header", "Changes")
            .when_some(branch.clone(), |d, branch| {
                d.child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(icon("git-branch-hi", 12.).text_color(content(0.5)))
                        .child(div().min_w_0().truncate().child(branch))
                        .when(index.ahead > 0, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .font_features(tabular())
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .child(format!("↑{}", index.ahead)),
                            )
                        })
                        .when(index.behind > 0, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .font_features(tabular())
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .child(format!("↓{}", index.behind)),
                            )
                        }),
                )
            })
            .when(branch.is_none(), |d| {
                d.child(div().flex_shrink_0().child("Changes"))
            })
            .when_some(self.status.clone(), |d, status| {
                d.child(
                    div()
                        .id("changes-status")
                        .role(Role::Status)
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(status),
                )
            })
            .child(div().ml_auto())
            .when(branch.is_some(), |d| {
                d.child(
                    div()
                        .id("branch-actions")
                        .role(Role::Button)
                        .aria_label("Branch actions")
                        .tip("Branch actions")
                        .aria_expanded(menu_open)
                        .group("branch-actions")
                        .relative()
                        .size(px(24.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .child(self.branch_anchor.probe())
                        .when(busy, |d| d.opacity(0.4))
                        .when(menu_open, |d| d.bg(content(0.1)))
                        .when(!busy && !menu_open, |d| d.hover(|s| s.bg(content(0.05))))
                        .child(if self.busy.as_deref() == Some("pull") {
                            spinner("pull-spin", "loader", 14.)
                        } else {
                            icon("more-horizontal", 16.)
                                .text_color(content(if menu_open { 1. } else { 0.5 }))
                                .group_hover("branch-actions", |s| s.text_color(content(1.)))
                                .into_any_element()
                        })
                        .when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, window, cx| {
                                this.open_menu(MenuKind::Branch, window, cx)
                            }))
                        }),
                )
            })
    }

    fn commit_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.git.read(cx).index.clone();
        let has_files = index.as_ref().is_some_and(|i| !i.files.is_empty());
        let generating = self.busy.as_deref() == Some("generate");
        let can_generate = has_files && self.busy.is_none();
        let can_commit = self.can_commit(cx);
        let amend = self.amend.is_some();
        let can_open_menu =
            index.as_ref().is_some_and(|i| i.branch.is_some()) && self.busy.is_none();
        let menu_open = self
            .menu
            .as_ref()
            .is_some_and(|m| m.kind == MenuKind::Commit);
        let placeholder = if amend {
            format!("Amend message ({MOD}↩ to amend)")
        } else {
            format!("Message ({MOD}↩ to commit)")
        };
        self.message
            .update(cx, |m, cx| m.set_placeholder(placeholder, cx));
        // `bg-content` buttons with `text-background-base`, 40% when unavailable.
        let solid: Hsla = content(1.);
        let face = if can_commit { solid } else { content(0.4) };
        let ink = crate::theme::background_base();
        let generate_label = if generating {
            "Cancel commit message generation"
        } else {
            "Generate commit message"
        };
        div()
            .flex_shrink_0()
            .border_b_1()
            .border_color(stroke())
            .p_2()
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .w_full()
                            .rounded_md()
                            .bg(content(0.1))
                            .py_1()
                            .pl_2()
                            .pr_8()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(content(1.))
                            .when(self.busy.is_some(), |d| d.opacity(0.4))
                            .child(self.message.clone()),
                    )
                    .child(
                        div()
                            .id("generate-commit-message")
                            .role(Role::Button)
                            .aria_label(generate_label)
                            .tip(generate_label)
                            .group("generate")
                            .absolute()
                            .top_1()
                            .right_1()
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(content(0.1))
                            .when(!generating && !can_generate, |d| d.opacity(0.4))
                            .when(generating || can_generate, |d| {
                                d.hover(|s| s.bg(content(0.2)))
                            })
                            .child(if generating {
                                div()
                                    .relative()
                                    .size(px(14.))
                                    .child(
                                        div()
                                            .absolute()
                                            .group_hover("generate", |s| s.opacity(0.))
                                            .child(spinner("generate-spin", "loader", 14.)),
                                    )
                                    .child(
                                        icon("x", 14.)
                                            .absolute()
                                            .text_color(content(1.))
                                            .opacity(0.)
                                            .group_hover("generate", |s| s.opacity(1.)),
                                    )
                                    .into_any_element()
                            } else {
                                icon("wand", 12.).text_color(content(1.)).into_any_element()
                            })
                            .when(generating || can_generate, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    if generating {
                                        this.cancel_generate();
                                        cx.notify();
                                    } else {
                                        this.generate(window, cx);
                                    }
                                }))
                            }),
                    ),
            )
            .child(
                div()
                    .relative()
                    .mt(px(6.))
                    .flex()
                    .child(self.commit_anchor.probe())
                    .child(
                        div()
                            .id("commit")
                            .role(Role::Button)
                            .aria_label(if amend { "Amend Commit" } else { "Commit" })
                            .h(px(28.))
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .rounded_l_md()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .bg(face)
                            .text_color(ink)
                            .child(icon("check-2", 14.).text_color(ink))
                            .child(if amend { "Amend Commit" } else { "Commit" })
                            .when(can_commit, |d| {
                                d.on_click(cx.listener(|this, _, window, cx| {
                                    this.commit(false, false, window, cx)
                                }))
                            }),
                    )
                    .child(
                        div()
                            .id("commit-options")
                            .role(Role::Button)
                            .aria_label("Commit options")
                            .tip("Commit options")
                            .aria_expanded(menu_open)
                            .size(px(28.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r_md()
                            .border_l_1()
                            .border_color(ink.opacity(0.1))
                            .bg(if menu_open { solid } else { face })
                            .when(can_open_menu && !menu_open, |d| {
                                d.hover(move |s| {
                                    s.bg(if can_commit { content(0.8) } else { solid })
                                })
                            })
                            .child(icon("chevron-down-2", 14.).text_color(ink))
                            .when(can_open_menu, |d| {
                                d.on_click(cx.listener(|this, _, window, cx| {
                                    this.open_menu(MenuKind::Commit, window, cx)
                                }))
                            }),
                    ),
            )
            .when_some(index, |d, index| d.child(self.sync_actions(&index, cx)))
    }

    fn sync_actions(&self, index: &DiffIndex, cx: &mut Context<Self>) -> AnyElement {
        if index.remote.is_none() {
            return div().into_any_element();
        }
        let busy = self.busy.is_some();
        let (ahead, behind) = (index.ahead, index.behind);
        let has_open_pr = self.pr.as_ref().is_some_and(|p| p.state == "open");
        let on_default = Self::on_default(Some(index));
        let diverged = ahead > 0 && behind > 0;
        let files = index.files.len();
        let can_publish = index.upstream.is_none();
        let can_sync = index.upstream.is_some() && (ahead > 0 || behind > 0);
        let can_create_pr = index.branch.is_some()
            && index.default_branch.is_some()
            && !has_open_pr
            && !on_default
            && !diverged
            && files == 0
            && index.ahead_of_default > 0
            && behind == 0;
        let can_view_pr = has_open_pr && self.pr.as_ref().is_some_and(|p| !p.url.is_empty());
        let show_create = !has_open_pr && !on_default;
        if !can_publish && !can_sync && !show_create && !has_open_pr {
            return div().into_any_element();
        }
        let dest = index.upstream.clone().unwrap_or_else(|| {
            format!(
                "{}/{}",
                index.remote.as_deref().unwrap_or("origin"),
                index.branch.as_deref().unwrap_or("HEAD")
            )
        });
        let syncing = self.busy.as_deref() == Some("sync");
        let plural = |n: i64| if n == 1 { "" } else { "s" };
        let sync_title = if syncing {
            "Synchronizing Changes...".to_owned()
        } else if can_publish {
            index.branch.as_ref().map_or("Publish Branch".into(), |b| {
                format!("Publish Branch \"{b}\"")
            })
        } else if behind > 0 && ahead > 0 {
            format!("Pull {behind} and push {ahead} commits between {dest}")
        } else if behind > 0 {
            format!("Pull {behind} commit{} from {dest}", plural(behind))
        } else {
            format!("Push {ahead} commit{} to {dest}", plural(ahead))
        };
        let button = |id: &'static str, title: String, enabled: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(SharedString::from(title))
                .h(px(28.))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .px_2()
                .rounded_md()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .bg(content(0.1))
                .text_color(content(1.))
                .when(!enabled, |d| d.opacity(0.4))
                .when(enabled, |d| d.hover(|s| s.bg(content(0.15))))
        };
        let glyph = |path: &'static str| icon(path, 14.).text_color(content(1.)).into_any_element();
        div()
            .mt(px(6.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(can_publish, |d| {
                d.child(
                    button("publish-branch", sync_title.clone(), !busy)
                        .child(if syncing {
                            spinner("sync-spin", "loader", 14.)
                        } else {
                            glyph("cloud-upload")
                        })
                        .child(div().min_w_0().truncate().child("Publish Branch"))
                        .when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)))
                        }),
                )
            })
            .when(!can_publish && can_sync, |d| {
                d.child(
                    button("sync-changes", sync_title.clone(), !busy)
                        .child(if syncing {
                            spinner("sync-spin", "refresh-cw", 14.)
                        } else {
                            glyph("refresh-cw")
                        })
                        .child(div().min_w_0().truncate().child("Sync Changes"))
                        .when(behind > 0, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .font_features(tabular())
                                    .text_color(content(0.55))
                                    .child(format!("↓{behind}")),
                            )
                        })
                        .when(ahead > 0, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .font_features(tabular())
                                    .text_color(content(0.55))
                                    .child(format!("↑{ahead}")),
                            )
                        })
                        .when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)))
                        }),
                )
            })
            .when(show_create, |d| {
                let title = index
                    .default_branch
                    .as_ref()
                    .map_or("Create pull request".to_owned(), |b| {
                        format!("Create a pull request into {b}")
                    });
                let enabled = can_create_pr && !busy;
                d.child(
                    button("create-pr", title, enabled)
                        .child(if self.busy.as_deref() == Some("pr") {
                            spinner("pr-spin", "loader", 14.)
                        } else {
                            glyph("git-pull-request")
                        })
                        .child("Create PR")
                        .when(enabled, |d| {
                            d.on_click(
                                cx.listener(|this, _, window, cx| this.create_pr(window, cx)),
                            )
                        }),
                )
            })
            .when(has_open_pr, |d| {
                let pr = self.pr.clone().unwrap_or(Pr {
                    number: 0,
                    title: String::new(),
                    url: String::new(),
                    state: String::new(),
                });
                let title = if pr.title.is_empty() {
                    "View pull request".to_owned()
                } else {
                    format!("View PR #{}: {}", pr.number, pr.title)
                };
                let enabled = can_view_pr && !busy;
                let url = pr.url.clone();
                d.child(
                    button("view-pr", title, enabled)
                        .child(glyph("external-link"))
                        .child(div().min_w_0().truncate().child(if pr.number > 0 {
                            format!("View PR #{}", pr.number)
                        } else {
                            "View PR".into()
                        }))
                        .when(enabled, |d| d.on_click(move |_, _, cx| cx.open_url(&url))),
                )
            })
            .into_any_element()
    }

    fn hover_line(&mut self, ix: usize, hovering: bool, cx: &mut Context<Self>) {
        let next = if hovering {
            Some(ix)
        } else if self.hovered == Some(ix) {
            None
        } else {
            self.hovered
        };
        if next != self.hovered {
            self.hovered = next;
            cx.notify();
        }
    }
    fn line(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.git.read(cx).index.clone();
        let files = index.as_ref().map_or(&[][..], |i| &i.files[..]);
        let busy = self.busy.is_some();
        match self.lines[ix].clone() {
            Line::Section(kind) => {
                let (title, open) = match kind {
                    Kind2::Staged => ("Staged Changes", self.staged_open),
                    Kind2::Unstaged => ("Changes", self.changes_open),
                };
                let count = files
                    .iter()
                    .filter(|f| match kind {
                        Kind2::Staged => f.staged,
                        Kind2::Unstaged => f.unstaged,
                    })
                    .count();
                let tree = self.view == ChangesView::Tree;
                let actions: Vec<(&'static str, &'static str, &'static str)> = match kind {
                    Kind2::Staged => vec![
                        ("open-all-staged", "Open All Changes", "file-diff"),
                        ("unstage-all", "Unstage All Changes", "minus-sign"),
                    ],
                    Kind2::Unstaged => vec![
                        ("open-all-unstaged", "Open All Changes", "file-diff"),
                        ("discard-all", "Discard All Changes", "undo"),
                        ("stage-all", "Stage All Changes", "plus"),
                    ],
                };
                div()
                    .h(px(ROW))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px(px(6.))
                    .child(
                        div()
                            .id(("section", ix))
                            .role(Role::Button)
                            .aria_label(title)
                            .aria_expanded(open)
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                icon(
                                    if open {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    14.,
                                )
                                .text_color(content(0.5)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(0.55))
                                    .child(title.to_uppercase()),
                            )
                            .child(
                                div()
                                    .ml_1()
                                    .h_4()
                                    .min_w_4()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(accent_color().opacity(0.8))
                                    .px_1()
                                    .text_size(px(8.))
                                    .text_color(white(1.))
                                    .child(count.to_string()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match kind {
                                    Kind2::Staged => this.staged_open = !this.staged_open,
                                    Kind2::Unstaged => this.changes_open = !this.changes_open,
                                }
                                this.rebuild(cx);
                            })),
                    )
                    .child(
                        icon_action(
                            ("view", ix),
                            if tree { "View as List" } else { "View as Tree" },
                            if tree { "list-bullet" } else { "folder-tree" },
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.view = match this.view {
                                ChangesView::List => ChangesView::Tree,
                                ChangesView::Tree => ChangesView::List,
                            };
                            cx.emit(ChangesEvent::View(this.view));
                            this.rebuild(cx);
                        })),
                    )
                    .children(actions.into_iter().map(|(id, label, path)| {
                        icon_action(id, label, path, true)
                            .on_click(cx.listener(move |this, _, window, cx| match id {
                                "unstage-all" => this.run_all(FileAction::Unstage, window, cx),
                                "discard-all" => this.run_all(FileAction::Discard, window, cx),
                                "stage-all" => this.run_all(FileAction::Stage, window, cx),
                                _ => cx.emit(ChangesEvent::OpenDiff {
                                    cwd: this.cwd.clone(),
                                    path: None,
                                    staged: kind == Kind2::Staged,
                                }),
                            }))
                            .into_any_element()
                    }))
                    .into_any_element()
            }
            Line::Dir {
                kind,
                path,
                name,
                depth,
                status,
                open,
            } => {
                let key = format!("{}:{path}", kind.key());
                let stage = kind == Kind2::Unstaged;
                let folder = path.clone();
                div()
                    .id(("change-dir", ix))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        this.hover_line(ix, *hovering, cx)
                    }))
                    .h(px(ROW))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl(px(8. + depth as f32 * 12.))
                    .pr_2()
                    .text_color(content(1.))
                    .hover(|s| s.bg(content(0.05)))
                    .child(
                        div()
                            .id(("change-dir-toggle", ix))
                            .role(Role::TreeItem)
                            .aria_label(SharedString::from(path.clone()))
                            .aria_expanded(open)
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .size_4()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        icon(
                                            if open {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            },
                                            14.,
                                        )
                                        .text_color(content(0.5)),
                                    ),
                            )
                            .child(file_type_icon(&name, true, open, 16.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed.remove(&key) {
                                    this.collapsed.insert(key.clone());
                                }
                                this.rebuild(cx);
                            })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .map(|d| {
                                if self.hovered == Some(ix) {
                                    d.flex()
                                } else {
                                    d.hidden()
                                }
                            })
                            .items_center()
                            .child(
                                icon_action(
                                    ("dir-action", ix),
                                    format!(
                                        "{} Changes in {path}",
                                        if stage { "Stage" } else { "Unstage" }
                                    ),
                                    if stage { "plus" } else { "minus-sign" },
                                    !busy,
                                )
                                .when(!busy, |d| {
                                    d.on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_folder(folder.clone(), stage, window, cx)
                                    }))
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(status.as_deref().map_or(content(0.4), status_color)),
                            ),
                    )
                    .into_any_element()
            }
            Line::File { kind, file, depth } => {
                let Some(file) = files.get(file).cloned() else {
                    return div().h(px(ROW)).into_any_element();
                };
                let active = self
                    .selected
                    .as_ref()
                    .is_some_and(|(rel, k)| rel == &file.relative && *k == kind);
                let name = basename(&file.relative).to_owned();
                let dir = if depth.is_some() {
                    ""
                } else {
                    dirname(&file.relative)
                };
                let relative = file.relative.clone();
                let open_path = file.path.clone();
                let can_open = file.status != "deleted";
                let row_file = file.clone();
                let stage_file = file.clone();
                div()
                    .id(("change-row", ix))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        this.hover_line(ix, *hovering, cx)
                    }))
                    .h(px(ROW))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pr_2()
                    .map(|d| match depth {
                        Some(depth) => d.pl(px(8. + depth as f32 * 12.)),
                        None => d.pl_2(),
                    })
                    .text_color(content(1.))
                    .when(active, |d| d.bg(selection()))
                    .when(!active, |d| d.hover(|s| s.bg(content(0.05))))
                    .child(
                        div()
                            .id(("change-open", ix))
                            .role(Role::Button)
                            .aria_label(SharedString::from(file.relative.clone()))
                            .h_full()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(depth.is_some(), |d| d.child(div().size_4().flex_shrink_0()))
                            .child(file_type_icon(&name, false, false, 16.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .items_baseline()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(name),
                                    )
                                    .when(!dir.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .ml(px(6.))
                                                .min_w_0()
                                                .truncate()
                                                .text_size(px(11.))
                                                .text_color(content(0.4))
                                                .child(dir.to_owned()),
                                        )
                                    }),
                            )
                            .when(can_open, |d| {
                                d.on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected = Some((relative.clone(), kind));
                                    cx.emit(ChangesEvent::OpenDiff {
                                        cwd: this.cwd.clone(),
                                        path: Some(open_path.clone()),
                                        staged: kind == Kind2::Staged,
                                    });
                                    cx.notify();
                                }))
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .items_center()
                            .map(|d| {
                                if active || self.hovered == Some(ix) {
                                    d.flex()
                                } else {
                                    d.hidden()
                                }
                            })
                            .when(kind == Kind2::Unstaged, |d| {
                                d.child(
                                    icon_action(("discard", ix), "Discard Changes", "undo", !busy)
                                        .when(!busy, |d| {
                                            d.on_click(cx.listener(move |this, _, window, cx| {
                                                this.run_file(
                                                    row_file.clone(),
                                                    FileAction::Discard,
                                                    window,
                                                    cx,
                                                )
                                            }))
                                        }),
                                )
                            })
                            .child(
                                icon_action(
                                    ("stage", ix),
                                    if kind == Kind2::Staged {
                                        "Unstage Changes"
                                    } else {
                                        "Stage Changes"
                                    },
                                    if kind == Kind2::Staged {
                                        "minus-sign"
                                    } else {
                                        "plus"
                                    },
                                    !busy,
                                )
                                .when(!busy, |d| {
                                    d.on_click(cx.listener(move |this, _, window, cx| {
                                        let action = if kind == Kind2::Staged {
                                            FileAction::Unstage
                                        } else {
                                            FileAction::Stage
                                        };
                                        this.run_file(stage_file.clone(), action, window, cx)
                                    }))
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .font_family(crate::fonts::mono())
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color(&file.status))
                            .child(status_letter(&file.status)),
                    )
                    .into_any_element()
            }
        }
    }
}

/// pickTextHarness: `preferred` when installed, else Claude Code, then Codex.
pub(crate) fn pick_text_harness(preferred: Kind) -> Option<(Kind, PathBuf)> {
    [preferred, Kind::Claude, Kind::Codex]
        .into_iter()
        .find_map(|kind| provider::resolve(kind).map(|binary| (kind, binary)))
}

/// generateCommitMessage: a message for the staged changes, written by a
/// read-only turn of `kind`. Blocking.
pub(crate) fn generate_commit_message(
    kind: Kind,
    binary: PathBuf,
    cwd: PathBuf,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let context = scm::staged_context(&cwd)?;
    let prompt =
        scm::commit_message_prompt(context.branch.as_deref(), &context.summary, &context.patch);
    let output = text_prompt(kind, binary, cwd, prompt, cancel)?;
    scm::parse_commit_message(&output).ok_or_else(|| {
        let snippet: String = output
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect();
        if snippet.is_empty() {
            format!(
                "Could not generate a commit message. {} returned no text.",
                kind.label()
            )
        } else {
            format!("Could not generate a commit message. Model replied: {snippet}")
        }
    })
}

/// generatePrContent, falling back to the commits, then `gh pr create`; the
/// new pull request's URL. Blocking.
pub(crate) fn create_pr_blocking(
    harness: Option<(Kind, PathBuf)>,
    cwd: &std::path::Path,
) -> Result<String, String> {
    let range = scm::range_context(cwd)?;
    let parsed = harness.and_then(|(kind, binary)| {
        let output = text_prompt(
            kind,
            binary,
            cwd.to_path_buf(),
            scm::pr_content_prompt(&range),
            Arc::new(AtomicBool::new(false)),
        )
        .ok()?;
        scm::parse_pr_content(&output)
    });
    let first = range.commit_summary.lines().next().unwrap_or("").trim();
    let title = parsed
        .as_ref()
        .map(|(t, _)| t.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| (!first.is_empty()).then(|| first.to_owned()))
        .unwrap_or_else(|| format!("Update {}", range.head));
    let body = parsed
        .map(|(_, b)| b)
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| range.commit_summary.trim().to_owned());
    scm::pr_create(cwd, &title, &body, &range.base, &range.head)
}

/// A read-only one-shot turn, as the React text harness runs: the final
/// assistant message's text.
fn text_prompt(
    kind: Kind,
    binary: PathBuf,
    cwd: PathBuf,
    prompt: String,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let request = Request {
        kind,
        binary,
        cwd,
        prompt,
        provider_id: None,
        model: None,
        settings: Settings {
            access: AccessMode::ReadOnly,
            ..Default::default()
        },
    };
    let mut text = String::new();
    provider::run(request, cancel.clone(), |events| {
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".into());
        }
        for event in events {
            match event {
                Event::BeginAssistant => text.clear(),
                Event::Text(chunk) => text.push_str(&chunk),
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(text)
}

impl Render for Changes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let available = !self.cwd.is_empty() && self.cwd != "~";
        if !available {
            return div()
                .px_3()
                .py_2()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child("No project folder")
                .into_any_element();
        }
        let index = self.git.read(cx).index.clone();
        let empty = match &index {
            None => Some("Loading changes…".to_owned()),
            Some(i) if i.files.is_empty() => Some(if i.ahead > 0 || i.behind > 0 {
                sync_status_label(i)
            } else {
                "No uncommitted changes".into()
            }),
            _ => None,
        };
        let count = self.lines.len();
        let entity = cx.entity();
        let menu = self.menu.as_ref().map(|open| {
            let anchor = match open.kind {
                MenuKind::Branch => self.branch_anchor.clone(),
                MenuKind::Commit => self.commit_anchor.clone(),
            };
            menu_popover(&open.menu, Anchor::Slot(anchor), cx).align(Align::End)
        });
        div()
            .id("changes")
            .role(Role::TabPanel)
            .aria_label("Changes")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.header(cx))
            .child(self.commit_box(cx))
            .map(|d| match empty {
                Some(text) => d.child(
                    div().py_1().child(
                        div()
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(content(0.45))
                            .child(text),
                    ),
                ),
                None => d.child(
                    uniform_list("change-lines", count, move |range, _, cx| {
                        entity.update(cx, |this, cx| {
                            range.map(|ix| this.line(ix, cx)).collect::<Vec<_>>()
                        })
                    })
                    .py_1()
                    .flex_1()
                    .min_h_0(),
                ),
            })
            .children(menu)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn enabling_an_enabled_changes_panel_does_not_reload(cx: &mut gpui::TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                let git = cx.new(|cx| ProjectGit::new("~".into(), cx));
                cx.new(|cx| Changes::new(git, window, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |changes, _, cx| {
                changes.set_enabled(true, cx);
                let request = changes.pr_request;
                changes.set_enabled(true, cx);
                assert_eq!(request, changes.pr_request);
                changes.set_enabled(false, cx);
                changes.set_enabled(false, cx);
                assert_eq!(request, changes.pr_request);
            })
            .unwrap();
    }
    fn f(relative: &str, status: &str) -> ChangedFile {
        ChangedFile {
            path: format!("/r/{relative}"),
            relative: relative.into(),
            status: status.into(),
            additions: 0,
            deletions: 0,
            staged: false,
            unstaged: true,
        }
    }
    #[test]
    fn tree_nests_sorts_and_rolls_status_up() {
        let files = vec![
            f("src/b.rs", "modified"),
            f("src/a.rs", "modified"),
            f("README.md", "added"),
            f("src/ui/x.rs", "untracked"),
        ];
        let tree = build_tree(&files, &[0, 1, 2, 3]);
        assert_eq!(tree.files, [2]);
        let src = &tree.dirs[0];
        assert_eq!(src.name, "src");
        assert_eq!(src.files, [1, 0]);
        assert_eq!(src.status, None);
        assert_eq!(src.dirs[0].status.as_deref(), Some("untracked"));
        assert_eq!(status_letter("untracked"), "U");
        assert_eq!(dirname("a/b/c.rs"), "a/b");
        assert_eq!(dirname("c.rs"), "");
    }
}
