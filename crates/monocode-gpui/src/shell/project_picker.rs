use super::*;
use crate::project_picker::{Picked, Project, ProjectPicker};
pub(super) struct Open {
    picker: Entity<ProjectPicker>,
    pub(super) restore: Option<FocusHandle>,
    _events: Subscription,
}
impl Shell {
    pub(super) fn close_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.project_picker.take() {
            if let Some(restore) = open.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn open_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore = self
            .project_picker
            .as_ref()
            .and_then(|o| o.restore.clone())
            .or_else(|| self.quick_open.take().and_then(|o| o.restore))
            .or_else(|| window.focused(cx));
        self.close_menu(cx);
        self.close_preview(cx);
        let projects = self
            .workspace
            .projects
            .iter()
            .map(|path| Project {
                path: path.clone(),
                label: self.project_label(path),
                appearance: self.appearance(path),
                current: *path == self.workspace.project,
            })
            .collect();
        let picker = cx.new(|cx| ProjectPicker::new(projects, cx));
        let events = cx.subscribe_in(&picker, window, |this, _, event: &Picked, window, cx| {
            this.close_project_picker(window, cx);
            if let Picked::Project(path) = event {
                this.new_conversation_in(path.clone(), window, cx);
            }
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.project_picker = Some(Open {
            picker,
            restore,
            _events: events,
        });
        cx.notify();
    }
    /// Switching projects already lands on a blank conversation when one is
    /// free or being made, so only start another when it lands on real work.
    pub(super) fn new_conversation_in(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path != self.workspace.project {
            self.select_project(path.clone(), window, cx);
            let pending = format!("{path}\0");
            if self
                .pending_project_blanks
                .iter()
                .any(|key| key.starts_with(&pending))
                || self
                    .workspace
                    .active
                    .is_some_and(|id| self.is_blank_session(id, cx))
            {
                return;
            }
        }
        self.create(self.default_kind(), cx);
    }
    fn is_blank_session(&self, pane: i64, cx: &App) -> bool {
        let id = self.workspace.session_for_pane(pane);
        let s = self.session_metadata(id, cx);
        !s.summary.archived
            && !s.summary.has_messages
            && !s.busy
            && !s.has_saved_draft()
            && self
                .sessions
                .get(&id)
                .is_some_and(|v| v.read(cx).input.read(cx).content.is_empty())
    }
    pub(super) fn project_picker_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.project_picker.as_ref()?;
        let viewport = window.viewport_size();
        let width = 520f32.min(viewport.width.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() * 0.16).max(32.)),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("project-picker-scrim"))
                .child(
                    popover("project-picker-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .priority(80)
                        .on_dismiss(move |window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.close_project_picker(window, cx));
                        })
                        .child(open.picker.clone()),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    fn active_cwd(shell: &Shell, cx: &App) -> (i64, String, bool) {
        let id = shell.workspace.active.expect("an active session");
        let summary = &shell.sessions[&id].read(cx).summary;
        (id, summary.cwd.clone(), summary.has_messages)
    }
    #[gpui::test]
    fn cmd_shift_n_starts_a_conversation_in_the_searched_project(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::input::bind(cx);
            super::super::bind_keys(cx);
            crate::popover::bind(cx);
        });
        let (window, busy, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.add_project("/work/empty");
                shell.focus_active(w, cx);
            })
            .unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        let pick = |cx: &mut TestAppContext, query: &str| {
            cx.simulate_keystrokes(window.into(), &format!("{modifier}-shift-n"));
            cx.run_until_parked();
            window
                .update(cx, |shell, _, _| assert!(shell.project_picker.is_some()))
                .unwrap();
            cx.simulate_input(window.into(), query);
            cx.simulate_keystrokes(window.into(), "enter");
            cx.run_until_parked();
        };
        // The other project's last tab holds history, so a fresh chat opens.
        pick(cx, "another");
        let count = window
            .update(cx, |shell, _, cx| {
                assert!(shell.project_picker.is_none());
                assert_eq!(shell.workspace.project, "/work/another-project");
                let (id, cwd, has_messages) = active_cwd(shell, cx);
                assert_ne!(id, busy);
                assert_eq!(cwd, "/work/another-project");
                assert!(!has_messages);
                shell.sessions.len()
            })
            .unwrap();
        // A project with no chats gets exactly one blank, not two.
        pick(cx, "empty");
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(active_cwd(shell, cx).1, "/work/empty");
                assert_eq!(shell.sessions.len(), count + 1);
            })
            .unwrap();
        // Escape closes it without switching.
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-shift-n"));
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.project_picker.is_none());
                assert_eq!(shell.workspace.project, "/work/empty");
            })
            .unwrap();
    }
}
