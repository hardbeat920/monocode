//! All title-strip commands operate on the same visible, ordered snapshot.
use super::*;
use crate::workspace::TitleTab;
use monocode_core::files::path_key;

impl Shell {
    /// The workspace keys a tab by one of its sessions. Replacing that session
    /// changes the key, but the same slot and selected width stay on screen.
    pub(super) fn replace_title_tab_key(&mut self, from: i64, to: i64) {
        if from == to {
            return;
        }
        let previous = TitleTab::Session(from);
        let next = TitleTab::Session(to);
        self.title_tab_motion.replace_key(&previous, next.clone());
        for selected in [
            &mut self.title_tab_selection.0,
            &mut self.title_tab_selection.1,
        ] {
            if selected.as_ref() == Some(&previous) {
                *selected = Some(next.clone());
            }
        }
        let shift = self.tab_shifts.remove(&previous);
        self.tab_shifts.remove(&next);
        if let Some(shift) = shift {
            self.tab_shifts.insert(next, shift);
        }
        self.pane_ticks.remove(&from);
        self.pane_ticks.remove(&to);
        if self.tab_hover == Some(from) {
            self.tab_hover = Some(to);
        }
    }

    pub(crate) fn click_mono_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.skip_tab_click) {
            return;
        }
        self.open_title_tab(&self.mono_tab(id.into()), window, cx);
    }
    pub(super) fn tab_scope(&self) -> String {
        format!(
            "{}\0{}",
            path_key(&self.workspace.project),
            path_key(self.working_copy())
        )
    }

    pub(super) fn mono_tab(&self, id: String) -> TitleTab {
        TitleTab::Mono {
            id,
            project: self.workspace.project.clone(),
            worktree: self.working_copy().into(),
        }
    }

    pub(super) fn visible_tabs(&self, cx: &App) -> Vec<TitleTab> {
        self.workspace
            .title_order
            .iter()
            .filter(|tab| match tab {
                TitleTab::Search => true,
                TitleTab::App(view) => {
                    !self.workspace.classic_layout
                        && (*view != app_views::AppView::Notes || self.workspace.preferences.notes)
                }
                TitleTab::Session(tab) => {
                    self.workspace.tabs.contains(tab) && {
                        let id = self.workspace.scope_pane(*tab);
                        self.session_summaries
                            .contains_key(&self.workspace.session_for_pane(id))
                            && self
                                .session_metadata(self.workspace.session_for_pane(id), cx)
                                .summary
                                .cwd
                                == self.workspace.project
                            && self.in_worktree_scope(id, cx)
                    }
                }
                TitleTab::Mono {
                    id,
                    project,
                    worktree,
                } => {
                    !self.workspace.classic_layout
                        && *project == self.workspace.project
                        && path_key(worktree) == path_key(self.working_copy())
                        && self.workspace.preferences.monos
                        && self
                            .monos
                            .as_ref()
                            .is_some_and(|m| m.read(cx).roster.get(id).is_some())
                }
            })
            .cloned()
            .collect()
    }

    pub(super) fn active_title_tab(&self, cx: &App) -> Option<TitleTab> {
        if let Some(view) = self.active_app_view {
            return Some(TitleTab::App(view));
        }
        if self.app_search.is_some() {
            return Some(TitleTab::Search);
        }
        self.monos
            .as_ref()
            .and_then(|m| m.read(cx).active.clone())
            .map(|id| self.mono_tab(id))
            .or_else(|| self.workspace.active_tab().map(TitleTab::Session))
    }

    /// The active tab follows live focus; other tabs keep their last displayed
    /// pane instead of borrowing the currently selected tab's preview focus.
    pub(super) fn tab_review_focused(&self, tab: i64, cx: &App) -> bool {
        if self.active_title_tab(cx) == Some(TitleTab::Session(tab)) {
            self.review_focused
        } else {
            self.pane_ticks
                .get(&tab)
                .is_some_and(|tick| tick.shown == (self.workspace.tab_focus(tab), true))
        }
    }

    pub(super) fn remember_title_tab(&mut self, tab: TitleTab) {
        if self.workspace.classic_layout && matches!(tab, TitleTab::Mono { .. }) {
            return;
        }
        if !self.workspace.title_order.contains(&tab) {
            self.workspace.title_order.push(tab.clone());
        }
        self.workspace.scope_focus.insert(self.tab_scope(), tab);
    }

    pub(super) fn mono_opened(&mut self, id: String, cx: &mut Context<Self>) {
        if !self
            .monos
            .as_ref()
            .is_some_and(|m| m.read(cx).active.as_deref() == Some(&id))
            || self.redirect_mono_to_pane(&id, cx)
        {
            return;
        }
        self.navigation_revision += 1;
        self.selected_sessions.clear();
        self.selection_anchor = None;
        self.sidebar_target = None;
        self.context_selection = false;
        self.leave_app_views(cx);
        self.leave_app_search(cx);
        self.close_preview(cx);
        self.review_focused = false;
        self.remember_title_tab(self.mono_tab(id));
        self.reveal_active_tab(cx);
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn open_title_tab(
        &mut self,
        tab: &TitleTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match tab {
            TitleTab::Search => self.open_app_search(window, cx),
            TitleTab::App(view) => match view {
                app_views::AppView::Inbox => self.open_inbox(window, cx),
                app_views::AppView::Notes => self.open_notes(window, cx),
                app_views::AppView::Automations => self.open_automations(window, cx),
            },
            TitleTab::Session(id) => {
                let focus = self.workspace.tab_focus(*id);
                let review_focused = self.tab_review_focused(*id, cx);
                self.open(focus, window, cx);
                if review_focused
                    && self.workspace.active == Some(focus)
                    && let Some(review) = self.active_review(cx)
                {
                    self.review_focused = true;
                    review.update(cx, |r, cx| r.focus_active(window, cx));
                }
            }
            TitleTab::Mono { id, .. } => {
                if let Some(monos) = self.monos.clone() {
                    monos.update(cx, |m, cx| m.open(id, window, cx));
                }
            }
        }
    }

    pub(super) fn restore_scope_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let tab = self.workspace.scope_focus.get(&self.tab_scope()).cloned();
        if let Some(tab) = tab.filter(|tab| self.visible_tabs(cx).contains(tab)) {
            if matches!(tab, TitleTab::Mono { .. }) {
                self.workspace.active = None;
            }
            self.open_title_tab(&tab, window, cx);
            return true;
        }
        false
    }

    pub(crate) fn close_mono_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.classic_layout {
            if self
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).active.as_deref() == Some(id))
            {
                self.classic_rail_back(window, cx);
            }
            return;
        }
        self.close_title_tabs(&[self.mono_tab(id.into())], None, window, cx);
    }

    pub(super) fn run_title_tab_menu(
        &mut self,
        tab: TitleTab,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs = self.visible_tabs(cx);
        let Some(index) = tabs.iter().position(|entry| *entry == tab) else {
            return;
        };
        if action == "close" {
            if let Some(id) = tab.session() {
                self.close(id, window, cx);
            } else {
                self.close_title_tabs(&[tab], None, window, cx);
            }
            return;
        }
        let closing = tabs
            .into_iter()
            .enumerate()
            .filter(|(ix, entry)| match action {
                "others" => *entry != tab,
                "right" => *ix > index,
                "left" => *ix < index,
                "all" => true,
                _ => false,
            })
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        self.close_title_tabs(&closing, (action == "others").then_some(tab), window, cx);
    }

    pub(super) fn close_title_tabs(
        &mut self,
        tabs: &[TitleTab],
        preferred: Option<TitleTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.visible_tabs(cx);
        let closing: HashSet<_> = tabs
            .iter()
            .filter(|tab| self.workspace.title_order.contains(tab))
            .cloned()
            .collect();
        if closing.is_empty() {
            return;
        }
        let active = self.active_title_tab(cx);
        let replace = active.as_ref().is_some_and(|tab| closing.contains(tab));
        let next = if replace {
            preferred
                .filter(|tab| before.contains(tab) && !closing.contains(tab))
                .or_else(|| {
                    let index = before.iter().position(|tab| Some(tab) == active.as_ref())?;
                    // Browsers select the next tab to the right, then the one to the left.
                    before[index + 1..]
                        .iter()
                        .chain(before[..index].iter().rev())
                        .find(|tab| !closing.contains(*tab))
                        .cloned()
                })
        } else {
            None
        };
        self.navigation_revision += 1;
        self.close_menu(cx);
        self.tab_drag = None;
        self.tab_shifts.clear();
        self.tab_hover = None;
        self.tab_scroll_task = None;
        self.cancel_pane_drag(cx);
        let session_active = self.workspace.active;
        for tab in &closing {
            if let TitleTab::App(view) = tab
                && self.active_app_view == Some(*view)
            {
                self.leave_app_views(cx);
            }
            if *tab == TitleTab::Search {
                self.leave_app_search(cx);
                self.saved_search = None;
            }
            if let TitleTab::Session(id) = tab {
                self.workspace.close(*id);
            }
        }
        self.workspace
            .title_order
            .retain(|tab| !closing.contains(tab));
        self.workspace
            .scope_focus
            .retain(|_, tab| !closing.contains(tab));
        if replace {
            if let Some(monos) = self.monos.clone() {
                monos.update(cx, |m, cx| m.close(cx));
            }
            self.workspace.active = None;
            self.review_focused = false;
            self.review_slide = None;
            self.pane_motion = None;
            if let Some(tab) = next {
                self.open_title_tab(&tab, window, cx);
            } else {
                self.ensure_project_session(window, cx);
            }
        } else {
            self.workspace.active =
                session_active.filter(|id| self.workspace.tab_of(*id).is_some());
        }
        self.settle_mono_panes(window, cx);
        self.history
            .prune(&self.workspace.open_sessions(), self.workspace.active);
        if self.workspace.active.is_none() && !self.mono_active(cx) && !self.app_view_open() {
            self.focus_active(window, cx);
        }
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn reorder_title_tab(&mut self, tab: &TitleTab, to: usize, cx: &App) {
        let mut visible = self.visible_tabs(cx);
        let Some(from) = visible.iter().position(|entry| entry == tab) else {
            return;
        };
        let entry = visible.remove(from);
        visible.insert(to.min(visible.len()), entry);
        let set: HashSet<_> = visible.iter().cloned().collect();
        let mut ordered = visible.into_iter();
        for entry in &mut self.workspace.title_order {
            if set.contains(entry) {
                *entry = ordered.next().unwrap();
            }
        }
        self.workspace.tabs = self
            .workspace
            .title_order
            .iter()
            .filter_map(TitleTab::session)
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::tests::window;
    use gpui::{TestAppContext, WindowHandle};

    fn add_session(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> i64 {
        let summary = locked(&shell.store)
            .unwrap()
            .create("codex", &shell.workspace.project)
            .unwrap();
        let id = summary.id;
        shell.add(summary, cx);
        let focus = shell.worktree_focus.get(&shell.workspace.project).cloned();
        shell.seed_worktree(id, focus.as_ref(), cx);
        shell.open(id, window, cx);
        id
    }

    fn add_mono(window: WindowHandle<Shell>, cx: &mut TestAppContext) -> TitleTab {
        window
            .update(cx, |shell, w, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.create(Vec::new(), w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| shell.active_title_tab(cx).unwrap())
            .unwrap()
    }

    #[gpui::test]
    fn mixed_tabs_open_in_order_and_close_to_the_visible_neighbor(cx: &mut TestAppContext) {
        let (window, other_project, first) = window(cx);
        let mono = add_mono(window, cx);
        let second = window.update(cx, add_session).unwrap();
        cx.run_until_parked();
        let last = add_mono(window, cx);
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(
                    shell.visible_tabs(cx),
                    [
                        TitleTab::Session(first),
                        mono.clone(),
                        TitleTab::Session(second),
                        last.clone()
                    ]
                );
                shell.open_title_tab(&mono, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                shell.close_title_tabs(std::slice::from_ref(&mono), None, w, cx);
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(second)));
                shell.close_title_tabs(&[last], None, w, cx);
                assert_eq!(
                    shell.active_title_tab(cx),
                    Some(TitleTab::Session(second)),
                    "inactive close preserves focus"
                );
                assert!(
                    shell.sessions[&second]
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(w)
                );
                shell.close(second, w, cx);
                assert_eq!(shell.workspace.active, Some(first));
                assert_eq!(shell.workspace.project, "/work/monocode");
                assert!(shell.workspace.tabs.contains(&other_project));
            })
            .unwrap();
    }

    #[gpui::test]
    fn mono_close_button_and_cmd_w_share_selection_and_focus(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        cx.update(|cx| {
            bind_keys(cx);
            input::bind(cx);
            cx.set_reduce_motion(true);
        });
        let mono = add_mono(window, cx);
        let second = window.update(cx, add_session).unwrap();
        window
            .update(cx, |shell, w, cx| shell.open_title_tab(&mono, w, cx))
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let close = visual.debug_bounds("close-mono-tab-button").unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.workspace.active, Some(second));
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(first), TitleTab::Session(second)]
                );
                assert!(
                    shell.sessions[&second]
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(w)
                );
            })
            .unwrap();
        let mono = add_mono(window, visual);
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        visual.simulate_keystrokes(&format!("{modifier}-w"));
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(!shell.visible_tabs(cx).contains(&mono));
                assert_eq!(shell.workspace.active, Some(second));
                assert!(
                    shell.sessions[&second]
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(w)
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn worktree_tabs_cycle_close_and_return_without_touching_other_scopes(cx: &mut TestAppContext) {
        let (window, other_project, main) = window(cx);
        let mono_main = add_mono(window, cx);
        let feature = crate::worktree_switcher::WorktreeFocus {
            path: "/work/feature".into(),
            branch: Some("feature".into()),
        };
        window
            .update(cx, |shell, w, cx| {
                shell.select_worktree(Some(feature.clone()), w, cx)
            })
            .unwrap();
        cx.run_until_parked();
        let feature_session = window
            .update(cx, |shell, _, _| shell.workspace.active.unwrap())
            .unwrap();
        let mono_feature = add_mono(window, cx);
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(feature_session), mono_feature.clone()]
                );
                shell.cycle(1, w, cx);
                assert_eq!(shell.workspace.active, Some(feature_session));
                shell.activate_tab(-1, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(mono_feature.clone()));
                let TitleTab::Mono { id, .. } = &mono_feature else {
                    panic!("expected a Mono");
                };
                shell.run_menu(MenuTarget::MonoTab(id.clone()), "others", None, w, cx);
                assert_eq!(
                    shell.visible_tabs(cx).as_slice(),
                    std::slice::from_ref(&mono_feature)
                );
                assert!(shell.workspace.tabs.contains(&main));
                assert!(shell.workspace.tabs.contains(&other_project));
                shell.select_worktree(None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(mono_main.clone()));
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(main), mono_main.clone()]
                );
                shell.select_worktree(Some(feature), w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(mono_feature));
                shell.close_title_tabs(&shell.visible_tabs(cx), None, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert_eq!(shell.workspace.project, "/work/monocode");
                assert_eq!(shell.working_copy(), "/work/feature");
                assert_eq!(shell.visible_tabs(cx).len(), 1);
                assert!(shell.workspace.title_order.contains(&mono_main));
                assert!(shell.workspace.tabs.contains(&other_project));
            })
            .unwrap();
    }

    #[gpui::test]
    fn mixed_reorder_and_restart_preserve_order_and_selected_mono(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        let mono = add_mono(window, cx);
        let second = window.update(cx, add_session).unwrap();
        window
            .update(cx, |shell, w, cx| {
                shell.reorder_title_tab(&mono, 0, cx);
                shell.open_title_tab(&mono, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let (store, summaries, saved) = window
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell.visible_tabs(cx),
                    [
                        mono.clone(),
                        TitleTab::Session(first),
                        TitleTab::Session(second)
                    ]
                );
                let db = locked(&shell.store).unwrap();
                (
                    shell.store.clone(),
                    db.sessions().unwrap(),
                    serde_json::to_string(&shell.workspace).unwrap(),
                )
            })
            .unwrap();
        let restored = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Shell::new(store, summaries, "/work/monocode".into(), Some(saved), cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        restored
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell.visible_tabs(cx),
                    [
                        mono.clone(),
                        TitleTab::Session(first),
                        TitleTab::Session(second)
                    ]
                );
                assert_eq!(shell.active_title_tab(cx), Some(mono));
            })
            .unwrap();
    }

    #[gpui::test]
    fn rapid_new_tabs_follow_request_order(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        let providers = [Kind::Claude, Kind::Codex, Kind::Claude, Kind::Codex];
        window
            .update(cx, |shell, _, cx| {
                for provider in providers {
                    shell.create(provider, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let tabs = shell.visible_tabs(cx);
                assert_eq!(tabs.len(), 5);
                assert_eq!(tabs[0], TitleTab::Session(first));
                for (tab, provider) in tabs[1..].iter().zip(providers) {
                    assert_eq!(
                        shell.sessions[&tab.session().unwrap()]
                            .read(cx)
                            .summary
                            .provider,
                        provider.id()
                    );
                }
                assert_eq!(shell.active_title_tab(cx).as_ref(), tabs.last());
            })
            .unwrap();
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::shell::tests::window;
    use gpui::{Modifiers, TestAppContext, point};

    fn sidebar_highlight(
        window: gpui::WindowHandle<Shell>,
        selector: String,
        cx: &mut TestAppContext,
    ) -> bool {
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let selector = Box::leak(selector.into_boxed_str());
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the sidebar row {selector} is visible"));
        window
            .update(visual, |_, window, _| {
                let scale = window.scale_factor();
                window.painted_quads().into_iter().any(|quad| {
                    (quad.bounds.left().0 - f32::from(bounds.left()) * scale).abs() < 1.
                        && (quad.bounds.top().0 - f32::from(bounds.top()) * scale).abs() < 1.
                        && (quad.bounds.size.width.0 - f32::from(bounds.size.width) * scale).abs()
                            < 1.
                        && (quad.bounds.size.height.0 - f32::from(bounds.size.height) * scale).abs()
                            < 1.
                        && quad
                            .background
                            .as_solid()
                            .is_some_and(|color| (color.a - 0.1).abs() < 0.001)
                })
            })
            .unwrap()
    }

    #[gpui::test]
    fn sidebar_highlights_only_the_visible_mono_or_session(cx: &mut TestAppContext) {
        let (window, other, session) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                for id in [other, session] {
                    shell.sessions[&id].update(cx, |view, _| {
                        view.summary.cwd = "/work/monocode".into();
                        view.summary.has_messages = true;
                    });
                }
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.create(vec!["/work/monocode".into()], w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        let mono = window
            .update(cx, |shell, w, cx| {
                let mono = shell.active_title_tab(cx).unwrap();
                shell.open(session, w, cx);
                mono
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        let TitleTab::Mono { id, .. } = &mono else {
            panic!("expected a Mono");
        };
        let mono_row = format!("mono-rail-{id}");
        let session_row = format!("sidebar-session-{session}");
        assert!(sidebar_highlight(window, session_row.clone(), cx));
        assert!(!sidebar_highlight(window, mono_row.clone(), cx));
        let click = |selector: &str, cx: &mut TestAppContext| {
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            let bounds = visual
                .debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
                .unwrap();
            visual.simulate_click(bounds.center(), Default::default());
            visual.simulate_mouse_move(point(px(5.), px(5.)), None, Default::default());
            visual.run_until_parked();
            visual.refresh().unwrap();
        };
        click(&mono_row, cx);
        assert!(sidebar_highlight(window, mono_row.clone(), cx));
        assert!(
            !sidebar_highlight(window, session_row.clone(), cx),
            "a remembered session must not look active while a Mono is visible"
        );
        window
            .update(cx, |shell, _, _| {
                assert_eq!(shell.workspace.active, Some(session));
            })
            .unwrap();
        click(&session_row, cx);
        assert!(sidebar_highlight(window, session_row.clone(), cx));
        assert!(!sidebar_highlight(window, mono_row.clone(), cx));
        // Opening from the title strip must also clear an old bulk selection.
        window
            .update(cx, |shell, w, cx| {
                for id in [other, session] {
                    shell.select_sidebar_session(
                        id,
                        Modifiers {
                            platform: true,
                            ..Default::default()
                        },
                        w,
                        cx,
                    );
                }
                assert_eq!(shell.selected_sessions.len(), 2);
                shell.open_title_tab(&mono, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.selected_sessions.is_empty());
                assert!(shell.selection_anchor.is_none());
                assert!(shell.sidebar_target.is_none());
            })
            .unwrap();
        assert!(sidebar_highlight(window, mono_row, cx));
        assert!(!sidebar_highlight(window, session_row, cx));
        assert!(!sidebar_highlight(
            window,
            format!("sidebar-session-{other}"),
            cx
        ));
    }

    #[gpui::test]
    fn closing_a_session_button_selects_the_adjacent_mono_and_restores_its_composer(
        cx: &mut TestAppContext,
    ) {
        let (window, _, first) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.create(Vec::new(), w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        let mono = window
            .update(cx, |shell, w, cx| {
                let mono = shell.active_title_tab(cx).unwrap();
                shell.open(first, w, cx);
                mono
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let close = visual
            .debug_bounds(Box::leak(
                format!("close-title-tab-{first}").into_boxed_str(),
            ))
            .unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(mono));
                let chat = shell.mono_manager().read(cx).chat().unwrap();
                assert!(chat.read(cx).input.focus_handle(cx).is_focused(w));
                assert_eq!(shell.workspace.project, "/work/monocode");
            })
            .unwrap();
    }

    #[gpui::test]
    fn dragging_a_mono_uses_the_mixed_strip_slots_and_keeps_the_active_session(
        cx: &mut TestAppContext,
    ) {
        let (window, _, first) = window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.create(Vec::new(), w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        let mono = window
            .update(cx, |shell, w, cx| {
                let mono = shell.active_title_tab(cx).unwrap();
                shell.open(first, w, cx);
                mono
            })
            .unwrap();
        let TitleTab::Mono { id, .. } = &mono else {
            panic!("expected a Mono");
        };
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let session_bounds = visual
            .debug_bounds(Box::leak(format!("title-tab-{first}").into_boxed_str()))
            .unwrap();
        let mono_bounds = visual
            .debug_bounds(Box::leak(format!("mono-title-tab-{id}").into_boxed_str()))
            .unwrap();
        visual.simulate_mouse_down(
            mono_bounds.center(),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.simulate_mouse_move(
            point(session_bounds.center().x, mono_bounds.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.run_until_parked();
        visual.simulate_mouse_up(
            point(session_bounds.center().x, mono_bounds.center().y),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.visible_tabs(cx), [mono, TitleTab::Session(first)]);
                assert_eq!(shell.workspace.active, Some(first));
                assert!(!shell.mono_active(cx));
            })
            .unwrap();
    }
}
