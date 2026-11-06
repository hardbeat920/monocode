//! The sidebar's origin tile: which Mono started a session, with a filter and
//! menu entries to find the rest of its work.
use super::*;
use monocode_core::{mono::Origin, session_filters::YOU};

/// A Mono's colour at 55% brightness, for sprites drawn on a light theme.
fn darkened(color: u32) -> u32 {
    let dim = |shift: u32| ((color >> shift & 0xff) * 11 / 20) << shift;
    dim(16) | dim(8) | dim(0)
}

impl Shell {
    /// The Mono that started `id`, when the Monos feature is on.
    pub(super) fn session_origin(&self, id: i64, cx: &App) -> Option<Origin> {
        if !self.workspace.preferences.monos {
            return None;
        }
        self.monos.as_ref()?.read(cx).roster.origin_of(id)
    }

    /// A small tile with the Mono's mascot, leading the row's title. It opens
    /// the Mono, unless that Mono was deleted.
    pub(super) fn origin_tile(
        &self,
        id: i64,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let origin = self.session_origin(id, cx)?;
        let label = if origin.deleted {
            format!("Started by {} (deleted)", origin.name)
        } else {
            format!("Started by {}", origin.name)
        };
        // Pastel sprites wash out on a light theme, so they are darkened there.
        let color = if origin.deleted {
            0x8a8a8a
        } else if crate::theme::is_light() {
            darkened(origin.color)
        } else {
            origin.color
        };
        let tint = if origin.deleted {
            content(0.06)
        } else {
            gpui::Hsla::from(rgb(origin.color)).opacity(0.22)
        };
        let deleted = origin.deleted;
        let base = origin.color;
        let tip = label.clone();
        Some(
            div()
                .id(("session-origin", id as usize))
                .debug_selector(move || format!("session-origin-{id}"))
                .role(Role::Button)
                .aria_label(label)
                .size(px(side))
                .flex_shrink_0()
                .rounded(px(side * 0.3))
                .bg(tint)
                .when(deleted, |d| d.opacity(0.6))
                .when(!deleted, |d| {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(gpui::Hsla::from(rgb(base)).opacity(0.34)))
                })
                .child(crate::monos::avatar::avatar(
                    &origin.mascot,
                    color,
                    "",
                    side,
                    true,
                ))
                .tooltip(move |_, cx| cx.new(|_| project_rail::ProjectTooltip(tip.clone())).into())
                // The row below starts a drag and selects on click.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_origin(id, window, cx);
                }))
                .into_any_element(),
        )
    }

    pub(super) fn open_origin(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(origin) = self.session_origin(id, cx).filter(|o| !o.deleted) else {
            return;
        };
        if let Some(monos) = self.monos.clone() {
            monos.update(cx, |monos, cx| monos.open(&origin.mono, window, cx));
        }
    }

    /// Who started the sessions the sidebar can list in this project, as
    /// filter keys with their labels, the user's own first.
    fn listed_origins(&self, cx: &App) -> Vec<(String, String)> {
        let mut origins: Vec<(String, String)> = Vec::new();
        for id in &self.order {
            let session = self.session_metadata(*id, cx);
            if self
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).hidden_session(*id))
                || session.summary.cwd != self.workspace.project
                || !session.summary.has_messages
                || !self.in_sidebar_worktree_scope(*id, cx)
            {
                continue;
            }
            let entry = match self.session_origin(*id, cx) {
                Some(o) if o.deleted => (o.mono, format!("{} (deleted)", o.name)),
                Some(o) => (o.mono, o.name),
                None => (YOU.to_owned(), "You".to_owned()),
            };
            if !origins.iter().any(|(key, _)| *key == entry.0) {
                origins.push(entry);
            }
        }
        origins.sort_by_key(|(key, _)| key != YOU);
        origins
    }

    /// The filter menu's "Started by" section; only worth showing once a Mono
    /// has started a session that is listed.
    pub(super) fn origin_filter_items(&self, cx: &App) -> Vec<MenuItem> {
        let origins = self.listed_origins(cx);
        if !origins.iter().any(|(key, _)| key != YOU) {
            return Vec::new();
        }
        let filters = &self.workspace.session_filters;
        let mut items = vec![MenuItem::Header("Started by".into())];
        items.extend(origins.into_iter().map(|(key, label)| {
            MenuAction::new(format!("origin:{key}"), label)
                .checked(filters.origin_matches(&key))
                .keep_open()
                .into()
        }));
        items
    }

    pub(super) fn origin_visible(&self, id: i64, cx: &App) -> bool {
        let filters = &self.workspace.session_filters;
        if filters.hidden_origins.is_empty() {
            return true;
        }
        match self.session_origin(id, cx) {
            Some(origin) => filters.origin_matches(&origin.mono),
            None => filters.origin_matches(YOU),
        }
    }

    /// Row menu entries for a session a Mono started.
    pub(super) fn origin_menu_items(&self, id: i64, cx: &App) -> Vec<MenuItem> {
        let Some(origin) = self.session_origin(id, cx) else {
            return Vec::new();
        };
        vec![
            MenuAction::new(
                "origin-open",
                if origin.deleted {
                    format!("Open {} (deleted)", origin.name)
                } else {
                    format!("Open {}", origin.name)
                },
            )
            .disabled(origin.deleted)
            .into(),
            MenuAction::new("origin-only", format!("Show only {}", origin.name)).into(),
            MenuItem::Separator,
        ]
    }

    pub(super) fn narrow_to_origin(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(origin) = self.session_origin(id, cx) else {
            return;
        };
        self.change_session_filter(&format!("origin-only:{}", origin.mono), cx);
    }

    /// `Show only` keeps one origin of those the sidebar lists.
    pub(super) fn only_origin(&mut self, key: &str, cx: &App) {
        let keys: Vec<_> = self
            .listed_origins(cx)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        self.workspace
            .session_filters
            .only_origin(key, keys.iter().map(String::as_str));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, WindowHandle};
    use monocode_core::mono::Launch;

    fn launch(session_id: i64, adopted: bool) -> Launch {
        Launch {
            session_id,
            cwd: String::new(),
            title: "Work".into(),
            harness: "claude".into(),
            model: None,
            origin_turn: (1 << 32) | 2,
            notify_on_complete: true,
            completed: false,
            dismissed: false,
            adopted,
        }
    }

    /// Session `a` was started by a Mono named TaskGiver, `b` is the user's own.
    fn setup(cx: &mut TestAppContext, classic: bool) -> (WindowHandle<Shell>, i64, i64, String) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, a, b) = super::super::tests::window(cx);
        cx.simulate_window_resize(window.into(), gpui::size(px(1500.), px(900.)));
        let mono = window
            .update(cx, |shell, window, cx| {
                shell.workspace.classic_layout = classic;
                shell.workspace.sidebar_visible = true;
                shell.workspace.project_rail_open = true;
                shell.workspace.right_panel_open = false;
                shell.workspace.preferences.monos = true;
                let project = shell.workspace.project.clone();
                for id in [a, b] {
                    shell.sessions[&id].update(cx, |s, _| {
                        s.summary.cwd = project.clone();
                        s.summary.has_messages = true;
                        s.summary.updated_at = monocode_core::time::now_ms();
                    });
                }
                let id = shell.mono_manager().update(cx, |monos, _| {
                    let id = monos.roster.create(vec![project]).id.clone();
                    let mono = monos.roster.get_mut(&id).unwrap();
                    mono.name = "TaskGiver".into();
                    mono.show_started_sessions_in_sidebar = true;
                    mono.launches.push(launch(a, false));
                    id
                });
                shell.open(b, window, cx);
                shell.layout_changed(cx);
                id
            })
            .unwrap();
        cx.run_until_parked();
        (window, a, b, mono)
    }

    #[test]
    fn darkening_a_colour_never_overflows_a_channel() {
        assert_eq!(darkened(0xffffff), 0x8c8c8c);
        assert_eq!(darkened(0xf5a524), 0x865a13);
        assert_eq!(darkened(0), 0);
    }

    fn name(selector: String) -> &'static str {
        Box::leak(selector.into_boxed_str())
    }

    fn tile(visual: &mut VisualTestContext, id: i64) -> Option<gpui::Bounds<Pixels>> {
        visual.refresh().unwrap();
        visual.debug_bounds(name(format!("session-origin-{id}")))
    }

    #[gpui::test]
    fn a_started_session_shows_its_mono_and_the_tile_opens_it(cx: &mut TestAppContext) {
        for classic in [false, true] {
            let (window, a, b, mono) = setup(cx, classic);
            let visual = &mut VisualTestContext::from_window(window.into(), cx);
            let bounds = tile(visual, a).expect("the started session has a tile");
            assert!(tile(visual, b).is_none(), "the user's own session has none");
            assert!(bounds.size.width <= px(20.) && bounds.size.height <= px(20.));
            let title = visual
                .debug_bounds(name(format!("session-title-{a}")))
                .unwrap();
            assert!(bounds.right() <= title.left(), "the tile leads the title");
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
            window
                .update(visual, |shell, _, cx| {
                    assert_eq!(
                        shell.mono_manager().read(cx).active.as_deref(),
                        Some(mono.as_str())
                    );
                    assert!(
                        shell.selected_sessions.is_empty() && shell.workspace.active == Some(b),
                        "the click is the tile's, not the row's"
                    );
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn an_adopted_session_has_no_tile(cx: &mut TestAppContext) {
        let (window, a, b, mono) = setup(cx, false);
        window
            .update(cx, |shell, _, cx| {
                shell.mono_manager().update(cx, |monos, _| {
                    let mono = monos.roster.get_mut(&mono).unwrap();
                    mono.launches = vec![launch(b, true)];
                });
                shell.invalidate_chrome(cx);
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        assert!(tile(visual, a).is_none());
        assert!(tile(visual, b).is_none(), "taking over is not starting");
    }

    #[gpui::test]
    fn the_started_by_filter_narrows_the_list_and_clears(cx: &mut TestAppContext) {
        let (window, a, b, mono) = setup(cx, false);
        window
            .update(cx, |shell, window, cx| {
                assert_eq!(shell.sidebar_session_ids(cx).len(), 2);
                shell.open_menu(
                    MenuTarget::SessionFilter,
                    Anchor::Point(gpui::point(px(220.), px(180.))),
                    window,
                    cx,
                );
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("menu-action-origin:you").is_some());
        let row = visual
            .debug_bounds(name(format!("menu-action-origin:{mono}")))
            .unwrap();
        visual.simulate_click(row.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.menu.is_some(), "checkboxes keep the menu open");
                assert_eq!(shell.sidebar_session_ids(cx), [b]);
                let state: Workspace =
                    serde_json::from_str(&shell.latest_state.lock().unwrap()).unwrap();
                assert_eq!(
                    state.session_filters.hidden_origins,
                    std::slice::from_ref(&mono)
                );
                shell.change_session_filter("clear", cx);
                assert_eq!(shell.sidebar_session_ids(cx).len(), 2);
                shell.change_session_filter("origin:you", cx);
                assert_eq!(shell.sidebar_session_ids(cx), [a]);
                shell.change_session_filter("clear", cx);
                shell.change_session_filter(&format!("origin-only:{mono}"), cx);
                assert_eq!(shell.sidebar_session_ids(cx), [a]);
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_row_menu_opens_the_mono_or_shows_only_its_sessions(cx: &mut TestAppContext) {
        let (window, a, b, mono) = setup(cx, false);
        window
            .update(cx, |shell, window, cx| {
                shell.open_menu(
                    MenuTarget::Session(b),
                    Anchor::Point(gpui::point(px(220.), px(180.))),
                    window,
                    cx,
                );
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("menu-action-origin-open").is_none(),
            "no Mono started the user's own session"
        );
        window
            .update(visual, |shell, window, cx| {
                shell.open_menu(
                    MenuTarget::Session(a),
                    Anchor::Point(gpui::point(px(220.), px(180.))),
                    window,
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        let only = visual.debug_bounds("menu-action-origin-only").unwrap();
        visual.simulate_click(only.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.sidebar_session_ids(cx), [a]);
                assert!(shell.mono_manager().read(cx).active.is_none());
            })
            .unwrap();
        window
            .update(visual, |shell, window, cx| {
                shell.open_menu(
                    MenuTarget::Session(a),
                    Anchor::Point(gpui::point(px(220.), px(180.))),
                    window,
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        let open = visual.debug_bounds("menu-action-origin-open").unwrap();
        visual.simulate_click(open.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(
                    shell.mono_manager().read(cx).active.as_deref(),
                    Some(mono.as_str())
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_deleted_mono_leaves_a_dimmed_tile_that_opens_nothing(cx: &mut TestAppContext) {
        let (window, a, _, mono) = setup(cx, false);
        window
            .update(cx, |shell, _, cx| {
                shell.mono_manager().update(cx, |monos, _| {
                    monos.roster.remove(&mono);
                });
                shell.invalidate_chrome(cx);
                assert!(shell.session_origin(a, cx).unwrap().deleted);
                assert_eq!(shell.sidebar_session_ids(cx).len(), 2);
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let bounds = tile(visual, a).expect("the tile outlives its Mono");
        visual.simulate_click(bounds.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert!(shell.mono_manager().read(cx).active.is_none());
            })
            .unwrap();
    }
}
