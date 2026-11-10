//! File panes share the session pane tree and title strip. Moving a pane
//! transfers its entity, preserving dirty documents, selection and scroll.
use super::*;

impl Shell {
    pub(super) fn independent_file_pane(&mut self, id: i64, cx: &mut Context<Self>) -> Option<i64> {
        let pane = self.take_review_pane(id, cx)?;
        self.workspace.open(pane);
        Some(pane)
    }

    /// On a committed chat drop, leave its file surface in the original
    /// layout slot and give the existing conversation its own movable pane.
    pub(super) fn separate_session_review(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Option<i64> {
        if self.workspace.reviews.get(&id)?.chat_hidden {
            return None;
        }
        let pane = self.take_review_pane(id, cx)?;
        self.workspace.swap_session(id, pane);
        self.workspace.open(id);
        Some(pane)
    }

    /// A chat can be dropped on its attached file, but never on itself.
    pub(super) fn session_review_hit(&self, id: i64, position: Point<Pixels>) -> Option<Edge> {
        let state = self.workspace.reviews.get(&id)?;
        if state.chat_hidden {
            return None;
        }
        let bounds = self.split_bounds.get(&id)?.get()?;
        let width = bounds.size.width.as_f32();
        let rect = Rect {
            x: width * (1. - state.ratio),
            y: 0.,
            w: width * state.ratio,
            h: bounds.size.height.as_f32(),
        };
        let x = (position.x - bounds.left()).as_f32();
        let y = (position.y - bounds.top()).as_f32();
        (bounds.contains(&position) && x >= rect.x)
            .then(|| crate::pane_layout::edge_from_point(x, y, rect))
    }

    fn take_review_pane(&mut self, id: i64, cx: &mut Context<Self>) -> Option<i64> {
        if self.workspace.review_owners.contains_key(&id)
            || self
                .workspace
                .reviews
                .get(&id)
                .is_some_and(|r| r.chat_hidden)
        {
            return Some(id);
        }
        let review = self.reviews.remove(&id)?;
        let pane = self.workspace.next_pane_id();
        self.workspace.review_owners.insert(pane, id);
        self.workspace.reviews.remove(&id);
        self.reviews.insert(pane, review.clone());
        self.review_slide = None;
        review.update(cx, |r, cx| {
            r.state.chat_hidden = true;
            r.set_deferred(false, cx);
            cx.emit(crate::review::ReviewEvent::Changed);
            cx.notify();
        });
        self.workspace
            .reviews
            .insert(pane, review.read(cx).state.clone());
        Some(pane)
    }

    pub(super) fn open_file_pane(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let owner = self.workspace.session_for_pane(id);
        if self.deleting.contains(&owner) {
            return;
        }
        let Some(session) = self.ensure_session(owner, cx) else {
            return;
        };
        self.workspace.project = session.read(cx).summary.cwd.clone();
        if let Some(monos) = &self.monos {
            monos.update(cx, |m, cx| m.close(cx));
        }
        self.navigation_revision += 1;
        self.leave_app_views(cx);
        self.close_preview(cx);
        self.workspace.open(id);
        self.workspace
            .project_focus
            .insert(self.workspace.project.clone(), id);
        self.record_worktree_session(id, cx);
        self.history.record(id);
        self.review_focused = true;
        if let Some(review) = self.review_for(id, cx) {
            review.update(cx, |r, cx| r.focus_active(window, cx));
        }
        self.remember_title_tab(TitleTab::Session(self.workspace.tab_of(id).unwrap()));
        self.reveal_active_tab(cx);
        self.persist(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{review::ReviewTab, shell::tests::window};
    use gpui::{EntityInputHandler, TestAppContext, VisualTestContext, point};

    #[gpui::test]
    fn mono_file_link_opens_visible_editor_and_preserves_its_chat(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, session) = window(cx);
        let folder =
            std::env::temp_dir().join(format!("monocode-mono-file-link-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("README.md").to_string_lossy().into_owned();
        std::fs::write(&path, "# Heading\nsecond line\nthird line\n").unwrap();
        window
            .update(cx, |shell, w, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.create(Vec::new(), w, cx));
            })
            .unwrap();
        cx.run_until_parked();
        let (mono, chat) = window
            .update(cx, |shell, _, cx| {
                assert!(shell.mono_active(cx));
                let mono = shell.active_title_tab(cx).unwrap();
                let chat = shell.mono_manager().read(cx).chat().unwrap();
                chat.read(cx).input.clone().update(cx, |input, cx| {
                    input.set_text("Keep my Mono draft".into(), cx)
                });
                (mono, chat)
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("mono-composer").is_some(),
            "the action starts in the visible Mono"
        );
        window
            .update(visual, |_, w, cx| {
                w.dispatch_action(
                    Box::new(crate::file_editor::OpenPath(format!("{path}:2:3"))),
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("review-tab-strip").is_some(),
            "the file pane is rendered"
        );
        assert!(visual.debug_bounds("mono-composer").is_none());
        let (pane, editor) = window
            .update(visual, |shell, w, cx| {
                assert!(!shell.mono_active(cx));
                let pane = shell.workspace.active.unwrap();
                assert_ne!(pane, session, "the file has its own title tab");
                assert_eq!(shell.workspace.session_for_pane(pane), session);
                assert!(shell.visible_tabs(cx).contains(&mono));
                assert!(shell.visible_tabs(cx).contains(&TitleTab::Session(session)));
                let review = shell.active_review(cx).unwrap();
                assert!(review.read(cx).state.chat_hidden);
                let editor = review
                    .read(cx)
                    .active_editor()
                    .expect("the file editor is mounted");
                assert_eq!(editor.read(cx).path, path);
                assert!(
                    editor.read(cx).focus_handle(cx).contains_focused(w, cx),
                    "focus is inside the visible editor"
                );
                assert_eq!(
                    editor.read(cx).input.read(cx).selection_range(),
                    12..12,
                    "line and column survive navigation"
                );
                editor.read(cx).input.clone().update(cx, |input, cx| {
                    input.replace_text_in_range(None, "unsaved", w, cx)
                });
                shell.open_title_tab(&mono, w, cx);
                (pane, editor)
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("mono-composer").is_some());
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(mono.clone()));
                let restored = shell.mono_manager().read(cx).chat().unwrap();
                assert_eq!(
                    restored, chat,
                    "the original live Mono conversation is retained"
                );
                assert_eq!(
                    restored.read(cx).input.read(cx).content.as_ref(),
                    "Keep my Mono draft"
                );
                assert!(restored.read(cx).input.focus_handle(cx).is_focused(w));
                w.dispatch_action(Box::new(crate::file_editor::OpenPath(path.clone())), cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.workspace.active, Some(pane));
                let reopened = shell
                    .active_review(cx)
                    .unwrap()
                    .read(cx)
                    .active_editor()
                    .unwrap();
                assert_eq!(
                    reopened, editor,
                    "reopening keeps the existing dirty editor"
                );
                assert!(reopened.read(cx).dirty);
                assert!(reopened.read(cx).focus_handle(cx).contains_focused(w, cx));
                assert!(shell.visible_tabs(cx).contains(&mono));
            })
            .unwrap();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn file_word_wrap_is_shared_and_persisted_across_restart(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, session) = window(cx);
        let folder =
            std::env::temp_dir().join(format!("monocode-wrap-prefs-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for file in ["one.txt", "two.txt"] {
            std::fs::write(folder.join(file), "a long line ".repeat(50)).unwrap();
        }
        window
            .update(cx, |shell, w, cx| {
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: folder.join("one.txt").to_string_lossy().into_owned(),
                    },
                    w,
                    cx,
                );
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |_, w, cx| {
                w.dispatch_action(Box::new(crate::file_editor::ToggleWordWrap), cx)
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(!shell.workspace.preferences.file_word_wrap);
                assert!(
                    !shell.reviews[&session]
                        .read(cx)
                        .active_editor()
                        .unwrap()
                        .read(cx)
                        .input
                        .read(cx)
                        .word_wrap()
                );
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: folder.join("two.txt").to_string_lossy().into_owned(),
                    },
                    w,
                    cx,
                );
            })
            .unwrap();
        visual.run_until_parked();
        let (store, summaries, saved, project) = window
            .update(visual, |shell, _, cx| {
                assert!(
                    !shell.reviews[&session]
                        .read(cx)
                        .active_editor()
                        .unwrap()
                        .read(cx)
                        .input
                        .read(cx)
                        .word_wrap(),
                    "new files inherit the setting"
                );
                let store = locked(&shell.store).unwrap();
                let saved = store.ui_state().unwrap();
                let persisted: Workspace = serde_json::from_str(saved.as_ref().unwrap()).unwrap();
                assert!(!persisted.preferences.file_word_wrap);
                (
                    shell.store.clone(),
                    store.sessions().unwrap(),
                    saved,
                    shell.workspace.project.clone(),
                )
            })
            .unwrap();
        let restored = visual.update(|_, cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Shell::new(store, summaries, project, saved, cx))
            })
            .unwrap()
        });
        visual.run_until_parked();
        visual.refresh().unwrap();
        restored
            .update(visual, |shell, _, cx| {
                assert!(!shell.workspace.preferences.file_word_wrap);
                let editor = shell.reviews[&session].read(cx).active_editor().unwrap();
                assert!(
                    !editor.read(cx).input.read(cx).word_wrap(),
                    "restart restores the setting"
                );
            })
            .unwrap();
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn numbered_shortcuts_return_from_file_only_tabs_to_sessions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        // File-only tabs can retain a hidden chat or be detached from it.
        for detached in [false, true] {
            let (window, first, session) = window(cx);
            let file = window
                .update(cx, |shell, w, cx| {
                    shell.sessions[&first]
                        .update(cx, |s, _| s.summary.cwd = shell.workspace.project.clone());
                    shell.open(first, w, cx);
                    shell.open_review(
                        ReviewTab::File {
                            cwd: shell.workspace.project.clone(),
                            path: "/missing/shortcut-test.txt".into(),
                        },
                        w,
                        cx,
                    );
                    shell.open_review(
                        ReviewTab::Changes {
                            cwd: shell.workspace.project.clone(),
                            path: None,
                            staged: None,
                        },
                        w,
                        cx,
                    );
                    let file = if detached {
                        let file = shell.independent_file_pane(first, cx).unwrap();
                        shell.workspace.close(first);
                        shell.reorder_title_tab(&TitleTab::Session(file), 0, cx);
                        file
                    } else {
                        shell.reviews[&first].update(cx, |r, cx| {
                            r.state.chat_hidden = true;
                            cx.emit(crate::review::ReviewEvent::Changed);
                        });
                        first
                    };
                    shell.open(session, w, cx);
                    assert_eq!(
                        shell.visible_tabs(cx),
                        [TitleTab::Session(file), TitleTab::Session(session)]
                    );
                    file
                })
                .unwrap();
            let visual = &mut VisualTestContext::from_window(window.into(), cx);
            visual.run_until_parked();
            for (nested_tab, restored) in [(1, false), (0, false), (1, true)] {
                if restored {
                    window
                        .update(visual, |shell, _, _| {
                            // A restored tab has saved state before its pane is mounted.
                            shell.reviews.remove(&file);
                            shell.workspace.reviews.get_mut(&file).unwrap().active = nested_tab;
                        })
                        .unwrap();
                }
                visual.simulate_keystrokes(&format!("{modifier}-1"));
                visual.run_until_parked();
                window
                    .update(visual, |shell, _, _| {
                        assert_eq!(shell.workspace.active, Some(file));
                    })
                    .unwrap();
                if nested_tab == 0 {
                    let tab = visual.debug_bounds("file-tab-0").unwrap();
                    visual.simulate_click(tab.center(), Default::default());
                    visual.run_until_parked();
                }
                visual.simulate_keystrokes(&format!("{modifier}-2"));
                visual.run_until_parked();
                window
                    .update(visual, |shell, w, cx| {
                        assert_eq!(
                            shell.workspace.active,
                            Some(session),
                            "Cmd+2 returns from nested tab {nested_tab} (detached={detached}, restored={restored})"
                        );
                        assert!(
                            shell.sessions[&session]
                                .read(cx)
                                .input
                                .focus_handle(cx)
                                .is_focused(w)
                        );
                    })
                    .unwrap();
            }
        }
    }

    #[gpui::test]
    fn session_pane_grip_detaches_and_redocks_preserving_chat_and_dirty_files(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            bind_keys(cx);
            cx.set_reduce_motion(true);
        });
        let (window, _, session) = window(cx);
        let path = std::env::temp_dir().join(format!(
            "monocode-chat-pane-drag-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "original\n").unwrap();
        let (chat, review) = window
            .update(cx, |shell, w, cx| {
                let chat = shell.sessions[&session].clone();
                chat.read(cx).input.clone().update(cx, |input, cx| {
                    input.set_text("Keep this draft".into(), cx);
                });
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: path.to_string_lossy().into_owned(),
                    },
                    w,
                    cx,
                );
                (chat, shell.reviews[&session].clone())
            })
            .unwrap();
        cx.run_until_parked();
        let editor = window
            .update(cx, |_, w, cx| {
                let editor = review.read(cx).active_editor().unwrap();
                editor.read(cx).input.clone().update(cx, |input, cx| {
                    input.replace_text_in_range(None, "unsaved ", w, cx);
                });
                editor
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let grip_selector = Box::leak(format!("drag-session-pane-{session}").into_boxed_str());
        let grip = visual.debug_bounds(grip_selector).unwrap().center();

        // Clicks, tiny moves, self drops, outside drops and Escape do not
        // split the attached file from its session.
        for end in [
            grip,
            grip + point(px(2.), px(0.)),
            grip + point(px(30.), px(0.)),
            point(px(6.), px(100.)),
        ] {
            visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
            visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
        }
        let strip = visual
            .debug_bounds(Box::leak(format!("title-tab-{session}").into_boxed_str()))
            .unwrap();
        let before = point(strip.left() + px(8.), strip.center().y);
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(before, MouseButton::Left, Default::default());
        visual.simulate_keystrokes("escape");
        visual.simulate_mouse_up(before, MouseButton::Left, Default::default());
        window
            .update(visual, |shell, _, _| {
                assert!(shell.workspace.review_owners.is_empty());
                assert_eq!(shell.reviews[&session], review);
                assert!(shell.pane_drag.is_none());
            })
            .unwrap();

        // Pulling the chat onto the title strip leaves the file in its tab.
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(before, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(before, MouseButton::Left, Default::default());
        visual.run_until_parked();
        let file = window
            .update(visual, |shell, w, cx| {
                let file = *shell.workspace.review_owners.keys().next().unwrap();
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(session), TitleTab::Session(file)]
                );
                assert_eq!(shell.workspace.active, Some(session));
                assert!(!shell.reviews.contains_key(&session));
                assert_eq!(shell.reviews[&file], review);
                shell.open(file, w, cx);
                file
            })
            .unwrap();
        visual.run_until_parked();

        // Redock the session next to the file, then reorder it with its grip.
        let tab = visual
            .debug_bounds(Box::leak(format!("title-tab-{session}").into_boxed_str()))
            .unwrap()
            .center();
        let bounds = window
            .update(visual, |shell, _, _| shell.pane_bounds.get().unwrap())
            .unwrap();
        let right = point(bounds.right() - px(12.), bounds.center().y);
        visual.simulate_mouse_down(tab, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(right, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(right, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert_eq!(shell.workspace.layout(file).leaf_ids(), [file, session]);
            })
            .unwrap();
        let grip = visual.debug_bounds(grip_selector).unwrap().center();
        let left = point(bounds.left() + px(12.), bounds.center().y);
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(left, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(left, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.workspace.layout(file).leaf_ids(), [session, file]);
                assert_eq!(shell.workspace.active, Some(session));
                assert_eq!(shell.sessions[&session], chat);
                assert_eq!(
                    chat.read(cx).input.read(cx).content.as_ref(),
                    "Keep this draft"
                );
                assert_eq!(shell.reviews[&file], review);
                assert_eq!(review.read(cx).active_editor().unwrap(), editor);
                assert!(editor.read(cx).dirty);
                let saved = serde_json::to_string(&shell.workspace).unwrap();
                let sessions: Vec<_> = shell
                    .sessions
                    .iter()
                    .map(|(id, s)| (*id, s.read(cx).summary.cwd.clone()))
                    .collect();
                let restored =
                    Workspace::restore(Some(&saved), &sessions, &shell.workspace.project);
                assert_eq!(restored.layout(file), shell.workspace.layout(file));
                assert_eq!(restored.active, Some(session));
                assert_eq!(restored.review_owners, shell.workspace.review_owners);
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[gpui::test]
    fn session_pane_grip_places_chat_on_each_edge_of_its_attached_file(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let (window, _, session) = window(cx);
            window
                .update(cx, |shell, w, cx| {
                    shell.open_review(
                        ReviewTab::File {
                            cwd: shell.workspace.project.clone(),
                            path: "/tmp/chat-pane-edge.rs".into(),
                        },
                        w,
                        cx,
                    );
                })
                .unwrap();
            let visual = &mut VisualTestContext::from_window(window.into(), cx);
            visual.run_until_parked();
            let grip = visual
                .debug_bounds(Box::leak(
                    format!("drag-session-pane-{session}").into_boxed_str(),
                ))
                .unwrap()
                .center();
            let file_bounds = visual.debug_bounds("review-frame").unwrap();
            let end = match edge {
                Edge::Left => point(file_bounds.left() + px(12.), file_bounds.center().y),
                Edge::Right => point(file_bounds.right() - px(12.), file_bounds.center().y),
                Edge::Top => point(file_bounds.center().x, file_bounds.top() + px(12.)),
                Edge::Bottom => point(file_bounds.center().x, file_bounds.bottom() - px(12.)),
            };
            visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
            window
                .update(visual, |shell, _, _| {
                    assert_eq!(
                        shell.pane_drag.as_ref().unwrap().over,
                        Some((session, edge))
                    );
                })
                .unwrap();
            visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
            visual.run_until_parked();
            window
                .update(visual, |shell, _, _| {
                    let file = *shell.workspace.review_owners.keys().next().unwrap();
                    let tab = shell.workspace.tab_of(session).unwrap();
                    let leaves = shell.workspace.layout(tab).leaves();
                    let chat_rect = leaves.iter().find(|l| l.id == session).unwrap().rect;
                    let file_rect = leaves.iter().find(|l| l.id == file).unwrap().rect;
                    assert_eq!(shell.workspace.active, Some(session));
                    assert!(match edge {
                        Edge::Left => chat_rect.x < file_rect.x,
                        Edge::Right => chat_rect.x > file_rect.x,
                        Edge::Top => chat_rect.y < file_rect.y,
                        Edge::Bottom => chat_rect.y > file_rect.y,
                    });
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn file_pane_grip_detaches_and_redocks_without_recreating_dirty_editors(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, session) = window(cx);
        let path =
            std::env::temp_dir().join(format!("monocode-pane-drag-{}.txt", std::process::id()));
        std::fs::write(&path, "original\n").unwrap();
        let file = path.to_string_lossy().into_owned();
        let review = window
            .update(cx, |shell, w, cx| {
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: file.clone(),
                    },
                    w,
                    cx,
                );
                shell.reviews[&session].clone()
            })
            .unwrap();
        cx.run_until_parked();
        let editor = window
            .update(cx, |_, w, cx| {
                let editor = review.read(cx).active_editor().unwrap();
                let input = editor.read(cx).input.clone();
                input.update(cx, |i, cx| i.replace_text_in_range(None, "unsaved ", w, cx));
                editor
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let grip = visual.debug_bounds("drag-file-pane").unwrap().center();
        // A click and a drop outside the workspace leave the pane attached.
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        let outside = point(px(6.), px(100.));
        visual.simulate_mouse_move(outside, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(outside, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, _| {
                assert!(shell.workspace.review_owners.is_empty());
                assert_eq!(shell.reviews[&session], review);
            })
            .unwrap();
        let tab = visual
            .debug_bounds(Box::leak(format!("title-tab-{session}").into_boxed_str()))
            .unwrap();
        let drop = point(tab.left() + px(8.), tab.center().y);
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(drop, MouseButton::Left, Default::default());
        window
            .update(visual, |shell, _, _| {
                assert_eq!(shell.pane_drag.as_ref().unwrap().insert, Some(0));
            })
            .unwrap();
        visual.simulate_mouse_up(drop, MouseButton::Left, Default::default());
        visual.run_until_parked();
        let pane = window
            .update(visual, |shell, _, cx| {
                let pane = shell.workspace.active.unwrap();
                assert!(pane < 0);
                assert_eq!(shell.workspace.session_for_pane(pane), session);
                assert!(!shell.reviews.contains_key(&session));
                assert_eq!(shell.reviews[&pane], review);
                assert_eq!(review.read(cx).active_editor().unwrap(), editor);
                assert!(editor.read(cx).dirty);
                assert_eq!(
                    shell.visible_tabs(cx),
                    [TitleTab::Session(pane), TitleTab::Session(session)]
                );
                assert_eq!(
                    shell.tab_copy(pane, cx).headline,
                    path.file_name().unwrap().to_string_lossy()
                );
                let saved = serde_json::to_string(&shell.workspace).unwrap();
                let sessions: Vec<_> = shell
                    .sessions
                    .iter()
                    .map(|(id, s)| (*id, s.read(cx).summary.cwd.clone()))
                    .collect();
                let restored =
                    Workspace::restore(Some(&saved), &sessions, &shell.workspace.project);
                assert_eq!(restored.active, Some(pane));
                assert_eq!(restored.tabs, shell.workspace.tabs);
                assert_eq!(restored.review_owners, shell.workspace.review_owners);
                pane
            })
            .unwrap();
        assert!(
            visual
                .debug_bounds(Box::leak(format!("title-tab-{pane}").into_boxed_str()))
                .is_some()
        );

        // A closed title tab retains its dirty documents for reopening.
        window
            .update(visual, |shell, w, cx| shell.close(pane, w, cx))
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert!(shell.workspace.tab_of(pane).is_none());
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: file.clone(),
                    },
                    w,
                    cx,
                );
                assert_eq!(shell.workspace.active, Some(pane));
                assert_eq!(shell.reviews[&pane], review);
                assert_eq!(review.read(cx).active_editor().unwrap(), editor);
                assert!(editor.read(cx).dirty);
            })
            .unwrap();
        visual.run_until_parked();

        // The independent file tab can be pulled back into a split.
        window
            .update(visual, |shell, w, cx| shell.open(session, w, cx))
            .unwrap();
        visual.run_until_parked();
        let tab = visual
            .debug_bounds(Box::leak(format!("title-tab-{pane}").into_boxed_str()))
            .unwrap()
            .center();
        let bounds = window
            .update(visual, |shell, _, _| shell.pane_bounds.get().unwrap())
            .unwrap();
        let left = point(bounds.left() + px(12.), bounds.center().y);
        visual.simulate_mouse_down(tab, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(left, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(left, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.workspace.layout(session).leaf_ids(), [pane, session]);
                assert_eq!(shell.reviews[&pane], review);
                assert!(editor.read(cx).dirty);
            })
            .unwrap();

        // Its grip also reorders panes within the split.
        let grip = visual.debug_bounds("drag-file-pane").unwrap().center();
        let right = point(bounds.right() - px(12.), bounds.center().y);
        visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(right, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(right, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.workspace.layout(session).leaf_ids(), [session, pane]);
                assert_eq!(review.read(cx).active_editor().unwrap(), editor);
                assert!(editor.read(cx).dirty);
                let saved = serde_json::to_string(&shell.workspace).unwrap();
                let sessions: Vec<_> = shell
                    .sessions
                    .iter()
                    .map(|(id, s)| (*id, s.read(cx).summary.cwd.clone()))
                    .collect();
                let restored =
                    Workspace::restore(Some(&saved), &sessions, &shell.workspace.project);
                assert_eq!(restored.layout(session).leaf_ids(), [session, pane]);
                assert_eq!(restored.active, Some(pane));
                let remaining: Vec<_> = sessions
                    .into_iter()
                    .filter(|(id, _)| *id != session)
                    .collect();
                let pruned = Workspace::restore(Some(&saved), &remaining, &shell.workspace.project);
                assert!(!pruned.review_owners.contains_key(&pane));
                assert!(!pruned.reviews.contains_key(&pane));
                assert!(pruned.tab_of(pane).is_none());
            })
            .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[gpui::test]
    fn file_tab_drag_reorders_and_keeps_the_selected_editor(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, session) = window(cx);
        let review = window
            .update(cx, |shell, w, cx| {
                for name in ["a.rs", "b.rs", "c.rs"] {
                    shell.open_review(
                        ReviewTab::File {
                            cwd: shell.workspace.project.clone(),
                            path: format!("/tmp/{name}"),
                        },
                        w,
                        cx,
                    );
                }
                shell.reviews[&session].clone()
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let first = visual.debug_bounds("file-tab-0").unwrap();
        let last = visual.debug_bounds("file-tab-2").unwrap();
        let start = point(first.left() + px(20.), first.center().y);
        let end = start + point(last.left() - first.left(), px(0.));
        visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
        let editor = window
            .update(visual, |_, _, cx| review.read(cx).active_editor().unwrap())
            .unwrap();
        visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let state = &review.read(cx).state;
                assert_eq!(
                    state.tabs.iter().map(ReviewTab::label).collect::<Vec<_>>(),
                    ["b.rs", "c.rs", "a.rs"]
                );
                assert_eq!(state.active, 2);
                assert_eq!(shell.workspace.reviews[&session].active, 2);
                assert_eq!(review.read(cx).active_editor().unwrap(), editor);
            })
            .unwrap();
    }

    #[gpui::test]
    fn file_pane_grip_places_an_embedded_review_on_each_edge(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, session) = window(cx);
        let review = window
            .update(cx, |shell, w, cx| {
                shell.open_review(
                    ReviewTab::File {
                        cwd: shell.workspace.project.clone(),
                        path: "/tmp/pane-edge.rs".into(),
                    },
                    w,
                    cx,
                );
                shell.reviews[&session].clone()
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        for edge in [Edge::Left, Edge::Top, Edge::Bottom, Edge::Right] {
            let grip = visual.debug_bounds("drag-file-pane").unwrap().center();
            let target = visual
                .debug_bounds(Box::leak(format!("pane-{session}").into_boxed_str()))
                .unwrap_or_else(|| {
                    window
                        .update(visual, |shell, _, _| shell.pane_bounds.get().unwrap())
                        .unwrap()
                });
            let end = match edge {
                Edge::Left => point(target.left() + px(12.), target.center().y),
                Edge::Right => point(target.right() - px(12.), target.center().y),
                Edge::Top => point(target.center().x, target.top() + px(12.)),
                Edge::Bottom => point(target.center().x, target.bottom() - px(12.)),
            };
            visual.simulate_mouse_down(grip, MouseButton::Left, Default::default());
            visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
            visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
            visual.run_until_parked();
            window
                .update(visual, |shell, _, _| {
                    let pane = shell.workspace.active.unwrap();
                    assert_eq!(shell.workspace.review_owners.len(), 1);
                    assert_eq!(shell.reviews[&pane], review);
                    assert!(!shell.workspace.reviews.contains_key(&session));
                    let leaves = shell.workspace.layout(session).leaves();
                    let chat = leaves.iter().find(|l| l.id == session).unwrap().rect;
                    let file = leaves.iter().find(|l| l.id == pane).unwrap().rect;
                    assert!(match edge {
                        Edge::Left => file.x < chat.x,
                        Edge::Right => file.x > chat.x,
                        Edge::Top => file.y < chat.y,
                        Edge::Bottom => file.y > chat.y,
                    });
                })
                .unwrap();
        }
    }
}
