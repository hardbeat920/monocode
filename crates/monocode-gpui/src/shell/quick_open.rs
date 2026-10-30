use super::*;
use crate::quick_open::{Command, Picked, QuickOpen};
pub(super) struct Open {
    picker: Entity<QuickOpen>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}
impl Shell {
    pub(super) fn close_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.quick_open.take() {
            if let Some(restore) = open.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_active(window, cx);
            }
        }
        cx.notify();
    }
    pub(super) fn open_quick_open(
        &mut self,
        palette: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restore = self
            .quick_open
            .as_ref()
            .and_then(|o| o.restore.clone())
            .or_else(|| window.focused(cx));
        self.close_menu(cx);
        self.close_preview(cx);
        let picker = cx.new(|cx| QuickOpen::new(self.working_copy().to_owned(), palette, cx));
        let events = cx.subscribe_in(&picker, window, |this, _, event: &Picked, window, cx| {
            this.close_quick_open(window, cx);
            match event {
                Picked::File(path) => {
                    let cwd = this.working_copy().to_owned();
                    this.open_review(
                        crate::review::ReviewTab::File {
                            cwd,
                            path: path.clone(),
                        },
                        window,
                        cx,
                    );
                }
                Picked::Command(command) => match command {
                    Command::New => window.dispatch_action(Box::new(NewCodex), cx),
                    Command::Project => window.dispatch_action(Box::new(OpenProject), cx),
                    Command::Model => window.dispatch_action(Box::new(SwitchModel), cx),
                    Command::Effort => window.dispatch_action(Box::new(ChangeEffort), cx),
                    Command::Settings => window.dispatch_action(Box::new(OpenSettings), cx),
                    Command::Sidebar => window.dispatch_action(Box::new(ToggleSidebar), cx),
                    Command::Panel => window.dispatch_action(Box::new(ToggleRightPanel), cx),
                    Command::Terminal => window.dispatch_action(Box::new(ToggleTerminal), cx),
                    Command::Layout => window.dispatch_action(Box::new(ToggleLayout), cx),
                    Command::WorkspaceStyle => {
                        window.dispatch_action(Box::new(ToggleWorkspaceStyle), cx)
                    }
                    Command::Plan | Command::Operator | Command::Orchestrator => {
                        if let Some(view) =
                            this.workspace.active.and_then(|id| this.sessions.get(&id))
                        {
                            view.update(cx, |v, cx| {
                                v.toggle_mode(
                                    match command {
                                        Command::Plan => "plan",
                                        Command::Operator => "operator",
                                        _ => "orchestrator",
                                    },
                                    cx,
                                )
                            });
                        }
                    }
                },
                Picked::Dismiss => (),
            }
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.quick_open = Some(Open {
            picker,
            restore,
            _events: events,
        });
        cx.notify();
    }
    pub(super) fn quick_open_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.quick_open.as_ref()?;
        let viewport = window.viewport_size();
        let width = 560f32.min(viewport.width.as_f32() - 32.);
        let anchor = Anchor::Point(gpui::point(
            px((viewport.width.as_f32() - width) / 2.),
            px((viewport.height.as_f32() * 0.16).max(32.)),
        ));
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("quick-open-scrim"))
                .child(
                    popover("quick-open-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .priority(80)
                        .on_dismiss(move |window, cx| {
                            let _ = weak.update(cx, |this, cx| this.close_quick_open(window, cx));
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
    #[gpui::test]
    fn titlebar_search_opens_files_and_sidebar_search_opens_global_search(cx: &mut TestAppContext) {
        let (window, _, active) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                let input = shell.sessions[&active].read(cx).input.clone();
                input.update(cx, |input, cx| input.set_text("keep this draft".into(), cx));
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        for classic in [false, true] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.classic_layout = classic;
                    shell.invalidate_chrome(cx);
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            let search = visual.debug_bounds("titlebar-file-search").unwrap();
            visual.simulate_click(search.center(), Default::default());
            visual.run_until_parked();
            window
                .update(visual, |shell, w, cx| {
                    assert!(shell.quick_open.is_some());
                    assert!(shell.app_search.is_none());
                    assert_eq!(
                        shell.sessions[&active].read(cx).input.read(cx).content,
                        "keep this draft"
                    );
                    shell.close_quick_open(w, cx);
                })
                .unwrap();
            visual.run_until_parked();
            let search = visual.debug_bounds("sidebar-search-field").unwrap();
            visual.simulate_click(search.center(), Default::default());
            visual.run_until_parked();
            window
                .update(visual, |shell, w, cx| {
                    assert!(shell.app_search.is_some());
                    assert!(shell.quick_open.is_none());
                    shell.close_app_search(w, cx);
                })
                .unwrap();
            visual.run_until_parked();
        }
    }
    #[gpui::test]
    fn cmd_p_and_command_palette_preserve_the_composer_draft(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::input::bind(cx);
            super::super::bind_keys(cx);
            crate::popover::bind(cx);
        });
        let (window, _, active) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, w, cx| {
                let input = shell.sessions[&active].read(cx).input.clone();
                input.update(cx, |i, cx| i.set_text("keep this draft".into(), cx));
                w.focus(&input.focus_handle(cx), cx);
            })
            .unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-p"));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| assert!(shell.quick_open.is_some()))
            .unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert!(shell.quick_open.is_none());
                let input = &shell.sessions[&active].read(cx).input;
                assert_eq!(input.read(cx).content, "keep this draft");
                assert!(input.focus_handle(cx).is_focused(w));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), &format!("{modifier}-shift-p"));
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell
                        .quick_open
                        .as_ref()
                        .unwrap()
                        .picker
                        .read(cx)
                        .search
                        .read(cx)
                        .content,
                    ">"
                )
            })
            .unwrap();
    }
}
