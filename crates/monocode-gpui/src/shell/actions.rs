//! Actions: saved project commands the footer runs in a dock terminal with
//! one click. The button shows the last action run; its chevron lists the
//! rest and opens the editor.
use super::*;
use crate::workspace::{ACTION_ICONS, ProjectAction};

pub(super) struct ActionDialog {
    /// The action being edited, or `None` for a new one.
    editing: Option<String>,
    name: Entity<input::Input>,
    command: Entity<input::Input>,
    icon: &'static str,
    error: Option<SharedString>,
    _events: Vec<Subscription>,
}

impl Shell {
    fn project_actions(&self) -> &[ProjectAction] {
        self.workspace
            .actions
            .get(&self.workspace.project)
            .map_or(&[], Vec::as_slice)
    }
    /// The action the footer button runs: the last one run, else the first.
    fn current_action(&self) -> Option<&ProjectAction> {
        let actions = self.project_actions();
        self.workspace
            .last_actions
            .get(&self.workspace.project)
            .and_then(|id| actions.iter().find(|a| a.id == *id))
            .or_else(|| actions.first())
    }

    /// Runs `id` in its own dock terminal, reusing that terminal while it
    /// sits idle at a prompt and opening a fresh one otherwise.
    pub(super) fn run_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.workspace.project.clone();
        if project.starts_with("remote://") {
            self.error = Some("Actions are not available on remote projects.".into());
            cx.notify();
            return;
        }
        let Some(action) = self.project_actions().iter().find(|a| a.id == id).cloned() else {
            return;
        };
        self.workspace
            .last_actions
            .insert(project.clone(), action.id.clone());
        self.ensure_terminals(&project, cx);
        let key = (project.clone(), action.id.clone());
        let live = self.terminals.get(&project).cloned().unwrap_or_default();
        let reuse = self
            .action_terminals
            .get(&key)
            .and_then(|weak| weak.upgrade())
            .and_then(|terminal| {
                let ix = live.iter().position(|t| *t == terminal)?;
                let t = terminal.read(cx);
                (t.foreground.is_none() && !t.exited).then_some((ix, terminal))
            });
        let reveal = self.workspace.preferences.action_opens_terminal;
        // The right panel shows the terminal instead of the dock.
        let in_panel = self.terminal_in_panel(cx);
        let viewport = self.viewport();
        let dock = self.workspace.docks.entry(project.clone()).or_default();
        let was_open = dock.open && !dock.terminals.is_empty();
        let terminal = match reuse {
            Some((ix, terminal)) => {
                if reveal {
                    dock.active = ix;
                    dock.open |= !in_panel;
                }
                terminal
            }
            None => {
                let cwd = self.terminal_cwd(cx);
                let dock = self.workspace.docks.entry(project.clone()).or_default();
                dock.size = dock.side.clamp(dock.size, viewport);
                dock.add(cwd.clone());
                // `add` opens the dock; a background run leaves it as it was.
                dock.open = (reveal && !in_panel) || was_open;
                let terminal = self.spawn_terminal(cwd, cx);
                self.terminals
                    .entry(project.clone())
                    .or_default()
                    .push(terminal.clone());
                self.action_terminals.insert(key, terminal.downgrade());
                terminal
            }
        };
        terminal.update(cx, |t, cx| t.run(&action.command, cx));
        if reveal && in_panel {
            self.show_panel_terminal(window, cx);
        } else if reveal {
            if let Some(dock) = self.workspace.docks.get(&project) {
                self.dock_tab_scroll.scroll_to_item(dock.active);
            }
            if !was_open {
                self.slide_dock(true, cx);
            }
            self.focus_terminal(window, cx);
            self.fit_sidebar(cx);
        }
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn actions_menu(&self) -> Vec<MenuItem> {
        let actions = self.project_actions();
        let mut items: Vec<MenuItem> = actions
            .iter()
            .map(|a| {
                MenuAction::new(SharedString::from(format!("run:{}", a.id)), a.name.clone())
                    .icon(a.icon())
                    .into()
            })
            .collect();
        if !actions.is_empty() {
            items.push(MenuItem::Separator);
            items.push(
                MenuAction::new("edit", "Edit action")
                    .icon("pencil-edit-02")
                    .submenu(
                        actions
                            .iter()
                            .map(|a| {
                                MenuAction::new(
                                    SharedString::from(format!("edit:{}", a.id)),
                                    a.name.clone(),
                                )
                                .icon(a.icon())
                            })
                            .collect(),
                    )
                    .into(),
            );
        }
        items.push(MenuAction::new("add", "Add action").icon("add-01").into());
        items
    }
    pub(super) fn run_actions_menu(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == "add" {
            self.open_action_dialog(None, window, cx);
        } else if let Some(id) = action.strip_prefix("run:") {
            self.run_action(id, window, cx);
        } else if let Some(id) = action.strip_prefix("edit:") {
            self.open_action_dialog(Some(id.to_owned()), window, cx);
        }
    }

    /// The footer's split button: run the current action, or pick another.
    pub(super) fn actions_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.workspace.project.starts_with("remote://") {
            return None;
        }
        let classic = self.workspace.boxy_layout();
        let current = self.current_action().cloned();
        let menu_open = self.menu_open(&MenuTarget::Actions);
        // Each part rounds fully inside the frame's 2px inset.
        let outer = if classic { 6. } else { 10. };
        let segment = |id: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .h_full()
                .flex()
                .items_center()
                .rounded(px(outer - 2.))
                .cursor_pointer()
                .text_color(content(0.4))
                // The whole button lights up; the part under the pointer more.
                .group_hover("footer-actions", |s| s.text_color(content(0.8)))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        };
        let pad = px(if classic { 4. } else { 6. });
        let main = match &current {
            Some(action) => {
                let id = action.id.clone();
                segment("footer-action-run")
                    .aria_label(SharedString::from(format!("Run {}", action.name)))
                    .aria_description(SharedString::from(action.command.clone()))
                    .px(pad)
                    .gap(px(6.))
                    .max_w(px(180.))
                    .min_w_0()
                    .child(icon(action.icon(), 12.).text_color(content(0.55)))
                    .child(div().min_w_0().truncate().child(action.name.clone()))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.run_action(&id, window, cx)),
                    )
            }
            None => segment("footer-action-add")
                .aria_label("Add action")
                .px(pad)
                .gap(px(6.))
                .child(icon("play", 12.).text_color(content(0.55)))
                .child("Actions")
                .on_click(
                    cx.listener(|this, _, window, cx| this.open_action_dialog(None, window, cx)),
                ),
        };
        let chevron = segment("footer-action-menu")
            .aria_label("Actions")
            .aria_expanded(menu_open)
            .relative()
            .px(px(3.))
            .when(menu_open, |d| d.bg(content(0.08)).text_color(content(1.)))
            .child(self.actions_anchor.probe())
            .child(icon("chevron-down", 12.).text_color(content(0.55)))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.menu_open(&MenuTarget::Actions) {
                    this.close_menu(cx);
                } else {
                    let anchor = Anchor::Slot(this.actions_anchor.clone());
                    this.open_menu(MenuTarget::Actions, anchor, window, cx);
                }
            }));
        Some(
            div()
                .id("footer-actions")
                .group("footer-actions")
                .role(Role::Group)
                .aria_label("Actions")
                .h(px(20.))
                .p(px(2.))
                .gap(px(1.))
                .flex()
                .flex_shrink_0()
                .items_stretch()
                .whitespace_nowrap()
                .rounded(px(outer))
                .when(menu_open, |d| d.bg(content(0.06)))
                .hover(|s| s.bg(content(0.06)))
                .child(main)
                .child(chevron)
                .into_any_element(),
        )
    }

    pub(super) fn open_action_dialog(
        &mut self,
        editing: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_preview(cx);
        let existing = editing
            .as_ref()
            .and_then(|id| self.project_actions().iter().find(|a| a.id == *id))
            .cloned();
        let name = cx.new(|cx| input::Input::field("Action name", "Test", content(0.3), cx));
        let command = cx.new(input::Input::action_command);
        if let Some(action) = &existing {
            name.update(cx, |i, cx| i.set_text(action.name.clone(), cx));
            command.update(cx, |i, cx| i.set_text(action.command.clone(), cx));
        }
        let clear = |this: &mut Self, cx: &mut Context<Self>| {
            if let Some(dialog) = &mut this.action_dialog
                && dialog.error.take().is_some()
            {
                cx.notify();
            }
        };
        let events = vec![
            cx.subscribe_in(
                &name,
                window,
                |this, _, _: &input::Submitted, window, cx| this.save_action(window, cx),
            ),
            cx.subscribe_in(
                &command,
                window,
                |this, _, _: &input::Submitted, window, cx| this.save_action(window, cx),
            ),
            cx.subscribe(&name, move |this, _, _: &input::Changed, cx| {
                clear(this, cx)
            }),
            cx.subscribe(&command, move |this, _, _: &input::Changed, cx| {
                clear(this, cx)
            }),
        ];
        window.focus(&name.read(cx).focus_handle(cx), cx);
        self.action_dialog = Some(ActionDialog {
            icon: existing
                .as_ref()
                .map_or(ACTION_ICONS[0], ProjectAction::icon),
            editing: existing.map(|a| a.id),
            name,
            command,
            error: None,
            _events: events,
        });
        cx.notify();
    }
    pub(super) fn close_action_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.action_dialog.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
        }
    }
    fn save_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.action_dialog else {
            return;
        };
        let name = dialog.name.read(cx).content.trim().to_owned();
        let command = dialog.command.read(cx).content.trim().to_owned();
        if name.is_empty() || command.is_empty() {
            dialog.error = Some("Give the action a name and a command.".into());
            cx.notify();
            return;
        }
        let action = ProjectAction {
            id: dialog
                .editing
                .clone()
                .unwrap_or_else(|| format!("{:x}", monocode_core::time::now_ms())),
            name,
            command,
            icon: dialog.icon.to_owned(),
        };
        let project = self.workspace.project.clone();
        let actions = self.workspace.actions.entry(project.clone()).or_default();
        match actions.iter_mut().find(|a| a.id == action.id) {
            Some(slot) => *slot = action.clone(),
            None => actions.push(action.clone()),
        }
        self.workspace.last_actions.insert(project, action.id);
        self.persist(cx);
        self.close_action_dialog(window, cx);
    }
    fn delete_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.action_dialog.as_ref().and_then(|d| d.editing.clone()) else {
            return;
        };
        let project = self.workspace.project.clone();
        if let Some(actions) = self.workspace.actions.get_mut(&project) {
            actions.retain(|a| a.id != id);
            if actions.is_empty() {
                self.workspace.actions.remove(&project);
            }
        }
        if self.workspace.last_actions.get(&project) == Some(&id) {
            self.workspace.last_actions.remove(&project);
        }
        self.action_terminals.remove(&(project, id));
        self.persist(cx);
        self.close_action_dialog(window, cx);
    }

    /// The Add/Edit action dialog, centred over the window.
    pub(super) fn action_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.action_dialog.as_ref()?;
        let editing = dialog.editing.is_some();
        let viewport = window.viewport_size();
        let width = 420f32.min(viewport.width.as_f32() - 24.);
        let anchor = Anchor::Center;
        let weak = cx.weak_entity();
        let label = |text: &'static str| {
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(0.8))
                .child(text)
        };
        let field = |input: Entity<input::Input>, multiline: bool| {
            div()
                .when(!multiline, |d| d.h(px(36.)).flex().items_center())
                .when(multiline, |d| d.py(px(8.)))
                .px(px(10.))
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(content(0.05))
                .text_size(px(13.))
                .text_color(content(1.))
                .child(input)
        };
        let icons = div()
            .flex()
            .gap(px(6.))
            .children(ACTION_ICONS.iter().enumerate().map(|(ix, name)| {
                let selected = *name == dialog.icon;
                div()
                    .id(("action-icon", ix))
                    .role(Role::RadioButton)
                    .aria_label(SharedString::from(*name))
                    .aria_toggled(if selected {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .size(px(34.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .cursor_pointer()
                    .map(|d| {
                        if selected {
                            d.border_color(accent_color()).bg(content(0.08))
                        } else {
                            d.border_color(content(0.1)).hover(|s| s.bg(content(0.06)))
                        }
                    })
                    .child(icon(name, 16.).text_color(if selected {
                        content(1.)
                    } else {
                        content(0.6)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(dialog) = &mut this.action_dialog {
                            dialog.icon = name;
                            cx.notify();
                        }
                    }))
            }));
        let title = if editing { "Edit action" } else { "Add action" };
        Some(
            div()
                .absolute()
                .size_full()
                .child(crate::modal::scrim("action-scrim"))
                .child(
                    popover("action-modal", anchor)
                        .modal_surface(self.workspace.boxy_layout())
                        .width(width)
                        .gap(0.)
                        .priority(60)
                        .on_dismiss(move |window, cx| {
                            let _ =
                                weak.update(cx, |this, cx| this.close_action_dialog(window, cx));
                        })
                        .child(
                            div()
                                .id("action-dialog")
                                .role(Role::Dialog)
                                .aria_label(title)
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .text_size(px(12.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(content(1.))
                                                .child(title),
                                        )
                                        .when(!editing, |d| {
                                            d.child(
                                                div()
                                                    .line_height(px(18.))
                                                    .text_color(content(0.6))
                                                    .child("Save a project command to run with one click."),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(label("Icon"))
                                        .child(icons),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(label("Name"))
                                        .child(field(dialog.name.clone(), false)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .child(label("Command"))
                                        .child(field(dialog.command.clone(), true))
                                        .child(match &dialog.error {
                                            Some(error) => div()
                                                .id("action-error")
                                                .role(Role::Alert)
                                                .text_size(px(11.))
                                                .text_color(rgb(0xf87171))
                                                .child(error.clone()),
                                            None => div()
                                                .id("action-help")
                                                .text_size(px(11.))
                                                .text_color(content(0.45))
                                                .child("Runs in a terminal at the project root."),
                                        }),
                                )
                                // Full-width buttons, the confirming one first.
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            crate::modal::primary_button(
                                                "action-save",
                                                if editing { "Save" } else { "Add action" },
                                            )
                                            .w_full()
                                            .cursor_pointer()
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.save_action(window, cx)
                                                }),
                                            ),
                                        )
                                        .when(editing, |d| {
                                            d.child(
                                                crate::modal::danger_ghost_button(
                                                    "action-delete",
                                                    "Delete action",
                                                )
                                                .debug_selector(|| "action-delete".into())
                                                .w_full()
                                                .bg(gpui::hsla(0., 0.66, 0.47, 0.08))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.delete_action(window, cx)
                                                })),
                                            )
                                        })
                                        .child(
                                            crate::modal::ghost_button("action-cancel", "Cancel")
                                                .w_full()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.close_action_dialog(window, cx)
                                                })),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    #[gpui::test]
    fn saved_actions_run_in_one_reused_terminal(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                shell.open_action_dialog(None, window, cx);
                let dialog = shell.action_dialog.as_mut().unwrap();
                dialog.icon = "rocket";
                let (name, command) = (dialog.name.clone(), dialog.command.clone());
                name.update(cx, |i, cx| i.set_text("Deploy".into(), cx));
                command.update(cx, |i, cx| i.set_text("echo hi".into(), cx));
                shell.save_action(window, cx);
                assert!(shell.action_dialog.is_none());
                let project = shell.workspace.project.clone();
                let action = shell.workspace.actions[&project][0].clone();
                assert_eq!((action.name.as_str(), action.icon()), ("Deploy", "rocket"));
                assert_eq!(
                    shell.current_action().map(|a| a.id.clone()),
                    Some(action.id.clone())
                );

                shell.run_action(&action.id, window, cx);
                assert_eq!(shell.terminals[&project].len(), 1);
                assert!(shell.workspace.docks[&project].open);
                // The terminal is still busy with the first run, so a second
                // run opens another; once idle, it is reused.
                shell.run_action(&action.id, window, cx);
                assert_eq!(shell.terminals[&project].len(), 2);
                for t in &shell.terminals[&project] {
                    t.update(cx, |t, _| t.foreground = None);
                }
                shell.run_action(&action.id, window, cx);
                assert_eq!(shell.terminals[&project].len(), 2);
                shell.open_action_dialog(Some(action.id), window, cx);
            })
            .unwrap();
        // The edit dialog, with its Delete button, renders.
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("action-delete").is_some());
    }
}
