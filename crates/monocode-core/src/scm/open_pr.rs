//! Open a pull request for a session's branch: the preconditions, the draft
//! title and body, the push and `gh pr create`. Every `git` and `gh` call goes
//! through [`PrTools`], so the rules here are tested with a fake and nothing in
//! the tests can touch a real remote.
use super::*;

/// How this module runs `git` and `gh`. Err carries the message to show.
pub trait PrTools {
    /// `git <args>` in `root`; trimmed stdout.
    fn git(&self, root: &Path, args: &[&str]) -> Result<String, String>;
    /// `gh <args>` in `root`; trimmed stdout, which may be empty.
    fn gh(&self, root: &Path, args: &[&str]) -> Result<String, String>;
}

/// The installed `git` and `gh`, run the way the rest of the Git panes do.
pub struct SystemPrTools;

impl PrTools for SystemPrTools {
    fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        git_capture(root, args)
    }
    fn gh(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        gh_exec(root, args)
    }
}

/// What `/pr` proposes: the pull request as it would be created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrDraft {
    pub branch: String,
    /// The default branch the pull request targets.
    pub base: String,
    pub remote: String,
    pub title: String,
    pub body: String,
    /// "abc1234 Subject" for each commit the branch adds, oldest first.
    pub commits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrPlan {
    /// The branch already has an open pull request.
    Existing(Pr),
    New(PrDraft),
}

/// The title and body the user settled on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRequest {
    pub title: String,
    pub body: String,
    /// Open it as a GitHub draft pull request.
    pub draft: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedPr {
    pub url: String,
    pub branch: String,
    /// True when the branch already had an open pull request and nothing
    /// was pushed or created.
    pub existing: bool,
}

/// Check that `cwd`'s branch can have a pull request and propose one, or
/// return the open pull request it already has. `title` and `summary` come
/// from the session; either may be empty.
pub fn prepare_pull_request(cwd: &Path, title: &str, summary: &str) -> Result<PrPlan, String> {
    local_only(cwd)?;
    prepare_with(&SystemPrTools, &expand_home(cwd), title, summary)
}

/// Push the branch with upstream tracking and create the pull request, after
/// repeating the checks. An existing open pull request is returned as is.
pub fn open_pull_request(cwd: &Path, request: &PrRequest) -> Result<OpenedPr, String> {
    local_only(cwd)?;
    open_with(&SystemPrTools, &expand_home(cwd), request)
}

fn local_only(cwd: &Path) -> Result<(), String> {
    if remote_workspace::is_remote(cwd) {
        return Err("Pull requests can only be opened from a local checkout.".into());
    }
    Ok(())
}

enum Checked {
    Existing(Pr),
    Ready(Ready),
}

struct Ready {
    branch: String,
    base: String,
    remote: String,
    commits: Vec<String>,
}

fn prepare_with(
    tools: &dyn PrTools,
    root: &Path,
    title: &str,
    summary: &str,
) -> Result<PrPlan, String> {
    Ok(match check(tools, root)? {
        Checked::Existing(pr) => PrPlan::Existing(pr),
        Checked::Ready(ready) => {
            let title = draft_title(title, &ready.branch, &ready.commits);
            let body = compose_pr_body(summary, &ready.commits);
            PrPlan::New(PrDraft {
                branch: ready.branch,
                base: ready.base,
                remote: ready.remote,
                title,
                body,
                commits: ready.commits,
            })
        }
    })
}

fn open_with(tools: &dyn PrTools, root: &Path, request: &PrRequest) -> Result<OpenedPr, String> {
    let title = request.title.trim();
    if title.is_empty() {
        return Err("Pull request title cannot be empty".into());
    }
    let ready = match check(tools, root)? {
        Checked::Existing(pr) => {
            let branch = git_branch_with(tools, root)?;
            return Ok(OpenedPr {
                url: pr.url,
                branch,
                existing: true,
            });
        }
        Checked::Ready(ready) => ready,
    };
    tools
        .git(root, &["push", "--set-upstream", &ready.remote, "HEAD"])
        .map_err(|error| {
            format!(
                "Could not push `{}` to {}: {error}",
                ready.branch, ready.remote
            )
        })?;
    let mut args = vec![
        "pr",
        "create",
        "--title",
        title,
        "--body",
        request.body.trim(),
        "--base",
        &ready.base,
        "--head",
        &ready.branch,
    ];
    if request.draft {
        args.push("--draft");
    }
    let url = tools
        .gh(root, &args)
        .and_then(|output| pr_url_from_output(&output))
        .map_err(|error| {
            format!(
                "Pushed `{}`, but creating the pull request failed: {error}",
                ready.branch
            )
        })?;
    Ok(OpenedPr {
        url,
        branch: ready.branch,
        existing: false,
    })
}

fn git_branch_with(tools: &dyn PrTools, root: &Path) -> Result<String, String> {
    tools
        .git(root, &["symbolic-ref", "--short", "HEAD"])
        .ok()
        .filter(|branch| !branch.is_empty() && branch != "HEAD")
        .ok_or_else(|| "HEAD is detached. Check out a branch before opening a pull request.".into())
}

/// Every precondition, cheapest first, with the message that says what to do.
fn check(tools: &dyn PrTools, root: &Path) -> Result<Checked, String> {
    if tools
        .git(root, &["rev-parse", "--is-inside-work-tree"])
        .as_deref()
        != Ok("true")
    {
        return Err("This folder is not a git repository.".into());
    }
    let branch = git_branch_with(tools, root)?;
    let remote = remote_with(tools, root)
        .ok_or_else(|| "This repository has no git remote to push to.".to_string())?;
    let base = default_branch_with(tools, root, &remote).ok_or_else(|| {
        "Could not work out the default branch. Set it with \
         `git remote set-head origin --auto` and try again."
            .to_string()
    })?;
    if branch == base {
        return Err(format!(
            "You are on the default branch (`{base}`). Switch to a feature branch before \
             opening a pull request."
        ));
    }
    if tools.gh(root, &["--version"]).is_err() {
        return Err(
            "GitHub CLI (`gh`) is not installed. Install it from https://cli.github.com, \
             then run `gh auth login`."
                .into(),
        );
    }
    if tools.gh(root, &["auth", "status"]).is_err() {
        return Err(
            "GitHub CLI (`gh`) is not signed in. Run `gh auth login` in a terminal, \
             then try again."
                .into(),
        );
    }
    let listed = tools
        .gh(
            root,
            &[
                "pr",
                "list",
                "--head",
                &branch,
                "--state",
                "open",
                "--json",
                "number,title,url,state",
                "--limit",
                "5",
            ],
        )
        .map_err(|error| format!("Could not look up pull requests for `{branch}`: {error}"))?;
    if let Some(pr) = parse_gh_pr_list(&listed).filter(|pr| pr.state == "open") {
        return Ok(Checked::Existing(pr));
    }
    let status = tools
        .git(root, &["status", "--porcelain"])
        .map_err(|error| format!("Could not read the git status: {error}"))?;
    if let Some(message) = uncommitted_message(&status) {
        return Err(message);
    }
    let base_ref = if tools
        .git(
            root,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/{remote}/{base}"),
            ],
        )
        .is_ok()
    {
        format!("{remote}/{base}")
    } else {
        base.clone()
    };
    let log = tools
        .git(
            root,
            &[
                "log",
                "--reverse",
                "--format=%h %s",
                &format!("{base_ref}..HEAD"),
            ],
        )
        .map_err(|error| format!("Could not list the commits on `{branch}`: {error}"))?;
    let commits: Vec<String> = log
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if commits.is_empty() {
        return Err(format!(
            "`{branch}` has no commits ahead of `{base_ref}`, so there is nothing to open a \
             pull request for. Run /commit first."
        ));
    }
    Ok(Checked::Ready(Ready {
        branch,
        base,
        remote,
        commits,
    }))
}

/// The error for a dirty tree, naming a few files and the way out.
fn uncommitted_message(porcelain: &str) -> Option<String> {
    // `git status --porcelain` is "XY path"; the capture trimmed the first
    // line's leading space, so split on the first gap instead.
    let paths: Vec<&str> = porcelain
        .lines()
        .filter_map(|line| line.trim_start().split_once(char::is_whitespace))
        .map(|(_, path)| path.trim())
        .collect();
    if paths.is_empty() {
        return None;
    }
    const SHOWN: usize = 5;
    let mut list = paths[..paths.len().min(SHOWN)].join(", ");
    if paths.len() > SHOWN {
        list.push_str(&format!(", and {} more", paths.len() - SHOWN));
    }
    Some(format!(
        "You have uncommitted changes ({list}). Run /commit first, then /pr."
    ))
}

fn remote_with(tools: &dyn PrTools, root: &Path) -> Option<String> {
    let remotes = tools.git(root, &["remote"]).ok()?;
    let mut names = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let first = names.next()?.to_owned();
    if first == "origin" || names.any(|name| name == "origin") {
        return Some("origin".into());
    }
    Some(first)
}

/// The remote's HEAD, then `main` or `master` on the remote, then locally.
fn default_branch_with(tools: &dyn PrTools, root: &Path, remote: &str) -> Option<String> {
    let exists = |reference: String| {
        tools
            .git(root, &["show-ref", "--verify", "--quiet", &reference])
            .is_ok()
    };
    let head = tools
        .git(
            root,
            &[
                "symbolic-ref",
                "--short",
                &format!("refs/remotes/{remote}/HEAD"),
            ],
        )
        .ok()
        .filter(|head| !head.is_empty());
    if let Some(head) = head {
        return Some(
            head.strip_prefix(&format!("{remote}/"))
                .unwrap_or(&head)
                .to_owned(),
        );
    }
    ["main", "master"]
        .into_iter()
        .find(|name| exists(format!("refs/remotes/{remote}/{name}")))
        .or_else(|| {
            ["main", "master"]
                .into_iter()
                .find(|name| exists(format!("refs/heads/{name}")))
        })
        .map(str::to_owned)
}

/// The session's title when it has a real one, else the lone commit's
/// subject, else the branch name as words.
fn draft_title(session_title: &str, branch: &str, commits: &[String]) -> String {
    let title = session_title.trim();
    if !title.is_empty() {
        return title.to_owned();
    }
    if let [only] = commits {
        if let Some((_, subject)) = only.split_once(' ') {
            return subject.trim().to_owned();
        }
    }
    let words = branch
        .rsplit('/')
        .next()
        .unwrap_or(branch)
        .replace(['-', '_'], " ");
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => format!("Update {branch}"),
    }
}

const SUMMARY_LIMIT: usize = 600;
const COMMITS_SHOWN: usize = 30;

/// A short summary of the session, then the commits on the branch.
pub fn compose_pr_body(summary: &str, commits: &[String]) -> String {
    let mut body = String::new();
    let summary = clip_summary(summary);
    if !summary.is_empty() {
        body.push_str(&summary);
        body.push_str("\n\n");
    }
    if !commits.is_empty() {
        body.push_str("## Commits\n\n");
        for commit in commits.iter().take(COMMITS_SHOWN) {
            body.push_str(&format!("- {commit}\n"));
        }
        if commits.len() > COMMITS_SHOWN {
            body.push_str(&format!("- …and {} more\n", commits.len() - COMMITS_SHOWN));
        }
    }
    body.trim_end().to_owned()
}

/// Whole paragraphs from the start of `text` up to the limit; a first
/// paragraph that is itself too long is cut at a word.
fn clip_summary(text: &str) -> String {
    let mut out = String::new();
    for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let needed = out.chars().count() + paragraph.chars().count() + 2;
        if !out.is_empty() && needed > SUMMARY_LIMIT {
            break;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        if paragraph.chars().count() > SUMMARY_LIMIT {
            let cut: String = paragraph.chars().take(SUMMARY_LIMIT).collect();
            let cut = cut
                .rsplit_once(char::is_whitespace)
                .map_or(&*cut, |(head, _)| head);
            out.push_str(cut.trim_end());
            out.push('…');
            break;
        }
        out.push_str(paragraph);
    }
    out
}

/// What the composer holds for `/pr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrCommand {
    pub draft: bool,
    /// The title and body typed after the command; None for a bare `/pr`,
    /// which proposes them instead.
    pub edited: Option<(String, String)>,
}

/// `rest` is the text after `/pr`: optional `--draft`, then the title on the
/// same or the next line and the body below it.
pub fn parse_pr_command(rest: &str) -> Result<PrCommand, String> {
    let mut draft = false;
    let mut text = rest.trim_start();
    while let Some(flag) = text.starts_with('-').then(|| {
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        &text[..end]
    }) {
        match flag {
            "--draft" | "-d" => draft = true,
            _ => {
                return Err(format!(
                    "Unknown option `{flag}`. Usage: /pr [--draft], then an optional title and \
                     body."
                ));
            }
        }
        text = text[flag.len()..].trim_start_matches([' ', '\t']);
    }
    let text = text.trim();
    if text.is_empty() {
        return Ok(PrCommand {
            draft,
            edited: None,
        });
    }
    let (title, body) = text.split_once('\n').unwrap_or((text, ""));
    Ok(PrCommand {
        draft,
        edited: Some((title.trim().to_owned(), body.trim().to_owned())),
    })
}

/// The composer text that [`parse_pr_command`] reads back as `title` and `body`.
pub fn pr_command_text(draft: bool, title: &str, body: &str) -> String {
    let flag = if draft { " --draft" } else { "" };
    if body.trim().is_empty() {
        format!("/pr{flag}\n{}", title.trim())
    } else {
        format!("/pr{flag}\n{}\n\n{}", title.trim(), body.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers by the longest matching argument prefix and records every call,
    /// so a test can assert that nothing was pushed or created.
    struct Fake {
        answers: Vec<(&'static str, &'static str, Result<String, String>)>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        /// A clean feature branch with two commits, `gh` signed in, no open PR.
        fn ready() -> Self {
            let ok = |text: &str| Ok(text.to_owned());
            Self {
                answers: vec![
                    ("git", "rev-parse --is-inside-work-tree", ok("true")),
                    ("git", "symbolic-ref --short HEAD", ok("feat/open-pr")),
                    ("git", "remote", ok("origin")),
                    (
                        "git",
                        "symbolic-ref --short refs/remotes/origin/HEAD",
                        ok("origin/main"),
                    ),
                    ("gh", "--version", ok("gh version 2.0.0")),
                    ("gh", "auth status", ok("")),
                    ("gh", "pr list", ok("[]")),
                    ("git", "status --porcelain", ok("")),
                    (
                        "git",
                        "show-ref --verify --quiet refs/remotes/origin/main",
                        ok(""),
                    ),
                    (
                        "git",
                        "log --reverse",
                        ok("1111111 Add the thing\n2222222 Fix the thing"),
                    ),
                    ("git", "push --set-upstream origin HEAD", ok("")),
                    (
                        "gh",
                        "pr create",
                        ok("Creating pull request\nhttps://github.com/acme/app/pull/9"),
                    ),
                ],
                calls: RefCell::new(Vec::new()),
            }
        }
        fn answer(
            mut self,
            program: &'static str,
            prefix: &'static str,
            to: Result<&str, &str>,
        ) -> Self {
            let to = to.map(str::to_owned).map_err(str::to_owned);
            self.answers
                .retain(|(p, a, _)| (*p, *a) != (program, prefix));
            self.answers.push((program, prefix, to));
            self
        }
        fn called(&self, program: &str, prefix: &str) -> bool {
            self.calls
                .borrow()
                .iter()
                .any(|call| call.starts_with(&format!("{program} {prefix}")))
        }
        fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
            let line = format!("{program} {}", args.join(" "));
            self.calls.borrow_mut().push(line);
            let joined = args.join(" ");
            self.answers
                .iter()
                .filter(|(p, prefix, _)| *p == program && joined.starts_with(prefix))
                .max_by_key(|(_, prefix, _)| prefix.len())
                .map(|(_, _, answer)| answer.clone())
                .unwrap_or_else(|| Err(format!("unexpected {program} {joined}")))
        }
    }

    impl PrTools for Fake {
        fn git(&self, _: &Path, args: &[&str]) -> Result<String, String> {
            self.run("git", args)
        }
        fn gh(&self, _: &Path, args: &[&str]) -> Result<String, String> {
            self.run("gh", args)
        }
    }

    fn prepare(fake: &Fake) -> Result<PrPlan, String> {
        prepare_with(
            fake,
            Path::new("/repo"),
            "Open PR from a session",
            "Added /pr.",
        )
    }
    fn request(draft: bool) -> PrRequest {
        PrRequest {
            title: " Add /pr ".into(),
            body: "Body\n".into(),
            draft,
        }
    }
    fn open(fake: &Fake, draft: bool) -> Result<OpenedPr, String> {
        open_with(fake, Path::new("/repo"), &request(draft))
    }

    #[test]
    fn a_clean_feature_branch_gets_a_draft_with_summary_and_commits() {
        let fake = Fake::ready();
        let Ok(PrPlan::New(draft)) = prepare(&fake) else {
            panic!("expected a new pull request");
        };
        assert_eq!(draft.title, "Open PR from a session");
        assert_eq!(
            (draft.branch.as_str(), draft.base.as_str()),
            ("feat/open-pr", "main")
        );
        assert_eq!(
            draft.body,
            "Added /pr.\n\n## Commits\n\n- 1111111 Add the thing\n- 2222222 Fix the thing"
        );
        assert!(!fake.called("git", "push"), "preparing never pushes");
        assert!(!fake.called("gh", "pr create"), "preparing never creates");
    }

    #[test]
    fn opening_pushes_with_upstream_then_creates_with_the_edited_text() {
        let fake = Fake::ready();
        let opened = open(&fake, false).unwrap();
        assert_eq!(opened.url, "https://github.com/acme/app/pull/9");
        assert!(!opened.existing);
        let calls = fake.calls.borrow();
        let push = calls
            .iter()
            .position(|c| c == "git push --set-upstream origin HEAD");
        let create = calls.iter().position(|c| c.starts_with("gh pr create"));
        assert!(push < create && push.is_some(), "{calls:?}");
        assert_eq!(
            calls[create.unwrap()],
            "gh pr create --title Add /pr --body Body --base main --head feat/open-pr"
        );
    }

    #[test]
    fn draft_adds_the_flag() {
        let fake = Fake::ready();
        open(&fake, true).unwrap();
        assert!(
            fake.called("gh", "pr create")
                && fake.calls.borrow().last().unwrap().ends_with("--draft")
        );
    }

    #[test]
    fn an_existing_open_pr_is_returned_without_pushing_or_creating() {
        let fake = Fake::ready()
            .answer(
                "gh",
                "pr list",
                Ok(r#"[{"number":4,"title":"T","url":"https://github.com/acme/app/pull/4","state":"OPEN"}]"#),
            )
            // Even a dirty tree does not matter once the pull request exists.
            .answer("git", "status --porcelain", Ok("?? stray.txt"));
        let Ok(PrPlan::Existing(pr)) = prepare(&fake) else {
            panic!("expected the existing pull request");
        };
        assert_eq!(pr.number, 4);
        let opened = open(&fake, false).unwrap();
        assert_eq!(opened.url, "https://github.com/acme/app/pull/4");
        assert!(opened.existing);
        assert!(!fake.called("git", "push") && !fake.called("gh", "pr create"));
    }

    #[test]
    fn a_closed_pr_does_not_count_as_existing() {
        let fake = Fake::ready().answer(
            "gh",
            "pr list",
            Ok(r#"[{"number":4,"title":"T","url":"u","state":"MERGED"}]"#),
        );
        assert!(matches!(prepare(&fake), Ok(PrPlan::New(_))));
    }

    #[test]
    fn each_precondition_has_a_clear_error_and_nothing_is_pushed() {
        let cases: Vec<(Fake, &str)> = vec![
            (
                Fake::ready().answer("git", "rev-parse --is-inside-work-tree", Err("fatal")),
                "not a git repository",
            ),
            (
                Fake::ready().answer("git", "symbolic-ref --short HEAD", Err("fatal")),
                "HEAD is detached",
            ),
            (
                Fake::ready().answer("git", "remote", Ok("")),
                "no git remote",
            ),
            (
                Fake::ready().answer("git", "symbolic-ref --short HEAD", Ok("main")),
                "default branch (`main`)",
            ),
            (
                Fake::ready().answer("gh", "--version", Err("not found")),
                "`gh`) is not installed",
            ),
            (
                Fake::ready().answer("gh", "auth status", Err("not logged in")),
                "gh auth login",
            ),
            (
                Fake::ready().answer("gh", "pr list", Err("HTTP 502")),
                "Could not look up pull requests for `feat/open-pr`: HTTP 502",
            ),
            (
                Fake::ready().answer("git", "status --porcelain", Ok(" M src/a.rs\n?? b.rs")),
                "uncommitted changes (src/a.rs, b.rs). Run /commit first",
            ),
            (
                Fake::ready().answer("git", "log --reverse", Ok("")),
                "no commits ahead of `origin/main`",
            ),
        ];
        for (fake, expected) in cases {
            let error = prepare(&fake).unwrap_err();
            assert!(error.contains(expected), "{error:?} lacks {expected:?}");
            let error = open(&fake, false).unwrap_err();
            assert!(error.contains(expected), "{error:?} lacks {expected:?}");
            assert!(!fake.called("git", "push") && !fake.called("gh", "pr create"));
        }
    }

    #[test]
    fn a_failed_push_stops_before_creating() {
        let fake = Fake::ready().answer("git", "push --set-upstream origin HEAD", Err("rejected"));
        let error = open(&fake, false).unwrap_err();
        assert_eq!(error, "Could not push `feat/open-pr` to origin: rejected");
        assert!(!fake.called("gh", "pr create"));
    }

    #[test]
    fn a_failed_create_says_the_branch_was_pushed() {
        let fake = Fake::ready().answer("gh", "pr create", Err("GraphQL: boom"));
        let error = open(&fake, false).unwrap_err();
        assert!(error.starts_with("Pushed `feat/open-pr`, but"), "{error}");
        assert!(error.contains("boom"));
    }

    #[test]
    fn an_empty_title_is_refused_before_any_call() {
        let fake = Fake::ready();
        let blank = PrRequest {
            title: "  ".into(),
            body: String::new(),
            draft: false,
        };
        assert!(open_with(&fake, Path::new("/repo"), &blank).is_err());
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn the_default_branch_falls_back_to_main_then_master() {
        let fake = Fake::ready()
            .answer(
                "git",
                "symbolic-ref --short refs/remotes/origin/HEAD",
                Err("no head"),
            )
            .answer(
                "git",
                "show-ref --verify --quiet refs/remotes/origin/main",
                Err(""),
            )
            .answer(
                "git",
                "show-ref --verify --quiet refs/remotes/origin/master",
                Ok(""),
            );
        let Ok(PrPlan::New(draft)) = prepare(&fake) else {
            panic!("expected a draft");
        };
        assert_eq!(draft.base, "master");
        let local = Fake::ready()
            .answer(
                "git",
                "symbolic-ref --short refs/remotes/origin/HEAD",
                Err(""),
            )
            .answer("git", "show-ref --verify --quiet refs/remotes", Err(""))
            .answer("git", "show-ref --verify --quiet refs/heads/main", Ok(""));
        assert_eq!(
            default_branch_with(&local, Path::new("/"), "origin").as_deref(),
            Some("main")
        );
    }

    #[test]
    fn origin_is_preferred_among_remotes() {
        let fake = Fake::ready().answer("git", "remote", Ok("fork\norigin"));
        assert_eq!(
            remote_with(&fake, Path::new("/")).as_deref(),
            Some("origin")
        );
        let fake = Fake::ready().answer("git", "remote", Ok("fork\nother"));
        assert_eq!(remote_with(&fake, Path::new("/")).as_deref(), Some("fork"));
    }

    #[test]
    fn titles_fall_back_to_the_commit_then_the_branch() {
        let commits = |list: &[&str]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        assert_eq!(draft_title("  Fix it ", "b", &[]), "Fix it");
        assert_eq!(
            draft_title("", "feat/x", &commits(&["abc1234 Add the thing"])),
            "Add the thing"
        );
        assert_eq!(
            draft_title("", "feat/open-pr_flow", &commits(&["a One", "b Two"])),
            "Open pr flow"
        );
    }

    #[test]
    fn bodies_clip_long_summaries_and_cap_the_commit_list() {
        assert_eq!(compose_pr_body("", &[]), "");
        assert_eq!(
            compose_pr_body("  ", &["abc One".into()]),
            "## Commits\n\n- abc One"
        );
        let long = "word ".repeat(300);
        let clipped = clip_summary(&long);
        assert!(clipped.ends_with("word…") && clipped.chars().count() <= SUMMARY_LIMIT + 1);
        let paragraphs = format!("{}\n\n{}", "a".repeat(400), "b".repeat(400));
        assert_eq!(clip_summary(&paragraphs), "a".repeat(400));
        let many: Vec<String> = (0..32).map(|n| format!("h{n} s")).collect();
        let body = compose_pr_body("", &many);
        assert!(body.ends_with("- …and 2 more") && !body.contains("h30"));
    }

    #[test]
    fn the_command_reads_flags_title_and_body() {
        let edited = |draft, title: &str, body: &str| PrCommand {
            draft,
            edited: Some((title.into(), body.into())),
        };
        assert_eq!(
            parse_pr_command("").unwrap(),
            PrCommand {
                draft: false,
                edited: None
            }
        );
        assert_eq!(
            parse_pr_command("--draft").unwrap(),
            PrCommand {
                draft: true,
                edited: None
            }
        );
        assert_eq!(
            parse_pr_command("Fix it").unwrap(),
            edited(false, "Fix it", "")
        );
        assert_eq!(
            parse_pr_command("-d --draft Fix it\n\nWhy\nmore").unwrap(),
            edited(true, "Fix it", "Why\nmore")
        );
        assert_eq!(
            parse_pr_command("--draft\nFix it\n\n## Commits\n- a").unwrap(),
            edited(true, "Fix it", "## Commits\n- a")
        );
        let error = parse_pr_command("--force x").unwrap_err();
        assert!(error.contains("`--force`"), "{error}");
    }

    #[test]
    fn the_proposed_text_reads_back_unchanged() {
        for draft in [false, true] {
            for body in ["", "Body\n\n## Commits\n\n- a b"] {
                let text = pr_command_text(draft, "A title", body);
                let rest = text.strip_prefix("/pr").unwrap();
                assert_eq!(
                    parse_pr_command(rest).unwrap(),
                    PrCommand {
                        draft,
                        edited: Some(("A title".into(), body.into())),
                    },
                    "{text:?}"
                );
            }
        }
    }

    #[test]
    fn urls_come_from_the_last_line_of_gh_output() {
        assert_eq!(
            pr_url_from_output("see https://x.y\nhttps://github.com/a/b/pull/1\n").unwrap(),
            "https://github.com/a/b/pull/1"
        );
        assert_eq!(
            pr_url_from_output("").unwrap_err(),
            "gh returned no pull request URL"
        );
        assert_eq!(pr_url_from_output("nope").unwrap_err(), "nope");
    }
}
