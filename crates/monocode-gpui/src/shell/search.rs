use super::*;
use crate::search::{Event, SearchView, Sources};
use monocode_core::search::Target;

pub(super) struct Open {
    pub(super) view: Entity<SearchView>,
    restore: Option<FocusHandle>,
    _events: Subscription,
}

impl Shell {
    pub(super) fn leave_app_search(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = self.app_search.take() {
            self.saved_search = Some(search.view.read(cx).draft(cx));
        }
    }
    pub(super) fn close_search_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_title_tabs(&[TitleTab::Search], None, window, cx);
    }
    pub(super) fn search_title_tab(
        &self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let active = self.app_search.is_some();
        let close = active || width >= TAB_CLOSE_MIN_WIDTH;
        div()
            .id("search-title-tab")
            .debug_selector(|| "search-title-tab".into())
            .group("search-title-tab")
            .role(Role::Tab)
            .aria_label("Search")
            .aria_selected(active)
            .cursor_pointer()
            .w(px(width))
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .relative()
            .child(
                title_tab_plate(active, false)
                    .text_color(content(if active { 1. } else { 0.5 }))
                    .w_full()
                    .min_w_0()
                    .when(close && width >= TAB_CLOSE_MIN_WIDTH, |d| d.pr_7())
                    .when(width < 30., |d| d.px(px(((width - 14.) / 2.).max(0.))))
                    .gap(px(6.))
                    .group_hover("search-title-tab", |s| {
                        s.text_color(content(1.)).bg(title_tab_fill(active, true))
                    })
                    .child(icon("search", 14.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(TITLE_TAB_TEXT_SIZE))
                            .child("Search"),
                    ),
            )
            .when(close, |d| {
                d.child(
                    div()
                        .id("close-search-tab")
                        .debug_selector(|| "close-search-tab".into())
                        .role(Role::Button)
                        .aria_label("Close Search")
                        .absolute()
                        .right_1()
                        .top(px(10.))
                        .size(px(20.))
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(0.)
                        .group_hover("search-title-tab", |s| s.opacity(1.))
                        .hover(|s| s.bg(content(0.1)))
                        .child(icon("close", 12.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_search_tab(window, cx);
                        })),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.start_tab_drag(TitleTab::Search, event, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(
                        MenuTarget::SearchTab,
                        Anchor::Point(event.position),
                        window,
                        cx,
                    )
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.close_search_tab(window, cx);
                }),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                if std::mem::take(&mut this.skip_tab_click) {
                    return;
                }
                this.open_app_search(window, cx);
            }))
    }
    pub(super) fn close_app_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = self.app_search.take() {
            self.saved_search = Some(search.view.read(cx).draft(cx));
            self.update_chat_visibility(cx);
            if let Some(restore) = search.restore {
                window.focus(&restore, cx);
            } else {
                self.focus_active(window, cx);
            }
            if let Some(tab) = self.active_title_tab(cx) {
                self.remember_title_tab(tab);
            }
            self.persist(cx);
            self.invalidate_chrome(cx);
            cx.notify();
        }
    }
    pub(super) fn open_app_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = &self.app_search {
            window.focus(&search.view.focus_handle(cx), cx);
            return;
        }
        self.close_menu(cx);
        self.close_preview(cx);
        self.leave_app_views(cx);
        if let Some(monos) = &self.monos {
            monos.update(cx, |m, cx| m.close(cx));
        }
        if self.settings.is_some() {
            self.close_settings(window, cx);
        }
        self.navigation_revision += 1;
        let restore = window.focused(cx);
        let conversations = self
            .sessions
            .values()
            .cloned()
            .chain(
                self.monos
                    .as_ref()
                    .map(|m| m.read(cx).search_conversations())
                    .unwrap_or_default(),
            )
            .collect::<Vec<_>>();
        let mut summaries: HashMap<_, _> = self
            .order
            .iter()
            .map(|&id| (id, self.session_metadata(id, cx).summary.clone()))
            .collect();
        summaries.extend(
            conversations
                .iter()
                .map(|s| (s.read(cx).summary.id, s.read(cx).summary.clone())),
        );
        let summaries = summaries.into_values().collect();
        let live = conversations
            .iter()
            // A paged Mono's saved search covers turns not loaded yet.
            .filter(|s| {
                s.read(cx).history_loaded() && s.read(cx).transcript.read(cx).history_complete()
            })
            .map(|s| {
                (
                    s.read(cx).summary.id,
                    Arc::new(s.read(cx).transcript.read(cx).history_rows().to_vec()),
                )
            })
            .collect();
        let cwd = self.working_copy().to_owned();
        let store = self.store.clone();
        let projects = self.workspace.projects.clone();
        let recent = self
            .workspace
            .recent_files
            .get(&cwd)
            .cloned()
            .unwrap_or_default();
        let view = cx.new(|cx| {
            SearchView::new(
                cwd,
                store,
                Sources {
                    summaries,
                    live,
                    projects,
                    recent,
                    classic: self.workspace.boxy_layout(),
                    logos: self
                        .workspace
                        .project_appearance
                        .iter()
                        .filter_map(|(path, a)| a.logo.clone().map(|logo| (path.clone(), logo)))
                        .collect(),
                },
                cx,
            )
        });
        if let Some(draft) = self.saved_search.clone() {
            view.update(cx, |v, cx| v.restore_draft(draft, cx));
        }
        let events = cx.subscribe_in(
            &view,
            window,
            |this, _, event: &Event, window, cx| match event {
                Event::Close => this.close_app_search(window, cx),
                Event::Open(target, query) => {
                    this.close_app_search(window, cx);
                    match target {
                        Target::Conversation(id) => this.open_search_session(*id, None, window, cx),
                        Target::Message { session, row } => this.open_search_session(
                            *session,
                            Some((*row, query.clone())),
                            window,
                            cx,
                        ),
                        Target::File { path, location } => {
                            let cwd = this.working_copy().to_owned();
                            this.open_review(
                                crate::review::ReviewTab::File {
                                    cwd,
                                    path: path.clone(),
                                },
                                window,
                                cx,
                            );
                            if let Some((line, column)) = location
                                && let Some(pane) = this.active_review(cx)
                            {
                                pane.update(cx, |p, cx| {
                                    p.navigate_file(*line as usize, *column as usize, cx)
                                });
                            }
                        }
                        Target::Project(path) => this.select_project(path.clone(), window, cx),
                    }
                }
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.app_search = Some(Open {
            view,
            restore,
            _events: events,
        });
        self.remember_title_tab(TitleTab::Search);
        self.reveal_active_tab(cx);
        self.update_chat_visibility(cx);
        self.persist(cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }
    fn open_search_session(
        &mut self,
        id: i64,
        jump: Option<(usize, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(view) = self.ensure_session(id, cx) {
            self.open(id, window, cx);
            if let Some((row, query)) = jump {
                view.update(cx, |v, cx| v.reveal_search(row, query, cx));
            }
            return;
        }
        if let Some(monos) = &self.monos
            && let Some(view) = monos.update(cx, |m, cx| m.open_search_session(id, window, cx))
        {
            if let Some((row, query)) = jump {
                view.update(cx, |v, cx| v.reveal_search(row, query, cx));
            }
            return;
        }
        let store = self.store.clone();
        let revision = self.navigation_revision;
        let work = cx.background_executor().spawn(async move {
            locked(&store)?
                .sessions()?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| "Conversation no longer exists".to_owned())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.navigation_revision != revision {
                    return;
                }
                match result {
                    Ok(summary) => {
                        this.add(summary, cx);
                        this.open(id, window, cx);
                        if let Some((row, query)) = jump {
                            this.sessions[&id].update(cx, |v, cx| v.reveal_search(row, query, cx));
                        }
                    }
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    pub(super) fn sync_mono_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &self.app_search else {
            return;
        };
        let views = self
            .monos
            .as_ref()
            .map(|m| m.read(cx).search_conversations())
            .unwrap_or_default();
        for view in views {
            let transcript = view.read(cx).transcript.read(cx);
            if !transcript.history_complete() {
                continue;
            }
            let rows = Arc::new(transcript.history_rows().to_vec());
            let summary = view.read(cx).summary.clone();
            search
                .view
                .update(cx, |v, cx| v.update_session(summary, rows, cx));
        }
    }
    pub(super) fn sync_search_session(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(search) = &self.app_search else {
            return;
        };
        let Some(session) = self.sessions.get(&id) else {
            return;
        };
        let session = session.read(cx);
        let transcript = session.transcript.read(cx);
        if !transcript.history_complete() {
            return;
        }
        let summary = session.summary.clone();
        let rows = Arc::new(transcript.history_rows().to_vec());
        search
            .view
            .update(cx, |v, cx| v.update_session(summary, rows, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    #[gpui::test]
    fn app_search_restores_draft_and_opens_cross_project_result(cx: &mut TestAppContext) {
        cx.update(|cx| {
            input::bind(cx);
            super::super::bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        let (window, a, b) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, w, cx| {
                shell.open(b, w, cx);
                let input = shell.sessions[&b].read(cx).input.clone();
                input.update(cx, |i, cx| i.set_text("preserve this draft".into(), cx));
                w.focus(&input.focus_handle(cx), cx);
                shell.sessions[&a]
                    .update(cx, |v, _| v.summary.title = "Needle conversation".into());
                shell.focus_search(w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.refresh().unwrap();
        window
            .update(cx, |shell, w, cx| {
                let view = &shell.app_search.as_ref().unwrap().view;
                assert!(view.focus_handle(cx).is_focused(w));
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        window
            .update(cx, |shell, w, cx| {
                assert!(shell.app_search.is_none());
                let input = &shell.sessions[&b].read(cx).input;
                assert_eq!(input.read(cx).content, "preserve this draft");
                assert!(input.focus_handle(cx).is_focused(w));
                shell.focus_search(w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_input(window.into(), "Needle");
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.app_search.is_none());
                assert_eq!(shell.workspace.active, Some(a));
                assert_eq!(shell.workspace.project, "/work/another-project");
                assert_eq!(
                    shell.sessions[&b].read(cx).input.read(cx).content,
                    "preserve this draft"
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn app_search_shares_tab_sizing_order_cycle_and_close(cx: &mut TestAppContext) {
        cx.update(|cx| {
            input::bind(cx);
            super::super::bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, w, cx| {
                for _ in 0..10 {
                    let summary = locked(&shell.store)
                        .unwrap()
                        .create("codex", &shell.workspace.project)
                        .unwrap();
                    let id = summary.id;
                    shell.add(summary, cx);
                    shell.open(id, w, cx);
                }
                shell.open_app_search(w, cx);
                assert_eq!(shell.visible_tabs(cx).last(), Some(&TitleTab::Search));
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Search));
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let strip = visual.debug_bounds("title-tab-strip").unwrap();
        let search = visual.debug_bounds("search-title-tab").unwrap();
        let new = visual.debug_bounds("new-tab").unwrap();
        assert!(
            new.right() <= strip.right(),
            "Search and every session must fit before the new-tab button"
        );
        assert!(search.right() <= new.left());
        assert!(
            new.left() - search.right() <= px(4.5),
            "Search uses the same 2px gap and button margin"
        );
        assert_eq!(search.size.width, px(SELECTED_TAB_MIN_WIDTH));
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.tab_scroll.max_offset().x, px(0.));
                shell.cycle(-1, w, cx);
                assert!(shell.app_search.is_none());
                assert!(matches!(
                    shell.active_title_tab(cx),
                    Some(TitleTab::Session(_))
                ));
                shell.cycle(1, w, cx);
                assert!(shell.app_search.is_some());
                shell.reorder_title_tab(&TitleTab::Search, 0, cx);
                assert_eq!(shell.visible_tabs(cx).first(), Some(&TitleTab::Search));
                shell.run_title_tab_menu(TitleTab::Search, "right", w, cx);
                assert_eq!(shell.visible_tabs(cx), vec![TitleTab::Search]);
                assert!(shell.app_search.is_some());
                shell.close_search_tab(w, cx);
                assert!(!shell.workspace.title_order.contains(&TitleTab::Search));
                assert!(shell.app_search.is_none() && shell.saved_search.is_none());
            })
            .unwrap();
        visual.run_until_parked();
    }
}
