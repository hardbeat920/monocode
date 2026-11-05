//! Terminals share the session pane tree: a dock tab pulled off the dock's
//! strip becomes a pane beside a chat, or its own title tab. The live shell
//! moves with it, so a running command keeps going. Like a file pane, a
//! terminal pane borrows its owner session's project and worktree scope.
use super::*;
use crate::workspace::TerminalPane;

/// The pointer's ghost while a terminal is dragged.
pub(super) fn terminal_drag_ghost(position: Point<Pixels>, title: String) -> AnyElement {
    div()
        .absolute()
        .left(position.x + px(14.))
        .top(position.y + px(10.))
        .max_w(px(240.))
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded_md()
        .border_1()
        .border_color(content(0.1))
        .bg(pane_fill())
        .shadow_md()
        .text_size(px(12.))
        .text_color(content(1.))
        .child(icon("terminal", 14.).text_color(content(0.7)))
        .child(div().min_w_0().truncate().child(title))
        .into_any_element()
}

impl Shell {
    /// Pane and title-tab closes must ask before dropping a running shell,
    /// just like closing a terminal in the dock.
    pub(super) fn confirm_close_terminal_panes(
        &mut self,
        panes: &[i64],
        window: &mut Window,
        cx: &mut Context<Self>,
        close: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let running = panes
            .iter()
            .filter_map(|pane| self.terminal_pane_views.get(pane))
            .filter_map(|terminal| {
                let terminal = terminal.read(cx);
                let process = terminal.foreground.as_deref()?;
                Some((project_name(&terminal.cwd), process.to_owned()))
            })
            .collect::<Vec<_>>();
        let message = match running.as_slice() {
            [] => {
                close(self, window, cx);
                return;
            }
            [(label, process)] => {
                format!("\"{process}\" is still running in {label}. Close this terminal anyway?")
            }
            many => format!(
                "These terminals are still running:\n{}\n\nClose them anyway?",
                many.iter()
                    .map(|(label, process)| format!("• {label} ({process})"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &message,
            None,
            &["Close", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update_in(cx, close);
            }
        })
        .detach();
    }

    /// A terminal pane's shell, reopened in its directory after a restart.
    fn terminal_pane_view(
        &mut self,
        pane: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Terminal>> {
        if let Some(view) = self.terminal_pane_views.get(&pane) {
            return Some(view.clone());
        }
        let cwd = self.workspace.terminal_panes.get(&pane)?.cwd.clone();
        let view = self.spawn_terminal(cwd, cx);
        self.terminal_pane_views.insert(pane, view.clone());
        Some(view)
    }

    pub(super) fn terminal_pane_title(&self, pane: i64, cx: &App) -> Option<String> {
        let state = self.workspace.terminal_panes.get(&pane)?;
        Some(
            self.terminal_pane_views
                .get(&pane)
                .map(|t| t.read(cx).title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| project_name(&state.cwd)),
        )
    }

    /// The title a dragged dock tab or terminal pane carries under the
    /// pointer; None for anything else.
    pub(super) fn dragged_terminal_title(&self, source: DragSource, cx: &App) -> Option<String> {
        match source {
            DragSource::DockTerminal(ix) => self
                .terminals
                .get(&self.workspace.project)?
                .get(ix)
                .map(|t| t.read(cx).title.clone()),
            DragSource::Pane(id) => self.terminal_pane_title(id, cx),
            _ => None,
        }
    }

    /// The session a pulled-out terminal borrows its scope from: the
    /// focused chat, else the project's last focused one.
    fn terminal_owner(&self) -> Option<i64> {
        let known = |id: i64| self.session_summaries.contains_key(&id).then_some(id);
        self.workspace
            .active
            .and_then(|id| known(self.workspace.session_for_pane(id)))
            .or_else(|| {
                let focus = *self.workspace.project_focus.get(&self.workspace.project)?;
                known(self.workspace.session_for_pane(focus))
            })
    }

    /// A dock tab dropped on a pane edge or on the title strip leaves the
    /// dock with its shell. Anywhere else it stays in the dock.
    pub(super) fn drop_dock_terminal(
        &mut self,
        ix: usize,
        over: Option<(i64, Edge)>,
        insert: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if over.is_none() && insert.is_none() {
            return;
        }
        let Some(owner) = self.terminal_owner() else {
            return;
        };
        let project = self.workspace.project.clone();
        let Some(terminal) = self
            .terminals
            .get(&project)
            .and_then(|t| t.get(ix))
            .cloned()
        else {
            return;
        };
        let over = over.map(|(target, edge)| (self.resolve_mono_target(target, cx), edge));
        // Never take the shell out of the dock without somewhere to put it:
        // a terminal pane no tab holds is pruned, and its shell stops.
        if insert.is_none()
            && over.is_none_or(|(target, _)| self.workspace.tab_of(target).is_none())
        {
            return;
        }
        let before = self.active_leaves();
        if let Some(live) = self.terminals.get_mut(&project) {
            live.remove(ix);
        }
        self.dock_shifts.remove(&terminal.entity_id());
        if let Some(dock) = self.workspace.docks.get_mut(&project)
            && !dock.close(ix)
        {
            dock.open = false;
            self.slide_dock(false, cx);
            // The panel's Terminal tab is never left empty: a fresh shell
            // takes the place of the one pulled out, and the panel stays.
            if self.terminal_in_panel(cx) {
                self.ensure_panel_terminal(cx);
            }
        }
        let cwd = terminal.read(cx).cwd.clone();
        let pane = self.workspace.next_pane_id();
        self.workspace
            .terminal_panes
            .insert(pane, TerminalPane { owner, cwd });
        self.terminal_pane_views.insert(pane, terminal);
        if let Some(to) = insert {
            self.workspace.open(pane);
            self.reorder_title_tab(&TitleTab::Session(pane), to, cx);
            self.pane_motion = None;
        } else if let Some((target, edge)) = over {
            self.workspace.place(pane, target, edge);
            self.begin_pane_motion(before, cx);
        }
        self.fit_sidebar(cx);
        self.open_terminal_pane(pane, window, cx);
    }

    /// Title bar placement: a fresh shell opens as its own title tab. With
    /// no session to borrow a scope from, it opens in the dock instead.
    pub(super) fn new_title_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.terminal_owner() else {
            self.new_terminal(window, cx);
            return;
        };
        let cwd = self.terminal_cwd(cx);
        let terminal = self.spawn_terminal(cwd.clone(), cx);
        let pane = self.workspace.next_pane_id();
        self.workspace
            .terminal_panes
            .insert(pane, TerminalPane { owner, cwd });
        self.terminal_pane_views.insert(pane, terminal);
        self.workspace.open(pane);
        self.pane_motion = None;
        self.fit_sidebar(cx);
        self.open_terminal_pane(pane, window, cx);
    }

    /// Title bar placement, cmd-shift-j: a fresh shell as a pane beside the
    /// focused one. With no focused pane it opens as its own title tab.
    pub(super) fn split_title_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app_view_open() {
            return;
        }
        let target = self
            .workspace
            .active
            .filter(|id| self.workspace.tab_of(*id).is_some());
        let (Some(owner), Some(_)) = (self.terminal_owner(), target) else {
            self.new_title_terminal(window, cx);
            return;
        };
        let cwd = self.terminal_cwd(cx);
        let terminal = self.spawn_terminal(cwd.clone(), cx);
        let pane = self.workspace.next_pane_id();
        self.workspace
            .terminal_panes
            .insert(pane, TerminalPane { owner, cwd });
        self.terminal_pane_views.insert(pane, terminal);
        let before = self.active_leaves();
        self.workspace.split(pane, SplitDir::Right);
        self.begin_pane_motion(before, cx);
        self.fit_sidebar(cx);
        self.open_terminal_pane(pane, window, cx);
    }

    /// Focus a terminal pane, showing the tab that holds it.
    pub(super) fn open_terminal_pane(
        &mut self,
        pane: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.workspace.terminal_panes.get(&pane).map(|t| t.owner) else {
            return;
        };
        let Some(tab) = self.workspace.tab_of(pane) else {
            return;
        };
        if let Some(session) = self.ensure_session(owner, cx) {
            self.workspace.project = session.read(cx).summary.cwd.clone();
        }
        if let Some(monos) = &self.monos {
            monos.update(cx, |m, cx| m.close(cx));
        }
        self.navigation_revision += 1;
        self.leave_app_views(cx);
        self.leave_app_search(cx);
        self.close_preview(cx);
        self.workspace.open(pane);
        self.workspace
            .project_focus
            .insert(self.workspace.project.clone(), pane);
        self.history.record(pane);
        self.review_focused = false;
        self.remember_title_tab(TitleTab::Session(tab));
        self.reveal_active_tab(cx);
        self.focus_terminal_pane(window, cx);
        self.persist(cx);
        cx.notify();
    }

    /// The focused terminal pane's shell takes the keyboard.
    pub(super) fn focus_terminal_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self
            .workspace
            .active
            .filter(|id| self.workspace.terminal_panes.contains_key(id))
        else {
            return false;
        };
        match self.terminal_pane_view(pane, cx) {
            Some(view) => window.focus(&view.focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
        true
    }

    /// A terminal pane: its split header over the shell.
    pub(super) fn terminal_pane_surface(
        &mut self,
        pane: i64,
        split: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.terminal_pane_view(pane, cx);
        div()
            .id(("terminal-pane", pane.unsigned_abs() as usize))
            .debug_selector(move || format!("terminal-pane-{pane}"))
            .role(Role::Region)
            .aria_label("Terminal")
            .size_full()
            .flex()
            .flex_col()
            .when_some(split.filter(|p| *p > 0.), |d, p| {
                d.child(self.split_header(pane, p, true, cx))
            })
            .child(div().flex_1().min_h_0().min_w_0().children(
                // The grid shapes every line; redraw it only when the
                // terminal itself changes.
                view.map(|t| t.cached(StyleRefinement::default().size_full())),
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pane_layout::{SplitDir, leaf},
        shell::tests::window,
        workspace::Workspace,
    };
    use gpui::{EntityId, Modifiers, TestAppContext, VisualTestContext, WindowHandle, point};

    /// Opens the dock on session `b` and returns the visual context, the
    /// project and the dock's one shell.
    fn dock_with_one_terminal(
        window: WindowHandle<Shell>,
        b: i64,
        cx: &mut TestAppContext,
    ) -> (VisualTestContext, String, EntityId) {
        cx.update(|cx| {
            bind_keys(cx);
            crate::terminal::bind(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        visual.simulate_keystrokes("cmd-j");
        visual.run_until_parked();
        let (project, shell_id) = window
            .update(&mut visual, |shell, _, _| {
                let project = shell.workspace.project.clone();
                (project.clone(), shell.terminals[&project][0].entity_id())
            })
            .unwrap();
        (visual, project, shell_id)
    }

    /// Press a dock tab, pull it off the strip, and release it at `to`.
    fn pull_dock_tab(visual: &mut VisualTestContext, to: Point<Pixels>) {
        let tab = visual.debug_bounds("terminal-tab-0").unwrap().center();
        visual.simulate_mouse_down(tab, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(
            point(tab.x, tab.y - DOCK_PULL_TEST),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
    }
    const DOCK_PULL_TEST: Pixels = px(60.);

    fn title_terminal(
        cx: &mut TestAppContext,
        split: bool,
    ) -> (WindowHandle<Shell>, VisualTestContext, i64, i64) {
        let (window, _, owner) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            crate::terminal::bind(cx);
            cx.set_reduce_motion(true);
        });
        let pane = window
            .update(cx, |shell, w, cx| {
                shell.workspace.preferences.set_terminal_placement("title");
                shell.open(owner, w, cx);
                if split {
                    shell.split_title_terminal(w, cx);
                } else {
                    shell.new_title_terminal(w, cx);
                }
                shell.workspace.active.unwrap()
            })
            .unwrap();
        let visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        (window, visual, owner, pane)
    }

    #[gpui::test]
    fn cmd_w_asks_before_closing_a_running_terminal_tab_or_split_pane(cx: &mut TestAppContext) {
        for split in [false, true] {
            let (window, mut visual, owner, pane) = title_terminal(cx, split);
            let terminal = window
                .update(&mut visual, |shell, _, cx| {
                    let terminal = shell.terminal_pane_views[&pane].clone();
                    terminal.update(cx, |terminal, _| terminal.foreground = Some("sleep".into()));
                    terminal.downgrade()
                })
                .unwrap();
            for answer in ["Cancel", "Close"] {
                visual.simulate_keystrokes("cmd-w");
                visual.run_until_parked();
                assert!(visual.has_pending_prompt());
                assert!(visual.pending_prompt().unwrap().0.contains("sleep"));
                window
                    .update(&mut visual, |shell, _, _| {
                        assert_eq!(shell.workspace.active, Some(pane));
                        assert!(shell.workspace.tab_of(pane).is_some());
                        assert_eq!(
                            shell.terminal_pane_views[&pane],
                            terminal.upgrade().unwrap()
                        );
                    })
                    .unwrap();
                visual.simulate_prompt_answer(answer);
                visual.run_until_parked();
                assert!(!visual.has_pending_prompt());
                window
                    .update(&mut visual, |shell, _, _| {
                        let kept = answer == "Cancel";
                        assert_eq!(shell.workspace.terminal_panes.contains_key(&pane), kept);
                        assert_eq!(shell.terminal_pane_views.contains_key(&pane), kept);
                        assert_eq!(terminal.upgrade().is_some(), kept);
                        assert!(shell.workspace.tab_of(owner).is_some());
                    })
                    .unwrap();
            }
        }
    }

    #[gpui::test]
    fn cmd_w_closes_idle_terminal_tabs_and_split_panes_without_prompting(cx: &mut TestAppContext) {
        for split in [false, true] {
            let (window, mut visual, owner, pane) = title_terminal(cx, split);
            visual.simulate_keystrokes("cmd-w");
            visual.run_until_parked();
            assert!(!visual.has_pending_prompt());
            window
                .update(&mut visual, |shell, _, _| {
                    assert!(!shell.workspace.terminal_panes.contains_key(&pane));
                    assert!(!shell.terminal_pane_views.contains_key(&pane));
                    assert!(shell.workspace.tab_of(owner).is_some());
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn closing_title_tabs_asks_for_all_running_terminal_panes_before_changing_tabs(
        cx: &mut TestAppContext,
    ) {
        let (window, mut visual, _, pane) = title_terminal(cx, true);
        let (tabs, other) = window
            .update(&mut visual, |shell, w, cx| {
                shell.new_title_terminal(w, cx);
                let other = shell.workspace.active.unwrap();
                for (id, process) in [(pane, "sleep"), (other, "vite")] {
                    shell.terminal_pane_views[&id]
                        .update(cx, |terminal, _| terminal.foreground = Some(process.into()));
                }
                (shell.visible_tabs(cx), other)
            })
            .unwrap();
        for answer in ["Cancel", "Close"] {
            window
                .update(&mut visual, |shell, w, cx| {
                    shell.close_title_tabs(&tabs, None, w, cx);
                    assert_eq!(shell.visible_tabs(cx), tabs);
                    assert!(shell.workspace.tab_of(pane).is_some());
                    assert!(shell.workspace.tab_of(other).is_some());
                })
                .unwrap();
            visual.run_until_parked();
            assert!(visual.has_pending_prompt());
            let message = visual.pending_prompt().unwrap().0;
            assert!(message.contains("sleep") && message.contains("vite"));
            visual.simulate_prompt_answer(answer);
            visual.run_until_parked();
            assert!(!visual.has_pending_prompt());
            window
                .update(&mut visual, |shell, _, cx| {
                    if answer == "Cancel" {
                        assert_eq!(shell.visible_tabs(cx), tabs);
                        assert_eq!(shell.workspace.active, Some(other));
                        assert_eq!(shell.terminal_pane_views.len(), 2);
                    } else {
                        assert!(shell.workspace.terminal_panes.is_empty());
                        assert!(shell.terminal_pane_views.is_empty());
                        assert!(shell.workspace.tab_of(pane).is_none());
                        assert!(shell.workspace.tab_of(other).is_none());
                    }
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn closing_a_chat_pane_keeps_its_running_terminal_without_prompting(cx: &mut TestAppContext) {
        let (window, mut visual, owner, pane) = title_terminal(cx, true);
        window
            .update(&mut visual, |shell, w, cx| {
                shell.terminal_pane_views[&pane]
                    .update(cx, |terminal, _| terminal.foreground = Some("sleep".into()));
                shell.close_pane(owner, w, cx);
                assert!(shell.workspace.tab_of(owner).is_none());
                assert!(shell.workspace.tab_of(pane).is_some());
                assert!(shell.terminal_pane_views.contains_key(&pane));
            })
            .unwrap();
        visual.run_until_parked();
        assert!(!visual.has_pending_prompt());
    }

    #[gpui::test]
    fn a_dock_tab_pulled_onto_a_pane_edge_splits_with_its_running_shell(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let (mut visual, project, shell_id) = dock_with_one_terminal(window, b, cx);
        let panes = window
            .update(&mut visual, |shell, _, _| shell.pane_bounds.get().unwrap())
            .unwrap();
        let right = point(
            panes.right() - panes.size.width * 0.05,
            panes.top() + panes.size.height * 0.3,
        );
        pull_dock_tab(&mut visual, right);
        let pane = window
            .update(&mut visual, |shell, _, _| {
                assert_eq!(shell.workspace.terminal_panes.len(), 1);
                let (&pane, state) = shell.workspace.terminal_panes.iter().next().unwrap();
                assert_eq!(state.owner, b);
                // The very shell moved; nothing was respawned.
                assert_eq!(shell.terminal_pane_views[&pane].entity_id(), shell_id);
                assert!(shell.terminals[&project].is_empty());
                assert!(!shell.workspace.docks[&project].open);
                let tab = shell.workspace.tab_of(pane).unwrap();
                assert_eq!(shell.workspace.tab_of(b), Some(tab));
                assert_eq!(shell.workspace.layout(tab).leaf_ids(), [b, pane]);
                assert_eq!(shell.workspace.active, Some(pane));
                pane
            })
            .unwrap();
        let selector = Box::leak(format!("terminal-pane-{pane}").into_boxed_str());
        assert!(visual.debug_bounds(selector).is_some());
        // Closing the pane stops its shell.
        window
            .update(&mut visual, |shell, w, cx| {
                shell.close_pane(pane, w, cx);
                assert!(shell.workspace.terminal_panes.is_empty());
                assert!(shell.terminal_pane_views.is_empty());
                assert_eq!(
                    shell
                        .workspace
                        .layout(shell.workspace.tab_of(b).unwrap())
                        .leaf_ids(),
                    [b]
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_dock_tab_pulled_onto_the_title_strip_becomes_its_own_tab(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let (mut visual, project, shell_id) = dock_with_one_terminal(window, b, cx);
        let strip = window
            .update(&mut visual, |shell, _, _| shell.tab_scroll.bounds())
            .unwrap();
        pull_dock_tab(&mut visual, point(strip.right() - px(4.), strip.center().y));
        window
            .update(&mut visual, |shell, _, cx| {
                let (&pane, _) = shell.workspace.terminal_panes.iter().next().unwrap();
                assert_eq!(shell.terminal_pane_views[&pane].entity_id(), shell_id);
                assert!(shell.terminals[&project].is_empty());
                assert_eq!(shell.workspace.tab_of(pane), Some(pane));
                assert!(!shell.workspace.layout(pane).is_split());
                let visible = shell.visible_tabs(cx);
                assert_eq!(visible.last(), Some(&TitleTab::Session(pane)));
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(pane)));
                assert_eq!(
                    shell.tab_copy(pane, cx).headline,
                    shell.terminal_pane_title(pane, cx).unwrap()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn pulling_the_panels_last_terminal_out_leaves_a_fresh_one_behind(cx: &mut TestAppContext) {
        use crate::right_panel::RightTab;
        let (window, _, b) = window(cx);
        for edge in [None, Some(Edge::Right)] {
            window
                .update(cx, |shell, w, cx| {
                    cx.set_reduce_motion(true);
                    shell.workspace.classic_layout = false;
                    shell.workspace.new_layout_boxy = true;
                    shell.layout_changed(cx);
                    shell.workspace.preferences.set_terminal_placement("panel");
                    shell.open(b, w, cx);
                    shell.show_panel_terminal(w, cx);
                    let project = shell.workspace.project.clone();
                    assert_eq!(shell.terminals[&project].len(), 1);
                    let pulled = shell.terminals[&project][0].clone();
                    // Onto the title strip, then onto the chat's right edge.
                    let (over, insert) = match edge {
                        None => (None, Some(shell.visible_tabs(cx).len())),
                        Some(edge) => (Some((b, edge)), None),
                    };
                    shell.drop_dock_terminal(0, over, insert, w, cx);
                    assert!(shell.terminal_pane_views.values().any(|t| *t == pulled));
                    // The panel stays open on a new shell.
                    assert!(shell.workspace.right_panel_open);
                    let panel = shell.right_panel.clone().unwrap();
                    assert_eq!(panel.read(cx).tab(), RightTab::Terminal);
                    assert_eq!(shell.terminals[&project].len(), 1);
                    assert_ne!(shell.terminals[&project][0], pulled);
                    assert_eq!(shell.workspace.docks[&project].terminals.len(), 1);
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn cmd_shift_j_splits_a_title_bar_terminal_beside_the_chat(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            crate::terminal::bind(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.preferences.set_terminal_placement("title");
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        visual.simulate_keystrokes("cmd-shift-j");
        visual.run_until_parked();
        window
            .update(&mut visual, |shell, _, _| {
                let project = shell.workspace.project.clone();
                assert!(shell.terminals.get(&project).is_none_or(|t| t.is_empty()));
                let panes: Vec<i64> = shell.workspace.terminal_panes.keys().copied().collect();
                assert_eq!(panes.len(), 1);
                let pane = panes[0];
                // Shares the chat's title tab rather than opening its own.
                assert_eq!(shell.workspace.tab_of(pane), shell.workspace.tab_of(b));
                assert_eq!(shell.workspace.active, Some(pane));
                assert_eq!(shell.workspace.terminal_panes[&pane].owner, b);
            })
            .unwrap();
    }

    #[gpui::test]
    fn title_bar_terminals_open_as_their_own_tabs(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            crate::terminal::bind(cx);
            cx.set_reduce_motion(true);
        });
        window
            .update(cx, |shell, w, cx| {
                shell.workspace.preferences.set_terminal_placement("title");
                shell.open(b, w, cx);
                shell.focus_active(w, cx);
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        // Each cmd-j opens another terminal tab; the dock stays closed.
        visual.simulate_keystrokes("cmd-j");
        visual.run_until_parked();
        visual.simulate_keystrokes("cmd-j");
        visual.run_until_parked();
        let first = window
            .update(&mut visual, |shell, _, cx| {
                let project = shell.workspace.project.clone();
                assert!(shell.terminals.get(&project).is_none_or(|t| t.is_empty()));
                assert_eq!(shell.workspace.terminal_panes.len(), 2);
                let mut panes: Vec<i64> = shell.workspace.terminal_panes.keys().copied().collect();
                panes.sort();
                for pane in &panes {
                    assert_eq!(shell.workspace.tab_of(*pane), Some(*pane));
                    assert_eq!(shell.workspace.terminal_panes[pane].owner, b);
                }
                let active = shell.active_title_tab(cx).unwrap();
                assert!(matches!(active, TitleTab::Session(id) if panes.contains(&id)));
                panes
            })
            .unwrap();
        assert!(visual.debug_bounds("terminal-dock").is_none());
        // The tabs replace the footer's terminal button.
        assert!(visual.debug_bounds("footer-terminal").is_none());
        // The plus button asks for a session or a terminal.
        let plus = visual.debug_bounds("new-tab").unwrap().center();
        visual.simulate_click(plus, Modifiers::default());
        visual.run_until_parked();
        window
            .update(&mut visual, |shell, window, cx| {
                assert!(shell.menu_open(&MenuTarget::NewTab));
                shell.close_menu(cx);
                shell.run_menu(MenuTarget::NewTab, "terminal", None, window, cx);
                assert_eq!(shell.workspace.terminal_panes.len(), 3);
                assert!(
                    first
                        .iter()
                        .all(|p| shell.workspace.terminal_panes.contains_key(p))
                );
                // Back at the bottom, cmd-j toggles the dock again.
                shell.workspace.preferences.set_terminal_placement("bottom");
                assert!(!shell.terminal_in_title());
            })
            .unwrap();
    }

    #[test]
    fn terminal_placement_falls_back_to_the_legacy_panel_flag() {
        let mut p = crate::settings::Preferences::default();
        assert_eq!(p.terminal_placement(), "bottom");
        p.terminal_in_panel = true;
        assert_eq!(p.terminal_placement(), "panel");
        p.set_terminal_placement("title");
        assert_eq!(p.terminal_placement(), "title");
        assert!(!p.terminal_in_panel);
    }

    #[gpui::test]
    fn a_dock_tab_released_elsewhere_stays_in_the_dock(cx: &mut TestAppContext) {
        let (window, _, b) = window(cx);
        let (mut visual, project, shell_id) = dock_with_one_terminal(window, b, cx);
        let tab = visual.debug_bounds("terminal-tab-0").unwrap().center();
        // Pulled off the strip but dropped back on the dock itself.
        pull_dock_tab(&mut visual, point(tab.x + px(30.), tab.y + px(20.)));
        window
            .update(&mut visual, |shell, _, _| {
                assert!(shell.workspace.terminal_panes.is_empty());
                assert_eq!(shell.terminals[&project][0].entity_id(), shell_id);
                assert!(shell.workspace.docks[&project].open);
            })
            .unwrap();
    }

    #[test]
    fn restore_keeps_terminal_panes_of_known_sessions_in_open_tabs() {
        let mut state = Workspace {
            tabs: vec![1, -2],
            title_order: vec![TitleTab::Session(1), TitleTab::Session(-2)],
            ..Workspace::default()
        };
        state
            .layouts
            .insert(1, leaf(1).split_pane(1, SplitDir::Right, -1));
        let pane = |owner| TerminalPane {
            owner,
            cwd: "/w".into(),
        };
        state.terminal_panes.insert(-1, pane(1));
        // Its own tab, but its owner is gone.
        state.terminal_panes.insert(-2, pane(9));
        // No tab holds it.
        state.terminal_panes.insert(-3, pane(1));
        let saved = serde_json::to_string(&state).unwrap();
        let restored = Workspace::restore(Some(&saved), &[(1, "/w".into())], "/w");
        assert_eq!(
            restored.terminal_panes.keys().copied().collect::<Vec<_>>(),
            [-1]
        );
        assert_eq!(restored.layout(1).leaf_ids(), [1, -1]);
        assert_eq!(restored.tabs, [1]);
        assert_eq!(restored.next_pane_id(), -2);
    }
}
