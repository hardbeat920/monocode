//! Archive the focused conversation through the existing persistent archive flow.
use super::*;

impl Shell {
    pub(super) fn archive_active_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A held shortcut must not keep archiving the newly selected neighbour.
        // The workspace capture listener resets this on key release, including
        // releases handled by a child input or menu.
        if self.archive_shortcut_held {
            return;
        }
        if self.session_shortcut_blocked(window, cx) {
            cx.propagate();
            return;
        }
        let Some(id) = self.workspace.active else {
            cx.propagate();
            return;
        };
        let Some(session) = self.sessions.get(&id).map(|view| view.read(cx)) else {
            cx.propagate();
            return;
        };
        if session.summary.archived || session.archive_shortcut_blocked(cx) {
            cx.propagate();
            return;
        }
        self.archive_shortcut_held = true;
        self.toggle_archive(id, cx);
    }

    /// An overlay, another view or a focused side surface owns the keyboard.
    pub(super) fn session_shortcut_blocked(&self, window: &Window, cx: &App) -> bool {
        self.app_view_open()
            || self.settings.is_some()
            || self.app_search.is_some()
            || self.mono_active(cx)
            || self.quick_open.is_some()
            || self.image_lightbox.is_some()
            || self.menu.is_some()
            || self.renaming.is_some()
            || self.link_dialog.is_some()
            || self.action_dialog.is_some()
            || self.background_dialog.is_some()
            || self.resources.read(cx).is_open()
            || self.review_focused
            || self.dock_focused(window, cx).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyUpEvent, Keystroke, TestAppContext};

    fn shortcut() -> String {
        format!(
            "{}-shift-a",
            if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            }
        )
    }

    fn setup(cx: &mut TestAppContext) {
        cx.update(|cx| {
            input::bind(cx);
            bind_keys(cx);
            crate::popover::bind(cx);
            cx.set_reduce_motion(true);
        });
    }

    #[gpui::test]
    fn archive_shortcut_preserves_history_and_ignores_a_held_key(cx: &mut TestAppContext) {
        setup(cx);
        let (window, first, other) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| shell.open(first, window, cx))
            .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        cx.run_until_parked();
        let next = window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions[&first].read(cx).summary.archived);
                assert!(!shell.workspace.open_sessions().contains(&first));
                assert!(
                    !locked(&shell.store)
                        .unwrap()
                        .load(first)
                        .unwrap()
                        .rows
                        .is_empty()
                );
                assert!(!shell.sessions[&other].read(cx).summary.archived);
                shell
                    .workspace
                    .active
                    .expect("archiving selects another pane")
            })
            .unwrap();
        // The test keyboard sends key-down events only, reproducing auto-repeat.
        cx.simulate_keystrokes(window.into(), &shortcut());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&next].read(cx).summary.archived);
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyUp(KeyUpEvent {
                    keystroke: Keystroke::parse(&shortcut()).unwrap(),
                }),
                cx,
            );
        })
        .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.session_summaries[&next].archived);
            })
            .unwrap();
    }

    #[gpui::test]
    fn archive_shortcut_respects_disabled_and_remapped_bindings(cx: &mut TestAppContext) {
        setup(cx);
        let (window, first, active) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open(active, window, cx);
                shell
                    .workspace
                    .preferences
                    .keybindings
                    .insert("Session: Archive".into(), None);
                crate::settings::apply_keybindings(&shell.workspace.preferences, cx);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&active].read(cx).summary.archived);
                let modifier = if cfg!(target_os = "macos") {
                    "cmd"
                } else {
                    "ctrl"
                };
                shell
                    .workspace
                    .preferences
                    .keybindings
                    .insert("Session: Archive".into(), Some(format!("{modifier}-alt-a")));
                crate::settings::apply_keybindings(&shell.workspace.preferences, cx);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&active].read(cx).summary.archived)
            })
            .unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-alt-a"));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions[&active].read(cx).summary.archived);
                assert!(!shell.sessions[&first].read(cx).summary.archived);
            })
            .unwrap();
    }

    #[gpui::test]
    fn archive_shortcut_does_not_archive_behind_a_picker_or_composer_menu(cx: &mut TestAppContext) {
        setup(cx);
        let (window, _, active) = super::super::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open(active, window, cx);
                shell.open_quick_open(false, window, cx);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        window
            .update(cx, |shell, window, cx| {
                assert!(!shell.sessions[&active].read(cx).summary.archived);
                shell.close_quick_open(window, cx);
                shell.sessions[&active]
                    .update(cx, |session, cx| session.toggle_model_menu(window, cx));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &shortcut());
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(!shell.sessions[&active].read(cx).summary.archived)
            })
            .unwrap();
    }
}
