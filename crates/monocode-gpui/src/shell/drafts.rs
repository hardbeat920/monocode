//! Drafts the agent opens in the open conversation. Each is a card in the
//! thread; this holds the live pages behind those cards. Requests from the
//! agent's MCP tools arrive here, run on the main thread, and answer with what
//! the page looks like.
use super::*;
use crate::draft::{self, Draft, DraftEvent};
use monocode_core::visual_bridge::{
    DraftAction, DraftContent, DraftFeedback, DraftReply, DraftRequest,
};

/// Live pages kept per conversation; opening another retires the oldest, and
/// its card says so. Only one of them runs at a time, the one last opened or
/// used; the others sleep, showing a picture of their page until the person
/// clicks one (see `keep_running`).
const LIVE_PER_CONVERSATION: usize = 3;

impl Shell {
    pub(super) fn init_draft_inbox(&mut self, cx: &mut Context<Self>) {
        if cx.is_test() {
            return;
        }
        let reopens = crate::draft::reopen_requests();
        cx.spawn(async move |this, cx| {
            while let Ok((session, id)) = reopens.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        // A card the person closed stays closed, with its
                        // button to open it again.
                        if this.draft_dismissed(session, &id, cx) {
                            draft::mark_saved(&id, cx);
                        } else {
                            this.reopen_draft(session, &id, window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let requests = crate::draft::inbox();
        cx.spawn(async move |this, cx| {
            while let Ok(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_draft_request(request, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// The session whose conversation is on screen.
    #[cfg(test)]
    pub(super) fn visible_session(&self) -> Option<i64> {
        self.workspace
            .active
            .map(|pane| self.workspace.session_for_pane(pane))
    }

    fn reply_draft(request: &DraftRequest, result: Result<DraftReply, String>) {
        let _ = request.reply.send(result);
    }

    pub(super) fn draft_named(&self, session: i64, id: &str, cx: &App) -> Option<Entity<Draft>> {
        self.drafts
            .get(&session)?
            .iter()
            .find(|draft| draft.read(cx).id() == id)
            .cloned()
    }

    pub(super) fn handle_draft_request(
        &mut self,
        request: DraftRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session = request.session;
        let draft = match (&request.draft, &request.content) {
            (Some(id), _) => {
                // A retired page, or one not drawn since a restart, comes
                // back from its saved file under the same id. One the person
                // closed does not: that is theirs to open again.
                let reopened = if self.draft_dismissed(session, id, cx) {
                    None
                } else {
                    self.reopen_draft(session, id, window, cx)
                };
                match reopened {
                    Some(draft) => draft,
                    None => {
                        return Self::reply_draft(
                        &request,
                        Err("That draft was closed or replaced. Start a new one with html or url.".into()),
                    );
                    }
                }
            }
            (None, Some(content)) => {
                self.create_draft(session, content.clone(), &request, window, cx)
            }
            (None, None) => {
                return Self::reply_draft(
                    &request,
                    Err("Pass html or url to start a draft".into()),
                );
            }
        };
        if matches!(request.action, DraftAction::Close) {
            let id = draft.read(cx).id().to_owned();
            self.drop_draft(session, &id, true, cx);
            cx.notify();
            return Self::reply_draft(&request, Ok(DraftReply::Closed { draft: id }));
        }
        cx.notify();
        self.keep_running(session, &draft, cx);
        // The page keeps running for the request even when its conversation
        // is in the background. That holds for a wait too: the person may be
        // looking at another conversation when they answer.
        let lease = draft.update(cx, |draft, cx| draft.lease(window, cx));
        let weak = draft.downgrade();
        cx.spawn(async move |this, cx| {
            let result = Draft::run(&weak, &request, cx).await;
            drop(lease);
            // A failed wait hands over no notes, so they go as a message.
            if result.is_err()
                && let Some(draft) = weak.upgrade()
            {
                let _ = this.update(cx, |this, cx| this.deliver_draft_notes(session, &draft, cx));
            }
            Self::reply_draft(&request, result);
        })
        .detach();
    }

    fn create_draft(
        &mut self,
        session: i64,
        content: DraftContent,
        request: &DraftRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Draft> {
        let draft = cx.new(|cx| {
            Draft::new(
                request.id.clone(),
                session,
                content,
                (request.width, request.height),
                request.light,
                request.title.clone(),
                cx,
            )
        });
        // The bridge names the drafts that have cards in the thread.
        if let Some(path) = request
            .id
            .as_deref()
            .and_then(|id| self.draft_file(session, id))
        {
            draft.update(cx, |draft, cx| draft.save_to(path, cx));
        }
        self.add_draft(session, &draft, window, cx);
        draft
    }

    /// Where a draft is saved in its conversation's folder.
    pub(super) fn draft_file(&self, session: i64, id: &str) -> Option<std::path::PathBuf> {
        crate::runtime::locked(&self.store)
            .ok()?
            .thread_draft_file(session, id)
            .ok()
            .flatten()
    }

    /// Whether the person closed this draft's card and has not opened it
    /// since. An open page is never dismissed.
    fn draft_dismissed(&self, session: i64, id: &str, cx: &App) -> bool {
        self.draft_named(session, id, cx).is_none()
            && self
                .draft_file(session, id)
                .and_then(|path| draft::Saved::read(&path))
                .is_some_and(|saved| saved.dismissed())
    }

    /// Opens a saved draft again under its own id, for a card whose page is
    /// not open: after a restart, or once it was retired. An open one is
    /// reused; `None` if it was never saved or was closed.
    pub(super) fn reopen_draft(
        &mut self,
        session: i64,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Draft>> {
        if let Some(draft) = self.draft_named(session, id, cx) {
            return Some(draft);
        }
        let path = self.draft_file(session, id)?;
        let saved = draft::Saved::read(&path)?;
        let was_dismissed = saved.dismissed();
        let draft = cx.new(|cx| Draft::reopen(id.to_owned(), session, saved, path, cx));
        if was_dismissed {
            // Open again for good: a restart must not find it closed.
            draft.read(cx).save_now(cx);
        }
        self.add_draft(session, &draft, window, cx);
        cx.notify();
        Some(draft)
    }

    fn add_draft(
        &mut self,
        session: i64,
        draft: &Entity<Draft>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = draft.read(cx).id().to_owned();
        self.observations.push(cx.subscribe_in(
            draft,
            window,
            move |this, draft, event: &DraftEvent, _, cx| match event {
                DraftEvent::Closed => {
                    // A saved page stays saved: the card offers to open it
                    // again. One that was never saved is gone for good.
                    let kept = draft.update(cx, |draft, _| draft.dismiss());
                    this.drop_draft(session, &id, !kept, cx);
                    cx.notify();
                }
                DraftEvent::Send => this.deliver_draft_notes(session, draft, cx),
                DraftEvent::Woke => this.keep_running(session, draft, cx),
            },
        ));
        let live = self.drafts.entry(session).or_default();
        live.push(draft.clone());
        let retired = (live.len() > LIVE_PER_CONVERSATION).then(|| live.remove(0));
        if let Some(old) = retired {
            old.read(cx).hide();
            old.read(cx).save_now(cx);
            let old_id = old.read(cx).id().to_owned();
            draft::unregister(&old_id, cx);
        }
        let id = draft.read(cx).id().to_owned();
        draft::register(&id, draft, cx);
        self.keep_running(session, draft, cx);
    }

    /// Runs `draft` and puts the conversation's other drafts to sleep, so
    /// variants side by side cost what one does.
    fn keep_running(&mut self, session: i64, draft: &Entity<Draft>, cx: &mut Context<Self>) {
        draft.update(cx, |draft, cx| draft.wake(cx));
        for other in self.drafts.get(&session).into_iter().flatten() {
            if other != draft {
                other.update(cx, |other, cx| other.sleep(cx));
            }
        }
    }

    /// The person sent their notes, or used a page control that sends a
    /// message. An agent that is waiting gets them as the
    /// result of its wait; otherwise they go to the conversation as a message,
    /// steering a running turn where the provider can take it.
    fn deliver_draft_notes(&mut self, session: i64, draft: &Entity<Draft>, cx: &mut Context<Self>) {
        if draft.read(cx).waiting() {
            return;
        }
        let outbox = draft.update(cx, |draft, _| draft.take_outbox());
        if outbox.notes.is_empty() && outbox.messages.is_empty() {
            return;
        }
        let title = draft.read(cx).title();
        let text = DraftFeedback::message(&title, &outbox.notes, &outbox.messages, &outbox.actions);
        // A Mono's chat is not among the sessions, and its agent drafts too.
        let view = self.sessions.get(&session).cloned().or_else(|| {
            let monos = self.monos.as_ref()?.read(cx);
            monos.open_chat(&monos.roster.of_session(session)?.id)
        });
        if let Some(view) = view {
            view.update(cx, |view, cx| view.send_draft_feedback(text, cx));
        }
    }

    /// Takes a draft out of its conversation. With `close` it is no longer
    /// saved, so its card stays closed with nothing to open again.
    fn drop_draft(&mut self, session: i64, id: &str, close: bool, cx: &mut Context<Self>) {
        let Some(live) = self.drafts.get_mut(&session) else {
            return;
        };
        let Some(at) = live.iter().position(|draft| draft.read(cx).id() == id) else {
            return;
        };
        let draft = live.remove(at);
        if live.is_empty() {
            self.drafts.remove(&session);
        }
        draft.read(cx).hide();
        if close {
            draft.update(cx, |draft, cx| draft.forget(cx));
        }
        draft::unregister(id, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn request(
        session: i64,
        draft: Option<String>,
        content: Option<DraftContent>,
        action: DraftAction,
    ) -> (DraftRequest, mpsc::Receiver<Result<DraftReply, String>>) {
        let (reply, receive) = mpsc::channel();
        (
            DraftRequest {
                session,
                draft,
                content,
                action,
                width: None,
                height: None,
                light: None,
                title: None,
                id: None,
                reply,
            },
            receive,
        )
    }

    #[gpui::test]
    fn a_draft_lives_in_its_conversation_whether_or_not_it_is_open(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                shell.workspace.classic_layout = false;
                shell.layout_changed(cx);
                let open = shell.visible_session().expect("a conversation is open");
                let html = || Some(DraftContent::Html("<p>Hi</p>".into()));

                // A conversation in the background gets its draft too; the
                // page runs out of sight for the agent.
                let hidden = open + 1000;
                let (mut background, _answer) =
                    request(hidden, None, html(), DraftAction::Snapshot);
                background.id = Some("draft-2-bg".into());
                shell.handle_draft_request(background, window, cx);
                assert_eq!(shell.drafts[&hidden].len(), 1);
                assert!(draft::live("draft-2-bg", cx).is_some());
                shell.drop_draft(hidden, "draft-2-bg", true, cx);
                assert!(shell.drafts.is_empty());

                // The open one gets a draft under the id the bridge chose, which
                // its card in the thread finds.
                let (mut shown, _answer) = request(open, None, html(), DraftAction::Snapshot);
                shown.id = Some("draft-1-abc".into());
                shell.handle_draft_request(shown, window, cx);
                assert_eq!(shell.drafts[&open].len(), 1);
                assert!(draft::live("draft-1-abc", cx).is_some());
                // No side panel opens for it.
                assert!(!shell.workspace.right_panel_open);

                // Trying again on that card with new content keeps one card.
                let (retry, _answer) = request(
                    open,
                    Some("draft-1-abc".into()),
                    html(),
                    DraftAction::Snapshot,
                );
                shell.handle_draft_request(retry, window, cx);
                assert_eq!(shell.drafts[&open].len(), 1);

                // A stale id is refused rather than acting on another page.
                let (stale, answer) =
                    request(open, Some("draft-0-0".into()), None, DraftAction::Snapshot);
                shell.handle_draft_request(stale, window, cx);
                assert!(answer.try_recv().unwrap().is_err());
                assert_eq!(shell.drafts[&open].len(), 1);

                // Closing frees it and its card stops finding a page.
                let (close, answer) =
                    request(open, Some("draft-1-abc".into()), None, DraftAction::Close);
                shell.handle_draft_request(close, window, cx);
                assert!(matches!(
                    answer.try_recv().unwrap(),
                    Ok(DraftReply::Closed { draft }) if draft == "draft-1-abc"
                ));
                assert!(shell.drafts.is_empty());
                assert!(draft::live("draft-1-abc", cx).is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_draft_is_saved_and_opens_again_after_a_restart_unless_it_was_closed(
        cx: &mut gpui::TestAppContext,
    ) {
        let dir = std::env::temp_dir().join(format!("monocode-draft-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = monocode_core::store::Store::open(&dir.join("monocode.db")).unwrap();
        let (window, _, _) = crate::shell::tests::window_with(cx, store);
        let id = "draft-1-5aved";
        let file = window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().unwrap();
                let page = Some(DraftContent::Html("<p>Kept</p>".into()));
                let (mut first, _answer) = request(open, None, page, DraftAction::Snapshot);
                first.id = Some(id.into());
                first.title = Some("Chart".into());
                first.width = Some(520);
                shell.handle_draft_request(first, window, cx);
                shell.draft_file(open, id).unwrap()
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let open = shell.visible_session().unwrap();
                let mut values = serde_json::Map::new();
                values.insert("zoom".into(), 3.into());
                shell
                    .draft_named(open, id, cx)
                    .unwrap()
                    .update(cx, |draft, cx| draft.page_reported(values, cx));
            })
            .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        assert!(file.is_file(), "the draft is saved in its conversation");

        // A restart: the page is gone, but its card asks for it.
        window
            .update(cx, |shell, window, cx| {
                let open = shell.visible_session().unwrap();
                shell.drafts.clear();
                draft::unregister(id, cx);
                let draft = shell.reopen_draft(open, id, window, cx).unwrap();
                let draft = draft.read(cx);
                assert_eq!(draft.id(), id);
                assert_eq!(draft.html(), Some("<p>Kept</p>"));
                assert_eq!(draft.title().as_ref(), "Chart");
                assert_eq!(draft.shared().state().unwrap()["zoom"], 3);
                assert!(draft::live(id, cx).is_some());

                // Closed by the agent: no longer saved, so it stays closed.
                let (close, _answer) = request(open, Some(id.into()), None, DraftAction::Close);
                shell.handle_draft_request(close, window, cx);
                assert!(!file.exists());
                assert!(shell.reopen_draft(open, id, window, cx).is_none());
            })
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn a_card_the_person_closed_stays_closed_but_opens_again(cx: &mut gpui::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("monocode-draft-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = monocode_core::store::Store::open(&dir.join("monocode.db")).unwrap();
        let (window, _, _) = crate::shell::tests::window_with(cx, store);
        let id = "draft-1-c1ose";
        let (file, open) = window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().unwrap();
                let page = Some(DraftContent::Html("<p>Mine</p>".into()));
                let (mut first, _answer) = request(open, None, page, DraftAction::Snapshot);
                first.id = Some(id.into());
                shell.handle_draft_request(first, window, cx);
                (shell.draft_file(open, id).unwrap(), open)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                // The person presses the card's close button.
                let draft = shell.draft_named(open, id, cx).unwrap();
                draft.update(cx, |_, cx| cx.emit(DraftEvent::Closed));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(draft::live(id, cx).is_none(), "the page is gone");
                assert!(file.is_file(), "its saved page is kept");
                assert!(draft::reopenable(id, cx), "the card offers to open it");
                assert!(shell.draft_dismissed(open, id, cx));
            })
            .unwrap();
        window
            .update(cx, |shell, window, cx| {
                let open = shell.visible_session().unwrap();
                // The agent cannot revive a page the person put away.
                let (agent, answer) = request(open, Some(id.into()), None, DraftAction::Snapshot);
                shell.handle_draft_request(agent, window, cx);
                assert!(answer.try_recv().unwrap().is_err());
                assert!(draft::live(id, cx).is_none());
                // The card's own button does.
                let reopened = shell.reopen_draft(open, id, window, cx).unwrap();
                assert_eq!(reopened.read(cx).html(), Some("<p>Mine</p>"));
                assert!(!shell.draft_dismissed(open, id, cx));
            })
            .unwrap();
        cx.run_until_parked();
        // Opened again for good: the saved page no longer says it is closed.
        assert!(!draft::Saved::read(&file).unwrap().dismissed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn drafts_stack_in_the_thread_and_the_oldest_is_retired(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                shell.workspace.classic_layout = false;
                shell.layout_changed(cx);
                let open = shell.visible_session().unwrap();
                let mut answers = Vec::new();
                for n in 0..=LIVE_PER_CONVERSATION {
                    let page = Some(DraftContent::Html(format!("<p>{n}</p>")));
                    let (mut next, answer) = request(open, None, page, DraftAction::Snapshot);
                    next.id = Some(format!("draft-1-{n}"));
                    shell.handle_draft_request(next, window, cx);
                    answers.push(answer);
                }
                assert_eq!(shell.drafts[&open].len(), LIVE_PER_CONVERSATION);
                assert!(draft::live("draft-1-0", cx).is_none());
                assert!(draft::live(&format!("draft-1-{LIVE_PER_CONVERSATION}"), cx).is_some());
                // Only the newest runs; the others sleep.
                let asleep = |shell: &Shell, cx: &App| {
                    shell.drafts[&open]
                        .iter()
                        .map(|draft| draft.read(cx).is_asleep())
                        .collect::<Vec<_>>()
                };
                assert_eq!(asleep(shell, cx), [true, true, false]);
                // Using an older one runs it instead.
                let (again, _answer) =
                    request(open, Some("draft-1-2".into()), None, DraftAction::Snapshot);
                shell.handle_draft_request(again, window, cx);
                assert_eq!(asleep(shell, cx), [true, false, true]);
            })
            .unwrap();
    }

    /// The page draws over everything GPUI paints, so any surface drawn in the
    /// thread's place has to leave it unpainted for the sentinel to hide.
    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn a_draft_steps_aside_for_every_surface_drawn_in_its_threads_place(
        cx: &mut gpui::TestAppContext,
    ) {
        use super::classic_surface_tests::Surface;
        use monocode_core::{transcript::Row, visual_bridge::DraftCard};

        cx.update(|cx| cx.set_reduce_motion(true));
        let (window, _, _) = crate::shell::tests::window(cx);
        let open = window
            .update(cx, |shell, window, cx| {
                shell.workspace.classic_layout = false;
                shell.layout_changed(cx);
                let open = shell.visible_session().expect("a conversation is open");
                let html = Some(DraftContent::Html("<p>Hi</p>".into()));
                let (mut shown, _answer) = request(open, None, html, DraftAction::Snapshot);
                shown.id = Some("draft-1-away".into());
                shell.handle_draft_request(shown, window, cx);
                shell.sessions[&open].update(cx, |view, cx| {
                    view.summary.has_messages = true;
                    cx.notify();
                });
                let card = DraftCard {
                    draft: "draft-1-away".into(),
                    title: "Pricing".into(),
                };
                let transcript = shell.sessions[&open].read(cx).transcript.clone();
                transcript.update(cx, |view, cx| {
                    view.patch(
                        0,
                        vec![
                            Row {
                                role: "user".into(),
                                text: "Mock a pricing page".into(),
                                first: true,
                                sent_at: None,
                            },
                            Row {
                                role: monocode_core::visual_bridge::DRAFT_ROLE.into(),
                                text: serde_json::to_string(&card).unwrap(),
                                first: true,
                                sent_at: None,
                            },
                        ],
                        cx,
                    )
                });
                open
            })
            .unwrap();
        let id = gpui::AnyWindowHandle::from(window).window_id();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let painted = |visual: &mut gpui::VisualTestContext| {
            visual.refresh().unwrap();
            let card = visual.debug_bounds("draft-card").is_some();
            let draft = visual
                .update(|_, cx| draft::live("draft-1-away", cx).map(|d| d.read(cx).painted_in(id)));
            (card, draft)
        };
        assert_eq!(
            painted(visual),
            (true, Some(true)),
            "the page is in the thread"
        );
        // While the agent works, the window redraws around a thread that did
        // not change, such as a spinner in the sidebar. The page stays put
        // rather than flashing.
        for _ in 0..3 {
            window.update(visual, |_, _, cx| cx.notify()).unwrap();
            assert_eq!(
                visual.update(
                    |_, cx| draft::live("draft-1-away", cx).map(|d| d.read(cx).painted_in(id))
                ),
                Some(true),
                "a redraw of the window alone"
            );
        }

        window
            .update(visual, |shell, window, cx| {
                shell.open_settings(crate::settings::Section::General, window, cx)
            })
            .unwrap();
        assert_eq!(painted(visual), (false, Some(false)), "Settings");
        window
            .update(visual, |shell, window, cx| shell.close_settings(window, cx))
            .unwrap();
        assert_eq!(painted(visual), (true, Some(true)), "back from Settings");

        for surface in [
            Surface::Search,
            Surface::Inbox,
            Surface::Notes,
            Surface::Automations,
            Surface::Mono,
        ] {
            window
                .update(visual, |shell, window, cx| surface.open(shell, window, cx))
                .unwrap();
            assert_eq!(painted(visual), (false, Some(false)), "{surface:?}");
            window
                .update(visual, |shell, window, cx| {
                    surface.close(shell, open, window, cx)
                })
                .unwrap();
            assert_eq!(painted(visual), (true, Some(true)), "back from {surface:?}");
        }
    }
}
