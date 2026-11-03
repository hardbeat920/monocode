//! A session's code contributions, found in the tool calls its agent ran:
//! commits, and pull requests it opened, merged or reviewed. Each is told
//! apart by its commit or pull request, so seeing a call twice counts once.
use crate::tool::{ToolCall, ToolState};
use regex::Regex;
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Commit,
    Review,
    Opened,
    Merged,
    /// A pull request the agent opened that has since merged, by anyone. It
    /// counts toward merged pull requests, not toward contributions.
    Landed,
}
impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Review => "review",
            Self::Opened => "opened",
            Self::Merged => "merged",
            Self::Landed => "landed",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Some(match id {
            "commit" => Self::Commit,
            "review" => Self::Review,
            "opened" => Self::Opened,
            "merged" => Self::Merged,
            "landed" => Self::Landed,
            _ => return None,
        })
    }
    /// Whether it fills the contribution graph.
    pub fn counts(self) -> bool {
        self != Self::Landed
    }
}

/// A contribution seen in one tool call, before Git fills in its details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub kind: Kind,
    /// A commit's id as printed, or a pull request as `owner/repo#123`.
    pub reference: String,
    /// The commit replaced the session's previous one.
    pub amend: bool,
    /// Checkouts used by commit commands, relative to the session unless absolute.
    pub checkouts: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contribution {
    pub session: i64,
    pub kind: Kind,
    /// A full commit id, a pull request, or a review of one.
    pub reference: String,
    /// Unix milliseconds.
    pub at: u64,
    pub additions: u64,
    pub deletions: u64,
}

static GIT_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s;&|(])(?P<git>git)\s+").unwrap());
/// `git commit`'s summary: `[main 3f2a91c] Fix…`, `[main (root-commit) 3f2a91c] …`.
static COMMIT_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\[[^\]\n]* ([0-9a-f]{7,40})\] ").unwrap());
static GH_PR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s;&|(])gh\s+pr\s+(create|merge|review)\b").unwrap());
static PR_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/pull/(\d+)").unwrap()
});
/// How `gh` names a pull request in its confirmations: `owner/repo#123`.
static PR_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)#(\d+)\b").unwrap());

/// The pull request a URL or `gh` confirmation in `text` names.
fn pull_request(text: &str) -> Option<String> {
    PR_URL
        .captures(text)
        .or_else(|| PR_NAME.captures(text))
        .map(|c| format!("{}#{}", &c[1], &c[2]))
}

/// The contributions a finished tool call made. Only successful calls whose
/// output confirms the work count: a commit's summary line, or `gh` naming
/// the pull request it acted on.
pub fn found(tool: &ToolCall) -> Vec<Found> {
    if tool.state != ToolState::Completed {
        return Vec::new();
    }
    if let Some(found) = github_tool(tool) {
        return vec![found];
    }
    let Some(command) = tool.presentation().command else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if let Some(checkouts) = commit_checkouts(tool, &command) {
        let amend = command.contains("--amend");
        found.extend(COMMIT_LINE.captures_iter(&tool.output).map(|c| Found {
            kind: Kind::Commit,
            reference: c[1].to_owned(),
            amend,
            checkouts: checkouts.clone(),
        }));
    }
    if let Some(action) = GH_PR.captures(&command).map(|c| c[1].to_owned()) {
        // An auto-merge only queues the pull request.
        let queued = command.contains("--auto") || tool.output.contains("automatically merge");
        let kind = match action.as_str() {
            "create" => Some(Kind::Opened),
            "merge" if !queued && tool.output.to_lowercase().contains("merged") => {
                Some(Kind::Merged)
            }
            "review" => Some(Kind::Review),
            _ => None,
        };
        if let (Some(kind), Some(pr)) = (kind, pull_request(&tool.output)) {
            found.push(Found {
                kind,
                reference: review_reference(kind, pr, &tool.id),
                amend: false,
                checkouts: Vec::new(),
            });
        }
    }
    found
}

fn raw_input(tool: &ToolCall) -> serde_json::Value {
    let input: serde_json::Value = serde_json::from_str(&tool.input).unwrap_or_default();
    if input["__desktop"].is_object() {
        input.get("rawInput").cloned().unwrap_or_default()
    } else {
        input
    }
}

/// Git applies each -C relative to the previous one. Parse only its literal
/// arguments; never execute the recorded shell command to recover a checkout.
fn commit_checkouts(tool: &ToolCall, command: &str) -> Option<Vec<PathBuf>> {
    let input = raw_input(tool);
    let base = ["workdir", "cwd", "workingDirectory", "working_directory"]
        .iter()
        .find_map(|key| input[*key].as_str())
        .map(PathBuf::from);
    let mut checkouts = Vec::new();
    for candidate in GIT_COMMAND.captures_iter(command) {
        let Some(words) = shlex::split(&command[candidate.name("git")?.start()..]) else {
            continue;
        };
        let mut cwd = base.clone();
        let mut args = words.iter().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "commit" => {
                    let cwd = cwd.unwrap_or_default();
                    if !checkouts.contains(&cwd) {
                        checkouts.push(cwd);
                    }
                    break;
                }
                "-C" => {
                    let Some(path) = args.next() else { break };
                    cwd.get_or_insert_default().push(path);
                }
                "-c" => {
                    if args.next().is_none() {
                        break;
                    }
                }
                arg if arg.starts_with("-C") => cwd.get_or_insert_default().push(&arg[2..]),
                arg if arg.starts_with("-c") => {}
                _ => break,
            }
        }
    }
    (!checkouts.is_empty()).then_some(checkouts)
}

/// Each review is its own contribution, even on the same pull request.
fn review_reference(kind: Kind, pr: String, call: &str) -> String {
    if kind == Kind::Review {
        format!("{pr}@{call}")
    } else {
        pr
    }
}

/// GitHub's own tools, as MCP servers name them.
fn github_tool(tool: &ToolCall) -> Option<Found> {
    let name = tool.name.to_lowercase();
    if !name.contains("github") {
        return None;
    }
    let input = raw_input(tool);
    let action = name.rsplit("__").next()?;
    let action = action.strip_prefix("github_").unwrap_or(action);
    let kind = match action {
        "create_pull_request_review"
        | "submit_pull_request_review"
        | "submit_pending_pull_request_review"
        | "add_review_to_pr" => Kind::Review,
        "pull_request_review_write"
            if input["method"] == "submit"
                || (input["method"] == "create"
                    && matches!(
                        input["event"].as_str(),
                        Some("APPROVE" | "COMMENT" | "REQUEST_CHANGES")
                    )) =>
        {
            Kind::Review
        }
        "merge_pull_request" => Kind::Merged,
        "create_pull_request" => Kind::Opened,
        _ => return None,
    };
    let number = input["pull_number"]
        .as_u64()
        .or_else(|| input["pullNumber"].as_u64())
        .or_else(|| input["pr_number"].as_u64());
    let repo = input["repo_full_name"]
        .as_str()
        .or_else(|| input["repository_full_name"].as_str())
        .map(str::to_owned)
        .or_else(|| {
            Some(format!(
                "{}/{}",
                input["owner"].as_str()?,
                input["repo"].as_str()?
            ))
        });
    let named = repo.zip(number).map(|(repo, n)| format!("{repo}#{n}"));
    let pr = pull_request(&tool.output).or(named)?;
    Some(Found {
        kind,
        reference: review_reference(kind, pr, &tool.id),
        amend: false,
        checkouts: Vec::new(),
    })
}

/// A commit's full id, time and line counts, from the repository it was made
/// in. A commit Git no longer has still counts, without its lines.
pub fn resolve(found: &Found, session: i64, cwd: &Path, at: u64) -> Contribution {
    let mut contribution = Contribution {
        session,
        kind: found.kind,
        reference: found.reference.clone(),
        at,
        additions: 0,
        deletions: 0,
    };
    if found.kind != Kind::Commit {
        return contribution;
    }
    let checkouts = if found.checkouts.is_empty() {
        vec![cwd.to_path_buf()]
    } else {
        found.checkouts.iter().map(|path| cwd.join(path)).collect()
    };
    // A batched command can commit in several checkouts. The confirmed hash
    // identifies which one contains each commit, even if another command failed.
    let Some(out) = checkouts.iter().find_map(|checkout| {
        crate::git::git(
            checkout,
            &[
                "show",
                "--numstat",
                "--no-renames",
                "--format=%H %ct",
                &found.reference,
                "--",
            ],
        )
        .ok()
    }) else {
        return contribution;
    };
    let mut lines = out.lines();
    if let Some((id, time)) = lines.next().and_then(|l| l.split_once(' ')) {
        contribution.reference = id.to_owned();
        if let Ok(seconds) = time.trim().parse::<u64>() {
            contribution.at = seconds * 1000;
        }
    }
    for line in lines {
        let mut parts = line.split('\t');
        // Binary files report `-` and have no lines to count.
        if let (Some(a), Some(d)) = (parts.next(), parts.next()) {
            contribution.additions += a.parse::<u64>().unwrap_or(0);
            contribution.deletions += d.parse::<u64>().unwrap_or(0);
        }
    }
    contribution
}

/// When a pull request named `owner/repo#123` merged, if it has (Unix ms).
/// Asks GitHub through `gh`, so it is for background checks only.
pub fn merged_at(pull_request: &str) -> Result<Option<u64>, String> {
    let (repo, number) = pull_request.split_once('#').ok_or("Not a pull request")?;
    let binary = crate::scm::shell::resolve_gui_binary("gh").ok_or("GitHub CLI not found")?;
    let mut cmd = std::process::Command::new(binary);
    cmd.args([
        "pr",
        "view",
        number,
        "--repo",
        repo,
        "--json",
        "state,mergedAt",
    ])
    .env("GH_PROMPT_DISABLED", "1");
    crate::scm::shell::apply_gui_env(&mut cmd);
    crate::process::configure_child(&mut cmd);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    if value["state"] != "MERGED" {
        return Ok(None);
    }
    Ok(Some(
        value["mergedAt"]
            .as_str()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map_or_else(crate::time::now_ms, |at| {
                at.timestamp_millis().max(0) as u64
            }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(command: &str, output: &str) -> Vec<Found> {
        let mut tool = ToolCall::new("call-1", "Bash");
        tool.input = serde_json::json!({ "command": command }).to_string();
        tool.output = output.into();
        tool.state = ToolState::Completed;
        found(&tool)
    }

    #[test]
    fn commits_count_from_gits_summary_line() {
        let found = run(
            "git add -A && git commit -m 'Fix login'",
            "[main 3f2a91c] Fix login\n 2 files changed, 10 insertions(+)",
        );
        assert_eq!(
            found,
            [Found {
                kind: Kind::Commit,
                reference: "3f2a91c".into(),
                amend: false,
                checkouts: vec![PathBuf::new()],
            }]
        );
        let root = run("git commit -m init", "[main (root-commit) 0a1b2c3] init");
        assert_eq!(root[0].reference, "0a1b2c3");
        assert!(run("git commit --amend --no-edit", "[main 9e8d7c6] Fix")[0].amend);
        assert!(run("git commit -m x", "nothing to commit, working tree clean").is_empty());
        assert!(run("git status", "[main 3f2a91c] Fix login").is_empty());
    }

    #[test]
    fn failed_or_running_calls_count_nothing() {
        let mut tool = ToolCall::new("call-1", "Bash");
        tool.input = serde_json::json!({ "command": "git commit -m x" }).to_string();
        tool.output = "[main 3f2a91c] x".into();
        tool.state = ToolState::Failed;
        assert!(found(&tool).is_empty());
        tool.state = ToolState::Running;
        assert!(found(&tool).is_empty());
    }

    #[test]
    fn pull_requests_count_when_gh_confirms_them() {
        let opened = run(
            "gh pr create --fill",
            "https://github.com/acme/app/pull/42\n",
        );
        assert_eq!(opened[0].kind, Kind::Opened);
        assert_eq!(opened[0].reference, "acme/app#42");
        let merged = run(
            "gh pr merge 42 --squash",
            "✓ Squashed and merged pull request acme/app#42 (Fix login)",
        );
        assert_eq!(merged[0].kind, Kind::Merged);
        assert_eq!(merged[0].reference, "acme/app#42");
        assert!(run(
            "gh pr merge 42 --auto",
            "✓ Pull request acme/app#42 will be automatically merged"
        )
        .is_empty());
        let review = run(
            "gh pr review 42 --approve",
            "✓ Approved pull request acme/app#42",
        );
        assert_eq!(review[0].kind, Kind::Review);
        assert_eq!(review[0].reference, "acme/app#42@call-1");
        assert!(run("gh pr review 42 --approve", "could not find pull request").is_empty());
    }

    #[test]
    fn github_tools_count_their_pull_requests() {
        let mut tool = ToolCall::new("call-2", "mcp__github__create_pull_request_review");
        tool.input =
            serde_json::json!({"owner": "acme", "repo": "app", "pull_number": 7}).to_string();
        tool.state = ToolState::Completed;
        assert_eq!(
            found(&tool),
            [Found {
                kind: Kind::Review,
                reference: "acme/app#7@call-2".into(),
                amend: false,
                checkouts: Vec::new(),
            }]
        );
        tool.name = "mcp__github__create_pull_request".into();
        tool.output = r#"{"html_url":"https://github.com/acme/app/pull/8"}"#.into();
        assert_eq!(found(&tool)[0].reference, "acme/app#8");
    }

    #[test]
    fn reading_reviews_does_not_record_a_review() {
        for name in [
            "mcp__codex_apps__github_list_pull_request_reviews",
            "mcp__github__get_pull_request_reviews",
            "mcp__github__list_pull_request_review_comments",
            "mcp__github__list_pull_request_review_threads",
        ] {
            let mut tool = ToolCall::new("read-1", name);
            tool.input =
                serde_json::json!({"repo_full_name":"acme/app","pr_number":42}).to_string();
            tool.output = r#"{"reviews":[{"url":"https://github.com/acme/app/pull/42#pullrequestreview-123"}]}"#.into();
            tool.state = ToolState::Completed;
            assert!(
                found(&tool).is_empty(),
                "{name} only reads existing reviews"
            );
        }
    }

    #[test]
    fn review_submissions_count_without_counting_pending_drafts_or_deletions() {
        let mut tool = ToolCall::new("review-1", "mcp__github__pull_request_review_write");
        tool.state = ToolState::Completed;
        for (method, event, submitted) in [
            ("submit", None, true),
            ("create", Some("APPROVE"), true),
            ("create", Some("COMMENT"), true),
            ("create", Some("REQUEST_CHANGES"), true),
            ("create", None, false),
            ("delete", None, false),
        ] {
            tool.input = serde_json::json!({"owner":"acme","repo":"app","pullNumber":42,"method":method,"event":event}).to_string();
            assert_eq!(!found(&tool).is_empty(), submitted, "{method} {event:?}");
        }
        tool.name = "mcp__codex_apps__github_add_review_to_pr".into();
        tool.input =
            serde_json::json!({"repo_full_name":"acme/app","pr_number":42,"action":"APPROVE"})
                .to_string();
        let review = found(&tool);
        assert_eq!(review[0].kind, Kind::Review);
        assert_eq!(review[0].reference, "acme/app#42@review-1");
    }

    #[test]
    fn commits_resolve_in_the_command_checkout() {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("session");
        let repo = root.path().join("checkouts").join("feature checkout");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| crate::git::git(&repo, args).unwrap();
        git(&["init", "-q"]);
        git(&["config", "user.email", "mono@example.com"]);
        git(&["config", "user.name", "Mono"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "a.txt"]);
        let output = git(&["commit", "-m", "Add a"]);
        let id = git(&["rev-parse", "HEAD"]).trim().to_owned();
        let path = repo.to_string_lossy();
        let quoted = shlex::try_quote(&path).unwrap();
        for input in [
            serde_json::json!({"command":format!("git -C {quoted} commit -m 'Add a'")}),
            serde_json::json!({"command":format!("git -C{quoted} -ccore.hooksPath=unused commit -m 'Add a'")}),
            serde_json::json!({"command":"git -C checkouts -C 'feature checkout' -c core.hooksPath=unused commit -m 'Add a'","cwd":root.path()}),
            serde_json::json!({"cmd":"git -C 'checkouts/feature checkout' commit -m 'Add a'","workdir":".."}),
            serde_json::json!({"cmd":"git commit -m 'Add a'","workdir":repo}),
            serde_json::json!({"command":"git -C '' commit -m 'Add a'","workingDirectory":repo}),
            serde_json::json!({"__desktop":{"kind":"shell"},"rawInput":{"cmd":"git commit -m 'Add a'","workdir":repo}}),
        ] {
            let mut tool = ToolCall::new("commit-1", "exec_command");
            tool.input = input.to_string();
            tool.output = output.clone();
            tool.state = ToolState::Completed;
            let commits = found(&tool);
            assert_eq!(commits.len(), 1, "{input}");
            let resolved = resolve(&commits[0], 3, &session, 0);
            assert_eq!(resolved.reference, id, "{input}");
            assert_eq!((resolved.additions, resolved.deletions), (2, 0), "{input}");
            assert!(resolved.at > 0);
        }
    }

    #[test]
    fn batched_commits_resolve_each_checkout() {
        let root = tempfile::tempdir().unwrap();
        let mut commands = Vec::new();
        let mut output = String::new();
        let mut expected = Vec::new();
        for lines in [1, 3] {
            let repo = root.path().join(format!("checkout {lines}"));
            std::fs::create_dir(&repo).unwrap();
            let git = |args: &[&str]| crate::git::git(&repo, args).unwrap();
            git(&["init", "-q"]);
            git(&["config", "user.email", "mono@example.com"]);
            git(&["config", "user.name", "Mono"]);
            git(&["config", "commit.gpgsign", "false"]);
            std::fs::write(repo.join("a.txt"), "line\n".repeat(lines)).unwrap();
            git(&["add", "a.txt"]);
            output.push_str(&git(&["commit", "-m", "Add a"]));
            expected.push((git(&["rev-parse", "HEAD"]).trim().to_owned(), lines as u64));
            let path = repo.to_string_lossy();
            commands.push(format!(
                "git -C {} commit -m 'Add a'",
                shlex::try_quote(&path).unwrap()
            ));
        }
        let commits = run(&commands.join(" && "), &output);
        assert_eq!(commits.len(), 2);
        for (found, (id, lines)) in commits.iter().zip(expected) {
            let resolved = resolve(found, 3, root.path(), 0);
            assert_eq!(resolved.reference, id);
            assert_eq!((resolved.additions, resolved.deletions), (lines, 0));
        }
    }

    #[test]
    fn commits_resolve_their_full_id_time_and_lines() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| crate::git::git(dir.path(), args).unwrap();
        git(&["init", "-q"]);
        git(&["config", "user.email", "mono@example.com"]);
        git(&["config", "user.name", "Mono"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "Add a"]);
        let id = git(&["rev-parse", "HEAD"]).trim().to_owned();
        let found = Found {
            kind: Kind::Commit,
            reference: id[..7].into(),
            amend: false,
            checkouts: Vec::new(),
        };
        let resolved = resolve(&found, 3, dir.path(), 0);
        assert_eq!(resolved.reference, id);
        assert_eq!((resolved.additions, resolved.deletions), (2, 0));
        assert!(resolved.at > 0);
        // A commit the repository lost still counts.
        let lost = resolve(
            &Found {
                reference: "deadbee".into(),
                ..found
            },
            3,
            dir.path(),
            5,
        );
        assert_eq!((lost.reference.as_str(), lost.at), ("deadbee", 5));
    }
}
