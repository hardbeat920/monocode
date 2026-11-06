//! For a few seconds after a thread is archived, Cmd/Ctrl+Z or the toast's
//! Undo button brings it back, reopening its tab if it had one.
use super::*;
use gpui::Task;
use std::time::Duration;

pub(super) const UNDO_WINDOW: Duration = Duration::from_secs(8);

pub(super) struct ArchiveUndo {
    id: i64,
    title: String,
    had_tab: bool,
    /// Dropping the timer cancels it, so a newer archive restarts the window.
    _expire: Task<()>,
}

impl Shell {
    /// Called once the store has the new archive state.
    pub(super) fn remember_archive(
        &mut self,
        id: i64,
        archived: bool,
        had_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !archived {
            if self.archive_undo.as_ref().is_some_and(|undo| undo.id == id) {
                self.archive_undo = None;
            }
            return;
        }
        let title = self.session_metadata(id, cx).display_title();
        let expire = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_WINDOW).await;
            let _ = this.update(cx, |this, cx| {
                if this.archive_undo.as_ref().is_some_and(|undo| undo.id == id) {
                    this.archive_undo = None;
                    cx.notify();
                }
            });
        });
        self.archive_undo = Some(ArchiveUndo {
            id,
            title,
            had_tab,
            _expire: expire,
        });
        // Closing the last pane can drop focus, and Undo needs a focused path.
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
    }

    pub(super) fn undo_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.archive_undo.is_none() || self.session_shortcut_blocked(window, cx) {
            cx.propagate();
            return;
        }
        self.restore_archived(cx);
    }

    fn restore_archived(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.archive_undo.take() else {
            return;
        };
        // The thread may have been deleted or unarchived another way since.
        if self
            .session_summaries
            .get(&undo.id)
            .is_some_and(|summary| summary.archived)
            && !self.deleting.contains(&undo.id)
        {
            self.set_archived(undo.id, false, undo.had_tab, cx);
        }
        cx.notify();
    }

    pub(super) fn archive_undo_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let undo = self.archive_undo.as_ref()?;
        let shortcut = if cfg!(target_os = "macos") {
            "⌘Z"
        } else {
            "Ctrl+Z"
        };
        Some(
            crate::notifications::surface::toast_frame()
                .id("archive-undo")
                .debug_selector(|| "archive-undo".into())
                .role(Role::Status)
                .child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon("archive", 14.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(content(0.8))
                                .child(format!("Archived {}", undo.title)),
                        )
                        .child(
                            div()
                                .id("archive-undo-button")
                                .debug_selector(|| "archive-undo-button".into())
                                .role(Role::Button)
                                .aria_label("Undo archive")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded(px(6.))
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(content(0.1))
                                .hover(|s| s.bg(content(0.15)))
                                .on_click(cx.listener(|this, _, _, cx| this.restore_archived(cx)))
                                .child("Undo")
                                .child(div().text_color(content(0.5)).child(shortcut)),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn modifier() -> &'static str {
        if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        }
    }

    fn setup(cx: &mut TestAppContext) {
        cx.update(|cx| {
            input::bind(cx);
            bind_keys(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
        });
    }

    fn archive(window: gpui::WindowHandle<Shell>, id: i64, cx: &mut TestAppContext) {
        window
            .update(cx, |shell, _, cx| shell.toggle_archive(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    fn undo(window: gpui::WindowHandle<Shell>, cx: &mut TestAppContext) {
        cx.simulate_keystrokes(window.into(), &format!("{}-z", modifier()));
        cx.run_until_parked();
    }

    #[gpui::test]
    fn undo_restores_the_thread_and_its_tab(cx: &mut TestAppContext) {
        setup(cx);
        let (window, first, _) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| shell.open(first, window, cx))
            .unwrap();
        archive(window, first, cx);
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions[&first].read(cx).summary.archived);
                assert!(!shell.workspace.open_sessions().contains(&first));
                assert!(shell.archive_undo.is_some());
            })
            .unwrap();
        undo(window, cx);
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&first].read(cx).summary.archived);
                assert!(!shell.session_summaries[&first].archived);
                assert!(shell.workspace.open_sessions().contains(&first));
                assert_eq!(shell.workspace.active, Some(first));
                assert!(shell.archive_undo.is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn undo_expires_and_only_restores_the_latest_archive(cx: &mut TestAppContext) {
        setup(cx);
        let (window, first, other) = super::super::tests::window(cx);
        archive(window, first, cx);
        cx.executor().advance_clock(UNDO_WINDOW);
        cx.run_until_parked();
        undo(window, cx);
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.archive_undo.is_none());
                assert!(shell.sessions[&first].read(cx).summary.archived);
            })
            .unwrap();

        window
            .update(cx, |shell, _, cx| shell.toggle_archive(first, cx))
            .unwrap();
        cx.run_until_parked();
        archive(window, first, cx);
        cx.executor().advance_clock(UNDO_WINDOW / 2);
        archive(window, other, cx);
        // Past the first archive's window, inside the second's.
        cx.executor()
            .advance_clock(UNDO_WINDOW / 2 + Duration::from_secs(1));
        cx.run_until_parked();
        undo(window, cx);
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions[&first].read(cx).summary.archived);
                assert!(!shell.sessions[&other].read(cx).summary.archived);
            })
            .unwrap();
    }

    #[gpui::test]
    fn undo_waits_while_an_overlay_owns_the_keyboard(cx: &mut TestAppContext) {
        setup(cx);
        let (window, first, active) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| shell.open(active, window, cx))
            .unwrap();
        archive(window, first, cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open_quick_open(false, window, cx)
            })
            .unwrap();
        undo(window, cx);
        window
            .update(cx, |shell, window, cx| {
                assert!(shell.sessions[&first].read(cx).summary.archived);
                assert!(shell.archive_undo.is_some());
                shell.close_quick_open(window, cx);
            })
            .unwrap();
        undo(window, cx);
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&first].read(cx).summary.archived);
            })
            .unwrap();
    }
}
