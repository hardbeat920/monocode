//! A paired phone shows a conversation's drafts in the thread. It draws its own
//! copy of the page and changes the same record the desktop's card does, so the
//! notes, the page's state and who is driving are one thing on either screen.
//!
//! The phone looks at a card just by scrolling past it, so a read with `peek`
//! never opens anything; only the person's own tap does.
use super::*;
use crate::draft::{self, Draft, DraftEvent};
use monocode_core::visual_bridge::{DraftAnchor, DraftContent, DraftFeedback};

/// How long a phone's read waits for the draft to change before answering.
const READ_WAIT: Duration = Duration::from_secs(15);
const READ_POLL: Duration = Duration::from_millis(150);
/// What one phone report may carry.
const MAX_NOTES: usize = 10;
const MAX_ACTIONS: usize = 20;
/// The widest picture of a dev-server draft a phone gets: a phone's width at
/// three pixels a point.
const FRAME_WIDTH: u32 = 1290;

impl Shell {
    /// `draft.read`: the draft's record, and its page when the phone does not
    /// have this version. With `since`, it answers once the record moves past
    /// that revision, or after a while with the same one. With `peek`, a draft
    /// that is not open only says so.
    pub(super) fn mobile_draft_read(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if request.params["peek"] == true {
            match self.mobile_draft_shut(&request.params, cx) {
                Ok(Some(shut)) => return request.reply(Ok(shut)),
                Ok(None) => {}
                Err(error) => return request.reply(Err(error)),
            }
        }
        let draft = match self.mobile_draft(&request.params, window, cx) {
            Ok(Some(draft)) => draft,
            Ok(None) => return request.reply(Ok(json!({"live": false}))),
            Err(error) => return request.reply(Err(error)),
        };
        let since = request.params["since"].as_u64();
        let known = request.params["generation"].as_u64();
        let light = request.params["light"].as_bool().unwrap_or(false);
        let weak = draft.downgrade();
        cx.spawn(async move |_, cx| {
            let started = std::time::Instant::now();
            loop {
                let Ok(revision) = weak.read_with(cx, |draft, _| draft.shared().revision()) else {
                    return request.reply(Ok(json!({"live": false})));
                };
                if since != Some(revision) || started.elapsed() >= READ_WAIT {
                    break;
                }
                cx.background_executor().timer(READ_POLL).await;
            }
            let reply = weak
                .read_with(cx, |draft, _| read(draft, known, light))
                .unwrap_or_else(|_| json!({"live": false}));
            request.reply(Ok(reply));
        })
        .detach();
    }

    /// `draft.frame`: a picture of a dev-server draft, which the phone cannot
    /// load from this Mac's localhost. The phone asks again while it shows it;
    /// each ask leases the page, so it runs with its card out of sight.
    pub(super) fn mobile_draft_frame(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = match self.mobile_draft(&request.params, window, cx) {
            Ok(Some(draft)) => draft,
            Ok(None) => return request.reply(Ok(json!({"live": false}))),
            Err(error) => return request.reply(Err(error)),
        };
        if draft.read(cx).is_html() {
            return request.reply(Err("The phone draws this draft's page itself".into()));
        }
        let width = request.params["width"]
            .as_u64()
            .map_or(FRAME_WIDTH, |width| (width as u32).clamp(320, FRAME_WIDTH));
        #[cfg_attr(not(target_os = "macos"), allow(clippy::let_unit_value))]
        let lease = draft.update(cx, |draft, cx| draft.lease(window, cx));
        let weak = draft.downgrade();
        cx.spawn(async move |_, cx| {
            let result = Draft::frame(&weak, width, cx).await;
            drop(lease);
            request.reply(result.map(|(jpeg, (width, height))| {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD.encode(jpeg);
                json!({
                    "live": true,
                    "frame": format!("data:image/jpeg;base64,{data}"),
                    "width": width,
                    "height": height,
                })
            }));
        })
        .detach();
    }

    /// `draft.action`: the person did something in the phone's copy.
    pub(super) fn mobile_draft_action(
        &mut self,
        params: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let draft = self
            .mobile_draft(params, window, cx)?
            .ok_or("That draft was closed")?;
        let action = params["action"].as_str().unwrap_or_default();
        if action == "close" {
            // As the card's own close: the page stays saved and can open again.
            draft.update(cx, |_, cx| cx.emit(DraftEvent::Closed));
            return Ok(json!({"live": false, "saved": true, "dismissed": true}));
        }
        draft.update(cx, |draft, cx| -> Result<(), String> {
            match action {
                "pin" => {
                    let notes: Vec<DraftFeedback> = serde_json::from_value(params["notes"].clone())
                        .map_err(|_| "Invalid notes")?;
                    let notes = notes.into_iter().take(MAX_NOTES).map(clean).collect();
                    draft.pin_from_phone(notes, cx);
                }
                "send" => draft.send(cx),
                "input" => {
                    let actions = params["actions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .take(MAX_ACTIONS)
                        .map(|action| clip(action, 100))
                        .collect();
                    draft.touch_from_phone(actions, cx);
                }
                "state" => {
                    let values = params["values"]
                        .as_object()
                        .ok_or("values must be an object")?;
                    // A page the agent has since replaced has no say in the new one.
                    let current = draft.shared().generation();
                    if params["generation"]
                        .as_u64()
                        .is_none_or(|sent| sent == current)
                    {
                        draft.patch_from_phone(values, cx)?;
                    }
                }
                "message" => {
                    let text = params["text"].as_str().unwrap_or_default().trim();
                    if !text.is_empty() {
                        draft.tell_from_phone(clip(text, 2000), cx);
                    }
                }
                "handBack" => draft.hand_back(cx),
                _ => return Err("Unsupported draft action".into()),
            }
            Ok(())
        })?;
        let known = params["generation"].as_u64();
        let light = params["light"].as_bool().unwrap_or(false);
        Ok(read(draft.read(cx), known, light))
    }

    /// The conversation and draft id a phone's request names.
    fn mobile_draft_name(
        &mut self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<(i64, String), String> {
        let session = self.mobile_session_of(params, cx)?;
        Ok((session, string(params, "draft").ok_or("Missing draft")?))
    }

    /// The saved session a phone's `conversationId` names, a Mono's included.
    pub(super) fn mobile_session_of(
        &mut self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<i64, String> {
        let conversation = string(params, "conversationId").unwrap_or_default();
        match self.mobile_target(&conversation, cx)? {
            Target::Session(session) => Ok(session),
            Target::Mono(mono) => self
                .monos
                .as_ref()
                .and_then(|monos| monos.read(cx).roster.get(&mono)?.session_id)
                .ok_or_else(|| "This Mono has no conversation yet".into()),
        }
    }

    /// What a phone is told about a draft that is not open, or `None` for one
    /// that is. Nothing opens here.
    fn mobile_draft_shut(
        &mut self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Option<Value>, String> {
        let (session, id) = self.mobile_draft_name(params, cx)?;
        if self.draft_named(session, &id, cx).is_some() {
            return Ok(None);
        }
        let saved = self
            .draft_file(session, &id)
            .and_then(|path| draft::Saved::read(&path));
        Ok(Some(json!({
            "live": false,
            "saved": saved.is_some(),
            "dismissed": saved.is_some_and(|saved| saved.dismissed()),
        })))
    }

    /// The draft a phone names. A saved one opens again for both screens, as
    /// its card does.
    fn mobile_draft(
        &mut self,
        params: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<Entity<Draft>>, String> {
        let (session, id) = self.mobile_draft_name(params, cx)?;
        Ok(self.reopen_draft(session, &id, window, cx))
    }
}

fn read(draft: &Draft, known: Option<u64>, light: bool) -> Value {
    let shared = draft.shared();
    let mut reply = shared.summary();
    reply["live"] = true.into();
    reply["title"] = draft.title().to_string().into();
    if let Some(height) = draft.phone_height() {
        reply["height"] = height.into();
    }
    if let DraftContent::Url(url) = shared.content() {
        // Shown, not loaded: the phone gets pictures of it with `draft.frame`.
        reply["url"] = url.as_str().into();
    } else if known != Some(shared.generation()) {
        reply["page"] = draft.phone_page(light).into();
    }
    reply
}

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// A note from the phone, held to what the desktop's page would send.
fn clean(note: DraftFeedback) -> DraftFeedback {
    let place = |value: f64| (value.clamp(0., 100.) * 10.).round() / 10.;
    DraftFeedback {
        note: clip(note.note.trim(), 600),
        tag: clip(&note.tag, 20),
        name: clip(&note.name, 60),
        x: place(note.x),
        y: place(note.y),
        at: note.at.and_then(DraftAnchor::checked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::visual_bridge::{DraftAction, DraftRequest};

    #[gpui::test]
    fn a_dev_server_draft_gives_the_phone_its_address_not_a_page(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Url("http://localhost:5173/".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: Some("App".into()),
                        id: Some("draft-1-server".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let params = json!({
                    "conversationId": shell.mobile_session_id(open),
                    "draft": "draft-1-server",
                    "action": "input",
                });
                let read = shell.mobile_draft_action(&params, window, cx).unwrap();
                assert_eq!(read["kind"], "url");
                assert_eq!(read["url"], "http://localhost:5173/");
                // The phone cannot load the desktop's localhost.
                assert!(read.get("page").is_none());
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_phone_changes_the_same_draft_the_desktop_card_shows(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                shell.workspace.classic_layout = false;
                shell.layout_changed(cx);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: Some("Pricing".into()),
                        id: Some("draft-1-phone".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let conversation = shell.mobile_session_id(open);
                let params = |more: Value| {
                    let mut params = json!({"conversationId": conversation, "draft": "draft-1-phone"});
                    params.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
                    params
                };

                // The phone gets the page in the same sandbox, for its own width.
                let first = shell
                    .mobile_draft_action(&params(json!({"action": "input", "actions": ["clicked \"Go\""]})), window, cx)
                    .unwrap();
                assert_eq!(first["live"], true);
                assert_eq!(first["title"], "Pricing");
                assert!(first["page"].as_str().unwrap().contains("ReactNativeWebView"));
                // Using the page there gives the person control here too.
                assert_eq!(first["control"], true);
                let draft = shell.draft_named(open, "draft-1-phone", cx).unwrap();
                assert!(draft.read(cx).shared().control());

                // A phone that has this version is not sent the page again.
                let generation = first["generation"].clone();
                let at = json!({"path": "0.1", "ax": 0.5, "ay": 0.25});
                let note = json!({"note": "Bigger", "tag": "button", "name": "Go", "x": 10, "y": 20, "at": at});
                let pinned = shell
                    .mobile_draft_action(
                        &params(json!({"action": "pin", "notes": [note], "generation": generation})),
                        window,
                        cx,
                    )
                    .unwrap();
                assert!(pinned.get("page").is_none());
                assert_eq!(pinned["pending"][0]["note"], "Bigger");
                assert_eq!(pinned["pins"][0]["side"], "phone");
                // The pin keeps the element it was made on; the agent's note does not.
                assert_eq!(pinned["pins"][0]["at"], at);
                assert!(pinned["pending"][0].get("at").is_none());
                assert_eq!(draft.read(cx).shared().pending().len(), 1);

                // No page has run here, so the phone's values start the record,
                // unless they come from a page the agent has since replaced.
                let old = json!(generation.as_u64().unwrap() - 1);
                let stale = json!({"action": "state", "values": {"a": 1}, "generation": old});
                shell.mobile_draft_action(&params(stale), window, cx).unwrap();
                assert!(draft.read(cx).shared().state().is_none());
                let started = shell
                    .mobile_draft_action(&params(json!({"action": "state", "values": {"a": 1}})), window, cx)
                    .unwrap();
                assert_eq!(started["state"]["a"], 1);
                assert!(shell
                    .mobile_draft_action(&params(json!({"action": "state", "values": {"b": 2}})), window, cx)
                    .is_err());
                assert!(shell
                    .mobile_draft_action(&params(json!({"action": "explode"})), window, cx)
                    .is_err());

                shell
                    .mobile_draft_action(&params(json!({"action": "send"})), window, cx)
                    .unwrap();
                let back = shell
                    .mobile_draft_action(&params(json!({"action": "handBack"})), window, cx)
                    .unwrap();
                assert_eq!(back["control"], false);
                assert_eq!(back["pending"], json!([]));
                let outbox = draft.update(cx, |draft, _| draft.take_outbox());
                assert_eq!(outbox.notes.len(), 1);
                assert_eq!(outbox.actions, ["clicked \"Go\""]);

                // A draft that is gone says so.
                let gone = json!({"conversationId": conversation, "draft": "draft-0-0", "action": "send"});
                assert!(shell.mobile_draft_action(&gone, window, cx).is_err());
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_phone_scrolling_past_a_closed_draft_leaves_it_closed(cx: &mut gpui::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("monocode-draft-peek-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = monocode_core::store::Store::open(&dir.join("monocode.db")).unwrap();
        let (window, _, _) = crate::shell::tests::window_with(cx, store);
        let id = "draft-1-peek";
        let (open, params) = window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: Some("Pricing".into()),
                        id: Some(id.into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let params = json!({"conversationId": shell.mobile_session_id(open), "draft": id});
                (open, params)
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                // An open draft reads as usual.
                assert_eq!(shell.mobile_draft_shut(&params, cx).unwrap(), None);
                // The card's close, from the phone.
                let mut close = params.clone();
                close["action"] = "close".into();
                let reply = shell.mobile_draft_action(&close, window, cx).unwrap();
                assert_eq!(reply["live"], false);
                assert_eq!(reply["dismissed"], true);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let shut = shell.mobile_draft_shut(&params, cx).unwrap().unwrap();
                assert_eq!(
                    shut,
                    json!({"live": false, "saved": true, "dismissed": true})
                );
                assert!(
                    shell.draft_named(open, id, cx).is_none(),
                    "a peek opens nothing"
                );
                // One that was never saved has nothing to open.
                let mut gone = params.clone();
                gone["draft"] = "draft-0-0".into();
                let shut = shell.mobile_draft_shut(&gone, cx).unwrap().unwrap();
                assert_eq!(shut["saved"], false);
            })
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui::test]
    fn a_monos_chat_reaches_the_drafts_in_its_conversation(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: Some("Plan".into()),
                        id: Some("draft-1-mono".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let monos = shell.mono_manager();
                let (chat, fresh) = monos.update(cx, |monos, _| {
                    let chat = monos.roster.create(Vec::new()).id.clone();
                    monos.roster.get_mut(&chat).unwrap().session_id = Some(open);
                    (chat, monos.roster.create(Vec::new()).id.clone())
                });
                let press = |mono: &str| {
                    json!({
                        "conversationId": format!("mono:{mono}"),
                        "draft": "draft-1-mono",
                        "action": "input",
                        "actions": ["clicked \"Go\""],
                    })
                };
                let read = shell
                    .mobile_draft_action(&press(&chat), window, cx)
                    .unwrap();
                assert_eq!(read["live"], true);
                assert_eq!(read["title"], "Plan");
                assert!(
                    shell
                        .mobile_draft_action(&press(&fresh), window, cx)
                        .is_err()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_session_says_when_the_agent_waits_on_a_draft(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: None,
                        id: Some("draft-1-wait".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.waiting_draft(open, cx), None);
                let draft = shell.draft_named(open, "draft-1-wait", cx).unwrap();
                draft.update(cx, |draft, _| draft.set_waiting_for_test(true));
                assert_eq!(
                    shell.waiting_draft(open, cx).as_deref(),
                    Some("draft-1-wait")
                );
            })
            .unwrap();
    }

    /// The Mac need not show the card for the person to answer on the phone.
    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn a_wait_keeps_the_page_running_with_no_card_on_the_mac(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, window, cx| {
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Wait { seconds: Some(1) },
                        width: None,
                        height: None,
                        light: None,
                        title: None,
                        id: Some("draft-1-away".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let draft = shell.draft_named(open, "draft-1-away", cx).unwrap();
                assert!(draft.read(cx).leased_for_test());
            })
            .unwrap();
    }

    #[gpui::test]
    fn notes_held_for_a_request_that_fails_reach_the_conversation(cx: &mut gpui::TestAppContext) {
        let (window, _, _) = crate::shell::tests::window(cx);
        let (open, draft) = window
            .update(cx, |shell, window, cx| {
                cx.set_reduce_motion(true);
                let open = shell.visible_session().expect("a conversation is open");
                let (reply, _answer) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: None,
                        content: Some(DraftContent::Html("<p>Hi</p>".into())),
                        action: DraftAction::Snapshot,
                        width: None,
                        height: None,
                        light: None,
                        title: None,
                        id: Some("draft-1-held".into()),
                        reply,
                    },
                    window,
                    cx,
                );
                let draft = shell.draft_named(open, "draft-1-held", cx).unwrap();

                // Sent during a wait, the notes are held for its reply.
                draft.update(cx, |draft, _| draft.set_waiting_for_test(true));
                let conversation = shell.mobile_session_id(open);
                let note =
                    json!({"note": "Bigger", "tag": "button", "name": "Go", "x": 10, "y": 20});
                for mut action in [
                    json!({"action": "input", "actions": ["clicked \"Go\""]}),
                    json!({"action": "pin", "notes": [note]}),
                    json!({"action": "send"}),
                ] {
                    action["conversationId"] = json!(conversation);
                    action["draft"] = json!("draft-1-held");
                    shell.mobile_draft_action(&action, window, cx).unwrap();
                }
                (open, draft)
            })
            .unwrap();
        // The event of the send runs here, while the wait still holds them.
        cx.run_until_parked();
        let refused = window
            .update(cx, |shell, window, cx| {
                draft.update(cx, |draft, _| draft.set_waiting_for_test(false));
                // The person has control, so the agent's change is refused.
                let (reply, refused) = std::sync::mpsc::channel();
                shell.handle_draft_request(
                    DraftRequest {
                        session: open,
                        draft: Some("draft-1-held".into()),
                        content: None,
                        action: DraftAction::Set {
                            values: serde_json::from_value(json!({"a": 1})).unwrap(),
                        },
                        width: None,
                        height: None,
                        light: None,
                        title: None,
                        id: None,
                        reply,
                    },
                    window,
                    cx,
                );
                refused
            })
            .unwrap();
        cx.run_until_parked();
        let error = refused.try_recv().unwrap().err().unwrap();
        assert!(error.contains("controls this draft"), "{error}");
        let outbox = draft.update(cx, |draft, _| draft.take_outbox());
        assert!(
            outbox.notes.is_empty(),
            "the notes went to the conversation"
        );
    }

    #[test]
    fn a_phones_note_is_held_to_what_a_page_would_send() {
        let note = clean(DraftFeedback {
            note: format!("  {}  ", "n".repeat(700)),
            tag: "button".repeat(10),
            name: "Go".into(),
            x: 140.,
            y: 33.333,
            at: Some(DraftAnchor {
                path: "0.1;alert(1)".into(),
                ax: 0.5,
                ay: 0.5,
            }),
        });
        assert_eq!(note.note.len(), 600);
        assert_eq!(note.tag.len(), 20);
        assert_eq!((note.x, note.y), (100., 33.3));
        // A path the page could not have sent is dropped; the pin falls back
        // to its percentages.
        assert_eq!(note.at, None);
    }
}
