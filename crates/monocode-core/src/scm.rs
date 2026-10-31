//! Source control for the Changes and History panes, ported from
//! src-tauri/src/fs.rs. Synchronous: callers run these on a background executor.
mod review;
pub(crate) mod shell;
pub(crate) mod text;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::files::remote as remote_workspace;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use review::{
    commit_file_diff, commit_files, diff_files, file_diff, file_diff_versions, review_index,
    stage_contents, FileDiff, FileDiffVersion, MAX_TEXT_FILE_BYTES,
};
pub(crate) use shell::{expand_home, path_to_js};

/// The PATH a login terminal would have, for shells the app starts: a
/// Finder-launched app inherits launchd's bare PATH.
pub fn terminal_search_path() -> String {
    shell::gui_search_path()
}
/// Resolve a CLI using the login-shell PATH available to Finder-launched apps.
pub fn resolve_gui_binary(name: &str) -> Option<std::path::PathBuf> {
    shell::resolve_gui_binary(name)
}
pub use text::{commit_message_prompt, parse_commit_message, parse_pr_content, pr_content_prompt};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffStats {
    pub files: i64,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub relative: String,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    pub staged: bool,
    pub unstaged: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffIndex {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub files: Vec<ChangedFile>,
    pub additions: i64,
    pub deletions: i64,
    pub remote: Option<String>,
    pub upstream: Option<String>,
    pub default_branch: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    pub ahead_of_default: i64,
    pub head_pushed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRef {
    pub name: String,
    /// `"local"`, `"remote"`, or `"tag"`.
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCommit {
    pub sha: String,
    pub short_sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
    pub refs: Vec<HistoryRef>,
    pub head: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub head: Option<String>,
    pub commits: Vec<HistoryCommit>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StagedContext {
    pub branch: Option<String>,
    pub summary: String,
    pub patch: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RangeContext {
    pub base: String,
    pub head: String,
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pr {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
}

const HISTORY_DEFAULT: u32 = 200;
const HISTORY_MAX: u32 = 500;
const MAX_UNTRACKED_BYTES: u64 = 1024 * 1024;

/// Uncommitted line counts: staged + unstaged vs HEAD, plus untracked
/// (gitignore-aware) files counted as additions.
pub fn diff_stats(cwd: &Path) -> Result<DiffStats, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_diff_stats", json!({"cwd":cwd}));
    }
    Ok(diff_stats_for(&expand_home(cwd)))
}

/// Changed files under `cwd` (paths relative to it), with per-file line counts,
/// status, and branch/upstream synchronization metadata.
pub fn diff_index(cwd: &Path) -> Result<DiffIndex, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_diff_index", json!({"cwd":cwd}));
    }
    Ok(diff_index_with(&expand_home(cwd), true))
}

/// Recent commits for the Graph view: HEAD, upstream, and the default branch.
/// Newest first, with parent SHAs for the graph.
pub fn history(cwd: &Path, limit: Option<u32>) -> Result<History, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_history", json!({"cwd":cwd,"limit":limit}));
    }
    history_for(&expand_home(cwd), limit)
}

/// Stage a changed file or folder (`git add`).
pub fn stage_file(cwd: &Path, relative: &str) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_stage_file", json!({"cwd":cwd,"relative":relative}));
    }
    let root = expand_home(cwd);
    let relative = resolve_repo_path(&root, relative)?;
    git_checked(&root, &["--literal-pathspecs", "add", "--", &relative])
}

/// Unstage a file or folder (`git restore --staged`).
pub fn unstage_file(cwd: &Path, relative: &str) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command(
            "git_unstage_file",
            json!({"cwd":cwd,"relative":relative}),
        );
    }
    let root = expand_home(cwd);
    let relative = resolve_repo_path(&root, relative)?;
    git_checked(
        &root,
        &[
            "--literal-pathspecs",
            "restore",
            "--staged",
            "--",
            &relative,
        ],
    )
}

/// Discard unstaged changes so the file matches the index (or delete if untracked).
pub fn discard_file(cwd: &Path, relative: &str) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command(
            "git_discard_file",
            json!({"cwd":cwd,"relative":relative}),
        );
    }
    discard_file_for(&expand_home(cwd), relative)
}

/// Stage every changed file in the repo.
pub fn stage_all(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_stage_all", json!({"cwd":cwd}));
    }
    git_checked(&expand_home(cwd), &["add", "-A", "--", "."])
}

/// Unstage every staged file.
pub fn unstage_all(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_unstage_all", json!({"cwd":cwd}));
    }
    git_checked(&expand_home(cwd), &["restore", "--staged", "--", "."])
}

/// Discard every unstaged change (restore tracked files; delete untracked).
pub fn discard_all(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_discard_all", json!({"cwd":cwd}));
    }
    let root = expand_home(cwd);
    let files: Vec<String> = diff_index_with(&root, true)
        .files
        .into_iter()
        .filter(|file| file.unstaged)
        .map(|file| file.relative)
        .collect();
    for relative in files {
        discard_file_for(&root, &relative)?;
    }
    Ok(())
}

/// Create a commit from the current index, or rewrite HEAD with it when `amend` is set.
pub fn commit(cwd: &Path, message: &str, amend: bool) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command(
            "git_commit",
            json!({"cwd":cwd,"message":message,"amend":amend}),
        );
    }
    let extra: &[&str] = if amend { &["--amend"] } else { &[] };
    commit_args(&expand_home(cwd), message, extra)
}

/// Full message (subject and body) of the commit at HEAD.
pub fn head_message(cwd: &Path) -> Result<String, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_head_message", json!({"cwd":cwd}));
    }
    git_stdout(&expand_home(cwd), &["log", "-1", "--pretty=%B"])
        .ok_or_else(|| "No commits yet".to_string())
}

/// Push the current branch to its upstream, or set upstream on first push.
pub fn push(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_push", json!({"cwd":cwd}));
    }
    push_for(&expand_home(cwd))
}

/// Fast-forward the current branch from its upstream.
pub fn pull(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_pull", json!({"cwd":cwd}));
    }
    git_checked(&expand_home(cwd), &["pull", "--ff-only"])
}

/// Pull incoming commits, then push local commits.
pub fn sync(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_sync", json!({"cwd":cwd}));
    }
    let root = expand_home(cwd);
    if git_stdout(&root, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_some() {
        git_checked(&root, &["pull", "--no-edit", "--ff"]).map_err(with_signing_hint)?;
        return git_checked(&root, &["push"]);
    }
    push_for(&root)
}

/// Staged diff (or unstaged vs HEAD if nothing is staged) for commit text generation.
pub fn staged_context(cwd: &Path) -> Result<StagedContext, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_staged_context", json!({"cwd":cwd}));
    }
    let root = expand_home(cwd);
    let root = root.as_path();
    let mut summary = git_run(root, &["diff", "--cached", "--stat", "--", "."]).unwrap_or_default();
    let mut patch =
        git_run(root, &["diff", "--cached", "--no-ext-diff", "--", "."]).unwrap_or_default();

    if summary.trim().is_empty() && patch.trim().is_empty() {
        summary = git_run(root, &["diff", "HEAD", "--stat", "--", "."]).unwrap_or_default();
        patch = git_run(root, &["diff", "HEAD", "--no-ext-diff", "--", "."]).unwrap_or_default();
        if let Some(untracked) = git_run(root, &["ls-files", "--others", "--exclude-standard"]) {
            let names = untracked.trim();
            if !names.is_empty() {
                if !summary.trim().is_empty() {
                    summary.push('\n');
                }
                summary.push_str("Untracked files:\n");
                summary.push_str(names);
            }
        }
    }

    if summary.trim().is_empty() && patch.trim().is_empty() {
        return Err("No changes to summarize".into());
    }

    Ok(StagedContext {
        branch: git_branch(root),
        summary,
        patch,
    })
}

/// Commits and diff between the default branch and HEAD, for PR text generation.
pub fn range_context(cwd: &Path) -> Result<RangeContext, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_range_context", json!({"cwd":cwd}));
    }
    let root = expand_home(cwd);
    let root = root.as_path();
    let head = git_branch(root).ok_or_else(|| "Not on a branch".to_string())?;
    let remote = git_remote_name(root);
    let default_branch = git_default_branch(root, remote.as_deref())
        .ok_or_else(|| "Could not resolve the default branch".to_string())?;
    let base_ref = match &remote {
        Some(remote)
            if git_ref_exists(root, &format!("refs/remotes/{remote}/{default_branch}")) =>
        {
            format!("{remote}/{default_branch}")
        }
        _ => default_branch.clone(),
    };
    let spec = format!("{base_ref}...HEAD");
    let commit_summary =
        git_run(root, &["log", "--format=%s", &format!("{base_ref}..HEAD")]).unwrap_or_default();
    let diff_summary = git_run(root, &["diff", "--stat", &spec]).unwrap_or_default();
    let diff_patch = git_run(root, &["diff", "--no-ext-diff", &spec]).unwrap_or_default();
    if commit_summary.trim().is_empty() && diff_patch.trim().is_empty() {
        return Err("No commits to include in a pull request".into());
    }
    Ok(RangeContext {
        base: default_branch,
        head,
        commit_summary,
        diff_summary,
        diff_patch,
    })
}

/// Latest pull request for the current branch, if `gh` can see one.
pub fn pr_status(cwd: &Path) -> Result<Option<Pr>, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command("git_pr_status", json!({"cwd":cwd}));
    }
    Ok(pr_status_for(&expand_home(cwd)))
}

/// The working copy's GitHub `owner/name`, via `gh` (Tauri's git_github_repo).
pub fn github_repo(cwd: &Path) -> Result<String, String> {
    if remote_workspace::is_remote(cwd) {
        return Err("GitHub repository lookup is local only".into());
    }
    github_repo_for(&expand_home(cwd))
}

/// An issue or pull request offered by the session link picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubWorkItem {
    pub item: crate::session_groups::WorkItem,
    pub title: String,
    /// "open", "closed", "merged" or "draft".
    pub state: String,
}

/// `repo`'s issues and pull requests for `query`: recently updated open ones
/// when empty, the item with that number for `123` or `#123`, otherwise a
/// GitHub search. A number with no item is an empty list, not an error.
pub fn github_work_items(
    cwd: &Path,
    repo: &str,
    query: &str,
) -> Result<Vec<GithubWorkItem>, String> {
    if remote_workspace::is_remote(cwd) {
        return Err("GitHub lookup is local only".into());
    }
    let root = expand_home(cwd);
    let (owner, name) = split_github_repo(repo)?;
    let query = query.trim();
    let number = query
        .strip_prefix('#')
        .unwrap_or(query)
        .parse::<u64>()
        .ok()
        .filter(|&n| n > 0);
    const FIELDS: &str = "fragment P on PullRequest{number title state isDraft updatedAt} \
                          fragment I on Issue{number title state updatedAt}";
    let mut args = vec![
        "api".to_string(),
        "graphql".into(),
        "-f".into(),
        format!("owner={owner}"),
        "-f".into(),
        format!("name={name}"),
    ];
    let graphql = if let Some(number) = number {
        args.extend(["-F".into(), format!("number={number}")]);
        "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){\
         issueOrPullRequest(number:$number){__typename ...P ...I}}}"
    } else if query.is_empty() {
        "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){\
         pullRequests(first:30,states:OPEN,orderBy:{field:UPDATED_AT,direction:DESC}){nodes{__typename ...P}} \
         issues(first:30,states:OPEN,orderBy:{field:UPDATED_AT,direction:DESC}){nodes{__typename ...I}}}}"
    } else {
        args.extend([
            "-f".into(),
            format!("search=repo:{owner}/{name} sort:updated-desc {query}"),
        ]);
        "query($search:String!){search(query:$search,type:ISSUE,first:30){nodes{__typename ...P ...I}}}"
    };
    args.extend(["-f".into(), format!("query={graphql} {FIELDS}")]);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let json = match gh_run(&root, &args) {
        Ok(json) => json,
        // GraphQL reports a missing number as an error alongside null data.
        Err(error) if number.is_some() && error.contains("Could not resolve") => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(parse_github_work_items(&value, &format!("{owner}/{name}")))
}

fn parse_github_work_items(value: &serde_json::Value, repo: &str) -> Vec<GithubWorkItem> {
    let data = &value["data"];
    let repository = &data["repository"];
    let mut nodes: Vec<&serde_json::Value> = Vec::new();
    for list in [
        &repository["pullRequests"]["nodes"],
        &repository["issues"]["nodes"],
        &data["search"]["nodes"],
    ] {
        nodes.extend(list.as_array().into_iter().flatten());
    }
    if repository["issueOrPullRequest"].is_object() {
        nodes.push(&repository["issueOrPullRequest"]);
    }
    let mut items: Vec<(&str, GithubWorkItem)> = nodes
        .into_iter()
        .filter_map(|node| {
            let kind = match node["__typename"].as_str()? {
                "PullRequest" => "pr",
                "Issue" => "issue",
                _ => return None,
            };
            let number = node["number"].as_u64().filter(|&n| n > 0)?;
            let state = node["state"].as_str().unwrap_or("OPEN").to_lowercase();
            let state = if state == "open" && node["isDraft"].as_bool() == Some(true) {
                "draft".into()
            } else {
                state
            };
            let url = format!(
                "https://github.com/{repo}/{}/{number}",
                if kind == "pr" { "pull" } else { "issues" }
            );
            Some((
                node["updatedAt"].as_str().unwrap_or(""),
                GithubWorkItem {
                    item: crate::session_groups::WorkItem {
                        kind: kind.into(),
                        repo: repo.into(),
                        number,
                        url,
                    },
                    title: node["title"].as_str().unwrap_or("").into(),
                    state,
                },
            ))
        })
        .collect();
    // RFC 3339 timestamps in UTC sort as text.
    items.sort_by(|a, b| b.0.cmp(a.0));
    items.into_iter().map(|(_, item)| item).collect()
}

/// Create a GitHub pull request with `gh` and return its URL.
pub fn pr_create(
    cwd: &Path,
    title: &str,
    body: &str,
    base: &str,
    head: &str,
) -> Result<String, String> {
    if remote_workspace::is_remote(cwd) {
        return remote_workspace::command(
            "git_pr_create",
            json!({"cwd":cwd,"title":title,"body":body,"base":base,"head":head}),
        );
    }
    let root = expand_home(cwd);
    let title = title.trim();
    if title.is_empty() {
        return Err("Pull request title cannot be empty".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let body_path = std::env::temp_dir().join(format!("monocode-pr-{stamp}.md"));
    std::fs::write(&body_path, body.trim()).map_err(|e| e.to_string())?;
    let result = gh_run(
        &root,
        &[
            "pr",
            "create",
            "--title",
            title,
            "--body-file",
            &body_path.to_string_lossy(),
            "--base",
            base.trim(),
            "--head",
            head.trim(),
        ],
    );
    let _ = std::fs::remove_file(&body_path);
    result.and_then(|output| {
        output
            .lines()
            .rev()
            .find(|line| line.starts_with("http://") || line.starts_with("https://"))
            .map(|line| line.trim().to_string())
            .ok_or_else(|| {
                if output.trim().is_empty() {
                    "gh returned no pull request URL".into()
                } else {
                    output
                }
            })
    })
}

fn diff_stats_for(root: &Path) -> DiffStats {
    if !git_is_work_tree(root) {
        return DiffStats::default();
    }
    let mut files: HashMap<String, FileAcc> = HashMap::new();
    if let Some(text) = git_run(
        root,
        &[
            "diff",
            "-z",
            "--no-ext-diff",
            "--numstat",
            "HEAD",
            "--",
            ".",
        ],
    ) {
        add_numstat_map(&text, &mut files);
    } else {
        if let Some(text) = git_run(
            root,
            &["diff", "-z", "--no-ext-diff", "--numstat", "--", "."],
        ) {
            add_numstat_map(&text, &mut files);
        }
        if let Some(text) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--no-ext-diff",
                "--cached",
                "--numstat",
                "--",
                ".",
            ],
        ) {
            add_numstat_map(&text, &mut files);
        }
    }
    add_untracked_map(root, &mut files);
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for acc in files.values() {
        additions += acc.additions;
        deletions += acc.deletions;
    }
    DiffStats {
        files: files.len() as i64,
        additions,
        deletions,
    }
}

#[derive(Clone, Default)]
struct FileAcc {
    additions: i64,
    deletions: i64,
    untracked: bool,
    staged: bool,
    unstaged: bool,
}

fn diff_index_with(root: &Path, include_sync: bool) -> DiffIndex {
    let mut files: HashMap<String, FileAcc> = HashMap::new();
    let mut statuses: HashMap<String, &'static str> = HashMap::new();

    if let Some(text) = git_run(
        root,
        &[
            "diff",
            "-z",
            "--relative",
            "--no-ext-diff",
            "--numstat",
            "HEAD",
            "--",
            ".",
        ],
    ) {
        add_numstat_map(&text, &mut files);
        if let Some(names) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--relative",
                "--no-ext-diff",
                "--name-status",
                "--no-renames",
                "HEAD",
                "--",
                ".",
            ],
        ) {
            add_name_status(&names, &mut statuses);
        }
    } else {
        if let Some(text) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--relative",
                "--no-ext-diff",
                "--numstat",
                "--",
                ".",
            ],
        ) {
            add_numstat_map(&text, &mut files);
        }
        if let Some(text) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--relative",
                "--no-ext-diff",
                "--cached",
                "--numstat",
                "--",
                ".",
            ],
        ) {
            add_numstat_map(&text, &mut files);
        }
        if let Some(names) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--relative",
                "--no-ext-diff",
                "--name-status",
                "--no-renames",
                "--",
                ".",
            ],
        ) {
            add_name_status(&names, &mut statuses);
        }
        if let Some(names) = git_run(
            root,
            &[
                "diff",
                "-z",
                "--relative",
                "--no-ext-diff",
                "--cached",
                "--name-status",
                "--no-renames",
                "--",
                ".",
            ],
        ) {
            add_name_status(&names, &mut statuses);
        }
    }
    add_untracked_map(root, &mut files);
    mark_cached_and_unstaged(root, &mut files);

    let mut out = Vec::with_capacity(files.len());
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for (relative, acc) in files {
        additions += acc.additions;
        deletions += acc.deletions;
        let abs = root.join(&relative);
        let status = if acc.untracked {
            "untracked"
        } else if let Some(status) = statuses.get(&relative) {
            *status
        } else if !abs.exists() {
            "deleted"
        } else {
            "modified"
        };
        out.push(ChangedFile {
            path: path_to_js(&abs),
            relative,
            status: status.to_string(),
            additions: acc.additions,
            deletions: acc.deletions,
            staged: acc.staged,
            unstaged: acc.untracked || acc.unstaged,
        });
    }
    out.sort_by(|a, b| a.relative.cmp(&b.relative));
    let sync = if include_sync {
        sync_for(root)
    } else {
        Sync::default()
    };
    DiffIndex {
        branch: git_branch(root),
        head: git_stdout(root, &["rev-parse", "HEAD"]),
        files: out,
        additions,
        deletions,
        remote: sync.remote,
        upstream: sync.upstream,
        default_branch: sync.default_branch,
        ahead: sync.ahead,
        behind: sync.behind,
        ahead_of_default: sync.ahead_of_default,
        head_pushed: sync.head_pushed,
    }
}

fn add_numstat_map(text: &str, files: &mut HashMap<String, FileAcc>) {
    // `git diff -z --numstat` ends each record with NUL. A rename is
    // `add <TAB> del <TAB> <NUL> old <NUL> new <NUL>`; every other record is
    // `add <TAB> del <TAB> path <NUL>`.
    let mut fields = text.split('\0');
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, '\t');
        let Some(add) = parts.next() else { continue };
        let Some(del) = parts.next() else { continue };
        let Some(path) = parts.next() else { continue };
        let path = if path.is_empty() {
            let _previous = fields.next();
            match fields.next() {
                Some(destination) if !destination.is_empty() => destination,
                _ => continue,
            }
        } else {
            path
        };
        let relative = literal_diff_path(path);
        if relative.is_empty() {
            continue;
        }
        let entry = files.entry(relative).or_default();
        if add != "-" && del != "-" {
            entry.additions += add.parse::<i64>().unwrap_or(0);
            entry.deletions += del.parse::<i64>().unwrap_or(0);
        }
    }
}

fn add_name_status(text: &str, statuses: &mut HashMap<String, &'static str>) {
    // `--no-renames -z` is `status <NUL> path <NUL>`, repeated.
    let mut fields = text.split('\0');
    while let Some(code) = fields.next() {
        if code.is_empty() {
            continue;
        }
        let Some(path) = fields.next() else {
            break;
        };
        let status = match code.as_bytes().first() {
            Some(b'A') => "added",
            Some(b'D') => "deleted",
            Some(b'M' | b'T') => "modified",
            _ => continue,
        };
        let relative = literal_diff_path(path);
        if !relative.is_empty() {
            statuses.insert(relative, status);
        }
    }
}

fn literal_diff_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        String::new()
    } else {
        path_to_js(Path::new(path))
    }
}

fn normalize_diff_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    let path = if let Some((_, new)) = path.split_once(" => ") {
        new.trim_end_matches('}')
    } else {
        path
    };
    path_to_js(Path::new(path))
}

fn add_untracked_map(root: &Path, files: &mut HashMap<String, FileAcc>) {
    let Some(stdout) = git_run(
        root,
        &["ls-files", "-o", "--exclude-standard", "-z", "--", "."],
    ) else {
        return;
    };
    for rel in stdout.split('\0') {
        if rel.is_empty() {
            continue;
        }
        let relative = path_to_js(Path::new(rel));
        let entry = files.entry(relative.clone()).or_default();
        entry.untracked = true;
        if entry.additions == 0 {
            entry.additions = text_line_count(&root.join(rel));
        }
    }
}

fn text_line_count(path: &Path) -> i64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_UNTRACKED_BYTES {
        return 0;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    if bytes.contains(&0) {
        return 0;
    }
    let mut lines = 1i64;
    for byte in &bytes {
        if *byte == b'\n' {
            lines += 1;
        }
    }
    if bytes.last() == Some(&b'\n') {
        lines -= 1;
    }
    lines
}

fn mark_cached_and_unstaged(root: &Path, files: &mut HashMap<String, FileAcc>) {
    if let Some(names) = git_run(
        root,
        &[
            "diff",
            "-z",
            "--relative",
            "--cached",
            "--name-only",
            "--no-renames",
            "--",
            ".",
        ],
    ) {
        for line in names.split('\0') {
            let relative = literal_diff_path(line);
            if !relative.is_empty() {
                files.entry(relative).or_default().staged = true;
            }
        }
    }
    if let Some(names) = git_run(
        root,
        &[
            "diff",
            "-z",
            "--relative",
            "--name-only",
            "--no-renames",
            "--",
            ".",
        ],
    ) {
        for line in names.split('\0') {
            let relative = literal_diff_path(line);
            if !relative.is_empty() {
                files.entry(relative).or_default().unstaged = true;
            }
        }
    }
}

/// Tips for the Graph filter: current HEAD, its upstream, and the repo
/// default branch. Omits `refs/stash` and unmerged local branches.
fn history_tips(root: &Path) -> Vec<String> {
    let mut tips = vec!["HEAD".to_string()];
    if git_stdout(root, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_some() {
        tips.push("@{upstream}".to_string());
    }
    if let Some(remote) = git_remote_name(root) {
        if let Some(branch) = git_default_branch(root, Some(remote.as_str())) {
            let spec = format!("{remote}/{branch}");
            if git_ref_exists(root, &format!("refs/remotes/{spec}")) {
                tips.push(spec);
            }
        }
    }
    tips
}

fn history_for(root: &Path, limit: Option<u32>) -> Result<History, String> {
    if !git_is_work_tree(root) {
        return Ok(History::default());
    }
    let n = limit.unwrap_or(HISTORY_DEFAULT).clamp(1, HISTORY_MAX);
    let count = n.to_string();
    let head = git_stdout(root, &["rev-parse", "HEAD"]);
    let remotes = git_remote_names(root);
    let tips = history_tips(root);
    let mut args = vec![
        "log".to_string(),
        "--topo-order".to_string(),
        "--decorate=short".to_string(),
        "--max-count".to_string(),
        count,
        "--format=%H%x00%h%x00%P%x00%an%x00%at%x00%D%x00%s%x1e".to_string(),
    ];
    args.extend(tips);
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(text) = git_run(root, &args_ref) else {
        return Ok(History {
            head,
            commits: Vec::new(),
        });
    };
    Ok(History {
        commits: parse_history_log(&text, head.as_deref(), &remotes),
        head,
    })
}

fn parse_history_log(text: &str, head: Option<&str>, remotes: &[String]) -> Vec<HistoryCommit> {
    let mut commits = Vec::new();
    for record in text.split('\u{1e}') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }
        let mut fields = record.split('\0');
        let Some(sha) = fields.next() else { continue };
        let Some(short_sha) = fields.next() else {
            continue;
        };
        let Some(parents) = fields.next() else {
            continue;
        };
        let Some(author) = fields.next() else {
            continue;
        };
        let Some(timestamp) = fields.next() else {
            continue;
        };
        let Some(decorations) = fields.next() else {
            continue;
        };
        let subject = fields.next().unwrap_or("");
        if sha.is_empty() {
            continue;
        }
        let (is_head, refs) = parse_decorations(decorations, head, sha, remotes);
        commits.push(HistoryCommit {
            sha: sha.to_string(),
            short_sha: if short_sha.is_empty() {
                sha.chars().take(7).collect()
            } else {
                short_sha.to_string()
            },
            parents: parents.split_whitespace().map(str::to_string).collect(),
            author: author.to_string(),
            timestamp: timestamp.parse().unwrap_or(0),
            subject: subject.to_string(),
            refs,
            head: is_head,
        });
    }
    commits
}

fn parse_decorations(
    raw: &str,
    head: Option<&str>,
    sha: &str,
    remotes: &[String],
) -> (bool, Vec<HistoryRef>) {
    let mut is_head = head == Some(sha);
    let mut refs = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(name) = part.strip_prefix("HEAD -> ") {
            is_head = true;
            if !name.is_empty() {
                refs.push(HistoryRef {
                    name: name.to_string(),
                    kind: "local".into(),
                });
            }
        } else if part == "HEAD" {
            is_head = true;
        } else if let Some(tag) = part.strip_prefix("tag: ") {
            if !tag.is_empty() {
                refs.push(HistoryRef {
                    name: tag.to_string(),
                    kind: "tag".into(),
                });
            }
        } else if part.ends_with("/HEAD") {
            continue;
        } else if remotes
            .iter()
            .any(|remote| part == remote || part.starts_with(&format!("{remote}/")))
        {
            refs.push(HistoryRef {
                name: part.to_string(),
                kind: "remote".into(),
            });
        } else {
            refs.push(HistoryRef {
                name: part.to_string(),
                kind: "local".into(),
            });
        }
    }
    (is_head, refs)
}

fn git_remote_names(root: &Path) -> Vec<String> {
    git_run(root, &["remote"])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn discard_file_for(root: &Path, relative: &str) -> Result<(), String> {
    let relative = resolve_repo_path(root, relative)?;
    let abs = root.join(&relative);
    if git_checked(root, &["ls-files", "--error-unmatch", "--", &relative]).is_err() {
        if abs.is_file() {
            std::fs::remove_file(&abs).map_err(|e| e.to_string())?;
        } else if abs.exists() {
            git_checked(root, &["clean", "-fd", "--", &relative])?;
        }
        return Ok(());
    }
    git_checked(root, &["restore", "--worktree", "--", &relative])
}

fn commit_args(root: &Path, message: &str, extra: &[&str]) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message cannot be empty".into());
    }
    let mut args = vec!["commit"];
    args.extend_from_slice(extra);
    args.extend(["--cleanup=strip", "-m", message]);
    git_checked(root, &args).map_err(with_signing_hint)
}

/// Explain why signing fails here when the same commit works in a terminal.
fn with_signing_hint(error: String) -> String {
    if !error.contains("failed to sign") && !error.contains("ssh-keygen") {
        return error;
    }
    format!(
        "{error}\n\nGit couldn't sign this commit. MonoCode runs git without a terminal, \
         so your signer needs a GUI passphrase prompt (e.g. pinentry-mac) or an unlocked agent."
    )
}

fn push_for(root: &Path) -> Result<(), String> {
    if git_stdout(root, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_some() {
        return git_checked(root, &["push"]);
    }
    let remote = git_remote_name(root).ok_or_else(|| "No git remote to push to".to_string())?;
    git_checked(root, &["push", "-u", &remote, "HEAD"])
}

fn pr_status_for(root: &Path) -> Option<Pr> {
    let branch = git_branch(root)?;
    let repo = github_repo_for(root).ok()?;
    let head = github_pr_head_filter(&repo, &branch)?;
    let json = gh_run(
        root,
        &[
            "pr",
            "list",
            "--head",
            &head,
            "--json",
            "number,title,url,state",
            "--limit",
            "20",
            "--state",
            "all",
        ],
    )
    .ok()?;
    parse_gh_pr_list(&json)
}

fn github_pr_head_filter(repo: &str, branch: &str) -> Option<String> {
    let (owner, _) = split_github_repo(repo).ok()?;
    Some(format!("{owner}:{branch}"))
}

fn github_repo_for(root: &Path) -> Result<String, String> {
    let json = gh_run(root, &["repo", "view", "--json", "nameWithOwner"])?;
    #[derive(Deserialize)]
    struct View {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }
    let view: View = serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let slug = view.name_with_owner.trim();
    if slug.is_empty() || !slug.contains('/') {
        return Err("GitHub did not return a repository".into());
    }
    Ok(slug.to_string())
}

fn split_github_repo(slug: &str) -> Result<(String, String), String> {
    let slug = slug.trim();
    let Some((owner, name)) = slug.split_once('/') else {
        return Err("GitHub did not return a repository".into());
    };
    let owner = owner.trim();
    let name = name.trim();
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || owner.chars().any(char::is_whitespace)
        || name.chars().any(char::is_whitespace)
    {
        return Err("GitHub did not return a repository".into());
    }
    Ok((owner.to_string(), name.to_string()))
}

fn parse_gh_pr_list(json: &str) -> Option<Pr> {
    #[derive(Deserialize)]
    struct Row {
        number: i64,
        title: String,
        url: String,
        state: String,
    }
    let rows: Vec<Row> = serde_json::from_str(json).ok()?;
    let mut best: Option<Pr> = None;
    for row in rows {
        let pr = Pr {
            number: row.number,
            title: row.title,
            url: row.url,
            state: row.state.to_lowercase(),
        };
        if pr.state == "open" {
            return Some(pr);
        }
        if best.is_none() {
            best = Some(pr);
        }
    }
    best
}

fn gh_run(root: &Path, args: &[&str]) -> Result<String, String> {
    let program = shell::resolve_gui_binary("gh")
        .ok_or_else(|| "GitHub CLI (`gh`) is not installed.".to_string())?;
    let mut cmd = Command::new(&program);
    cmd.current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("GIT_PAGER", "cat");
    shell::apply_gui_env(&mut cmd);
    crate::process::configure_child(&mut cmd);
    let output = cmd.output().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            "GitHub CLI (`gh`) is not installed.".to_string()
        } else {
            error.to_string()
        }
    })?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err("gh returned no output".into());
        }
        return Ok(text);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("gh {} failed", args.join(" "))
    };
    Err(detail)
}

fn resolve_repo_path(root: &Path, relative: &str) -> Result<String, String> {
    let relative = normalize_diff_path(relative);
    if relative.is_empty()
        || relative.starts_with('/')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "..")
    {
        return Err("Invalid path".into());
    }
    let abs = root.join(&relative);
    if !abs.starts_with(root) {
        return Err("Invalid path".into());
    }
    Ok(relative)
}

/// Any git can start a filter (git-lfs), which a Finder launch's bare PATH
/// lacks. The cached search path finds it without waiting on the login shell.
pub(crate) fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    crate::process::configure_child(&mut cmd);
    cmd.env("PATH", shell::cached_gui_search_path())
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn git_cmd_for_args_with_path(args: &[&str], gui_path: impl FnOnce() -> String) -> Command {
    let mut cmd = git_cmd();
    let args = args.strip_prefix(&["--literal-pathspecs"]).unwrap_or(args);
    if matches!(
        args.first().copied(),
        Some("commit" | "push" | "pull" | "fetch" | "clone")
    ) {
        // Signers, hooks, and credential helpers may only be on the login-shell PATH.
        cmd.env("PATH", gui_path());
    }
    cmd
}

fn git_cmd_for_args(args: &[&str]) -> Command {
    git_cmd_for_args_with_path(args, shell::gui_search_path)
}

fn git_checked(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = git_cmd_for_args(args);
    let output = cmd
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let msg = stderr.trim();
    if !msg.is_empty() {
        return Err(msg.to_string());
    }
    let msg = stdout.trim();
    if !msg.is_empty() {
        return Err(msg.to_string());
    }
    Err(format!("git {} failed", args.join(" ")))
}

fn git_run(root: &Path, args: &[&str]) -> Option<String> {
    git_output(root, args).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = git_cmd_for_args(args)
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if git_status_ok(&output.status, args) {
        return Some(output.stdout);
    }
    None
}

fn git_status_ok(status: &std::process::ExitStatus, args: &[&str]) -> bool {
    status.success()
        || (status.code() == Some(1) && matches!(args.first().copied(), Some("diff" | "grep")))
}

fn git_branch(root: &Path) -> Option<String> {
    git_head_branch(root).or_else(|| git_stdout(root, &["rev-parse", "--short", "HEAD"]))
}

fn git_head_branch(root: &Path) -> Option<String> {
    git_stdout(root, &["symbolic-ref", "--short", "HEAD"]).filter(|branch| branch != "HEAD")
}

fn git_is_work_tree(root: &Path) -> bool {
    git_stdout(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

#[derive(Default)]
struct Sync {
    remote: Option<String>,
    upstream: Option<String>,
    default_branch: Option<String>,
    ahead: i64,
    behind: i64,
    ahead_of_default: i64,
    head_pushed: bool,
}

fn sync_for(root: &Path) -> Sync {
    let remote = git_remote_name(root);
    let upstream = git_stdout(root, &["rev-parse", "--abbrev-ref", "@{upstream}"]);
    let default_branch = git_default_branch(root, remote.as_deref());
    let default_ref = match (&remote, &default_branch) {
        (Some(remote), Some(branch)) => Some(format!("{remote}/{branch}")),
        _ => None,
    };
    let (ahead, behind) = if upstream.is_some() {
        git_ahead_behind(root, "@{upstream}")
    } else if let Some(base) = default_ref.as_deref() {
        git_ahead_behind(root, base)
    } else {
        (0, 0)
    };
    let ahead_of_default = if let Some(base) = default_ref.as_deref() {
        git_ahead_behind(root, base).0
    } else {
        ahead
    };
    let head_pushed = git_stdout(
        root,
        &[
            "for-each-ref",
            "--count=1",
            "--contains",
            "HEAD",
            "refs/remotes",
        ],
    )
    .is_some();
    Sync {
        remote,
        upstream,
        default_branch,
        ahead,
        behind,
        ahead_of_default,
        head_pushed,
    }
}

fn git_remote_name(root: &Path) -> Option<String> {
    let remotes = git_stdout(root, &["remote"])?;
    let mut names = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let first = names.next()?.to_string();
    if first == "origin" || names.any(|name| name == "origin") {
        return Some("origin".into());
    }
    Some(first)
}

fn git_default_branch(root: &Path, remote: Option<&str>) -> Option<String> {
    if let Some(remote) = remote {
        if let Some(head) = git_stdout(
            root,
            &[
                "symbolic-ref",
                "--short",
                &format!("refs/remotes/{remote}/HEAD"),
            ],
        ) {
            if let Some((_, name)) = head.split_once('/') {
                return Some(name.to_string());
            }
            return Some(head);
        }
        for name in ["main", "master"] {
            if git_ref_exists(root, &format!("refs/remotes/{remote}/{name}")) {
                return Some(name.to_string());
            }
        }
    }
    for name in ["main", "master"] {
        if git_ref_exists(root, &format!("refs/heads/{name}")) {
            return Some(name.to_string());
        }
    }
    None
}

fn git_ref_exists(root: &Path, spec: &str) -> bool {
    git_output(root, &["show-ref", "--verify", "--quiet", spec]).is_some()
}

fn git_ahead_behind(root: &Path, base: &str) -> (i64, i64) {
    let spec = format!("{base}...HEAD");
    let Some(text) = git_stdout(root, &["rev-list", "--left-right", "--count", &spec]) else {
        return (0, 0);
    };
    let mut parts = text.split_whitespace();
    let behind = parts
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let ahead = parts
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    (ahead, behind)
}

/// The checkout's Git directory and, for a linked worktree, the shared one:
/// where HEAD, the index and refs live, for change watchers.
pub fn git_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    let Some(text) = git_stdout(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ],
    ) else {
        return Vec::new();
    };
    let mut dirs: Vec<std::path::PathBuf> = text.lines().map(Into::into).collect();
    dirs.dedup();
    dirs
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = git_cmd().arg("-C").arg(root).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests;
