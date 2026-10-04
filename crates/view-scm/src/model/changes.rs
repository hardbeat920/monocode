//! The pure parts of src/features/source-control/ui/GitChangesPanel.tsx:
//! which actions are available, their labels and confirmations, the
//! folder tree for tree view, and the pull request text for remote
//! projects.

use std::cmp::Ordering;

use crate::git::{GitChangedFile, GitDiffIndex, GitFileDiffKind, GitPr, GitRangeContext};
use crate::hooks::PrContent;
use crate::paths::{basename, is_remote_project_path};

/// What the panel is running. The React view kept one string for all of
/// these, so no two git mutations ever run against one checkout at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Busy {
    Pull,
    /// A per-file action, keyed by `relative`.
    File(String),
    /// `runAll`.
    All(FileAction),
    Generate,
    Commit,
    Pr,
    Sync,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    Stage,
    Unstage,
    Discard,
}

/// `AmendTarget`: the commit an amend started on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmendTarget {
    pub branch: Option<String>,
    pub head: Option<String>,
}

/// Every availability flag `ChangedFiles` computes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangesFlags {
    pub has_remote: bool,
    pub has_open_pr: bool,
    pub diverged: bool,
    pub on_default: bool,
    pub can_generate: bool,
    pub can_commit: bool,
    pub can_create_pr: bool,
    pub can_view_pr: bool,
    pub can_publish: bool,
    pub can_sync: bool,
    pub can_commit_push: bool,
    pub can_commit_push_pr: bool,
    pub can_edit_message: bool,
    pub can_open_menu: bool,
}

pub struct FlagInputs<'a> {
    pub cwd: &'a str,
    pub index: Option<&'a GitDiffIndex>,
    pub pr: Option<&'a GitPr>,
    pub busy: bool,
    pub message: &'a str,
    pub amend: bool,
}

impl ChangesFlags {
    pub fn compute(input: &FlagInputs) -> Self {
        let index = input.index;
        let files = index.map(|index| index.files.as_slice()).unwrap_or(&[]);
        let staged = files.iter().filter(|file| file.staged).count();
        let ahead = index.map_or(0, |index| index.ahead);
        let behind = index.map_or(0, |index| index.behind);
        let has_remote = index.is_some_and(|index| index.remote.is_some());
        let has_open_pr = input.pr.is_some_and(|pr| pr.state == "open");
        let diverged = ahead > 0 && behind > 0;
        let on_default = index.is_some_and(|index| {
            index.branch.is_some()
                && index.default_branch.is_some()
                && index.branch == index.default_branch
        });
        let busy = input.busy;
        let amend = input.amend;
        let can_generate = !files.is_empty() && !busy && !is_remote_project_path(input.cwd);
        let can_commit = (staged > 0 || amend) && !input.message.trim().is_empty() && !busy;
        let can_create_pr = has_remote
            && index.is_some_and(|index| index.branch.is_some() && index.default_branch.is_some())
            && !has_open_pr
            && !on_default
            && !diverged
            && files.is_empty()
            && index.map_or(0, |index| index.ahead_of_default) > 0
            && behind == 0;
        let can_view_pr = has_open_pr && input.pr.is_some_and(|pr| !pr.url.is_empty());
        let can_publish = has_remote && index.is_some_and(|index| index.upstream.is_none());
        let can_sync = has_remote
            && index.is_some_and(|index| index.upstream.is_some())
            && (ahead > 0 || behind > 0);
        let can_commit_push = can_commit
            && has_remote
            && !diverged
            && (!amend || !index.is_some_and(|index| index.head_pushed));
        let can_commit_push_pr = can_commit_push && !has_open_pr && !on_default;
        let can_edit_message = (staged > 0 || amend) && !busy;
        let can_open_menu = index.is_some_and(|index| index.branch.is_some()) && !busy;
        Self {
            has_remote,
            has_open_pr,
            diverged,
            on_default,
            can_generate,
            can_commit,
            can_create_pr,
            can_view_pr,
            can_publish,
            can_sync,
            can_commit_push,
            can_commit_push_pr,
            can_edit_message,
            can_open_menu,
        }
    }
}

/// `canPull`: the branch menu's Pull needs a remote and an upstream.
pub fn can_pull(index: Option<&GitDiffIndex>) -> bool {
    index.is_some_and(|index| index.remote.is_some() && index.upstream.is_some())
}

fn plural(n: i64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `syncStatusLabel`: the empty list's text when only commits differ.
pub fn sync_status_label(index: &GitDiffIndex) -> String {
    if index.ahead > 0 && index.behind > 0 {
        return format!(
            "Diverged from {}",
            index.upstream.as_deref().unwrap_or("upstream")
        );
    }
    if index.ahead > 0 {
        let n = index.ahead;
        return format!("{n} unpushed commit{}", plural(n));
    }
    if index.behind > 0 {
        let n = index.behind;
        return format!("{n} incoming commit{}", plural(n));
    }
    "No files".into()
}

/// The empty list's text.
pub fn empty_list_label(index: Option<&GitDiffIndex>) -> String {
    match index {
        Some(index) if index.ahead > 0 || index.behind > 0 => sync_status_label(index),
        Some(_) => "No uncommitted changes".into(),
        None => "Loading changes…".into(),
    }
}

/// `GitSyncActions`' sync or publish button tooltip.
pub fn sync_title(index: &GitDiffIndex, syncing: bool, can_publish: bool) -> String {
    let ahead = index.ahead;
    let behind = index.behind;
    let dest = index.upstream.clone().unwrap_or_else(|| {
        format!(
            "{}/{}",
            index.remote.as_deref().unwrap_or("origin"),
            index.branch.as_deref().unwrap_or("HEAD")
        )
    });
    if syncing {
        "Synchronizing Changes...".into()
    } else if can_publish {
        match &index.branch {
            Some(branch) => format!("Publish Branch \"{branch}\""),
            None => "Publish Branch".into(),
        }
    } else if behind > 0 && ahead > 0 {
        format!("Pull {behind} and push {ahead} commits between {dest}")
    } else if behind > 0 {
        format!("Pull {behind} commit{} from {dest}", plural(behind))
    } else {
        format!("Push {ahead} commit{} to {dest}", plural(ahead))
    }
}

pub fn create_pr_title(index: &GitDiffIndex) -> String {
    match &index.default_branch {
        Some(base) => format!("Create a pull request into {base}"),
        None => "Create pull request".into(),
    }
}

pub fn view_pr_title(pr: Option<&GitPr>) -> String {
    match pr {
        Some(pr) if !pr.title.is_empty() => format!("View PR #{}: {}", pr.number, pr.title),
        _ => "View pull request".into(),
    }
}

pub fn view_pr_label(pr: Option<&GitPr>) -> String {
    match pr {
        Some(pr) if pr.number != 0 => format!("View PR #{}", pr.number),
        _ => "View PR".into(),
    }
}

/// Which buttons `GitSyncActions` shows. `None` when it renders nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncButtons {
    pub publish: bool,
    pub sync: bool,
    pub create_pr: bool,
    pub view_pr: bool,
}

pub fn sync_buttons(flags: &ChangesFlags) -> Option<SyncButtons> {
    if !flags.has_remote {
        return None;
    }
    let show_create_pr = !flags.has_open_pr && !flags.on_default;
    let show_view_pr = flags.has_open_pr;
    if !flags.can_publish && !flags.can_sync && !show_create_pr && !show_view_pr {
        return None;
    }
    Some(SyncButtons {
        publish: flags.can_publish,
        sync: !flags.can_publish && flags.can_sync,
        create_pr: show_create_pr,
        view_pr: show_view_pr,
    })
}

/// The confirmation for discarding one file, and its button label.
pub fn discard_file_prompt(file: &GitChangedFile) -> (String, &'static str) {
    let name = basename(&file.relative);
    if file.status == "untracked" {
        (format!("Delete untracked file {name}?"), "Delete")
    } else {
        (
            format!("Discard changes in {name}? This cannot be undone."),
            "Discard",
        )
    }
}

/// The confirmation for "Discard All Changes", or `None` when there is
/// nothing to discard.
pub fn discard_all_prompt(unstaged: &[GitChangedFile]) -> Option<(String, &'static str)> {
    let n = unstaged.len();
    let only = unstaged.first()?;
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
    Some((message, if untracked_only { "Delete" } else { "Discard" }))
}

/// `confirmDefault`'s question, or `None` when the branch is not the
/// default branch.
pub fn confirm_default_message(
    index: Option<&GitDiffIndex>,
    on_default: bool,
    pr: bool,
) -> Option<String> {
    let branch = index.and_then(|index| index.branch.as_deref())?;
    if !on_default {
        return None;
    }
    Some(if pr {
        format!("Create a pull request from default branch \"{branch}\"?")
    } else {
        format!("Push to default branch \"{branch}\"?")
    })
}

pub const CONFIRM_AMEND_PUSHED: &str = "Amend a commit that is already pushed? MonoCode cannot push the result. You will need a force push from the terminal.";

/// `remotePrContent`: pull request text from the host's git range, for a
/// project on a connected machine where no local agent can write it.
pub fn remote_pr_content(range: &GitRangeContext) -> PrContent {
    let commits = range.commit_summary.trim();
    let first_commit = commits
        .split('\n')
        .next()
        .map(|line| line.trim_end_matches('\r'))
        .map(strip_sha)
        .map(str::trim)
        .unwrap_or("");
    let title = if first_commit.is_empty() {
        format!("Changes on {}", range.head)
    } else {
        first_commit.to_string()
    };
    let diff_summary = range.diff_summary.trim();
    let mut parts = Vec::new();
    if !commits.is_empty() {
        parts.push(format!("## Commits\n\n{commits}"));
    }
    if !diff_summary.is_empty() {
        parts.push(format!("## Changes\n\n{diff_summary}"));
    }
    let body = parts.join("\n\n");
    PrContent {
        body: if body.is_empty() { title.clone() } else { body },
        title,
        base: range.base.clone(),
        head: range.head.clone(),
    }
}

/// `replace(/^[0-9a-f]+\s+/i, "")`.
fn strip_sha(line: &str) -> &str {
    let hex = line
        .char_indices()
        .find(|(_, c)| !c.is_ascii_hexdigit())
        .map_or(line.len(), |(i, _)| i);
    if hex == 0 {
        return line;
    }
    let rest = &line[hex..];
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        // No whitespace after the hex run, so the pattern did not match.
        return line;
    }
    trimmed
}

/// The PR number in a `.../pull/<n>` URL.
pub fn pr_number_from_url(url: &str) -> Option<i64> {
    let start = url.find("/pull/")? + "/pull/".len();
    let rest = &url[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let tail = &rest[end..];
    if !(tail.is_empty() || tail.starts_with(['/', '?', '#'])) {
        return None;
    }
    rest[..end].parse().ok().filter(|n: &i64| *n > 0)
}

/// `ChangeDir`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeDir {
    pub name: String,
    /// Path relative to the repo root; empty for the implicit root.
    pub path: String,
    pub dirs: Vec<ChangeDir>,
    pub files: Vec<GitChangedFile>,
    /// Status shared by every descendant, or `None` when they differ.
    pub status: Option<String>,
}

/// `buildChangeTree`: nests changed files under their directories, as VS
/// Code's tree view does.
pub fn build_change_tree(files: &[GitChangedFile]) -> ChangeDir {
    let mut root = ChangeDir::default();
    for file in files {
        let segments: Vec<&str> = file.relative.split('/').collect();
        let mut node = &mut root;
        for segment in &segments[..segments.len() - 1] {
            let path = if node.path.is_empty() {
                segment.to_string()
            } else {
                format!("{}/{segment}", node.path)
            };
            let index = match node.dirs.iter().position(|dir| dir.path == path) {
                Some(index) => index,
                None => {
                    node.dirs.push(ChangeDir {
                        name: segment.to_string(),
                        path,
                        ..Default::default()
                    });
                    node.dirs.len() - 1
                }
            };
            node = &mut node.dirs[index];
        }
        node.files.push(file.clone());
    }
    sort_change_dir(&mut root);
    root
}

/// `sortChangeDir`: sorts each level and rolls descendant status upward.
fn sort_change_dir(dir: &mut ChangeDir) -> Option<String> {
    dir.dirs.sort_by(|a, b| locale_compare(&a.name, &b.name));
    dir.files
        .sort_by(|a, b| locale_compare(&basename(&a.relative), &basename(&b.relative)));
    let mut status: Option<String> = None;
    let mut mixed = false;
    let mut merge = |next: Option<String>| match next {
        None => mixed = true,
        Some(next) => match &status {
            None => status = Some(next),
            Some(current) if *current != next => mixed = true,
            Some(_) => {}
        },
    };
    for child in &mut dir.dirs {
        merge(sort_change_dir(child));
    }
    for file in &dir.files {
        merge(Some(file.status.clone()));
    }
    dir.status = if mixed { None } else { status };
    dir.status.clone()
}

/// `String.prototype.localeCompare` for file names with the OS default locale.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    monocode_locale::compare(a, b)
}

/// `dirname`: the folder part of a repo-relative path.
pub fn dirname(relative: &str) -> &str {
    match relative.rfind('/') {
        Some(i) if i > 0 => &relative[..i],
        _ => "",
    }
}

/// `statusLetter`.
pub fn status_letter(status: &str) -> &'static str {
    match status {
        "untracked" => "U",
        "added" => "A",
        "deleted" => "D",
        _ => "M",
    }
}

/// `statusColor`, as a role the view maps to a color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    /// `text-sky-400`.
    Untracked,
    /// `text-emerald-400`.
    Added,
    /// `text-red-400`.
    Deleted,
    /// `text-amber-400`.
    Modified,
}

pub fn status_tone(status: &str) -> StatusTone {
    match status {
        "untracked" => StatusTone::Untracked,
        "added" => StatusTone::Added,
        "deleted" => StatusTone::Deleted,
        _ => StatusTone::Modified,
    }
}

/// `isActive`.
pub fn is_active(
    file: &GitChangedFile,
    selected: Option<&str>,
    selected_kind: Option<GitFileDiffKind>,
    kind: GitFileDiffKind,
) -> bool {
    selected == Some(file.relative.as_str())
        && selected_kind.is_none_or(|selected| selected == kind)
}

/// `amendTarget` stops applying once the branch or HEAD moves.
pub fn amend_target_stale(target: &AmendTarget, index: Option<&GitDiffIndex>) -> bool {
    let branch = index.and_then(|index| index.branch.clone());
    let head = index.and_then(|index| index.head.clone());
    target.branch != branch || target.head != head
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn index() -> GitDiffIndex {
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

    fn file(relative: &str, status: &str) -> GitChangedFile {
        GitChangedFile {
            path: format!("/repo/{relative}"),
            relative: relative.into(),
            status: status.into(),
            additions: 1,
            deletions: 0,
            staged: false,
            unstaged: true,
        }
    }

    #[test]
    fn pull_needs_a_remote_and_an_upstream() {
        assert!(!can_pull(Some(&index())));
        let mut no_remote = index();
        no_remote.upstream = Some("origin/feature/pull".into());
        assert!(!can_pull(Some(&no_remote)));
        let mut ready = no_remote.clone();
        ready.remote = Some("origin".into());
        assert!(can_pull(Some(&ready)));
    }

    #[test]
    fn create_pr_needs_a_clean_branch_ahead_of_default() {
        let mut ready = index();
        ready.remote = Some("origin".into());
        ready.upstream = Some("origin/feature/pull".into());
        ready.ahead = 1;
        ready.ahead_of_default = 1;
        let flags = ChangesFlags::compute(&FlagInputs {
            cwd: "/repo",
            index: Some(&ready),
            pr: None,
            busy: false,
            message: "",
            amend: false,
        });
        assert!(flags.can_create_pr);
        assert!(flags.can_sync);
        assert!(!flags.can_publish);
        assert!(!flags.can_generate);
        let buttons = sync_buttons(&flags).unwrap();
        assert!(buttons.sync && buttons.create_pr && !buttons.view_pr);
    }

    #[test]
    fn generation_is_off_for_remote_projects() {
        let mut changed = index();
        changed.files.push(file("a.ts", "modified"));
        let flags = |cwd| {
            ChangesFlags::compute(&FlagInputs {
                cwd,
                index: Some(&changed),
                pr: None,
                busy: false,
                message: "",
                amend: false,
            })
        };
        assert!(flags("/repo").can_generate);
        assert!(!flags("remote://machine/repo").can_generate);
    }

    #[test]
    fn labels_follow_the_sync_state() {
        let mut state = index();
        state.ahead = 2;
        assert_eq!(sync_status_label(&state), "2 unpushed commits");
        state.behind = 1;
        state.upstream = Some("origin/x".into());
        assert_eq!(sync_status_label(&state), "Diverged from origin/x");
        state.ahead = 0;
        assert_eq!(sync_status_label(&state), "1 incoming commit");
        assert_eq!(
            sync_title(&state, false, false),
            "Pull 1 commit from origin/x"
        );
        assert_eq!(empty_list_label(None), "Loading changes…");
        assert_eq!(empty_list_label(Some(&index())), "No uncommitted changes");
    }

    #[test]
    fn remote_pr_content_comes_from_the_git_range() {
        let content = remote_pr_content(&GitRangeContext {
            base: "main".into(),
            head: "feature/pull".into(),
            commit_summary: "abc123 Fix remote flow\ndef456 Add coverage".into(),
            diff_summary: "2 files changed, 4 insertions(+)\n".into(),
            diff_patch: String::new(),
        });
        assert_eq!(content.title, "Fix remote flow");
        assert!(content.body.contains("## Changes\n\n2 files changed"));
        assert_eq!(
            (content.base.as_str(), content.head.as_str()),
            ("main", "feature/pull")
        );
        assert_eq!(pr_number_from_url("https://example.test/pull/42"), Some(42));
        assert_eq!(
            pr_number_from_url("https://example.test/pull/42/files"),
            Some(42)
        );
        assert_eq!(pr_number_from_url("https://example.test/pulls"), None);
    }

    #[test]
    fn the_tree_nests_folders_first_and_rolls_status_up() {
        let tree = build_change_tree(&[
            file("src/b.ts", "modified"),
            file("src/a.ts", "modified"),
            file("README.md", "untracked"),
            file("src/ui/x.ts", "added"),
        ]);
        assert_eq!(tree.dirs.len(), 1);
        let src = &tree.dirs[0];
        assert_eq!(src.path, "src");
        assert_eq!(src.dirs[0].path, "src/ui");
        assert_eq!(src.dirs[0].status.as_deref(), Some("added"));
        let names: Vec<&str> = src.files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(names, vec!["src/a.ts", "src/b.ts"]);
        assert_eq!(src.status, None);
        assert_eq!(tree.files[0].relative, "README.md");
    }

    #[test]
    fn discard_prompts_name_the_file() {
        let untracked = file("new.ts", "untracked");
        assert_eq!(
            discard_file_prompt(&untracked),
            ("Delete untracked file new.ts?".to_string(), "Delete")
        );
        let two = [file("a.ts", "modified"), file("b.ts", "modified")];
        assert_eq!(
            discard_all_prompt(&two).unwrap().0,
            "Discard all unstaged changes in 2 files? This cannot be undone."
        );
        assert_eq!(discard_all_prompt(&[]), None);
    }

    #[test]
    fn matches_intl_change_tree_directory_and_file_order() {
        let names = [
            "filez",
            "file.a",
            "fileé",
            "filee\u{301}",
            "file-a",
            "filee",
            "file_a",
        ];
        let mut files = Vec::new();
        for name in names {
            files.push(file(&format!("{name}/nested.rs"), "modified"));
            files.push(file(&format!("{name}.rs"), "modified"));
        }
        let tree = build_change_tree(&files);
        let expected = [
            "file_a",
            "file-a",
            "file.a",
            "filee",
            "fileé",
            "filee\u{301}",
            "filez",
        ];
        assert_eq!(
            tree.dirs
                .iter()
                .map(|dir| dir.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            tree.files
                .iter()
                .map(|entry| entry.relative.as_str())
                .collect::<Vec<_>>(),
            expected.map(|name| format!("{name}.rs"))
        );
        assert_eq!(tree.status.as_deref(), Some("modified"));
        assert!(tree.dirs.iter().all(|dir| dir.files.len() == 1));
    }

    #[test]
    fn small_helpers() {
        assert_eq!(dirname("src/a/b.ts"), "src/a");
        assert_eq!(dirname("b.ts"), "");
        assert_eq!(status_letter("deleted"), "D");
        assert_eq!(status_letter("renamed"), "M");
        assert_eq!(locale_compare("b", "A"), Ordering::Greater);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
    }
}
