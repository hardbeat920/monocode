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
            if self.terminal_in_panel(cx) && self.panel_terminal_shown(cx) {
                self.set_right_panel_open(false, window, cx);
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
