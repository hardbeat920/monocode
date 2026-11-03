//! What an agent's Stage, Commit or Push turn did, read back from the Git
//! commands it ran and their output. Nothing here runs Git: commits are
//! immutable, so a caller can look one up later by the hash found here.
use std::sync::LazyLock;

use regex::Regex;

/// One shell command of the turn and what it printed.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    pub command: &'a str,
    pub output: &'a str,
    pub ok: bool,
}

/// The step a command belongs to, for the live status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Review,
    Stage,
    Commit,
    Push,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub branch: String,
    pub files: Option<u64>,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub path: String,
    /// Changed lines from `--stat`; `None` for binary files or bare `git add` paths.
    pub changes: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Push {
    pub ok: bool,
    /// Nothing new reached the remote.
    pub up_to_date: bool,
    pub branch: Option<String>,
    /// `owner/repo` for hosted remotes.
    pub repo: Option<String>,
    /// The repository's web page, for hosted remotes.
    pub web: Option<String>,
    /// The "create a pull request" link the host printed.
    pub pull_request: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub commits: Vec<Commit>,
    pub staged: Vec<StagedFile>,
    /// Files, insertions and deletions from the staged `--stat` summary.
    pub staged_totals: Option<(u64, u64, u64)>,
    pub push: Option<Push>,
}

/// The furthest step `command` takes.
pub fn step(command: &str) -> Step {
    let mut step = Step::Review;
    for (sub, args) in invocations(command) {
        let next = match sub.as_str() {
            "push" => Step::Push,
            "commit" => Step::Commit,
            "add" | "stage" => Step::Stage,
            "apply" if args.iter().any(|a| a == "--cached" || a == "--index") => Step::Stage,
            _ => continue,
        };
        step = step.max_with(next);
    }
    step
}

impl Step {
    fn rank(self) -> u8 {
        match self {
            Self::Review => 0,
            Self::Stage => 1,
            Self::Commit => 2,
            Self::Push => 3,
        }
    }
    fn max_with(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

pub fn report(runs: &[Run]) -> Report {
    let mut report = Report::default();
    let mut added = Vec::new();
    for run in runs {
        let calls = invocations(run.command);
        let has = |name: &str| calls.iter().any(|(sub, _)| sub == name);
        if has("commit") {
            for commit in commits(run.output) {
                if !report.commits.iter().any(|c| c.hash == commit.hash) {
                    report.commits.push(commit);
                }
            }
        }
        if run.ok
            && calls.iter().any(|(sub, args)| {
                sub == "diff"
                    && args.iter().any(|a| a == "--cached" || a == "--staged")
                    && args
                        .iter()
                        .any(|a| a == "--stat" || a.starts_with("--stat="))
            })
        {
            let (files, totals) = stat(run.output);
            if !files.is_empty() {
                report.staged = files;
                report.staged_totals = totals;
            }
        }
        if run.ok {
            for (sub, args) in &calls {
                if sub == "add" || sub == "stage" {
                    added.extend(paths(args));
                }
            }
        }
        if has("push") {
            report.push = Some(push(run));
        }
    }
    if report.staged.is_empty() {
        for path in added {
            if !report.staged.iter().any(|f| f.path == path) {
                report.staged.push(StagedFile {
                    path,
                    changes: None,
                });
            }
        }
    }
    report
}

/// Each `git <subcommand> <args…>` in a shell command, global options skipped.
fn invocations(command: &str) -> Vec<(String, Vec<String>)> {
    let tokens = shlex::split(command)
        .unwrap_or_else(|| command.split_whitespace().map(str::to_owned).collect());
    let mut calls = Vec::new();
    let mut at = 0;
    // Only a command's first word runs Git, not `echo git push`.
    let mut starts = true;
    while at < tokens.len() {
        let token = tokens[at].as_str();
        at += 1;
        let first = std::mem::replace(
            &mut starts,
            matches!(token, "&&" | "||" | ";" | "|") || token.ends_with(';'),
        );
        if !first || (token != "git" && !token.ends_with("/git")) {
            continue;
        }
        while at < tokens.len() && tokens[at].starts_with('-') {
            let option = tokens[at].as_str();
            at += if matches!(option, "-C" | "-c") { 2 } else { 1 };
        }
        let Some(sub) = tokens.get(at) else {
            break;
        };
        let sub = sub.clone();
        at += 1;
        let mut args = Vec::new();
        while at < tokens.len() {
            let token = tokens[at].as_str();
            if matches!(token, "&&" | "||" | ";" | "|") {
                break;
            }
            at += 1;
            if let Some(last) = token.strip_suffix(';') {
                if !last.is_empty() {
                    args.push(last.to_owned());
                }
                break;
            }
            args.push(token.to_owned());
        }
        starts = true;
        calls.push((sub, args));
    }
    calls
}

/// Explicit paths of a `git add`, without its options.
fn paths(args: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut literal = false;
    for arg in args {
        if !literal && arg == "--" {
            literal = true;
        } else if literal || !arg.starts_with('-') {
            // Repository-wide forms name nothing in particular.
            if arg != "." && arg != ":/" {
                paths.push(arg.clone());
            }
        }
    }
    paths
}

static COMMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\[(.+?)(?: \(root-commit\))? ([0-9a-f]{7,40})\] (.+)$").unwrap()
});
static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?")
        .unwrap()
});
static STAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\S.*?)\s+\|\s+(?:(\d+)(?:\s+[+-]*)?|Bin\b.*)$").unwrap());
static PULL_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://\S+?/(?:pull/new|merge_requests/new|pull-requests/new|compare)/\S+")
        .unwrap()
});
static PUSHED_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s+.*?\S+ -> (\S+)").unwrap());

fn totals(text: &str) -> Option<(u64, u64, u64)> {
    let summary = SUMMARY.captures(text)?;
    let number = |ix: usize| {
        summary
            .get(ix)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0)
    };
    Some((number(1), number(2), number(3)))
}

fn commits(output: &str) -> Vec<Commit> {
    let found: Vec<_> = COMMIT.captures_iter(output).collect();
    found
        .iter()
        .enumerate()
        .map(|(ix, captures)| {
            let whole = captures.get(0).unwrap();
            let rest = &output[whole.end()..];
            let rest = found.get(ix + 1).map_or(rest, |next| {
                &output[whole.end()..next.get(0).unwrap().start()]
            });
            let (files, additions, deletions) = match totals(rest) {
                Some((files, additions, deletions)) => (Some(files), additions, deletions),
                None => (None, 0, 0),
            };
            Commit {
                branch: captures[1].to_owned(),
                hash: captures[2].to_owned(),
                subject: captures[3].trim().to_owned(),
                files,
                additions,
                deletions,
            }
        })
        .collect()
}

fn stat(output: &str) -> (Vec<StagedFile>, Option<(u64, u64, u64)>) {
    let files = output
        .lines()
        .filter_map(|line| STAT.captures(line))
        .map(|captures| StagedFile {
            path: captures[1].to_owned(),
            changes: captures.get(2).and_then(|m| m.as_str().parse().ok()),
        })
        .collect();
    (files, totals(output))
}

fn push(run: &Run) -> Push {
    let output = run.output;
    let rejected = output.contains("[rejected]")
        || output.contains("[remote rejected]")
        || output.contains("error: failed to push");
    let remote = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("To "))
        .map(str::trim);
    let hosted = remote.and_then(web_url);
    Push {
        ok: run.ok && !rejected,
        up_to_date: output.contains("Everything up-to-date"),
        branch: PUSHED_REF
            .captures(output)
            .map(|captures| captures[1].to_owned()),
        repo: hosted.as_ref().map(|(_, repo)| repo.clone()),
        web: hosted.map(|(web, _)| web),
        pull_request: PULL_REQUEST
            .find(output)
            .map(|m| m.as_str().trim_end_matches(['.', ',']).to_owned()),
    }
}

/// A hosted remote's web page and `owner/repo`, from its fetch URL.
fn web_url(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim().trim_end_matches('/');
    let remote = remote.strip_suffix(".git").unwrap_or(remote);
    let (host, path) = if let Some(rest) = remote
        .strip_prefix("https://")
        .or_else(|| remote.strip_prefix("http://"))
        .or_else(|| remote.strip_prefix("ssh://"))
    {
        let (host, path) = rest.split_once('/')?;
        let host = host.rsplit('@').next()?;
        (host.split(':').next()?, path)
    } else {
        let (user_host, path) = remote.split_once(':')?;
        if user_host.contains('/') {
            return None;
        }
        (user_host.rsplit('@').next()?, path)
    };
    let path = path.trim_matches('/');
    if host.is_empty() || !host.contains('.') || path.split('/').count() < 2 {
        return None;
    }
    Some((format!("https://{host}/{path}"), path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<'a>(command: &'a str, output: &'a str) -> Run<'a> {
        Run {
            command,
            output,
            ok: true,
        }
    }

    #[test]
    fn steps_follow_the_furthest_git_command() {
        assert_eq!(step("git status --short"), Step::Review);
        assert_eq!(step("git -C repo add -- a.rs"), Step::Stage);
        assert_eq!(step("git apply --cached patch.diff"), Step::Stage);
        assert_eq!(
            step("git add a.rs && git commit -m \"Fix it\" -- a.rs"),
            Step::Commit
        );
        assert_eq!(step("git commit -m x; git push -u origin main"), Step::Push);
        assert_eq!(step("echo git push"), Step::Review);
    }

    #[test]
    fn reads_commits_staged_stats_and_pushes() {
        let report = report(&[
            run(
                "git add -- src/a.rs 'src/b c.rs'",
                "",
            ),
            run(
                "git diff --cached --stat",
                " src/a.rs     | 56 ++++++++++++++++++++------\n src/b c.rs   |  2 +-\n logo.png     | Bin 0 -> 120 bytes\n 3 files changed, 45 insertions(+), 13 deletions(-)\n",
            ),
        ]);
        assert_eq!(
            report.staged,
            [
                StagedFile {
                    path: "src/a.rs".into(),
                    changes: Some(56)
                },
                StagedFile {
                    path: "src/b c.rs".into(),
                    changes: Some(2)
                },
                StagedFile {
                    path: "logo.png".into(),
                    changes: None
                },
            ]
        );
        assert_eq!(report.staged_totals, Some((3, 45, 13)));

        let bare = super::report(&[run("git add -- src/a.rs 'src/b c.rs' && git status", "")]);
        assert_eq!(
            bare.staged
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["src/a.rs", "src/b c.rs"]
        );

        let report = super::report(&[
            run(
                "git commit -m \"Center the jump button\" -- a.rs b.rs",
                "[mc/migrate 3bb0c42] Center the jump button\n 2 files changed, 44 insertions(+), 12 deletions(-)\n",
            ),
            run(
                "git push",
                "To github.com:chatbotkit/monocode.git\n   43b1107..3bb0c42  mc/migrate -> mc/migrate\n",
            ),
        ]);
        assert_eq!(
            report.commits,
            [Commit {
                hash: "3bb0c42".into(),
                subject: "Center the jump button".into(),
                branch: "mc/migrate".into(),
                files: Some(2),
                additions: 44,
                deletions: 12,
            }]
        );
        let push = report.push.unwrap();
        assert!(push.ok && !push.up_to_date);
        assert_eq!(push.branch.as_deref(), Some("mc/migrate"));
        assert_eq!(push.repo.as_deref(), Some("chatbotkit/monocode"));
        assert_eq!(
            push.web.as_deref(),
            Some("https://github.com/chatbotkit/monocode")
        );
    }

    #[test]
    fn new_branches_offer_their_pull_request_and_rejections_fail() {
        let report = report(&[run(
            "git push -u origin feature",
            "remote: Create a pull request for 'feature' on GitHub by visiting:\nremote:      https://github.com/o/r/pull/new/feature\nTo https://github.com/o/r.git\n * [new branch]      feature -> feature\nbranch 'feature' set up to track 'origin/feature'.\n",
        )]);
        let push = report.push.unwrap();
        assert!(push.ok);
        assert_eq!(push.branch.as_deref(), Some("feature"));
        assert_eq!(
            push.pull_request.as_deref(),
            Some("https://github.com/o/r/pull/new/feature")
        );

        let rejected = super::report(&[Run {
            command: "git push",
            output: "To github.com:o/r.git\n ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs\n",
            ok: false,
        }]);
        assert!(!rejected.push.unwrap().ok);
    }

    #[test]
    fn hosted_remotes_have_web_pages() {
        assert_eq!(
            web_url("git@github.com:o/r.git"),
            Some(("https://github.com/o/r".into(), "o/r".into()))
        );
        assert_eq!(
            web_url("ssh://git@gitlab.com:2222/group/sub/r.git"),
            Some((
                "https://gitlab.com/group/sub/r".into(),
                "group/sub/r".into()
            ))
        );
        assert_eq!(
            web_url("https://token@github.com/o/r"),
            Some(("https://github.com/o/r".into(), "o/r".into()))
        );
        assert_eq!(web_url("/srv/git/r.git"), None);
        assert_eq!(web_url("../remote.git"), None);
    }
}
