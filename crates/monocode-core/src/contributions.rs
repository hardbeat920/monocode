//! A session's code contributions, found in the tool calls its agent ran:
//! commits, and pull requests it opened, merged or reviewed. Each is told
//! apart by its commit or pull request, so seeing a call twice counts once.
use crate::tool::{ToolCall, ToolState};
use regex::Regex;
use std::{path::Path, sync::LazyLock};

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

static GIT_COMMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s;&|(])git(?:\s+-[cC]\s+\S+)*\s+commit\b").unwrap()
});
/// `git commit`'s summary: `[main 3f2a91c] Fix…`, `[main (root-commit) 3f2a91c] …`.
static COMMIT_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\[[^\]\n]* ([0-9a-f]{7,40})\] ").unwrap());
static GH_PR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s;&|(])gh\s+pr\s+(create|merge|review)\b").unwrap()
});
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
    if GIT_COMMIT.is_match(&command) {
        let amend = command.contains("--amend");
        found.extend(COMMIT_LINE.captures_iter(&tool.output).map(|c| Found {
            kind: Kind::Commit,
            reference: c[1].to_owned(),
            amend,
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
            });
        }
    }
    found
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
    let kind = if name.contains("pull_request_review") {
        Kind::Review
    } else if name.contains("merge_pull_request") {
        Kind::Merged
    } else if name.contains("create_pull_request") {
        Kind::Opened
    } else {
        return None;
    };
    let input: serde_json::Value = serde_json::from_str(&tool.input).unwrap_or_default();
    let input = if input["__desktop"].is_object() {
        input.get("rawInput").cloned().unwrap_or_default()
    } else {
        input
    };
    let number = input["pull_number"]
        .as_u64()
        .or_else(|| input["pullNumber"].as_u64());
    let named = match (input["owner"].as_str(), input["repo"].as_str(), number) {
        (Some(owner), Some(repo), Some(n)) => Some(format!("{owner}/{repo}#{n}")),
        _ => None,
    };
    let pr = pull_request(&tool.output).or(named)?;
    Some(Found {
        kind,
        reference: review_reference(kind, pr, &tool.id),
        amend: false,
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
    let Ok(out) = crate::git::git(
        cwd,
        &[
            "show",
            "--numstat",
            "--no-renames",
            "--format=%H %ct",
            &found.reference,
            "--",
        ],
    ) else {
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
    let (repo, number) = pull_request
        .split_once('#')
        .ok_or("Not a pull request")?;
    let binary = crate::scm::shell::resolve_gui_binary("gh").ok_or("GitHub CLI not found")?;
    let mut cmd = std::process::Command::new(binary);
    cmd.args(["pr", "view", number, "--repo", repo, "--json", "state,mergedAt"])
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
            .map_or_else(crate::time::now_ms, |at| at.timestamp_millis().max(0) as u64),
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
                amend: false
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
        let review = run("gh pr review 42 --approve", "✓ Approved pull request acme/app#42");
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
                amend: false
            }]
        );
        tool.name = "mcp__github__create_pull_request".into();
        tool.output = r#"{"html_url":"https://github.com/acme/app/pull/8"}"#.into();
        assert_eq!(found(&tool)[0].reference, "acme/app#8");
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
        };
        let resolved = resolve(&found, 3, dir.path(), 0);
        assert_eq!(resolved.reference, id);
        assert_eq!((resolved.additions, resolved.deletions), (2, 0));
        assert!(resolved.at > 0);
        // A commit the repository lost still counts.
        let lost = resolve(&Found { reference: "deadbee".into(), ..found }, 3, dir.path(), 5);
        assert_eq!((lost.reference.as_str(), lost.at), ("deadbee", 5));
    }
}
