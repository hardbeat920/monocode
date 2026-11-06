//! `/pr`: push the session's branch and open a GitHub pull request.
//!
//! A bare `/pr` (or `/pr --draft`) checks the branch and writes a proposed
//! title and body into the composer as `/pr` text. The user edits it and sends
//! again to push and create the pull request, which is then linked to the
//! session. The git and `gh` work lives in `monocode_core::scm`.
use super::*;
use monocode_core::scm::{self, PrDraft, PrPlan, PrRequest};
use monocode_core::transcript::Row;

/// The text of the session's latest assistant reply, the source of the
/// proposed body's summary.
fn last_reply(rows: &[Row]) -> String {
    let Some(end) = rows.iter().rposition(|row| {
        row.role == "assistant"
            && !row.text.trim().is_empty()
            && !row.text.trim().starts_with("<reaction>")
    }) else {
        return String::new();
    };
    let mut start = end;
    while start > 0 && !rows[start].first && rows[start - 1].role == "assistant" {
        start -= 1;
    }
    rows[start..=end]
        .iter()
        .filter(|row| row.role == "assistant")
        .map(|row| row.text.as_str())
        .collect()
}

impl Conversation {
    /// `/pr` typed in the composer. Returns true when `text` was handled here,
    /// leaving the draft in place when the command cannot run.
    pub(super) fn pr_command(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(rest) = monocode_core::composer::consume_command(text, "pr") else {
            return false;
        };
        if self.completion.read(cx).has_user_command("pr") {
            return false;
        }
        if self.input.read(cx).is_composing() {
            return true;
        }
        let command = match scm::parse_pr_command(rest) {
            Ok(command) => command,
            Err(error) => {
                self.status = error;
                cx.notify();
                return true;
            }
        };
        if self.pr_busy {
            return true;
        }
        if !self.git_actions_ready() {
            self.status = if self.busy || !self.follow_ups.state.messages.is_empty() {
                "Wait for the current turn and queued messages before /pr.".into()
            } else {
                "/pr is not available in this session.".into()
            };
            cx.notify();
            return true;
        }
        let cwd = self.execution_cwd().to_owned();
        let draft = command.draft;
        let text = text.to_owned();
        self.pr_busy = true;
        match command.edited {
            None => {
                self.status = "Checking the branch…".into();
                let title = self.pr_title_hint();
                let summary = last_reply(self.transcript.read(cx).source_rows());
                let work = cx.background_executor().spawn(async move {
                    scm::prepare_pull_request(Path::new(&cwd), &title, &summary)
                });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.pr_busy = false;
                        this.apply_pr_proposal(result, draft, &text, cx);
                    });
                })
                .detach();
            }
            Some((title, body)) => {
                self.status = "Pushing the branch and opening the pull request…".into();
                let request = PrRequest { title, body, draft };
                let work = cx
                    .background_executor()
                    .spawn(async move { scm::open_pull_request(Path::new(&cwd), &request) });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.pr_busy = false;
                        this.apply_pr_opened(result, &text, cx);
                    });
                })
                .detach();
            }
        }
        cx.notify();
        true
    }

    /// The session's title, unless it is still the placeholder.
    fn pr_title_hint(&self) -> String {
        let title = monocode_core::session_title::display_title(
            &self.summary.title,
            &self.summary.provider,
        );
        if title == "New session" {
            String::new()
        } else {
            title.to_owned()
        }
    }

    /// A bare `/pr` finished checking: propose the text for editing, or show
    /// the pull request the branch already has. `sent` is what was typed; a
    /// draft the user has since changed is left alone.
    fn apply_pr_proposal(
        &mut self,
        result: Result<PrPlan, String>,
        draft: bool,
        sent: &str,
        cx: &mut Context<Self>,
    ) {
        match result {
            Err(error) => self.status = error,
            Ok(PrPlan::Existing(pr)) => self.show_opened_pr(pr.url, true, sent, cx),
            Ok(PrPlan::New(PrDraft {
                branch,
                base,
                title,
                body,
                ..
            })) => {
                if self.input.read(cx).content.trim() == sent.trim() {
                    let text = scm::pr_command_text(draft, &title, &body);
                    self.input.update(cx, |input, cx| input.set_text(text, cx));
                }
                self.status = format!(
                    "Edit the title and body, then send to push {branch} and open a{} pull \
                     request into {base}.",
                    if draft { " draft" } else { "" }
                );
            }
        }
        cx.notify();
    }

    /// The pull request was created, or already existed.
    fn apply_pr_opened(
        &mut self,
        result: Result<scm::OpenedPr, String>,
        sent: &str,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(opened) => self.show_opened_pr(opened.url, opened.existing, sent, cx),
            // The draft stays so the user can retry or fix the cause first.
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    /// Links the pull request to the session, clears the command and opens it.
    fn show_opened_pr(&mut self, url: String, existing: bool, sent: &str, cx: &mut Context<Self>) {
        if let Some(item) = monocode_core::session_groups::parse_github_work_item(&url)
            .filter(|item| item.kind == "pr")
        {
            self.link_opened_pull_request(item, cx);
        }
        if self.input.read(cx).content.trim() == sent.trim() {
            self.input.update(cx, |input, cx| {
                input.take(cx);
            });
        }
        self.status = if existing {
            format!("This branch already has a pull request: {url}")
        } else {
            format!("Opened pull request {url}")
        };
        cx.open_url(&url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::session_groups::parse_github_work_item;
    use monocode_core::store::Store;
    use std::sync::Mutex;

    fn row(role: &str, text: &str, first: bool) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first,
            sent_at: None,
        }
    }

    #[test]
    fn the_summary_is_the_latest_assistant_reply() {
        assert_eq!(last_reply(&[]), "");
        let rows = [
            row("user", "Do it", true),
            row("assistant", "Old answer", true),
            row("user", "Again", true),
            row("assistant", "Part one. ", true),
            row("tool", "{}", false),
            row("assistant", "Part two.", false),
            row("assistant", "<reaction>👍", true),
        ];
        // The tool row splits the reply, so only the trailing part counts.
        assert_eq!(last_reply(&rows), "Part two.");
        assert_eq!(last_reply(&rows[..4]), "Part one. ");
    }

    fn conversation(cx: &mut TestAppContext, cwd: &str) -> gpui::WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("codex", cwd).unwrap();
        summary.provider = "fixture".into();
        summary.title = "Open PR from a session".into();
        let store = Arc::new(Mutex::new(store));
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, store, cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view
                })
            })
            .unwrap()
        })
    }

    fn send(
        window: gpui::WindowHandle<Conversation>,
        cx: &mut TestAppContext,
        text: &str,
    ) -> (String, String) {
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text(text.into(), cx));
                view.send(cx);
                (view.status.clone(), view.input.read(cx).content.to_string())
            })
            .unwrap()
    }

    #[gpui::test]
    fn pr_keeps_the_draft_when_it_cannot_run(cx: &mut TestAppContext) {
        let window = conversation(cx, "/missing/pr-command");
        let (status, draft) = send(window, cx, "/pr --force");
        assert!(status.contains("Unknown option `--force`"), "{status}");
        assert_eq!(draft, "/pr --force");

        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.busy = true;
                view.input
                    .update(cx, |input, cx| input.set_text("/pr".into(), cx));
                view.send(cx);
                assert_eq!(
                    view.status,
                    "Wait for the current turn and queued messages before /pr."
                );
                assert!(view.follow_ups.state.messages.is_empty(), "never queued");
                assert_eq!(view.input.read(cx).content, "/pr", "draft kept");
            })
            .unwrap();
    }

    #[gpui::test]
    fn pr_outside_a_repository_says_so_and_keeps_the_draft(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("monocode-pr-command-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let window = conversation(cx, &dir.to_string_lossy());
        send(window, cx, "/pr");
        cx.run_until_parked();
        // Wait for the background check, which stops at the first git call.
        for _ in 0..50 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            if !window.read_with(cx, |view, _| view.pr_busy).unwrap() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (status, draft) = window
            .read_with(cx, |view, cx| {
                (view.status.clone(), view.input.read(cx).content.to_string())
            })
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(status, "This folder is not a git repository.");
        assert_eq!(draft, "/pr");
    }

    #[gpui::test]
    fn a_proposal_fills_the_composer_and_a_sent_one_links_the_pull_request(
        cx: &mut TestAppContext,
    ) {
        let window = conversation(cx, "/missing/pr-command");
        let plan = || {
            Ok(PrPlan::New(PrDraft {
                branch: "feat/open-pr".into(),
                base: "main".into(),
                remote: "origin".into(),
                title: "Open PR from a session".into(),
                body: "Added /pr.\n\n## Commits\n\n- 1111111 Add it".into(),
                commits: vec!["1111111 Add it".into()],
            }))
        };
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.input
                    .update(cx, |input, cx| input.set_text("/pr --draft".into(), cx));
                view.apply_pr_proposal(plan(), true, "/pr --draft", cx);
                assert_eq!(
                    view.input.read(cx).content,
                    "/pr --draft\nOpen PR from a session\n\nAdded /pr.\n\n## Commits\n\n- 1111111 Add it"
                );
                assert!(view.status.contains("push feat/open-pr"), "{}", view.status);
                assert!(view.status.contains("draft pull request into main"));

                // Text the user has changed since is not overwritten.
                view.input
                    .update(cx, |input, cx| input.set_text("/pr mine".into(), cx));
                view.apply_pr_proposal(plan(), false, "/pr", cx);
                assert_eq!(view.input.read(cx).content, "/pr mine");

                let sent = view.input.read(cx).content.to_string();
                view.apply_pr_opened(Err("Could not push `feat/open-pr` to origin: rejected".into()), &sent, cx);
                assert_eq!(view.input.read(cx).content, "/pr mine", "kept on failure");
                assert!(view.summary.work_item.is_none());

                let opened = scm::OpenedPr {
                    url: "https://github.com/acme/app/pull/9".into(),
                    branch: "feat/open-pr".into(),
                    existing: false,
                };
                view.apply_pr_opened(Ok(opened), &sent, cx);
                assert!(view.input.read(cx).content.is_empty());
                assert_eq!(view.status, "Opened pull request https://github.com/acme/app/pull/9");
                assert_eq!(
                    view.summary.work_item,
                    parse_github_work_item("https://github.com/acme/app/pull/9")
                );
            })
            .unwrap();
    }
}
