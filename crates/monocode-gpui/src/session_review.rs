//! SessionReview.tsx: the latest turn's recorded workspace changes, under its
//! reply, with Undo, Keep and Review. The checkpoint runs off the UI thread.
use crate::{
    file_icons::file_type_icon,
    right_panel::{DIFF_ADD, DIFF_DEL, format_integer, tabular},
    theme::*,
};
use gpui::{
    Context, EventEmitter, FontWeight, Role, SharedString, Task, Window, div, prelude::*, px, rgb,
};
use monocode_core::checkpoint::{CheckpointFile, CheckpointStatus, Store as CheckpointStore};

/// The card shows this many files until expanded.
const COLLAPSED: usize = 3;

pub(crate) enum SessionReviewEvent {
    /// Review the session's changes, focused on one file when given.
    Open { path: Option<String> },
    /// Undo or Keep rewrote the workspace.
    Changed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Keep,
    Undo,
}

pub(crate) struct SessionReview {
    session: i64,
    cwd: String,
    files: Vec<CheckpointFile>,
    error: Option<String>,
    expanded: bool,
    acting: Option<Action>,
    /// The card represents a settled turn, so it hides while one runs.
    busy: bool,
    /// Another session is running in this project.
    undo_locked: bool,
    generation: u64,
    store: Option<CheckpointStore>,
    _load: Option<Task<()>>,
    _action: Option<Task<()>>,
}
impl EventEmitter<SessionReviewEvent> for SessionReview {}

impl SessionReview {
    pub(crate) fn new(session: i64, cwd: String) -> Self {
        Self {
            session,
            cwd,
            files: Vec::new(),
            error: None,
            expanded: false,
            acting: None,
            busy: false,
            undo_locked: false,
            generation: 0,
            store: monocode_core::checkpoint::store().cloned(),
            _load: None,
            _action: None,
        }
    }
    pub(crate) fn session(&self) -> (i64, &str) {
        (self.session, &self.cwd)
    }
    fn valid(&self) -> bool {
        !self.cwd.is_empty() && self.cwd != "~"
    }
    /// The checkout the session works in; a worktree replaces the project.
    pub(crate) fn set_cwd(&mut self, cwd: String, cx: &mut Context<Self>) {
        if self.cwd != cwd {
            self.cwd = cwd;
            self.generation += 1;
            self.publish(Ok(CheckpointStatus::default()), cx);
            self.refresh(cx);
        }
    }
    /// sessionReviewCache.startTurn: a new turn clears the card until it settles.
    pub(crate) fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy == busy {
            return;
        }
        self.busy = busy;
        // The settled turn is reread by the conversation's refresh.
        if busy {
            self.generation += 1;
            self._load = None;
            self.publish(Ok(CheckpointStatus::default()), cx);
        }
        cx.notify();
    }
    pub(crate) fn set_undo_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        if self.undo_locked != locked {
            self.undo_locked = locked;
            cx.notify();
        }
    }
    #[cfg(test)]
    pub(crate) fn set_files(&mut self, files: Vec<CheckpointFile>, cx: &mut Context<Self>) {
        self.publish(Ok(CheckpointStatus { files }), cx);
    }
    /// Reread the recorded turn, as after a turn, Git change or reload.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.valid() || self.busy {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let (session, cwd) = (self.session.to_string(), self.cwd.clone());
        let work = cx
            .background_executor()
            .spawn(async move { store.exclusive(|s| s.status(&session, &cwd)) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let status = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation && !this.busy {
                    this.publish(status, cx);
                }
            });
        }));
    }
    fn publish(&mut self, status: Result<CheckpointStatus, String>, cx: &mut Context<Self>) {
        let (files, error) = match status {
            Ok(status) => (status.files, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        if files.len() <= COLLAPSED {
            self.expanded = false;
        }
        if self.files != files || self.error != error {
            self.files = files;
            self.error = error;
            cx.notify();
        }
    }
    fn run(&mut self, action: Action, relative: Option<String>, cx: &mut Context<Self>) {
        if self.busy
            || self.acting.is_some()
            || self.files.is_empty()
            || !self.valid()
            || (action == Action::Undo && self.undo_locked)
        {
            return;
        }
        let applicable = |file: &CheckpointFile| action == Action::Keep || file.undoable;
        let allowed = match relative.as_deref() {
            Some(relative) => self
                .files
                .iter()
                .any(|file| file.relative == relative && applicable(file)),
            None => self.files.iter().all(applicable),
        };
        if !allowed {
            return;
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        self.acting = Some(action);
        cx.notify();
        self.generation += 1;
        let generation = self.generation;
        let (session, cwd) = (self.session.to_string(), self.cwd.clone());
        let work = cx.background_executor().spawn(async move {
            store.exclusive(|s| match action {
                Action::Keep => s.keep(&session, &cwd, relative.as_deref()),
                Action::Undo => s.undo(&session, &cwd, relative.as_deref()),
            })
        });
        // Refreshes and new turns can cancel a read, but a write must settle.
        self._action = Some(cx.spawn(async move |this, cx| {
            let status = work.await;
            let _ = this.update(cx, |this, cx| {
                this.acting = None;
                match status {
                    Ok(status) if this.generation == generation => {
                        this.publish(Ok(status), cx);
                        cx.emit(SessionReviewEvent::Changed);
                    }
                    Ok(_) => cx.emit(SessionReviewEvent::Changed),
                    // The workspace moved underneath; show what remains.
                    Err(_) => this.refresh(cx),
                }
                cx.notify();
            });
        }));
    }
}

fn text_button(id: &'static str, label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .flex_shrink_0()
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(11.))
        .text_color(content(0.5))
        .when(!enabled, |d| d.opacity(0.35))
        .when(enabled, |d| {
            d.cursor_pointer()
                .focusable()
                .tab_stop(true)
                .focus(|s| s.bg(content(0.08)).text_color(content(1.)))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        })
        .child(label)
}

/// DiffCounts: `+a -d` at 11px semibold, both shown even when zero.
fn counts(additions: i64, deletions: i64, gap: f32) -> gpui::Div {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(gap))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .font_features(tabular())
        .child(
            div()
                .text_color(rgb(DIFF_ADD))
                .child(format!("+{}", format_integer(additions))),
        )
        .child(
            div()
                .text_color(rgb(DIFF_DEL))
                .child(format!("-{}", format_integer(deletions))),
        )
}

fn card() -> gpui::Div {
    div()
        .w_full()
        .rounded(px(12.))
        .border_1()
        .border_color(content(0.06))
        .bg(content(0.03))
}

impl SessionReview {
    /// The recorded files' absolute paths.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }
    /// The session's pending file count and line totals, while there are files.
    pub(crate) fn totals(&self) -> Option<(usize, i64, i64)> {
        (!self.busy && !self.files.is_empty()).then(|| {
            (
                self.files.len(),
                self.files.iter().map(|f| f.additions).sum(),
                self.files.iter().map(|f| f.deletions).sum(),
            )
        })
    }
    /// The card's contents inside `frame`: the transcript's rounded card, or
    /// the composer ledge's changes shelf.
    pub(crate) fn contents(
        &mut self,
        frame: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.busy {
            return div().into_any_element();
        }
        if let Some(error) = self.error.clone() {
            return frame
                .id("session-review")
                .role(Role::Status)
                .px_3()
                .py(px(10.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(0.8))
                        .child("Couldn’t record changes"),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(error),
                )
                .into_any_element();
        }
        if self.files.is_empty() {
            return div().into_any_element();
        }
        let disabled = self.acting.is_some();
        let can_undo = !self.undo_locked && self.files.iter().all(|f| f.undoable);
        let visible = if self.expanded {
            self.files.len()
        } else {
            self.files.len().min(COLLAPSED)
        };
        let hidden = self.files.len() - visible;
        let additions = self.files.iter().map(|f| f.additions).sum();
        let deletions = self.files.iter().map(|f| f.deletions).sum();
        let count = self.files.len();
        let title: SharedString = format!(
            "Changed {count} {}",
            if count == 1 { "file" } else { "files" }
        )
        .into();
        let header = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .px_3()
            .py(px(10.))
            .child(
                div()
                    .size(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(content(0.08))
                    .child(icon("file-diff", 16.).text_color(content(0.55))),
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
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(0.8))
                            .child(title),
                    )
                    .child(
                        counts(additions, deletions, 6.)
                            .line_height(px(16.5))
                            .mt(px(-2.)),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        text_button("session-review-undo", "Undo", !disabled && can_undo)
                            .aria_label(if can_undo {
                                "Undo the recorded changes"
                            } else if self.undo_locked {
                                "Undo is unavailable while another session is running in this project"
                            } else {
                                "Undo is unavailable because the workspace was shared, the branch moved, or a recorded file changed afterward"
                            })
                            .when(!disabled && can_undo, |d| {
                                d.on_click(
                                    cx.listener(|this, _, _, cx| this.run(Action::Undo, None, cx)),
                                )
                            }),
                    )
                    .child(
                        text_button("session-review-keep", "Keep", !disabled)
                            .aria_label("Keep the recorded changes and dismiss this card")
                            .when(!disabled, |d| {
                                d.on_click(
                                    cx.listener(|this, _, _, cx| this.run(Action::Keep, None, cx)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("session-review-open")
                            .role(Role::Button)
                            .aria_label("Review changes")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(content(0.08))
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(0.75))
                            .cursor_pointer()
                            .hover(|s| s.bg(content(0.12)).text_color(content(1.)))
                            .child("Review")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SessionReviewEvent::Open { path: None })
                            })),
                    ),
            );
        let rows = self.files[..visible].iter().enumerate().map(|(ix, file)| {
            let name = std::path::Path::new(&file.relative)
                .file_name()
                .map_or_else(
                    || file.relative.clone(),
                    |n| n.to_string_lossy().into_owned(),
                );
            let path = file.path.clone();
            let undo_path = file.relative.clone();
            let keep_path = file.relative.clone();
            let undo_enabled = !disabled && !self.undo_locked && file.undoable;
            div()
                .id(format!("session-review-file-{}", file.relative))
                .h(px(32.))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .px_3()
                .child(
                    div()
                        .id("session-review-file")
                        .role(Role::Button)
                        .aria_label(format!("Review {}", file.relative))
                        .focusable()
                        .tab_stop(true)
                        .h_full()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pr_2()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_color(content(0.65))
                        .focus(|s| s.bg(content(0.05)).text_color(content(1.)))
                        .hover(|s| s.bg(content(0.05)).text_color(content(1.)))
                        .child(file_type_icon(&name, false, false, 15.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .font_family(crate::fonts::mono())
                                .text_size(px(12.))
                                .child(file.relative.clone()),
                        )
                        .child(if file.exact {
                            counts(file.additions, file.deletions, 8.).into_any_element()
                        } else {
                            div()
                                .flex_shrink_0()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(content(0.45))
                                .child("Diff unavailable")
                                .into_any_element()
                        })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(SessionReviewEvent::Open {
                                path: Some(path.clone()),
                            })
                        })),
                )
                .child(
                    text_button("session-review-file-undo", "Undo", undo_enabled)
                        .debug_selector(move || format!("session-review-file-undo-{ix}"))
                        .aria_label(format!("Undo changes in {}", file.relative))
                        .when(undo_enabled, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.run(Action::Undo, Some(undo_path.clone()), cx)
                            }))
                        }),
                )
                .child(
                    text_button("session-review-file-keep", "Keep", !disabled)
                        .debug_selector(move || format!("session-review-file-keep-{ix}"))
                        .aria_label(format!("Keep changes in {}", file.relative))
                        .when(!disabled, |d| {
                            d.on_click(cx.listener(move |this, _, _, cx| {
                                this.run(Action::Keep, Some(keep_path.clone()), cx)
                            }))
                        }),
                )
        });
        let list = div()
            .id("session-review-files")
            .border_t_1()
            .border_color(content(0.05))
            .py_1()
            .when(self.expanded, |d| d.max_h(px(256.)).overflow_y_scroll())
            .children(rows);
        let expanded = self.expanded;
        frame
            .id("session-review")
            .overflow_hidden()
            .child(header)
            .child(list)
            .when(count > COLLAPSED, |d| {
                d.child(
                    div()
                        .id("session-review-more")
                        .role(Role::Button)
                        .aria_expanded(expanded)
                        .group("session-review-more")
                        .h(px(32.))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .border_t_1()
                        .border_color(content(0.05))
                        .px_3()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .hover(|s| s.bg(content(0.05)).text_color(content(0.7)))
                        .child(
                            icon(
                                if expanded {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                14.,
                            )
                            .text_color(content(0.45))
                            .group_hover("session-review-more", |s| s.text_color(content(0.7))),
                        )
                        .child(if expanded {
                            "Show fewer files".to_string()
                        } else {
                            format!(
                                "Show {hidden} more {}",
                                if hidden == 1 { "file" } else { "files" }
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.expanded = !this.expanded;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }
}

impl Render for SessionReview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.contents(card(), cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[gpui::test]
    fn file_review_actions_only_apply_to_the_selected_file(cx: &mut gpui::TestAppContext) {
        let turn = monocode_core::checkpoint::new_turn_id();
        let scratch = Scratch(std::env::temp_dir().join(format!("monocode-file-review-{turn}")));
        let project = scratch.0.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&project)
                .output()
                .unwrap();
            assert!(status.status.success(), "{:?}", status.stderr);
        };
        git(&["init"]);
        let write_all = |text: &str| {
            for name in ["a.txt", "b.txt", "c.txt"] {
                std::fs::write(project.join(name), text).unwrap();
            }
        };
        write_all("base\n");
        git(&["add", "."]);
        git(&["commit", "-m", "baseline"]);

        let cwd = project.to_string_lossy().into_owned();
        let store = CheckpointStore::new(scratch.0.join("checkpoints"));
        store.begin_turn("1", &cwd, &turn).unwrap();
        write_all("edited\n");
        store.finish_turn("1", &cwd, &turn).unwrap();
        // The review can keep a file with later edits, but cannot undo it.
        std::fs::write(project.join("b.txt"), "later\n").unwrap();
        let status = store.status("1", &cwd).unwrap();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut review = SessionReview::new(1, cwd.clone());
                    review.store = Some(store.clone());
                    review.set_files(status.files.clone(), cx);
                    review
                })
            })
            .unwrap()
        });
        window
            .update(cx, |review, _, cx| {
                review.set_busy(true, cx);
                // A callback from the previous rendered card must not start.
                review.set_files(status.files.clone(), cx);
                review.run(Action::Undo, Some("a.txt".into()), cx);
                review.run(Action::Keep, Some("b.txt".into()), cx);
                assert!(review.acting.is_none());
                review.set_busy(false, cx);
                review.set_undo_locked(true, cx);
                review.run(Action::Undo, Some("a.txt".into()), cx);
                assert!(review.acting.is_none());
                review.set_undo_locked(false, cx);
                review.run(Action::Undo, Some("b.txt".into()), cx);
                assert!(review.acting.is_none());
            })
            .unwrap();

        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let undo = visual.debug_bounds("session-review-file-undo-0").unwrap();
        visual.simulate_click(undo.center(), Default::default());
        visual.run_until_parked();
        let contents = |name: &str| std::fs::read_to_string(project.join(name)).unwrap();
        assert_eq!(contents("a.txt"), "base\n");
        assert_eq!(contents("b.txt"), "later\n");
        assert_eq!(contents("c.txt"), "edited\n");

        visual.refresh().unwrap();
        let keep = visual.debug_bounds("session-review-file-keep-0").unwrap();
        visual.simulate_click(keep.center(), Default::default());
        visual.run_until_parked();
        assert_eq!(contents("b.txt"), "later\n");
        let remaining = store.status("1", &cwd).unwrap().files;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].relative, "c.txt");
    }
}
