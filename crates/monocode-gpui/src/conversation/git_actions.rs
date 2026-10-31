//! The commit pill above the composer. It shows while this idle session has
//! edited files that are still uncommitted; one click asks the session's own agent to stage or
//! commit exactly what it changed, leaving other agents' and the user's work
//! alone. The agent, not MonoCode's edit log, decides which changes are its.
//! Once a step lands the pill moves on: staged work offers Commit, and an
//! unpushed commit offers Push.
use super::*;
use crate::tooltip::Tip;

/// Read older pages without installing their transcript payload in the UI.
pub(super) fn restored_session_edits(
    store: &monocode_core::store::Store,
    session: i64,
    rows: &[monocode_core::transcript::Row],
) -> Result<Vec<String>, String> {
    let mut paths = crate::runtime::edited_paths(rows);
    if let Some(before) = store.history_before(session)? {
        for path in crate::runtime::edited_paths(&store.load_before(session, before, true)?.rows) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GitAction {
    Stage,
    Commit,
    CommitPush,
    /// The follow-up to Commit, never offered in the menu.
    Push,
}

/// The action the pill offers: the last one picked, across sessions.
#[derive(Clone, Copy)]
struct LastGitAction(GitAction);
impl gpui::Global for LastGitAction {}

impl GitAction {
    pub(crate) const ALL: [Self; 3] = [Self::Stage, Self::Commit, Self::CommitPush];
    const EVERY: [Self; 4] = [Self::Stage, Self::Commit, Self::CommitPush, Self::Push];
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Stage => "stage",
            Self::Commit => "commit",
            Self::CommitPush => "commit-push",
            Self::Push => "push",
        }
    }
    fn from_id(id: &str) -> Option<Self> {
        Self::EVERY.into_iter().find(|action| action.id() == id)
    }
    /// The action a sent pill message asked for.
    pub(crate) fn from_message(text: &str) -> Option<Self> {
        Self::EVERY
            .into_iter()
            .find(|action| action.message() == text.trim())
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stage => "Stage",
            Self::Commit => "Commit",
            Self::CommitPush => "Commit & push",
            Self::Push => "Push",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Stage => "Stage only this session's changes",
            Self::Commit => "Stage and commit with a written message",
            Self::CommitPush => "Commit, then push the branch",
            Self::Push => "Push this session's commits",
        }
    }
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Stage => "plus",
            Self::Commit => "git-commit",
            Self::CommitPush | Self::Push => "cloud-upload",
        }
    }
    /// The icon a sent action's message carries in the transcript.
    pub(crate) fn message_icon(text: &str) -> Option<&'static str> {
        Self::from_message(text).map(Self::icon)
    }
    /// What the transcript shows for the turn.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Stage => "Stage your changes",
            Self::Commit => "Commit your changes",
            Self::CommitPush => "Commit and push your changes",
            Self::Push => "Push your commits",
        }
    }
    /// What the agent receives instead.
    pub(crate) fn prompt(self) -> String {
        if self == Self::Push {
            return "Push the current branch with git.\n\nDo not stage, commit, amend or change anything else. Use `git push`, or `git push -u origin <branch>` when the branch has no upstream. Never force-push. If the push is rejected, stop and report it; do not pull, rebase or merge.\n\nFinish with a short reply saying where it was pushed.".into();
        }
        let task = match self {
            Self::Stage => "Stage the changes you made in this session with git. Do not commit.",
            Self::Commit => "Commit the changes you made in this session with git.",
            Self::CommitPush | Self::Push => {
                "Commit the changes you made in this session with git, then push the branch."
            }
        };
        let mut rules = vec![
            "Work out which files you created, edited, renamed or deleted in this session from this conversation, then check them against `git status` and `git diff`.",
            "Name every path explicitly. Never use `git add .`, `git add -A`, `git add -u`, `git commit -a` or any other repository-wide form.",
            "If a file also holds changes that are not yours, take only your own hunks when you can separate them reliably (for example with `git apply --cached`). Otherwise leave that file out and say why.",
            "Leave everything else as it is: do not unstage, stash, reset, check out, or discard any other change.",
        ];
        if self != Self::Stage {
            rules.extend([
                "Commit only your paths, listing them explicitly (`git commit -m \"…\" -- <paths>`), so anything someone else staged stays out of the commit. If you staged partial hunks, check that `git diff --cached` holds nothing but your changes and commit the index instead.",
                "Write the message from what you did and why: a short summary line, then a body only if the change needs explaining. Follow the style of `git log`.",
                "Do not amend, and do not skip hooks. If a hook fails because of your change, fix it and retry; otherwise stop and report the failure.",
            ]);
        }
        rules.push(match self {
            Self::Stage | Self::Commit => "Do not push.",
            Self::CommitPush | Self::Push => {
                "Then push the current branch (`git push`, or `git push -u origin <branch>` when it has no upstream). Never force-push. If the push is rejected, stop and report it; do not pull, rebase or merge."
            }
        });
        let report = match self {
            Self::Stage => "the output of `git diff --cached --stat`",
            Self::Commit => "the commit's short hash and summary line, and the files it includes",
            Self::CommitPush | Self::Push => {
                "the commit's short hash and summary line, the files it includes, and where it was pushed"
            }
        };
        format!(
            "{task}\n\nOther agents and the user may be working in this same checkout, so the working tree can hold changes that are not yours. Include only what you changed:\n\n{}\n\nIf you changed nothing in this session, say so and stop. Otherwise finish with a short reply listing {report}, plus any changes you deliberately left out.",
            rules
                .iter()
                .map(|rule| format!("- {rule}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

impl Conversation {
    pub(super) fn remember_session_edits(&mut self, paths: Vec<String>) {
        for path in paths {
            let path = monocode_core::files::document::resolve_path(
                Path::new(&path),
                Path::new(self.execution_cwd()),
            )
            .to_string_lossy()
            .into_owned();
            if !self.session_edits.contains(&path) {
                self.session_edits.push(path);
            }
        }
    }
    pub(super) fn collect_session_edits(&mut self, cx: &mut Context<Self>) {
        let paths = crate::runtime::edited_paths(self.transcript.read(cx).source_rows());
        self.remember_session_edits(paths);
        if self.review.read(cx).session().1 != self.execution_cwd() {
            self.session_git = Default::default();
            self.session_unpushed = false;
        }
        self.refresh_git_actions(cx);
    }
    /// Recheck only this conversation's files, also when Git changes outside
    /// a turn. A late read must never publish into another working copy.
    pub(crate) fn refresh_git_actions(&mut self, cx: &mut Context<Self>) {
        self.session_edits_generation = self.session_edits_generation.wrapping_add(1);
        let generation = self.session_edits_generation;
        if !self.history_loaded() || self.busy || self.is_remote() || self.settings.mono.is_some() {
            return;
        }
        if self.session_edits.is_empty() || self.settings.worktree_removed {
            if self.session_git.uncommitted() || self.session_unpushed {
                self.session_git = Default::default();
                self.session_unpushed = false;
                cx.notify();
            }
            return;
        }
        let cwd = self.execution_cwd().to_owned();
        let work_cwd = cwd.clone();
        let paths = self.session_edits.clone();
        let commit =
            crate::transcript::git_turns::latest_commit(self.transcript.read(cx).source_rows());
        let work = cx.background_executor().spawn(async move {
            let cwd = Path::new(&work_cwd);
            let changes = monocode_core::git::session_changes(cwd, &paths);
            let unpushed = commit.is_some_and(|commit| monocode_core::git::unpushed(cwd, &commit));
            (changes, unpushed)
        });
        cx.spawn(async move |this, cx| {
            let (changes, unpushed) = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.session_edits_generation == generation
                    && this.execution_cwd() == cwd
                    && (this.session_git, this.session_unpushed) != (changes, unpushed)
                {
                    this.session_git = changes;
                    this.session_unpushed = unpushed;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    /// The next steps for the session's work, the pill's default first:
    /// unstaged work offers every action, staged work skips Stage, and a
    /// committed but unpushed session offers only Push.
    fn git_offers(&self, cx: &gpui::App) -> Vec<GitAction> {
        let mut offers = if self.session_git.unstaged {
            GitAction::ALL.to_vec()
        } else if self.session_git.staged {
            vec![GitAction::Commit, GitAction::CommitPush]
        } else if self.session_unpushed {
            return vec![GitAction::Push];
        } else {
            return Vec::new();
        };
        let last = cx
            .try_global::<LastGitAction>()
            .map_or(GitAction::Commit, |last| last.0);
        let current = if offers.contains(&last) {
            last
        } else {
            GitAction::Commit
        };
        offers.retain(|action| *action != current);
        offers.insert(0, current);
        offers
    }
    /// Idle with nothing queued, so a click starts at once. Keep, Stage and
    /// turns without edits leave the pill available until the work is pushed
    /// or the files are clean.
    fn git_actions_shown(&self) -> bool {
        (self.session_git.uncommitted() || self.session_unpushed) && self.git_actions_ready()
    }
    /// Everything the pill needs except work to act on: `/stage` and
    /// `/commit` may still ask, and the agent says when it changed nothing.
    fn git_actions_ready(&self) -> bool {
        self.history_loaded()
            && !self.busy
            && !self.stopping
            && !self.preparing_worktree
            && self.follow_ups.state.messages.is_empty()
            && !self.is_remote()
            && self.settings.mono.is_none()
            && !self.settings.worktree_removed
    }
    pub(super) fn git_action_menu_items(&self, cx: &gpui::App) -> Vec<MenuItem> {
        let offers = self.git_offers(cx);
        let current = offers.first().copied();
        offers
            .into_iter()
            .map(|action| {
                MenuAction::new(
                    SharedString::from(format!("git-action:{}", action.id())),
                    action.label(),
                )
                .icon(action.icon())
                .description(action.description())
                .checked(Some(action) == current)
                .into()
            })
            .collect()
    }
    /// A menu pick: `true` when `id` was one of the pill's actions.
    pub(super) fn git_action_choice(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(action) = id.strip_prefix("git-action:").and_then(GitAction::from_id) else {
            return false;
        };
        if !self.git_offers(cx).contains(&action) {
            return true;
        }
        self.run_git_action(action, cx);
        true
    }
    /// Starts the turn straight away, without the composer's modes or
    /// attachments; the transcript shows the short message, the agent gets
    /// the full prompt.
    pub(crate) fn run_git_action(&mut self, action: GitAction, cx: &mut Context<Self>) {
        if !self.git_actions_shown() {
            return;
        }
        self.start_git_action(action, cx);
    }
    /// `/stage` or `/commit` typed in the composer: the pill's action, also
    /// when the pill is hidden, unless a skill of that name claims it.
    /// Returns true when `text` was handled here.
    pub(super) fn git_action_command(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(action) = [GitAction::Stage, GitAction::Commit]
            .into_iter()
            .find(|action| text.eq_ignore_ascii_case(&format!("/{}", action.id())))
        else {
            return false;
        };
        if self.completion.read(cx).has_user_command(action.id()) {
            return false;
        }
        if self.input.read(cx).is_composing() {
            return true;
        }
        if !self.git_actions_ready() {
            self.status = if self.busy || !self.follow_ups.state.messages.is_empty() {
                format!(
                    "Wait for the current turn and queued messages before /{}.",
                    action.id()
                )
            } else {
                format!("/{} is not available in this session.", action.id())
            };
            cx.notify();
            return true;
        }
        self.input.update(cx, |input, cx| {
            input.take(cx);
        });
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        self.start_git_action(action, cx);
        true
    }
    fn start_git_action(&mut self, action: GitAction, cx: &mut Context<Self>) {
        // Push is only ever the follow-up, never the pill's habit.
        if action != GitAction::Push {
            cx.set_global(LastGitAction(action));
        }
        let mut message = self.message(action.message().to_owned(), Vec::new(), None, cx);
        message.prepared_prompt = Some(action.prompt());
        self.launch_message(message, cx);
        cx.notify();
    }
    /// The split pill: run the current action, or pick another from the
    /// chevron, like the footer's Actions button.
    pub(super) fn git_action_pill(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.git_actions_shown() {
            return None;
        }
        let offers = self.git_offers(cx);
        let current = *offers.first()?;
        let has_menu = offers.len() > 1;
        let menu_open = self.menu_is(ComposerMenu::GitActions);
        let segment = |id: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .h_full()
                .flex()
                .items_center()
                .rounded_full()
                .text_color(content(0.7))
                .cursor_pointer()
                .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let main = segment("git-action-run")
            .aria_label(SharedString::from(current.message()))
            .tip(SharedString::from(current.message()))
            .pl(px(10.))
            .pr(px(if has_menu { 8. } else { 10. }))
            .gap(px(6.))
            .child(icon(current.icon(), 14.).text_color(content(0.55)))
            .child(div().text_size(px(12.)).child(current.label()))
            .on_click(cx.listener(move |this, _, _, cx| this.run_git_action(current, cx)));
        let chevron = segment("git-action-menu")
            .aria_label("Git actions")
            .aria_expanded(menu_open)
            .relative()
            .px(px(7.))
            .justify_center()
            .when(menu_open, |d| d.bg(content(0.06)).text_color(content(1.)))
            .child(icon("chevron-down", 14.))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_menu(ComposerMenu::GitActions, window, cx)
            }));
        Some(
            div()
                .id("git-actions")
                .debug_selector(|| "git-actions".into())
                .role(Role::Group)
                .aria_label("Git actions")
                .relative()
                .h(px(30.))
                .flex()
                .items_stretch()
                .rounded_full()
                .border_1()
                .border_color(content(0.1))
                // Hover fills stay clear of the rim.
                .p(px(3.))
                .group("git-actions")
                .child(crate::transcript::floating_surface())
                // The menu opens from the pill's left end, not the chevron.
                .child(self.git_action_anchor.probe())
                .child(main)
                .when(has_menu, |d| {
                    // Dropped while either half is lit, so the fill reads whole.
                    d.child(
                        div()
                            .w_px()
                            .my(px(4.))
                            .bg(content(0.1))
                            .group_hover("git-actions", |s| s.bg(gpui::transparent_black()))
                            .when(menu_open, |d| d.bg(gpui::transparent_black())),
                    )
                    .child(chevron)
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::store::Store;
    use monocode_core::{
        tool::{ToolCall, ToolState},
        transcript::Row,
    };
    use std::sync::Mutex;

    struct Repo(std::path::PathBuf);
    impl Repo {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "monocode-git-actions-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst),
            ));
            std::fs::create_dir_all(&root).unwrap();
            let repo = Self(std::fs::canonicalize(root).unwrap());
            repo.git(&["init"]);
            repo.git(&["config", "core.autocrlf", "false"]);
            for name in ["own.rs", "other.rs"] {
                std::fs::write(repo.0.join(name), "base\n").unwrap();
            }
            repo.git(&["add", "."]);
            repo.git(&["commit", "-m", "Base"]);
            std::fs::write(repo.0.join("other.rs"), "another session\n").unwrap();
            repo
        }
        fn git(&self, args: &[&str]) {
            let result = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&self.0)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        fn cwd(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[gpui::test]
    fn session_pill_survives_keep_questions_and_stage_but_clears_on_commit_or_undo(
        cx: &mut TestAppContext,
    ) {
        let repo = Repo::new();
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", &repo.cwd()).unwrap();
        summary.provider = "fixture".into();
        summary.has_messages = true;
        let other_summary = store.create("codex", &repo.cwd()).unwrap();
        let mut edit = ToolCall::new("edit", "Edit");
        edit.state = ToolState::Completed;
        edit.input =
            serde_json::json!({"file_path":repo.0.join("own.rs"), "cwd":repo.cwd()}).to_string();
        let row = |role: &str, text: String| Row {
            role: role.into(),
            text,
            first: true,
            sent_at: None,
        };
        store
            .write_rows(
                summary.id,
                0,
                &[
                    row("user", "Edit own.rs".into()),
                    row("tool", serde_json::to_string(&edit).unwrap()),
                    row("user", "Explain that change".into()),
                    row("assistant", "An explanation without further edits".into()),
                ],
            )
            .unwrap();
        let store = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store.clone(), cx);
                    view.load(cx);
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let shown = |visual: &mut gpui::VisualTestContext| {
            visual.run_until_parked();
            visual.debug_bounds("git-actions").is_some()
        };
        assert!(!shown(visual), "only another session's file is dirty");
        window
            .update(visual, |view, _, _| {
                assert!(view.loaded, "{}", view.status);
                assert_eq!(
                    view.session_edits,
                    [repo.0.join("own.rs").to_string_lossy()]
                );
            })
            .unwrap();

        std::fs::write(repo.0.join("own.rs"), "this session\n").unwrap();
        window
            .update(visual, |view, _, cx| view.sync_review(cx))
            .unwrap();
        assert!(
            shown(visual),
            "restored edits survive a later plain question"
        );

        window
            .update(visual, |view, _, cx| {
                view.review.update(cx, |review, cx| {
                    review.set_files(Vec::new(), cx);
                    cx.emit(crate::session_review::SessionReviewEvent::Changed);
                });
            })
            .unwrap();
        assert!(
            shown(visual),
            "Keep clears the card, not uncommitted session edits"
        );

        window
            .update(visual, |view, _, cx| {
                let at = view.transcript.read(cx).source_rows().len();
                view.transcript.update(cx, |view, cx| {
                    view.patch(
                        at,
                        vec![
                            row("user", "One more question".into()),
                            row("assistant", "Another explanation".into()),
                        ],
                        cx,
                    )
                });
                view.sync_review(cx);
            })
            .unwrap();
        assert!(shown(visual), "another turn with no edits keeps the pill");

        let other = window
            .update(visual, |_, _, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(other_summary, store, cx);
                    view.loaded = true;
                    view.sync_review(cx);
                    view
                })
            })
            .unwrap();
        visual.run_until_parked();
        visual.update(|_, cx| {
            assert!(
                !other.read(cx).git_actions_shown(),
                "a session with no edits never inherits the pill"
            )
        });

        let other_repo = Repo::new();
        std::fs::write(other_repo.0.join("own.rs"), "another checkout\n").unwrap();
        window
            .update(visual, |view, _, cx| {
                view.refresh_git_actions(cx);
                view.settings.worktree_cwd = Some(other_repo.cwd());
                view.sync_review(cx);
            })
            .unwrap();
        assert!(
            !shown(visual),
            "a late read cannot leak into another checkout"
        );
        window
            .update(visual, |view, _, cx| {
                view.settings.worktree_cwd = None;
                view.sync_review(cx);
            })
            .unwrap();
        assert!(shown(visual));

        repo.git(&["add", "own.rs"]);
        window
            .update(visual, |view, _, cx| view.refresh_git_actions(cx))
            .unwrap();
        assert!(shown(visual), "staged changes are still uncommitted");
        repo.git(&["commit", "-m", "Own changes"]);
        window
            .update(visual, |view, _, cx| view.refresh_git_actions(cx))
            .unwrap();
        assert!(
            !shown(visual),
            "commit hides the pill even with other dirty files"
        );

        std::fs::write(repo.0.join("own.rs"), "another edit\n").unwrap();
        window
            .update(visual, |view, _, cx| view.sync_review(cx))
            .unwrap();
        assert!(shown(visual));
        std::fs::write(repo.0.join("own.rs"), "this session\n").unwrap();
        window
            .update(visual, |view, _, cx| view.refresh_git_actions(cx))
            .unwrap();
        assert!(!shown(visual), "reverting the edits also hides the pill");
    }

    #[gpui::test]
    fn the_pill_requires_session_changes_and_sends_straight_away(cx: &mut TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        // Fails after saving the turn, without launching a real provider.
        let mut summary = store
            .create("codex", "/missing/git-action-project")
            .unwrap();
        summary.provider = "fixture".into();
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let shown = |visual: &mut gpui::VisualTestContext| {
            visual.run_until_parked();
            visual.debug_bounds("git-actions").is_some()
        };
        assert!(!shown(visual), "this session changed nothing");

        let file = monocode_core::checkpoint::CheckpointFile {
            path: "/missing/git-action-project/a.rs".into(),
            relative: "a.rs".into(),
            status: "modified".into(),
            additions: 1,
            deletions: 0,
            exact: true,
            undoable: true,
        };
        window
            .update(visual, |view, _, cx| {
                view.review.update(cx, |r, cx| r.set_files(vec![file], cx));
                view.session_git.unstaged = true;
                view.busy = true;
                cx.notify();
            })
            .unwrap();
        assert!(!shown(visual), "the agent is still working");

        window
            .update(visual, |view, _, cx| {
                view.busy = false;
                cx.notify();
            })
            .unwrap();
        assert!(shown(visual), "idle after a turn that changed files");

        window
            .update(visual, |view, _, cx| {
                view.settings.plan = true;
                view.run_git_action(GitAction::Stage, cx);
                // Sent at once, not parked in the queue above the composer.
                assert!(view.follow_ups.state.messages.is_empty());
                assert!(view.busy);
                let rows = view.transcript.read(cx).source_rows();
                assert_eq!(
                    rows.iter()
                        .filter(|row| row.is_prompt())
                        .map(|row| row.text.as_str())
                        .collect::<String>(),
                    "Stage your changes"
                );
                // The composer keeps its own selections for the next message.
                assert!(view.settings.plan);
                assert!(view.settings.prepared_prompt.is_none());
            })
            .unwrap();
        assert!(!shown(visual), "hidden while its turn runs");
    }

    #[gpui::test]
    fn the_pill_moves_on_to_the_next_step(cx: &mut TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", "/missing/git-next-step").unwrap();
        summary.provider = "fixture".into();
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                cx.set_global(LastGitAction(GitAction::Stage));
                view.session_git = monocode_core::git::SessionChanges {
                    staged: false,
                    unstaged: true,
                };
                assert_eq!(view.git_offers(cx), GitAction::ALL, "the habit leads");

                view.session_git.staged = true;
                view.session_git.unstaged = false;
                assert_eq!(
                    view.git_offers(cx),
                    [GitAction::Commit, GitAction::CommitPush],
                    "staged work skips Stage"
                );
                assert!(view.git_action_choice("git-action:stage", cx));
                assert!(!view.busy, "an action no longer offered does nothing");

                view.session_git = Default::default();
                assert!(!view.git_actions_shown());
                view.session_unpushed = true;
                assert!(view.git_actions_shown());
                assert_eq!(view.git_offers(cx), [GitAction::Push]);
                view.run_git_action(GitAction::Push, cx);
                assert!(view.busy);
                assert_eq!(
                    cx.global::<LastGitAction>().0,
                    GitAction::Stage,
                    "Push never becomes the habit"
                );
                assert_eq!(
                    GitAction::from_message("Push your commits"),
                    Some(GitAction::Push)
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn slash_commands_run_the_pills_actions(cx: &mut TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", "/missing/git-command").unwrap();
        summary.provider = "fixture".into();
        let window = cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.busy = true;
                view.input
                    .update(cx, |input, cx| input.set_text("/commit".into(), cx));
                view.send(cx);
                assert!(view.follow_ups.state.messages.is_empty(), "never queued");
                assert_eq!(view.input.read(cx).content, "/commit", "draft kept");

                // Works without the pill: the agent finds its own changes.
                view.busy = false;
                assert!(!view.git_actions_shown());
                view.input
                    .update(cx, |input, cx| input.set_text("/Commit ".into(), cx));
                view.send(cx);
                assert!(view.busy);
                assert!(view.input.read(cx).content.is_empty());
                let rows = view.transcript.read(cx).source_rows();
                assert_eq!(
                    rows.iter()
                        .filter(|row| row.is_prompt())
                        .map(|row| row.text.as_str())
                        .collect::<String>(),
                    "Commit your changes"
                );
                assert_eq!(cx.global::<LastGitAction>().0, GitAction::Commit);
            })
            .unwrap();
    }

    #[test]
    fn sent_actions_carry_their_icon() {
        assert_eq!(
            GitAction::message_icon("Commit and push your changes"),
            Some("cloud-upload")
        );
        assert_eq!(GitAction::message_icon("Commit your changes please"), None);
    }

    #[test]
    fn prompts_scope_the_work_to_the_agents_own_changes() {
        for action in GitAction::ALL {
            let prompt = action.prompt();
            assert!(prompt.contains("Never use `git add .`"));
            assert!(prompt.contains("only what you changed"));
            assert_eq!(
                prompt.contains("-- <paths>"),
                action != GitAction::Stage,
                "{action:?}"
            );
            assert_eq!(
                prompt.contains("Never force-push"),
                action == GitAction::CommitPush,
                "{action:?}"
            );
        }
        assert!(GitAction::Stage.prompt().contains("Do not commit."));
    }
}
