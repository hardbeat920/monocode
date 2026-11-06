//! Commit pill turns ("Stage your changes", …). While the agent works, one
//! line follows its Git steps; once it settles, a card read from the commands
//! it ran leads its Markdown reply.
use super::*;
use crate::conversation::git_actions::GitAction;
use crate::right_panel::{DIFF_ADD, DIFF_DEL, format_integer, tabular};
use gpui::{AnyElement, Div};
use monocode_core::git::{
    CommitFile,
    report::{self, Report, Run, Step},
};

/// The card shows this many files before a "more" line.
const FILES: usize = 4;

pub(super) struct GitCard {
    action: GitAction,
    /// The step the running turn is on; `None` once it settled.
    live: Option<Step>,
    report: Report,
    /// The reply the card sits above.
    pub(super) answer: Option<usize>,
}
impl GitCard {
    pub(super) fn live(&self) -> bool {
        self.live.is_some()
    }
}

/// Commit-loaded file lists, by hash; `None` while one loads.
pub(super) type CommitFiles = HashMap<String, Option<Arc<Vec<CommitFile>>>>;

/// The shell commands of `rows`' tool calls, with output and success.
fn runs(rows: &[Row]) -> Vec<(String, String, bool)> {
    rows.iter()
        .filter(|row| row.role == "tool")
        .filter_map(|row| serde_json::from_str::<ToolCall>(&row.text).ok())
        .filter_map(|tool| {
            let shown = tool.presentation();
            if shown.action != "Run" && shown.command.is_none() {
                return None;
            }
            let command = shown.command.unwrap_or(shown.target);
            Some((command, tool.output, tool.state == ToolState::Completed))
        })
        .collect()
}

fn report_of(runs: &[(String, String, bool)]) -> Report {
    let runs: Vec<_> = runs
        .iter()
        .map(|(command, output, ok)| Run {
            command,
            output,
            ok: *ok,
        })
        .collect();
    report::report(&runs)
}

/// The hash of the session's most recent commit, from any turn.
pub(crate) fn latest_commit(rows: &[Row]) -> Option<String> {
    rows.iter()
        .rev()
        .filter(|row| row.role == "tool" && row.text.contains("commit"))
        .find_map(|row| {
            report_of(&runs(std::slice::from_ref(row)))
                .commits
                .pop()
                .map(|commit| commit.hash)
        })
}

fn live_label(action: GitAction, step: Step) -> &'static str {
    match step {
        Step::Push => "Pushing…",
        Step::Commit => "Committing…",
        Step::Stage => "Staging…",
        Step::Review => match action {
            GitAction::Push => "Getting ready to push…",
            _ => "Checking your changes…",
        },
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn counts(additions: u64, deletions: u64) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .font_features(tabular())
        .child(
            div()
                .text_color(rgb(DIFF_ADD))
                .child(format!("+{}", format_integer(additions as i64))),
        )
        .child(
            div()
                .text_color(rgb(DIFF_DEL))
                .child(format!("-{}", format_integer(deletions as i64))),
        )
}

/// One file of the card: what changed and how much.
struct FileLine {
    path: String,
    counts: Option<(u64, u64)>,
    lines: Option<u64>,
}

impl TranscriptView {
    /// The card for the pill turn at `turn`, or `None` for other turns and
    /// for settled ones whose commands show nothing to present.
    pub(super) fn git_card(
        &self,
        turn: usize,
        turn_end: usize,
        final_answer: Option<usize>,
    ) -> Option<Arc<GitCard>> {
        if self.mono || turn == 0 {
            return None;
        }
        let prompt: String = self.source[prompt_start(&self.source, turn)..turn]
            .iter()
            .filter(|row| row.role == "user")
            .map(|row| row.text.as_str())
            .collect();
        let action = GitAction::from_message(&prompt)?;
        let runs = runs(&self.source[turn..turn_end]);
        if self.streaming && turn_end == self.source.len() {
            let step = runs
                .last()
                .map_or(Step::Review, |(command, _, _)| report::step(command));
            return Some(Arc::new(GitCard {
                action,
                live: Some(step),
                report: Report::default(),
                answer: None,
            }));
        }
        let answer = final_answer?;
        let report = report_of(&runs);
        let shown = match action {
            GitAction::Stage => !report.staged.is_empty() || !report.commits.is_empty(),
            GitAction::Commit => !report.commits.is_empty(),
            GitAction::CommitPush => !report.commits.is_empty() || report.push.is_some(),
            GitAction::Push => report.push.is_some(),
        };
        if !shown {
            return None;
        }
        Some(Arc::new(GitCard {
            action,
            live: None,
            report,
            answer: Some(answer),
        }))
    }

    pub(super) fn git_card_row(
        &mut self,
        card: &GitCard,
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(step) = card.live {
            return self.git_live_line(card.action, step, message_start, cx);
        }
        let report = &card.report;
        let commit = report.commits.last();
        let push = report.push.as_ref();
        let push_failed = push.is_some_and(|push| !push.ok);
        let files = self.git_card_files(card, message_start, cx);
        let (additions, deletions) = if commit.is_some() {
            report.commits.iter().fold((0, 0), |(a, d), commit| {
                (a + commit.additions, d + commit.deletions)
            })
        } else {
            report
                .staged_totals
                .map_or((0, 0), |(_, additions, deletions)| (additions, deletions))
        };
        let has_counts = additions + deletions > 0;
        let title: SharedString = match (commit, push) {
            (Some(commit), _) => commit.subject.clone().into(),
            (None, Some(push)) if push.up_to_date => "Already up to date".into(),
            (None, Some(push)) if !push.ok => "Push didn’t go through".into(),
            (None, Some(push)) => push
                .branch
                .as_ref()
                .map_or_else(|| "Pushed".into(), |branch| format!("Pushed {branch}"))
                .into(),
            (None, None) => format!(
                "Staged {}",
                plural(
                    report
                        .staged_totals
                        .map_or(report.staged.len(), |(files, _, _)| files as usize),
                    "file",
                    "files"
                )
            )
            .into(),
        };

        let dot = || div().text_color(content(0.25)).child("·");
        let mut subtitle = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(11.))
            .line_height(px(16.))
            .text_color(content(0.5))
            .overflow_hidden();
        let mut parts = 0;
        if let Some(commit) = commit {
            let copied = self
                .copied_commit
                .as_ref()
                .is_some_and(|(hash, _)| *hash == commit.hash);
            let hash = commit.hash.clone();
            subtitle = subtitle.child(
                div()
                    .id(("git-card-hash", message_start))
                    .role(Role::Button)
                    .aria_label(if copied {
                        "Copied the commit hash".to_string()
                    } else {
                        format!("Copy commit {}", commit.hash)
                    })
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(15.))
                    .px(px(4.))
                    .rounded(px(4.))
                    .bg(content(0.06))
                    .font_family(crate::fonts::mono())
                    .text_size(px(10.))
                    .text_color(content(0.7))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                    .child(if copied {
                        icon("check", 11.)
                            .text_color(rgb(DIFF_ADD))
                            .into_any_element()
                    } else {
                        div().child(commit.hash.clone()).into_any_element()
                    })
                    .when(copied, |d| d.child("Copied"))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(hash.clone()));
                        let born = Instant::now();
                        this.copied_commit = Some((hash.clone(), born));
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(2))
                                .await;
                            let _ = this.update(cx, |this, cx| {
                                if this.copied_commit.as_ref().is_some_and(|c| c.1 == born) {
                                    this.copied_commit = None;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    })),
            );
            parts += 1;
            if !commit.branch.is_empty() {
                subtitle = subtitle.child(dot()).child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("git-branch", 12.).text_color(content(0.4)))
                        .child(div().truncate().child(commit.branch.clone())),
                );
            }
            if report.commits.len() > 1 {
                subtitle = subtitle
                    .child(dot())
                    .child(div().flex_shrink_0().child(plural(
                        report.commits.len(),
                        "commit",
                        "commits",
                    )));
            }
        }
        if let Some(push) = push {
            if parts > 0 {
                subtitle = subtitle.child(dot());
            }
            parts += 1;
            subtitle = subtitle.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when(push_failed, |d| d.text_color(rgb(0xf87171)))
                    .child(
                        icon(if push_failed { "close" } else { "cloud-upload" }, 12.).text_color(
                            if push_failed {
                                rgb(0xf87171).into()
                            } else {
                                content(0.4)
                            },
                        ),
                    )
                    .child(div().truncate().child(if push_failed {
                        "Push rejected".to_string()
                    } else if commit.is_none() {
                        push.repo.clone().unwrap_or_else(|| "Remote updated".into())
                    } else {
                        push.repo
                            .as_ref()
                            .map_or_else(|| "Pushed".into(), |repo| format!("Pushed to {repo}"))
                    })),
            );
        }
        if commit.is_none() && push.is_none() {
            parts += 1;
            subtitle = subtitle.child(div().child("Ready to commit"));
        }
        if has_counts {
            if parts > 0 {
                subtitle = subtitle.child(dot());
            }
            subtitle = subtitle.child(counts(additions, deletions));
        }

        // A new branch's pull request beats a link to the commit.
        let link = push.filter(|push| push.ok).and_then(|push| {
            push.pull_request
                .clone()
                .map(|url| ("Open pull request", "git-pull-request", url))
                .or_else(|| {
                    let web = push.web.as_ref()?;
                    let commit = commit?;
                    web.starts_with("https://github.com/").then(|| {
                        (
                            "View commit",
                            "external-link",
                            format!("{web}/commit/{}", commit.hash),
                        )
                    })
                })
        });

        let (badge, badge_color) = if push_failed {
            ("close", rgb(0xf87171))
        } else {
            ("check", rgb(DIFF_ADD))
        };
        let header = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px_3()
            .py(px(10.))
            .child(
                div()
                    .relative()
                    .size(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(content(0.08))
                    .child(icon(card.action.icon(), 16.).text_color(content(0.6)))
                    .child(
                        div()
                            .absolute()
                            .right(px(-3.))
                            .bottom(px(-3.))
                            .size(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(rgb(CHAT))
                            .child(
                                div()
                                    .size(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(badge_color)
                                    .child(icon(badge, 8.).text_color(rgb(CHAT))),
                            ),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(content(0.85))
                            .child(title),
                    )
                    .child(subtitle),
            )
            .when_some(link, |d, (label, glyph, url)| {
                d.child(
                    div()
                        .id(("git-card-link", message_start))
                        .role(Role::Link)
                        .aria_label(label)
                        .flex_shrink_0()
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(6.))
                        .bg(content(0.08))
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(content(0.75))
                        .cursor_pointer()
                        .hover(|s| s.bg(content(0.12)).text_color(content(1.)))
                        .child(icon(glyph, 13.).text_color(content(0.6)))
                        .child(label)
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |event, window, cx| {
                            crate::inbox::follow_link(&url, event, window, cx)
                        }),
                )
            });

        let hidden = files.len().saturating_sub(FILES);
        let base = self.file_link_base.clone();
        let rows = files.into_iter().take(FILES).enumerate().map(|(ix, file)| {
            let name = std::path::Path::new(&file.path)
                .file_name()
                .map_or_else(|| file.path.clone(), |n| n.to_string_lossy().into_owned());
            let path = base.as_deref().map_or_else(
                || file.path.clone(),
                |base| {
                    std::path::Path::new(base)
                        .join(&file.path)
                        .to_string_lossy()
                        .into_owned()
                },
            );
            div()
                .id(("git-card-file", message_start * 64 + ix))
                .role(Role::Link)
                .aria_label(SharedString::from(format!("Open {}", file.path)))
                .h(px(30.))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .cursor_pointer()
                .text_color(content(0.65))
                .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                .child(crate::file_icons::file_type_icon(&name, false, false, 15.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_family(crate::fonts::mono())
                        .text_size(px(12.))
                        .child(file.path.clone()),
                )
                .map(|d| match (file.counts, file.lines) {
                    (Some((additions, deletions)), _) => d.child(counts(additions, deletions)),
                    (None, Some(lines)) => d.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.))
                            .font_features(tabular())
                            .text_color(content(0.45))
                            .child(plural(lines as usize, "line", "lines")),
                    ),
                    (None, None) => d,
                })
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    window
                        .dispatch_action(Box::new(crate::file_editor::OpenPath(path.clone())), cx);
                })
        });
        let rows: Vec<_> = rows.collect();
        div()
            .w_full()
            .py_1()
            .child(
                div()
                    .id(("git-card", message_start))
                    .debug_selector(move || format!("git-card-{message_start}"))
                    .role(Role::Group)
                    .aria_label(SharedString::from(format!(
                        "{}: {}",
                        card.action.label(),
                        title_text(card)
                    )))
                    .w_full()
                    .overflow_hidden()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(content(0.06))
                    .bg(content(0.03))
                    .child(header)
                    .when(!rows.is_empty(), |d| {
                        d.child(
                            div()
                                .border_t_1()
                                .border_color(content(0.05))
                                .py_1()
                                .children(rows)
                                .when(hidden > 0, |d| {
                                    d.child(
                                        div()
                                            .h(px(26.))
                                            .px_3()
                                            .flex()
                                            .items_center()
                                            .text_size(px(11.))
                                            .text_color(content(0.4))
                                            .child(format!(
                                                "+{} more",
                                                plural(hidden, "file", "files")
                                            )),
                                    )
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The running turn's single line: a spinner and the step it is on.
    fn git_live_line(
        &self,
        action: GitAction,
        step: Step,
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = live_label(action, step).to_owned();
        let still = cx.reduce_motion();
        div()
            .id(("git-live", message_start))
            .debug_selector(move || format!("git-live-{message_start}"))
            .role(Role::Status)
            .aria_label(label.clone())
            .flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .py_1()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(content(0.55))
            .child(
                div()
                    .size(px(14.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if still {
                        icon(action.icon(), 14.).into_any_element()
                    } else {
                        crate::session_mark::spinner(
                            ("git-live-loader", message_start),
                            crate::pulse::Rate::Half,
                        )
                        .into_any_element()
                    }),
            )
            .child(div().min_w_0().truncate().child(if still {
                div().child(label).into_any_element()
            } else {
                crate::activity::shimmer(label, self.started_at.elapsed().as_secs_f32())
            }))
            .into_any_element()
    }

    /// The files to list: a commit's own numbers once Git has read them, or
    /// the staged `--stat` lines.
    fn git_card_files(
        &mut self,
        card: &GitCard,
        message_start: usize,
        cx: &mut Context<Self>,
    ) -> Vec<FileLine> {
        if card.report.commits.is_empty() {
            return card
                .report
                .staged
                .iter()
                .map(|file| FileLine {
                    path: file.path.clone(),
                    counts: None,
                    lines: file.changes,
                })
                .collect();
        }
        let mut files: Vec<FileLine> = Vec::new();
        for commit in &card.report.commits {
            match self.commit_files.get(&commit.hash) {
                Some(Some(loaded)) => {
                    for file in loaded.iter() {
                        let counts = file.additions.zip(file.deletions);
                        match files.iter_mut().find(|f| f.path == file.path) {
                            Some(line) => {
                                line.counts =
                                    line.counts.zip(counts).map(|(a, b)| (a.0 + b.0, a.1 + b.1));
                            }
                            None => files.push(FileLine {
                                path: file.path.clone(),
                                counts,
                                lines: None,
                            }),
                        }
                    }
                }
                Some(None) => {}
                None => self.load_commit_files(commit.hash.clone(), message_start, cx),
            }
        }
        files
    }

    fn load_commit_files(&mut self, hash: String, message_start: usize, cx: &mut Context<Self>) {
        let Some(cwd) = self.file_link_base.clone() else {
            return;
        };
        self.commit_files.insert(hash.clone(), None);
        let work_hash = hash.clone();
        let work = cx.background_executor().spawn(async move {
            monocode_core::git::commit_files(std::path::Path::new(&cwd), &work_hash)
        });
        cx.spawn(async move |this, cx| {
            let files = work.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.commit_files.insert(hash, Some(Arc::new(files)));
                this.rebuild(message_start, cx);
            });
        })
        .detach();
    }
}

fn title_text(card: &GitCard) -> String {
    card.report
        .commits
        .last()
        .map(|commit| commit.subject.clone())
        .or_else(|| card.report.push.as_ref().and_then(|p| p.branch.clone()))
        .unwrap_or_else(|| plural(card.report.staged.len(), "file", "files"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: String) -> Row {
        Row {
            role: role.into(),
            text,
            first: true,
            sent_at: None,
        }
    }

    fn bash(command: &str, output: &str, state: ToolState) -> String {
        let mut call = ToolCall::new(command, "Bash");
        call.input = serde_json::json!({ "command": command }).to_string();
        call.output = output.into();
        call.state = state;
        serde_json::to_string(&call).unwrap()
    }

    fn cards(view: &TranscriptView) -> Vec<Arc<GitCard>> {
        view.rows
            .iter()
            .filter_map(|row| match &row.body {
                Body::GitAction(card) => Some(card.clone()),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn pill_turns_show_one_live_step_then_a_card_for_the_reply(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        let commit = "git add -- a.rs && git commit -m \"Center the jump button\" -- a.rs";
        window
            .update(cx, |view, _, cx| {
                view.streaming(true, cx);
                view.patch(
                    0,
                    vec![
                        row("user", "Commit and push your changes".into()),
                        row(
                            "tool",
                            bash("git status --short", " M a.rs\n", ToolState::Completed),
                        ),
                        row("tool", bash(commit, "", ToolState::Running)),
                    ],
                    cx,
                );
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                let cards = cards(view);
                assert_eq!(cards.len(), 1, "one line for the whole running turn");
                assert_eq!(cards[0].live, Some(Step::Commit));
                assert!(
                    !view
                        .rows
                        .iter()
                        .any(|row| matches!(row.body, Body::Tool { .. } | Body::Phase { .. })),
                    "its steps stay behind the line"
                );
            })
            .unwrap();
        assert!(visual.debug_bounds("git-live-1").is_some());

        window
            .update(visual, |view, _, cx| {
                view.patch(
                    2,
                    vec![
                        row(
                            "tool",
                            bash(
                                commit,
                                "[main 3bb0c42] Center the jump button\n 1 file changed, 2 insertions(+), 1 deletion(-)\n",
                                ToolState::Completed,
                            ),
                        ),
                        row(
                            "tool",
                            bash(
                                "git push",
                                "To github.com:o/r.git\n   43b1107..3bb0c42  main -> main\n",
                                ToolState::Completed,
                            ),
                        ),
                        row(
                            "assistant",
                            "Committed `3bb0c42` and pushed it.\n\n- a.rs".into(),
                        ),
                    ],
                    cx,
                );
                view.streaming(false, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                let cards = cards(view);
                assert_eq!(cards.len(), 1);
                assert!(!cards[0].live());
                assert_eq!(cards[0].report.commits[0].hash, "3bb0c42");
                assert!(cards[0].report.push.as_ref().unwrap().ok);
                let card = view
                    .rows
                    .iter()
                    .position(|row| matches!(row.body, Body::GitAction(_)))
                    .unwrap();
                assert!(
                    view.rows[card + 1..]
                        .iter()
                        .any(|row| matches!(row.body, Body::Markdown(_))),
                    "the reply follows the card"
                );
            })
            .unwrap();
        assert!(visual.debug_bounds("git-card-4").is_some());
    }

    #[gpui::test]
    fn ordinary_turns_and_empty_results_keep_their_reply(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    0,
                    vec![
                        row("user", "Please commit".into()),
                        row(
                            "tool",
                            bash(
                                "git commit -m x",
                                "[main abc1234] x\n",
                                ToolState::Completed,
                            ),
                        ),
                        row("assistant", "Done".into()),
                        row("user", "Commit your changes".into()),
                        row("assistant", "I changed nothing in this session.".into()),
                    ],
                    cx,
                );
                assert!(cards(view).is_empty());
                assert_eq!(
                    latest_commit(view.source_rows()).as_deref(),
                    Some("abc1234"),
                    "any turn's commit can still be pushed"
                );
            })
            .unwrap();
    }
}
