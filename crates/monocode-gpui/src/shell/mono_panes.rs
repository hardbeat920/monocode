//! Monos share the session pane tree: a Mono tab pulled onto a pane joins
//! its split, and a tab dropped on the visible Mono splits beside it. A Mono
//! pane renders the same chat as its tab, and becomes that tab again once it
//! stands alone.
use super::*;

/// The drop target standing for the visible Mono, before it becomes a pane.
pub(super) const MONO_VIEW: i64 = i64::MIN + 1;

impl Shell {
    pub(super) fn mono_pane_id(&self, id: i64) -> Option<&str> {
        match self.workspace.mono_panes.get(&id)? {
            TitleTab::Mono { id, .. } => Some(id),
            _ => None,
        }
    }

    /// The visible Mono as a drop target: its whole view, by edge.
    pub(super) fn mono_view_hit(&self, position: Point<Pixels>, cx: &App) -> Option<(i64, Edge)> {
        if !self.mono_active(cx) || self.app_view_open() {
            return None;
        }
        let bounds = self.mono_bounds.get()?;
        if !bounds.contains(&position) {
            return None;
        }
        let rect = Rect {
            x: 0.,
            y: 0.,
            w: bounds.size.width.as_f32(),
            h: bounds.size.height.as_f32(),
        };
        let x = (position.x - bounds.left()).as_f32();
        let y = (position.y - bounds.top()).as_f32();
        Some((MONO_VIEW, crate::pane_layout::edge_from_point(x, y, rect)))
    }

    /// A drop on the visible Mono first turns it into a lone pane in its own
    /// tab, so the drop can split beside it like any other pane.
    pub(super) fn resolve_mono_target(&mut self, target: i64, cx: &mut Context<Self>) -> i64 {
        if target != MONO_VIEW {
            return target;
        }
        let Some(mono) = self
            .active_title_tab(cx)
            .filter(|tab| matches!(tab, TitleTab::Mono { .. }))
        else {
            return target;
        };
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| m.close(cx));
        }
        self.workspace.mono_tab_pane(mono)
    }

    /// A Mono tab released over a pane, or over another visible Mono.
    pub(super) fn drop_mono_tab(
        &mut self,
        mono: TitleTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((target, edge)) = self.pane_drag.take().and_then(|d| d.over) else {
            return;
        };
        self.tabs_frame.update(cx, |_, cx| cx.notify());
        let target = self.resolve_mono_target(target, cx);
        let before = self.active_leaves();
        let Some(pane) = self.workspace.place_mono(mono, target, edge) else {
            return;
        };
        self.begin_pane_motion(before, cx);
        self.fit_sidebar(cx);
        self.open_mono_pane(pane, window, cx);
    }

    /// Focus a Mono's pane, showing the tab that holds it.
    pub(super) fn open_mono_pane(
        &mut self,
        pane: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.mono_pane_id(pane).map(str::to_owned) else {
            return;
        };
        let Some(tab) = self.workspace.tab_of(pane) else {
            return;
        };
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| {
                m.close(cx);
                m.pane_chat(&id, cx);
            });
        }
        self.navigation_revision += 1;
        self.leave_app_views(cx);
        self.leave_app_search(cx);
        self.close_preview(cx);
        self.history.record(pane);
        self.workspace.open(pane);
        self.review_focused = false;
        self.remember_title_tab(TitleTab::Session(tab));
        self.reveal_active_tab(cx);
        self.focus_active(window, cx);
        self.persist(cx);
        cx.notify();
    }

    /// The focused Mono pane's composer.
    pub(super) fn focus_mono_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self
            .workspace
            .active
            .and_then(|pane| self.mono_pane_id(pane))
        else {
            return false;
        };
        match self.monos.as_ref().and_then(|m| m.read(cx).open_chat(id)) {
            Some(chat) => window.focus(&chat.read(cx).input.focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
        true
    }

    /// Opening a Mono that lives in a split focuses its pane instead of
    /// showing the same chat a second time.
    pub(super) fn redirect_mono_to_pane(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.workspace.mono_pane(id) else {
            return false;
        };
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |m, cx| m.close(cx));
        }
        cx.spawn(async move |this, cx| {
            let _ = this.update_in(cx, |this, window, cx| this.open_mono_pane(pane, window, cx));
        })
        .detach();
        true
    }

    pub(super) fn mono_panes_unsettled(&self, cx: &App) -> bool {
        self.workspace.mono_panes.keys().any(|pane| {
            self.workspace
                .tab_of(*pane)
                .is_none_or(|tab| !self.workspace.layout(tab).is_split())
                || self.mono_pane_id(*pane).is_none_or(|id| {
                    self.monos
                        .as_ref()
                        .is_none_or(|m| m.read(cx).roster.get(id).is_none())
                })
        })
    }

    /// Panes whose Mono was deleted close; a Mono left alone in its tab
    /// becomes its Mono tab again, shown when it had focus.
    pub(super) fn settle_mono_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.mono_panes.is_empty() {
            return;
        }
        let gone: Vec<i64> = self
            .workspace
            .mono_panes
            .keys()
            .copied()
            .filter(|pane| {
                self.mono_pane_id(*pane).is_none_or(|id| {
                    self.monos
                        .as_ref()
                        .is_none_or(|m| m.read(cx).roster.get(id).is_none())
                })
            })
            .collect();
        for pane in gone {
            self.workspace.close_pane(pane);
            self.workspace.mono_panes.remove(&pane);
        }
        let Some(mono) = self.workspace.settle_mono_panes() else {
            return;
        };
        self.pane_motion = None;
        self.open_title_tab(&mono, window, cx);
        self.persist(cx);
    }

    /// A Mono pane: its split header over the Mono's chat.
    pub(super) fn mono_pane_surface(
        &mut self,
        pane: i64,
        split: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chat = self
            .mono_pane_id(pane)
            .and_then(|id| self.monos.as_ref()?.read(cx).open_chat(id));
        if let Some(chat) = &chat {
            chat.update(cx, |chat, cx| chat.set_beside_pane(split.is_some(), cx));
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .when_some(split.filter(|p| *p > 0.), |d, p| {
                d.child(self.split_header(pane, p, true, cx))
            })
            .child(div().flex_1().min_h_0().flex().flex_col().children(chat))
            .into_any_element()
    }

    pub(super) fn mono_pane_title(&self, pane: i64, cx: &App) -> Option<String> {
        let id = self.mono_pane_id(pane)?;
        Some(self.monos.as_ref()?.read(cx).roster.get(id)?.label())
    }

    pub(super) fn mono_pane_mark(&self, pane: i64, cx: &App) -> Option<PaneMark> {
        let id = self.mono_pane_id(pane)?;
        let mono = self.monos.as_ref()?.read(cx).roster.get(id)?.clone();
        Some(PaneMark::Mono(mono.mascot, mono.color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::tests::window;
    use gpui::{Modifiers, TestAppContext, VisualTestContext, point};

    fn pull(visual: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
        visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(
            point(from.x, px(TITLE_BAR_HEIGHT + PANE_DROP_PULL + 10.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        visual.run_until_parked();
    }

    #[gpui::test]
    fn a_mono_tab_pulled_onto_a_pane_splits_and_returns_to_its_tab(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
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
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.dismiss_settings(w, cx));
                shell.open(first, w, cx);
                mono
            })
            .unwrap();
        let TitleTab::Mono { id, .. } = mono.clone() else {
            panic!("expected a Mono");
        };
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let tab = visual
            .debug_bounds(Box::leak(format!("mono-title-tab-{id}").into_boxed_str()))
            .unwrap()
            .center();
        let pane = window
            .update(visual, |shell, _, _| shell.pane_bounds.get().unwrap())
            .unwrap();
        pull(visual, tab, point(pane.right() - px(20.), pane.center().y));
        let mono_pane = window
            .update(visual, |shell, w, cx| {
                let pane = shell
                    .workspace
                    .mono_pane(&id)
                    .expect("the Mono joined the split");
                assert_eq!(shell.workspace.layout(first).leaf_ids(), [first, pane]);
                assert_eq!(shell.workspace.active, Some(pane));
                assert!(!shell.mono_active(cx));
                assert_eq!(shell.visible_tabs(cx), [TitleTab::Session(first)]);
                assert_eq!(shell.tab_copy(first, cx).panes.len(), 2);
                let chat = shell.mono_manager().read(cx).open_chat(&id).unwrap();
                assert!(chat.read(cx).input.focus_handle(cx).is_focused(w));
                pane
            })
            .unwrap();
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("split-header-{mono_pane}").into_boxed_str()
                ))
                .is_some()
        );
        // Opening the Mono from elsewhere focuses its pane instead.
        window
            .update(visual, |shell, w, cx| {
                shell.open(first, w, cx);
                shell.mono_manager().update(cx, |m, cx| m.open(&id, w, cx));
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(!shell.mono_active(cx));
                assert_eq!(shell.workspace.active, Some(mono_pane));
            })
            .unwrap();
        // Closing the conversation's pane leaves the Mono as its own tab.
        window
            .update(visual, |shell, w, cx| shell.close_pane(first, w, cx))
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.workspace.mono_panes.is_empty());
                assert_eq!(shell.active_title_tab(cx), Some(mono.clone()));
                assert!(shell.visible_tabs(cx).contains(&mono));
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_tab_pulled_onto_the_visible_mono_splits_beside_it(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
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
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.dismiss_settings(w, cx));
                shell.active_title_tab(cx).unwrap()
            })
            .unwrap();
        let TitleTab::Mono { id, .. } = mono.clone() else {
            panic!("expected a Mono");
        };
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let tab = visual
            .debug_bounds(Box::leak(format!("title-tab-{first}").into_boxed_str()))
            .unwrap()
            .center();
        let view = window
            .update(visual, |shell, _, _| shell.mono_bounds.get().unwrap())
            .unwrap();
        pull(visual, tab, point(view.left() + px(20.), view.center().y));
        window
            .update(visual, |shell, _, cx| {
                let pane = shell
                    .workspace
                    .mono_pane(&id)
                    .expect("the Mono became a pane");
                let key = shell.workspace.tab_of(first).unwrap();
                assert_eq!(shell.workspace.layout(key).leaf_ids(), [first, pane]);
                assert_eq!(shell.workspace.active, Some(first));
                assert!(!shell.mono_active(cx));
                assert_eq!(shell.visible_tabs(cx), [TitleTab::Session(key)]);
            })
            .unwrap();
        // Its header pulls it back onto the strip as the Mono tab.
        let pane = window
            .update(visual, |shell, _, _| {
                shell.workspace.mono_pane(&id).unwrap()
            })
            .unwrap();
        let grip = visual
            .debug_bounds(Box::leak(
                format!("drag-session-pane-{pane}").into_boxed_str(),
            ))
            .unwrap()
            .center();
        let key = window
            .update(visual, |shell, _, _| shell.workspace.tab_of(first).unwrap())
            .unwrap();
        let strip = visual
            .debug_bounds(Box::leak(format!("title-tab-{key}").into_boxed_str()))
            .unwrap();
        let after = point(strip.right() - px(4.), strip.center().y);
        visual.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(
            grip + point(px(0.), px(-20.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        visual.simulate_mouse_move(after, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_up(after, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.workspace.mono_panes.is_empty());
                let key = shell.workspace.tab_of(first).unwrap();
                assert!(!shell.workspace.layout(key).is_split());
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(key), mono.clone()]
                );
                assert_eq!(shell.active_title_tab(cx), Some(mono.clone()));
            })
            .unwrap();
    }
}
