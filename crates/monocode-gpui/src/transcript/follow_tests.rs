use super::*;
use gpui::{TestAppContext, WindowBounds, WindowOptions, point};

fn row(role: &str, text: &str) -> Row {
    Row {
        role: role.into(),
        text: text.into(),
        first: true,
        sent_at: None,
    }
}

fn chat(cx: &mut TestAppContext) -> gpui::WindowHandle<TranscriptView> {
    chat_mode(cx, true)
}

fn chat_mode(cx: &mut TestAppContext, mono: bool) -> gpui::WindowHandle<TranscriptView> {
    cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(900.), px(600.)),
                ))),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| {
                    let mut view = TranscriptView::new();
                    if mono {
                        view.mono_mode("cat", 0xe86b5f, "MonoCat");
                    }
                    view
                })
            },
        )
        .unwrap()
    })
}

fn render(window: gpui::WindowHandle<TranscriptView>, cx: &mut TestAppContext) {
    window.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.refresh().unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn ordinary_send_keeps_the_previous_anchor_until_its_prompt_arrives(cx: &mut TestAppContext) {
    for reading_history in [false, true] {
        let window = chat_mode(cx, false);
        window
            .update(cx, |view, _, cx| {
                view.smooth_reveal = false;
                view.patch(
                    0,
                    vec![
                        row("user", "Earlier question"),
                        row("assistant", &"Earlier answer.\n\n".repeat(30)),
                    ],
                    cx,
                );
                view.streaming(true, cx);
                view.visible = false;
                view.patch(
                    2,
                    vec![row("user", "Previous prompt"), row("assistant", "Done.")],
                    cx,
                );
                view.streaming(false, cx);
                view.visible = true;
            })
            .unwrap();
        render(window, cx);
        let before = window
            .update(cx, |view, _, cx| {
                if reading_history {
                    view.list.scroll_by(px(-80.));
                }
                let before = view.list.logical_scroll_top();
                view.streaming(true, cx);
                assert_eq!(view.anchor_turn, Some(2));
                assert_eq!(view.list.is_following_tail(), !reading_history);
                before
            })
            .unwrap();
        // The background worker may not deliver the saved user row in the
        // same frame as begin_turn. That intermediate frame must stay still.
        render(window, cx);
        window
            .update(cx, |view, _, cx| {
                let after = view.list.logical_scroll_top();
                assert_eq!(after.item_ix, before.item_ix);
                assert_eq!(after.offset_in_item, before.offset_in_item);
                assert_eq!(view.follow.offset, 0.);
                assert!(view.prompt_rise.is_none());
                view.patch(4, vec![row("user", "New prompt")], cx);
                assert_eq!(view.anchor_turn, Some(4));
                assert_eq!(view.prompt_rise.map(|(at, _)| at), Some(4));
                assert!(view.prompt_rise_pending);
                assert!(view.list.is_following_tail());
                assert!(view.follow.measured.is_none());
            })
            .unwrap();
    }
}

#[gpui::test]
fn mono_new_turn_keeps_its_pending_message_below_the_previous_answer(cx: &mut TestAppContext) {
    for reduced in [false, true] {
        cx.update(|cx| cx.set_reduce_motion(reduced));
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![row("user", "Previous prompt"), row("assistant", "Final answer")],
                cx,
            );
            view.set_pending(
                vec![PendingMessage {
                    id: "next".into(),
                    text: "Next prompt".into(),
                    error: None,
                    sent_at: None,
                    attachments: Vec::new(),
                }],
                cx,
            );
            let assert_order = |view: &TranscriptView| {
                let answer = view.rows.iter().position(|r| {
                    r.key.0 == 1 && matches!(r.body, Body::Markdown(_))
                }).unwrap();
                let pending = view.rows.iter().position(|r| matches!(r.body, Body::Pending(0))).unwrap();
                assert!(pending > answer, "a new turn must stay below the previous final answer, including while delivery starts");
            };
            assert_order(view);
            assert!(!view.stacks_on_next(0), "a new turn does not stack onto the finished prompt");
            view.streaming(true, cx);
            assert_order(view);

            let entrance = view.pending_entrances.get("next").copied();
            view.submit_prompt(
                "Next prompt",
                &monocode_core::agent::Settings {
                    mono_message_id: Some("next".into()),
                    ..Default::default()
                },
                cx,
            );
            assert!(!view.rows.iter().any(|r| matches!(r.body, Body::Pending(_))),
                "the optimistic prompt replaces its pending bubble before storage acknowledges it");
            assert_eq!(view.entrances.get(&2).copied(), entrance,
                "delivery must keep the pending bubble's animation clock");
            let keys: Vec<_> = view.rows.iter().map(|r| r.key).collect();
            let accepted = view.source[2..].to_vec();
            view.patch(2, accepted, cx);
            view.set_pending(Vec::new(), cx);
            assert_eq!(view.rows.iter().map(|r| r.key).collect::<Vec<_>>(), keys,
                "storage acceptance must not move or duplicate the prompt");
            assert_eq!(view.mono_activity(), Some((3, "Thinking…".into())));
            assert_eq!(view.entrances.get(&2).copied(), entrance);

            view.set_pending(vec![PendingMessage {
                id: "follow-up".into(), text: "Another detail".into(), error: None,
                sent_at: None, attachments: Vec::new(),
            }], cx);
            assert!(view.stacks_on_next(2));
            let follow_up = view.rows.iter().position(|r| matches!(r.body, Body::Pending(0))).unwrap();
            let footer = view.rows.iter().position(|r| matches!(r.body, Body::Actions { complete: false, .. })).unwrap();
            assert!(follow_up < footer);
        });
    }
}

#[gpui::test]
fn mono_pending_messages_show_thinking_and_keep_the_same_height_on_delivery(
    cx: &mut TestAppContext,
) {
    for reduced in [false, true] {
        cx.update(|cx| cx.set_reduce_motion(reduced));
        for history in [false, true] {
            let window = chat(cx);
            window
                .update(cx, |view, _, cx| {
                    if history {
                        view.patch(
                            0,
                            vec![
                                row("user", "Previous prompt"),
                                row("assistant", "Final answer"),
                            ],
                            cx,
                        );
                    }
                    view.set_pending(
                        vec![PendingMessage {
                            id: "next".into(),
                            text: "Next prompt".into(),
                            error: None,
                            sent_at: None,
                            attachments: Vec::new(),
                        }],
                        cx,
                    );
                    view.pending_entrances.insert(
                        "next".into(),
                        Instant::now() - std::time::Duration::from_secs(1),
                    );
                })
                .unwrap();
            render(window, cx);
            let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
            assert!(
                visual.debug_bounds("mono-turn-ticker").is_some(),
                "a new message shows Thinking before the outbox is saved"
            );
            let span = |visual: &mut gpui::VisualTestContext| {
                let bottom = visual.debug_bounds("mono-turn-ticker").unwrap().bottom();
                let top = window
                    .update(visual, |_, window, _| {
                        let scale = window.scale_factor();
                        window
                            .painted_quads()
                            .into_iter()
                            .filter(|q| {
                                let height = q.bounds.size.height.0 / scale;
                                (35. ..=37.).contains(&height)
                                    && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
                            })
                            .map(|q| q.bounds.top().0 / scale)
                            .reduce(f32::max)
                            .expect("the latest user bubble is painted")
                    })
                    .unwrap();
                f32::from(bottom) - top
            };
            let before = span(visual);
            window
                .update(visual, |view, _, cx| {
                    view.streaming(true, cx);
                    view.submit_prompt(
                        "Next prompt",
                        &monocode_core::agent::Settings {
                            mono_message_id: Some("next".into()),
                            ..Default::default()
                        },
                        cx,
                    );
                })
                .unwrap();
            render(window, visual);
            assert!(visual.debug_bounds("mono-turn-ticker").is_some());
            let after = span(visual);
            assert!(
                (before - after).abs() < 0.25,
                "delivery must preserve the bubble and Thinking header's height: {before} -> {after}"
            );
        }
    }
}

#[gpui::test]
fn mono_steered_bubbles_align_and_stay_together_after_the_answer_and_reload(
    cx: &mut TestAppContext,
) {
    for reduced in [false, true] {
        cx.update(|cx| cx.set_reduce_motion(reduced));
        let window = chat(cx);
        window
            .update(cx, |view, _, cx| {
                view.smooth_reveal = false;
                view.streaming(true, cx);
                view.patch(
                    0,
                    // The Mono is still thinking: nothing said yet.
                    vec![row("user", "what"), row("assistant", "")],
                    cx,
                );
                view.entrances.clear();
                view.set_pending(
                    vec![PendingMessage {
                        id: "steer".into(),
                        text: "is".into(),
                        error: None,
                        sent_at: None,
                        attachments: Vec::new(),
                    }],
                    cx,
                );
                view.pending_entrances.insert(
                    "steer".into(),
                    Instant::now() - std::time::Duration::from_secs(1),
                );
            })
            .unwrap();
        let assert_alignment = |cx: &mut TestAppContext| {
            render(window, cx);
            window
                .update(cx, |_, window, _| {
                    let scale = window.scale_factor();
                    let mut bubbles: Vec<_> = window
                        .painted_quads()
                        .into_iter()
                        .filter(|q| {
                            (35. ..=37.).contains(&(q.bounds.size.height.0 / scale))
                                && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
                        })
                        .map(|q| {
                            (
                                q.bounds.top().0 / scale,
                                q.bounds.bottom().0 / scale,
                                q.bounds.right().0 / scale,
                            )
                        })
                        .collect();
                    bubbles.sort_by(|a, b| a.0.total_cmp(&b.0));
                    assert_eq!(bubbles.len(), 2, "one bubble per user message");
                    assert!(
                        (bubbles[0].2 - bubbles[1].2).abs() < 0.25,
                        "all user bubbles share the same right edge: {bubbles:?}"
                    );
                    assert!(
                        (bubbles[1].0 - bubbles[0].1 - 4.).abs() < 0.25,
                        "consecutive bubbles keep compact spacing: {bubbles:?}"
                    );
                })
                .unwrap();
        };
        let assert_group = |view: &TranscriptView| {
            let users: Vec<_> = view
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row.body, Body::Plain(_)))
                .map(|(ix, row)| (ix, row.key.0))
                .collect();
            assert_eq!(
                users.iter().map(|(_, at)| *at).collect::<Vec<_>>(),
                vec![0, 2]
            );
            let headers: Vec<_> = view
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| matches!(row.body, Body::WorkLine { .. }))
                .collect();
            assert_eq!(headers.len(), 1, "one Mono reply for the whole burst");
            assert!(
                users[1].0 < headers[0].0,
                "both user messages stay before the Mono reply"
            );
            assert_eq!(view.mono_group_start(3), 1);
            let (response, _) = view.response_rows(1);
            assert!(
                response
                    .iter()
                    .any(|row| row.text == "Final answer to both messages")
            );
            assert!(view.stacks_on_next(0));
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|row| row.key.0 == 1 && matches!(row.body, Body::Markdown(_))),
                "nothing was said between the user's messages"
            );
        };
        assert_alignment(cx);
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    2,
                    vec![
                        row("user", "is"),
                        row("assistant", "Final answer to both messages"),
                    ],
                    cx,
                );
                view.set_pending(Vec::new(), cx);
                assert_group(view);
            })
            .unwrap();
        assert_alignment(cx);
        let (rows, receipts) = window
            .update(cx, |view, _, cx| {
                view.streaming(false, cx);
                assert_group(view);
                (
                    view.source.clone(),
                    serde_json::to_string(&view.turns).unwrap(),
                )
            })
            .unwrap();
        assert_alignment(cx);
        let restored = cx.new(|cx| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view.turns = serde_json::from_str(&receipts).unwrap();
            view.restore_history(rows, cx);
            view
        });
        restored.update(cx, |view, cx| {
            assert_group(view);
            view.streaming(true, cx);
            view.patch(
                4,
                vec![row("user", "A new conversation turn"), row("assistant", "")],
                cx,
            );
            assert_eq!(
                view.mono_group_start(5),
                5,
                "a message after the final answer starts its own turn"
            );
        });
    }
}

#[gpui::test]
fn a_steer_after_the_mono_spoke_reads_in_order_as_a_new_turn(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let view = cx.new(|_| {
        let mut view = TranscriptView::new();
        view.mono_mode("cat", 0xe86b5f, "MonoCat");
        view
    });
    view.update(cx, |view, cx| {
        // Provider-side the steer joins the running turn; on screen it follows
        // the Mono's opening sentence, and the rest of the reply comes after it.
        view.turns.insert(
            3,
            monocode_core::agent::TurnInfo {
                joined_turn: Some(1),
                ..Default::default()
            },
        );
        view.patch(
            0,
            vec![
                row("user", "Check the recent changes"),
                row("assistant", "I'll start a background review."),
                row("user", "thanks"),
                row("assistant", "The review is running now."),
            ],
            cx,
        );
        assert_eq!(view.mono_group_start(3), 3);
        let at = |key: usize, user: bool| {
            view.rows
                .iter()
                .position(|r| {
                    r.key.0 == key
                        && if user {
                            matches!(r.body, Body::Plain(_))
                        } else {
                            matches!(r.body, Body::Markdown(_))
                        }
                })
                .unwrap()
        };
        assert!(at(0, true) < at(1, false));
        assert!(at(1, false) < at(2, true));
        assert!(at(2, true) < at(3, false));
        assert_eq!(
            view.rows
                .iter()
                .filter(|r| matches!(r.body, Body::WorkLine { .. }))
                .count(),
            2,
            "the Mono's words after the steer open a new turn"
        );
        assert!(!view.stacks_on_next(0));
    });
}

#[gpui::test]
fn mono_follow_ups_stay_above_thinking_until_the_final_answer(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let view = cx.new(|_| {
        let mut view = TranscriptView::new();
        view.mono_mode("cat", 0xe86b5f, "MonoCat");
        view
    });
    view.update(cx, |view, cx| {
        view.streaming(true, cx);
        view.patch(
            0,
            vec![row("user", "Original prompt"), row("assistant", "")],
            cx,
        );
        view.set_pending(
            vec![PendingMessage {
                id: "follow-up".into(),
                text: "Another detail".into(),
                error: None,
                sent_at: None,
                attachments: Vec::new(),
            }],
            cx,
        );
        for (text, spoke) in [("", false), ("Working on the answer", true)] {
            view.patch(1, vec![row("assistant", text)], cx);
            let pending = view
                .rows
                .iter()
                .position(|r| matches!(r.body, Body::Pending(0)))
                .unwrap();
            let thinking = view
                .rows
                .iter()
                .position(|r| matches!(r.body, Body::WorkLine { turn: 1, .. }))
                .unwrap();
            if spoke {
                // Once the Mono has said something, the chat reads in order.
                assert!(!view.stacks_on_next(0));
                assert!(
                    pending > thinking,
                    "a message sent after the Mono spoke follows its words"
                );
            } else {
                assert!(
                    view.stacks_on_next(0),
                    "the original bubble keeps compact spacing above its queued follow-up"
                );
                assert!(
                    pending < thinking,
                    "a mid-turn message joins the prompt above Thinking"
                );
            }
            assert_eq!(
                view.rows
                    .iter()
                    .filter(|r| matches!(r.body, Body::Pending(_)))
                    .count(),
                1
            );
        }
        view.streaming(false, cx);
        assert!(!view.stacks_on_next(0));
        let pending = view
            .rows
            .iter()
            .position(|r| matches!(r.body, Body::Pending(0)))
            .unwrap();
        let answer = view
            .rows
            .iter()
            .rposition(|r| matches!(r.body, Body::Markdown(_)))
            .unwrap();
        assert!(
            pending > answer,
            "a message awaiting a new provider turn follows the settled answer"
        );
    });
}

#[gpui::test]
fn optimistic_prompt_anchors_once_and_worker_acceptance_keeps_its_entrance(
    cx: &mut TestAppContext,
) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                vec![
                    row("user", "Earlier question"),
                    row("assistant", "Earlier answer"),
                ],
                cx,
            );
            view.visible = true;
            view.streaming(true, cx);
            view.submit_prompt(
                "New question",
                &monocode_core::agent::Settings::default(),
                cx,
            );
            assert_eq!(view.anchor_turn, Some(2));
            assert_eq!(view.introduced_prompt, Some(2));
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(&row.body, Body::Plain(text) if text == "New question"))
            );
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(row.body, Body::WorkLine { .. }))
            );
            let entrance = view.prompt_rise;
            assert!(entrance.is_some());
            let accepted = view.source[2..].to_vec();
            view.patch(2, accepted, cx);
            assert_eq!(view.source.len(), 4);
            assert_eq!(
                view.prompt_rise, entrance,
                "saving must not restart the message animation"
            );
            assert_eq!(view.anchor_turn, Some(2));
        })
        .unwrap();
}

// Patch with row entrances disabled to isolate follow motion from disclosures.
#[gpui::test]
fn ordinary_short_replies_fill_the_prompt_spacer_without_moving_the_chat(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.smooth_reveal = false;
            view.streaming(true, cx);
            view.visible = false;
            view.patch(
                0,
                vec![row("user", "Prompt"), row("assistant", "Short reply.")],
                cx,
            );
            view.visible = true;
            view.prompt_rise = None;
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.patch(
                1,
                vec![row("assistant", "Short reply.\n\nAnother paragraph.")],
                cx,
            );
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| {
            assert!(
                view.follow.offset.abs() < 0.25,
                "text should consume the spacer before the prompt moves"
            );
            assert!(view.follow.measured.as_ref().unwrap().footer > BOTTOM_INSET);
        })
        .unwrap();
}

#[gpui::test]
fn ordinary_overflow_glides_and_keeps_settling_after_streaming_ends(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.anchor_prompts = false;
            view.smooth_reveal = false;
            view.streaming(true, cx);
            view.visible = false;
            view.patch(
                0,
                vec![row("user", "Prompt"), row("assistant", "A short reply.")],
                cx,
            );
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.patch(
                1,
                vec![row("assistant", &"Another paragraph.\n\n".repeat(35))],
                cx,
            );
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    let before = window
        .update(cx, |view, _, _| {
            assert!(view.follow.offset > 0.);
            assert!(view.follow.offset <= 0.75 * f32::from(view.viewport.size.height));
            assert!(view.list.is_following_tail());
            view.follow.offset
        })
        .unwrap();
    window
        .update(cx, |view, _, cx| {
            view.streaming(false, cx);
            view.follow.last_frame = Some(Instant::now() - std::time::Duration::from_millis(16));
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| {
            assert!(
                view.follow.offset >= 0. && view.follow.offset < before,
                "completion must continue the glide rather than snap to the end"
            );
        })
        .unwrap();
}

#[gpui::test]
fn ordinary_height_collapse_eases_back_and_a_new_prompt_resets_follow(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.anchor_prompts = false;
            view.smooth_reveal = false;
            view.streaming(true, cx);
            view.visible = false;
            view.patch(
                0,
                vec![
                    row("user", "Prompt"),
                    row("assistant", &"A paragraph.\n\n".repeat(35)),
                ],
                cx,
            );
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.patch(1, vec![row("assistant", "A shorter result.")], cx);
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    let before = window
        .update(cx, |view, _, _| {
            assert!(
                view.follow.offset < 0.,
                "shrinking content should preserve its painted position"
            );
            view.follow.offset
        })
        .unwrap();
    window
        .update(cx, |view, _, _| {
            view.follow.last_frame = Some(Instant::now() - std::time::Duration::from_millis(16));
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            assert!(view.follow.offset > before && view.follow.offset <= 0.);
            view.patch(2, vec![row("user", "Next prompt")], cx);
            assert_eq!(view.follow.offset, 0.);
            assert!(
                view.follow.measured.is_none(),
                "a sent prompt starts a fresh baseline"
            );
        })
        .unwrap();
}

struct SubmitChat {
    transcript: gpui::Entity<TranscriptView>,
    input: gpui::Entity<crate::input::Input>,
}
impl gpui::Render for SubmitChat {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1().min_h_0().child(self.transcript.clone()))
            .child(div().w_full().p_2().child(self.input.clone()))
    }
}

fn input_chat(cx: &mut TestAppContext, text: &str) -> gpui::WindowHandle<SubmitChat> {
    cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(500.), px(700.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let input = cx.new(crate::input::Input::new);
                    input.update(cx, |input, cx| {
                        input.mono_composer("MonoCat", cx);
                        input.set_text(text.to_owned(), cx);
                    });
                    let transcript = cx.new(|cx| {
                        let mut view = TranscriptView::new();
                        view.mono_mode("cat", 0xe86b5f, "MonoCat");
                        view.patch(
                            0,
                            vec![row("user", "Earlier"), row("assistant", "Done")],
                            cx,
                        );
                        view
                    });
                    SubmitChat { transcript, input }
                })
            },
        )
        .unwrap()
    })
}

#[gpui::test]
fn input_height_changes_preserve_the_chats_painted_position_and_follow_momentum(
    cx: &mut TestAppContext,
) {
    let window = input_chat(cx, "First line\nSecond line\nThird line");
    cx.refresh().unwrap();
    cx.run_until_parked();
    let before = window
        .update(cx, |_, window, _| bubble_top(window))
        .unwrap();
    window
        .update(cx, |chat, _, cx| {
            let text = chat.input.update(cx, |input, cx| input.take(cx)).unwrap();
            chat.transcript.update(cx, |view, cx| {
                view.follow.frame_elapsed = Some(0.);
                view.streaming(true, cx);
                view.patch(2, vec![row("user", &text), row("assistant", "")], cx);
            });
            cx.notify();
        })
        .unwrap();
    cx.refresh().unwrap();
    cx.run_until_parked();
    let entering = window
        .update(cx, |chat, window, cx| {
            assert!(
                chat.transcript.read(cx).follow.offset > 40.,
                "clearing a multiline input must not cancel the chat's glide"
            );
            bubble_top(window)
        })
        .unwrap();
    assert!(
        (before - entering).abs() < 3.,
        "the input collapse must preserve the previous bubble's painted position on the first frame: {before} -> {entering}"
    );
    let mut previous = entering;
    for _ in 0..12 {
        let offset = window
            .update(cx, |chat, _, cx| {
                chat.transcript.update(cx, |view, cx| {
                    view.follow.frame_elapsed = Some(16.);
                    cx.notify();
                    view.follow.offset
                })
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |chat, window, cx| {
                let top = bubble_top(window);
                let follow = &chat.transcript.read(cx).follow;
                assert!(
                    follow.offset > offset * 0.5,
                    "the input resize must not erase the motion on the next frame"
                );
                assert!(top <= previous + 1., "the chat must keep moving upward");
                previous = top;
            })
            .unwrap();
    }
    // Growing the input preserves an in-flight glide even after the reply's grace period.
    let before_grow = previous;
    window
        .update(cx, |chat, _, cx| {
            chat.input.update(cx, |input, cx| {
                input.set_text("One\nTwo\nThree\nFour".into(), cx)
            });
            chat.transcript.update(cx, |view, cx| {
                view.streaming(false, cx);
                view.follow.until = None;
                view.follow.last_frame = None;
                view.follow.frame_elapsed = Some(0.);
                cx.notify();
            });
            cx.notify();
        })
        .unwrap();
    cx.refresh().unwrap();
    cx.run_until_parked();
    let after_grow = window
        .update(cx, |_, window, _| bubble_top(window))
        .unwrap();
    assert!(
        (before_grow - after_grow).abs() < 3.,
        "input growth must join the existing motion: {before_grow} -> {after_grow}"
    );
    // A real window resize remains a fresh layout baseline.
    cx.simulate_window_resize(window.into(), size(px(500.), px(750.)));
    cx.refresh().unwrap();
    cx.run_until_parked();
    window
        .update(cx, |chat, _, cx| {
            assert_eq!(chat.transcript.read(cx).follow.offset, 0.);
        })
        .unwrap();
}

#[gpui::test]
fn multiline_sends_keep_sliding_when_the_composer_shrinks_and_delivery_is_immediate(
    cx: &mut TestAppContext,
) {
    let texts = [
        "First line\nSecond line".to_owned(),
        "This message wraps across multiple lines in the bubble. ".repeat(5),
        (0..14)
            .map(|ix| format!("Line {ix}: 🙂 café 漢字"))
            .collect::<Vec<_>>()
            .join("\n"),
    ];
    for text in texts {
        for remove_first in [false, true] {
            let window = input_chat(cx, &text);
            cx.refresh().unwrap();
            cx.run_until_parked();
            let before = window
                .update(cx, |chat, _, cx| {
                    chat.transcript.read(cx).viewport.size.height
                })
                .unwrap();
            window.update(cx, |chat, _, cx| {
                let sent = chat.input.update(cx, |input, cx| input.take(cx)).unwrap();
                chat.transcript.update(cx, |view, cx| {
                    view.set_pending(vec![PendingMessage {id:"queued".into(),text:sent.clone().into(),error:None,sent_at:None,attachments:Vec::new()}], cx);
                    let born = Instant::now() - std::time::Duration::from_millis(10);
                    view.pending_entrances.insert("queued".into(), born);
                    if remove_first {
                        view.set_pending(Vec::new(), cx);
                    }
                    view.streaming(true, cx);
                    let mut delivered = monocode_core::transcript::Transcript::default();
                    delivered.begin("user");
                    delivered.append(&sent);
                    delivered.begin("assistant");
                    for row in &mut delivered.rows {row.sent_at=None;}
                    view.patch(2, delivered.rows, cx);
                    view.set_pending(Vec::new(), cx);
                    assert_eq!(view.entrances.get(&2), Some(&born),
                        "delivery must continue the queued entrance, including split multiline messages");
                });
                cx.notify();
            }).unwrap();
            // The input's new height reaches the transcript through layout.
            for _ in 0..2 {
                cx.refresh().unwrap();
                cx.run_until_parked();
                window.update(cx, |_, _, cx| cx.notify()).unwrap();
            }
            window
                .update(cx, |chat, _, cx| {
                    chat.transcript.update(cx, |view, cx| {
                        assert!(
                            view.viewport.size.height >= before + px(18.),
                            "submitting a multiline draft shrinks the composer"
                        );
                        assert!(
                            view.follow.offset > 0.,
                            "the composer collapse must preserve scroll easing"
                        );
                        // Sample the beginning of the retained entrance, independently of scroll easing.
                        view.follow.reset();
                        view.entrances
                            .insert(2, Instant::now() - std::time::Duration::from_millis(10));
                        cx.notify();
                    })
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            let top = |window: &mut Window| {
                window
                    .painted_quads()
                    .into_iter()
                    .find(|quad| {
                        (quad.corner_radii.top_left.0 / window.scale_factor() - 12.).abs() < 0.1
                            && quad.bounds.size.height.0 / window.scale_factor() > 40.
                    })
                    .map(|quad| quad.bounds.top().0 / window.scale_factor())
                    .expect("the multiline bubble is painted")
            };
            let entering = window
                .update(cx, |chat, window, cx| {
                    let entering = top(window);
                    chat.transcript.update(cx, |view, cx| {
                        view.entrances.remove(&2);
                        cx.notify();
                    });
                    entering
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            let settled = window.update(cx, |_, window, _| top(window)).unwrap();
            assert!(
                entering > settled + 5.,
                "the multiline bubble rises independently of scroll easing: {entering} -> {settled}"
            );
        }
    }
}

#[gpui::test]
fn delivery_does_not_restart_settled_bubbles_or_override_reduced_motion(cx: &mut TestAppContext) {
    for reduced in [false, true] {
        cx.update(|cx| cx.set_reduce_motion(reduced));
        for remove_first in [false, true] {
            let view = cx.new(|_| {
                let mut view = TranscriptView::new();
                view.mono_mode("cat", 0xe86b5f, "MonoCat");
                view
            });
            view.update(cx, |view, cx| {
                // UTF-8 byte chunking, independently of explicit newline chunking.
                let text = "🙂 café 漢字 ".repeat(200);
                view.set_pending(
                    vec![PendingMessage {
                        id: "q".into(),
                        text: text.clone().into(),
                        error: None,
                        sent_at: None,
                        attachments: Vec::new(),
                    }],
                    cx,
                );
                if !reduced {
                    view.pending_entrances.insert(
                        "q".into(),
                        Instant::now() - std::time::Duration::from_secs(1),
                    );
                }
                if remove_first {
                    view.set_pending(Vec::new(), cx);
                }
                view.streaming(true, cx);
                let mut delivered = monocode_core::transcript::Transcript::default();
                delivered.begin("user");
                delivered.append(&text);
                assert!(delivered.rows.len() > 1);
                view.patch(0, delivered.rows, cx);
                assert!(
                    !view.entrances.contains_key(&0),
                    "settled and reduced-motion messages stay settled on delivery"
                );
            });
        }
    }
}

#[gpui::test]
fn sending_the_next_message_keeps_the_previous_reply_settled(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                vec![
                    row("user", "First message"),
                    row("assistant", "The previous answer"),
                ],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            let answer: Vec<_> = view
                .rows
                .iter()
                .filter(|r| matches!(r.body, Body::Markdown(_)))
                .map(|r| r.key)
                .collect();
            assert!(!answer.is_empty());
            view.streaming(true, cx);
            assert!(
                answer.iter().all(|key| !view.motions.contains_key(key)),
                "starting a request must not animate the previous answer out"
            );
            view.patch(
                2,
                vec![row("user", "Next message"), row("assistant", "")],
                cx,
            );
            assert!(
                answer.iter().all(|key| !view.motions.contains_key(key)),
                "delivering the new prompt must not replay the previous answer's entrance"
            );
            assert!(!view.entrances.contains_key(&1));
        })
        .unwrap();
}

#[gpui::test]
fn mono_name_badge_stays_visible_after_the_live_ticker_finishes(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.streaming(true, cx);
            view.patch(0, vec![row("user", "Hello"), row("assistant", "")], cx);
            view.entrances.clear();
        })
        .unwrap();
    render(window, cx);
    assert!(
        gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mono-signature-pill")
            .is_some()
    );
    window
        .update(cx, |view, _, cx| {
            view.patch(1, vec![row("assistant", "Here is the reply")], cx);
            view.streaming(false, cx);
            assert!(
                view.entrances.contains_key(&1),
                "the response itself still enters"
            );
        })
        .unwrap();
    render(window, cx);
    assert!(
        gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mono-signature-pill")
            .is_some()
    );
    assert!(
        gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mono-turn-ticker")
            .is_none()
    );
}

#[gpui::test]
fn mono_badge_ticker_tracks_work_at_the_top_of_the_turn(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                vec![
                    row("user", "First message"),
                    row("assistant", "Already answered"),
                ],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| view.streaming(true, cx))
        .unwrap();
    render(window, cx);
    {
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("mono-turn-ticker").is_none(),
            "the completed turn shows no status while the new prompt is being delivered"
        );
    }
    window
        .update(cx, |view, _, cx| {
            view.patch(
                2,
                vec![row("user", "Next message"), row("assistant", "")],
                cx,
            );
            view.entrances.clear();
        })
        .unwrap();
    render(window, cx);
    {
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        assert!(
            visual.debug_bounds("mono-turn-ticker").is_some(),
            "Thinking appears as soon as the live prompt arrives"
        );
    }
    for (role, text, label) in [
        ("assistant", "", "Thinking…"),
        ("reasoning", "Let me consider this.", "Thinking…"),
        ("tool", "{}", "Thinking…"),
        ("assistant", "   ", "Thinking…"),
        ("assistant", "I'll check the files.", "Typing…"),
    ] {
        window
            .update(cx, |view, _, cx| {
                view.patch(3, vec![row(role, text)], cx);
                assert_eq!(view.mono_activity(), Some((3, label.into())));
            })
            .unwrap();
        render(window, cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("mono-turn-ticker").is_some());
        assert!(visual.debug_bounds("mono-signature-pill").is_some());
        assert!(visual.debug_bounds("mono-status").is_none());
    }
    // The ticker stays beside the badge above the opening reply.
    for (name, input, label) in [
        (
            "exec_command",
            r#"{"cmd":"cargo check"}"#,
            "Running command…",
        ),
        ("Read", r#"{"file_path":"notes.md"}"#, "Reading file…"),
        ("Write", r#"{"file_path":"notes.md"}"#, "Editing file…"),
        ("Grep", r#"{"pattern":"Mono"}"#, "Exploring project…"),
    ] {
        window
            .update(cx, |view, _, cx| {
                let tool = ToolCall {
                    id: "work".into(),
                    name: name.into(),
                    input: input.into(),
                    output: String::new(),
                    state: ToolState::Running,
                    panel: None,
                    display: None,
                };
                view.patch(
                    4,
                    vec![row("tool", &serde_json::to_string(&tool).unwrap())],
                    cx,
                );
                view.entrances.clear();
                assert_eq!(view.mono_activity(), Some((3, label.into())));
            })
            .unwrap();
        render(window, cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let ticker = visual.debug_bounds("mono-turn-ticker").unwrap();
        let opening = visual.debug_bounds("mono-prose-3-16").unwrap();
        assert!(
            ticker.bottom() <= opening.top(),
            "activity stays in the header above the reply"
        );
    }
    window
        .update(cx, |view, _, cx| {
            view.patch(5, vec![row("assistant", "Done.")], cx);
            assert_eq!(view.mono_activity(), Some((3, "Typing…".into())));
            view.streaming(false, cx);
            assert_eq!(view.mono_activity(), None);
            assert!(
                view.rows
                    .iter()
                    .any(|row| row.key.0 == 5 && matches!(row.body, Body::Markdown(_)))
            );
        })
        .unwrap();
    render(window, cx);
    assert!(
        gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mono-status")
            .is_none()
    );
    assert!(
        gpui::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("mono-turn-ticker")
            .is_none()
    );
}

#[gpui::test]
fn mono_reply_dividers_separate_messages_and_let_cards_do_the_job(cx: &mut TestAppContext) {
    for mono in [false, true] {
        for card in [None, Some("document"), Some("session"), Some("choices")] {
            let view = cx.new(|_| {
                let mut view = TranscriptView::new();
                if mono {
                    view.mono_mode("cat", 0xe86b5f, "MonoCat");
                }
                view
            });
            view.update(cx, |view, cx| {
                let mut source = vec![
                    row("user", "Check the result"),
                    row(
                        "assistant",
                        "The first reply.\n\nAnother paragraph in the same reply.",
                    ),
                ];
                if let Some(card) = card {
                    source.push(match card {
                        "document" => {
                            row("mono-document", r#"{"documentId":"doc","title":"Report"}"#)
                        }
                        "session" => row("mono-launch", r#"{"sessionId":7}"#),
                        _ => row(
                            "mono-card",
                            r#"{"type":"choices","title":"Next steps","options":[]}"#,
                        ),
                    });
                }
                source.extend([
                    row("mono-system", "The session finished"),
                    row("assistant", "The follow-up reply.\n\nIts second paragraph."),
                ]);
                for _ in 0..2 {
                    view.patch(0, source.clone(), cx);
                    let dividers = view
                        .rows
                        .iter()
                        .filter(|row| matches!(row.body, Body::MonoDivider))
                        .count();
                    assert_eq!(
                        dividers,
                        usize::from(mono && card.is_none()),
                        "Mono={mono}, ending={card:?}"
                    );
                }
            });
        }
    }
}

#[gpui::test]
fn mono_reply_divider_waits_for_the_reply_and_disappears_when_a_card_arrives(
    cx: &mut TestAppContext,
) {
    let view = cx.new(|_| {
        let mut view = TranscriptView::new();
        view.mono_mode("cat", 0xe86b5f, "MonoCat");
        view
    });
    view.update(cx, |view, cx| {
        let count = |view: &TranscriptView| {
            view.rows
                .iter()
                .filter(|row| matches!(row.body, Body::MonoDivider))
                .count()
        };
        view.patch(
            0,
            vec![
                row("user", "Start a session"),
                row("assistant", "Waiting for it."),
            ],
            cx,
        );
        view.streaming(true, cx);
        view.patch(
            2,
            vec![
                row("mono-system", "Session finished"),
                row("assistant", "The result is ready."),
            ],
            cx,
        );
        assert_eq!(
            count(view),
            0,
            "the live notification has no visible reply yet"
        );
        view.streaming(false, cx);
        assert_eq!(count(view), 1);
        view.patch(
            2,
            vec![
                row("mono-document", r#"{"documentId":"doc","title":"Report"}"#),
                row("mono-system", "Session finished"),
                row("assistant", "The result is ready."),
            ],
            cx,
        );
        assert_eq!(
            count(view),
            0,
            "an arriving card replaces the divider without an exit ghost"
        );
        view.patch(
            view.source.len(),
            vec![
                row("user", "Another question"),
                row("assistant", "Another reply."),
            ],
            cx,
        );
        assert_eq!(
            count(view),
            0,
            "a user message already separates distinct turns"
        );
    });
}

#[gpui::test]
fn mono_reply_divider_paints_a_short_dithered_zigzag(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                vec![
                    row("user", "Check it"),
                    row("assistant", "The opening reply.\n\nStill the same reply."),
                    row("tool", "{}"),
                    row("assistant", "The finished reply."),
                ],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    let divider = visual.debug_bounds("mono-response-divider").unwrap();
    let opening = visual.debug_bounds("mono-prose-1-17").unwrap();
    let reply = visual.debug_bounds("mono-prose-3-16").unwrap();
    assert!(divider.top() >= opening.bottom());
    assert!(divider.bottom() <= reply.top());
    window
        .update(visual, |_, window, _| {
            let scale = window.scale_factor();
            let ink: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    let bounds = quad.bounds / scale;
                    bounds.size.height.0 == 3.
                        && bounds.top().0 >= divider.top().as_f32()
                        && bounds.bottom().0 <= divider.bottom().as_f32()
                        && bounds.left().0 >= divider.left().as_f32()
                        && bounds.right().0 <= divider.left().as_f32() + 144.
                })
                .collect();
            assert!(
                ink.len() > 20,
                "the zigzag is drawn in separate chart-sized dither cells"
            );
            let left = ink
                .iter()
                .map(|quad| quad.bounds.left().0 / scale)
                .fold(f32::INFINITY, f32::min);
            let right = ink
                .iter()
                .map(|quad| quad.bounds.right().0 / scale)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((right - left - 144.).abs() <= 3.);
            assert!(
                ink.iter()
                    .all(|quad| quad.bounds.size.width.0 / scale <= 18.),
                "the fill keeps gaps rather than becoming a solid rule"
            );
        })
        .unwrap();
}

#[gpui::test]
fn mono_notifications_show_only_the_final_message_live_and_after_reload(cx: &mut TestAppContext) {
    let visible = |view: &TranscriptView, at: usize| {
        view.rows
            .iter()
            .any(|row| row.key.0 == at && matches!(row.body, Body::Markdown(_)))
    };
    for history in [false, true] {
        for tools in [false, true] {
            let view = cx.new(|_| {
                let mut view = TranscriptView::new();
                view.mono_mode("cat", 0xe86b5f, "MonoCat");
                view
            });
            let (source, opening, progress, answer, report) = view.update(cx, |view, cx| {
                let previous = if history {
                    vec![
                        row("user", "Review the heartbeat"),
                        row("assistant", "Waiting for the review."),
                    ]
                } else {
                    Vec::new()
                };
                view.patch(0, previous, cx);
                view.streaming(true, cx);
                let prompt = view.source.len();
                let opening = prompt + 1;
                view.patch(
                    prompt,
                    vec![
                        row("mono-system", "The review session finished"),
                        row(
                            "assistant",
                            "I'll verify the findings before summarizing them.",
                        ),
                    ],
                    cx,
                );
                let progress = view.source.len();
                view.patch(
                    progress,
                    vec![row("assistant", "Verifying the findings.")],
                    cx,
                );
                if tools {
                    view.patch(view.source.len(), vec![row("tool", "{}")], cx);
                }
                assert!(view.mono_prose_hidden(opening));
                assert!(!visible(view, opening));
                assert!(!visible(view, progress));
                if history {
                    assert!(visible(view, 1), "the earlier direct reply stays visible");
                }

                let answer = view.source.len();
                view.patch(
                    answer,
                    vec![
                        row("assistant", "The review found three heartbeat bugs.\n\n"),
                        Row {
                            first: false,
                            ..row("assistant", "The full report is in Documents.")
                        },
                    ],
                    cx,
                );
                assert!(
                    !visible(view, answer),
                    "wait for the final message to finish"
                );
                view.streaming(false, cx);
                assert!(!visible(view, opening));
                assert!(!visible(view, progress));
                assert!(visible(view, answer));
                let activity = view.response_rows(view.mono_group_start(opening)).0;
                assert!(
                    activity
                        .iter()
                        .any(|row| row.text.starts_with("I'll verify")),
                    "notification commentary remains in activity history"
                );
                let report = view.source.len() + 1;
                view.patch(
                    report - 1,
                    vec![
                        row("mono-system", "[Scheduled Mono report]"),
                        row("assistant", "The daily check is complete."),
                    ],
                    cx,
                );
                assert!(
                    visible(view, answer),
                    "a later report preserves the earlier final reply"
                );
                assert!(visible(view, report));
                (view.source.clone(), opening, progress, answer, report)
            });

            let reloaded = cx.new(|_| {
                let mut view = TranscriptView::new();
                view.mono_mode("cat", 0xe86b5f, "MonoCat");
                view
            });
            reloaded.update(cx, |view, cx| {
                view.patch(0, source, cx);
                assert!(!visible(view, opening));
                assert!(!visible(view, progress));
                assert!(visible(view, answer), "the saved final reply stays visible");
                assert!(visible(view, report));
                if history {
                    assert!(visible(view, 1));
                }
            });
        }
    }
}

#[gpui::test]
fn a_notification_continuing_a_turn_does_not_move_the_chat(cx: &mut TestAppContext) {
    let window = chat(cx);
    let mut tool = ToolCall::new("launch", "exec_command");
    tool.input = serde_json::json!({
        "cmd": "'/Applications/Monocode Native.app/Contents/MacOS/monocode-gpui' app sessions.start --json '{}'"
    }).to_string();
    tool.output =
        serde_json::json!({"ok": true, "result": {"sessionId": 776, "accepted": true}}).to_string();
    tool.state = ToolState::Completed;
    window
        .update(cx, |view, _, cx| {
            view.set_launches(
                HashMap::from([(
                    776,
                    LaunchView {
                        title: "Luna test".into(),
                        state: "done",
                        harness: "codex".into(),
                        cwd: "/repo".into(),
                    },
                )]),
                cx,
            );
            view.patch(
                0,
                vec![
                    row("user", "Wait for Luna"),
                    row("tool", &serde_json::to_string(&tool).unwrap()),
                    row("assistant", "Waiting for Luna's reply."),
                ],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    let before = gpui::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("mono-launch-card-776")
        .expect("a native launch receipt renders its session card");
    window
        .update(cx, |view, _, cx| {
            assert_eq!(view.follow.offset, 0.);
            view.streaming(true, cx);
        })
        .unwrap();
    render(window, cx);
    let waiting = gpui::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("mono-launch-card-776")
        .expect("the card stays visible before the notification prompt is stored");
    assert!((waiting.top() - before.top()).abs() < px(0.25));
    window
        .update(cx, |view, _, cx| {
            view.patch(
                3,
                vec![row("mono-system", "Luna replied"), row("assistant", "")],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    let notified = gpui::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("mono-launch-card-776")
        .expect("the session card remains above the notification's reply");
    assert!((notified.top() - before.top()).abs() < px(0.25));
    window.update(cx, |view, _, _| {
        assert!(view.follow.offset.abs() < 0.25,
            "a hidden notification and replacement footer have no new height, but introduced {}px of movement", view.follow.offset);
    }).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.patch(
                4,
                vec![
                    row("assistant", "I'll check the result."),
                    row("tool", "{}"),
                    row("assistant", "Luna is ready for your test."),
                ],
                cx,
            );
            view.streaming(false, cx);
            let launch = view
                .rows
                .iter()
                .position(|row| matches!(row.body, Body::Launch(776)))
                .unwrap();
            let reply = view
                .rows
                .iter()
                .position(|row| row.key.0 == 6 && matches!(row.body, Body::Markdown(_)))
                .unwrap();
            assert!(
                launch < reply,
                "the completion reply follows the original launch card"
            );
            assert_eq!(
                view.rows
                    .iter()
                    .filter(|row| matches!(row.body, Body::Launch(776)))
                    .count(),
                1
            );
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|row| row.key.0 == 4 && matches!(row.body, Body::Markdown(_)))
            );
        })
        .unwrap();
    // Measure the final spacing with the reply entrance settled.
    cx.update(|cx| cx.set_reduce_motion(true));
    render(window, cx);
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    let card = visual.debug_bounds("mono-launch-card-776").unwrap();
    let opening = visual.debug_bounds("mono-prose-2-16").unwrap();
    let answer = visual.debug_bounds("mono-prose-6-16").unwrap();
    let above = card.top() - opening.bottom();
    let below = answer.top() + px(12.) - card.bottom();
    assert_eq!(above, px(12.));
    assert_eq!(below, above, "the card sits equally far from both replies");
}

#[gpui::test]
fn queued_messages_contribute_their_height_once(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.streaming(true, cx);
            view.patch(
                0,
                vec![row("user", "Start writing"), row("assistant", "")],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.follow.reset();
            view.set_pending(
                vec![PendingMessage {
                    id: "q".into(),
                    text: "Write the document".into(),
                    error: None,
                    sent_at: None,
                    attachments: Vec::new(),
                }],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    let height = f32::from(
        visual
            .debug_bounds("mono-pending-message")
            .unwrap()
            .size
            .height,
    );
    window
        .update(visual, |view, _, _| {
            assert!(
                view.follow.offset <= height + 0.25,
                "one queued bubble is {height}px high but introduced {}px of movement",
                view.follow.offset
            );
        })
        .unwrap();
}

#[gpui::test]
fn changing_hidden_work_status_does_not_move_settled_messages(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(0, vec![row("user", "Write a document")], cx);
            view.streaming(true, cx);
            view.patch(1, vec![row("assistant", "")], cx);
        })
        .unwrap();
    render(window, cx);
    window.update(cx, |view, _, _| view.follow.reset()).unwrap();
    for text in [
        r#"{"id":"write","name":"Write","input":"{\"file_path\":\"notes.md\"}","output":"","state":"running"}"#,
        r#"{"id":"write","name":"Write","input":"{\"file_path\":\"notes.md\"}","output":"Saved the document","state":"completed"}"#,
    ] {
        window
            .update(cx, |view, _, cx| view.patch(2, vec![row("tool", text)], cx))
            .unwrap();
        render(window, cx);
        window
            .update(cx, |view, _, _| {
                assert!(
                    view.follow.offset.abs() < 0.25,
                    "a ticker-only change moved the chat {}px",
                    view.follow.offset
                );
            })
            .unwrap();
    }
}

#[test]
fn measurements_follow_surviving_rows_across_footer_changes_and_window_boundaries() {
    let measure = |rows: &[(usize, f32)]| FollowMeasure {
        viewport: size(px(900.), px(600.)),
        bottom: px(600.),
        window_size: size(px(900.), px(600.)),
        footer: 0.,
        slack: 0.,
        rows: rows
            .iter()
            .map(|(key, height)| ((*key, 0), *height))
            .collect(),
    };
    let old = measure(&[(0, 60.), (1, 28.), (2, 28.)]);
    assert_eq!(
        measure(&[(0, 60.), (1, 28.), (4, 28.)]).shift_from(&old),
        0.
    );
    assert_eq!(
        measure(&[(0, 60.), (5, 60.), (1, 28.), (2, 28.)]).shift_from(&old),
        60.
    );
    assert_eq!(
        measure(&[(1, 28.), (2, 28.), (3, 40.)]).shift_from(&old),
        40.
    );
    let mut resized = measure(&[(0, 80.), (1, 28.), (2, 28.)]);
    resized.viewport.width = px(800.);
    assert_eq!(resized.shift_from(&old), 0.);
    let mut taller = measure(&[(0, 80.), (1, 28.), (2, 28.)]);
    taller.viewport.height += px(36.);
    taller.bottom += px(36.);
    assert_eq!(taller.shift_from(&old), -16.);
    taller.window_size.height += px(36.);
    assert_eq!(taller.shift_from(&old), 0.);
    let mut shorter = measure(&[(0, 60.), (1, 28.), (2, 28.)]);
    shorter.viewport.height -= px(18.);
    shorter.bottom -= px(18.);
    assert_eq!(shorter.shift_from(&old), 18.);
    assert_eq!(
        measure(&[(6, 20.)]).shift_from(&old),
        0.,
        "disconnected history starts a fresh baseline"
    );
}

#[test]
fn the_follow_spring_never_overshoots_and_keeps_momentum_across_brief_pauses() {
    let mut follow = Follow::default();
    let mut previous = 940.;
    let mut offset = follow.step(previous, previous, 1., 1., CHAT_FOLLOW_MAX_SPEED);
    for _ in 0..240 {
        assert!(offset >= 0. && offset <= previous);
        assert!(previous - offset <= CHAT_FOLLOW_MAX_SPEED + 0.001);
        previous = offset;
        offset = follow.step(offset, 0., 1., 1., CHAT_FOLLOW_MAX_SPEED);
    }
    assert!(offset < 0.25);
    let mut follow = Follow {
        velocity: 4.,
        growth: 2.,
        offset: 0.1,
        ..Default::default()
    };
    follow.settle(Instant::now());
    assert_eq!(follow.offset, 0.);
    assert_eq!((follow.velocity, follow.growth), (4., 2.));
}

fn bubble_top(window: &mut Window) -> f32 {
    window
        .painted_quads()
        .into_iter()
        .filter(|q| {
            q.corner_radii.top_left.0 > 0.
                && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
        })
        .map(|q| q.bounds.top().0 / window.scale_factor())
        .next()
        .expect("the sent bubble is painted")
}

#[gpui::test]
fn a_document_and_final_reply_preserve_position_then_glide_up_without_reversing(
    cx: &mut TestAppContext,
) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(0, vec![row("user", "Write a document")], cx);
            view.streaming(true, cx);
            view.patch(1, vec![row("assistant", "")], cx);
        })
        .unwrap();
    render(window, cx);
    let before = window
        .update(cx, |view, window, _| {
            view.follow.reset();
            view.entrances.clear();
            bubble_top(window)
        })
        .unwrap();
    window
        .update(cx, |view, _, cx| {
            view.patch(
                2,
                vec![row(
                    "mono-card",
                    r#"{"id":"doc","type":"document","documentId":"d","title":"The document"}"#,
                )],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    let entering = window
        .update(cx, |view, window, _| {
            assert!(view.follow.offset > 0.);
            bubble_top(window)
        })
        .unwrap();
    assert!(
        (before - entering).abs() < 3.,
        "adding the card starts at the current painted position: {before} -> {entering}"
    );
    window
        .update(cx, |view, _, cx| {
            let reply = "Here is another paragraph about the finished document.\n\n".repeat(100);
            view.patch(3, vec![row("assistant", &reply)], cx);
            view.streaming(false, cx);
            view.entrances.clear();
            view.follow.last_frame = None;
        })
        .unwrap();
    render(window, cx);
    let mut previous = window
        .update(cx, |view, _, _| {
            assert!(
                view.follow.offset > 2000.,
                "keep the full reveal distance even beyond the 80-row measurement window"
            );
            view.follow.offset
        })
        .unwrap();
    for _ in 0..20 {
        window
            .update(cx, |view, _, _| {
                view.follow.last_frame =
                    Some(Instant::now() - std::time::Duration::from_millis(16));
            })
            .unwrap();
        render(window, cx);
        let next = window.update(cx, |view, _, _| view.follow.offset).unwrap();
        assert!(
            next >= 0. && next <= previous,
            "reveal reversed direction: {previous} -> {next}"
        );
        previous = next;
    }
}

#[gpui::test]
fn a_large_reveal_keeps_settling_after_the_reply_grace_period(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                vec![row("user", "Earlier"), row("assistant", "Done")],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.follow.offset = 940.;
            view.follow.until = Some(Instant::now() - std::time::Duration::from_secs(2));
            view.follow.last_frame = Some(Instant::now() - std::time::Duration::from_millis(16));
            cx.notify();
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| {
            assert!(
                view.follow.offset > 600. && view.follow.offset < 940.,
                "an existing glide should keep easing, not snap to the end: {}",
                view.follow.offset
            );
        })
        .unwrap();
}

#[gpui::test]
fn scrolling_away_hands_over_the_visible_position_once(cx: &mut TestAppContext) {
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                0,
                (0..40)
                    .map(|ix| {
                        row(
                            if ix % 2 == 0 { "user" } else { "assistant" },
                            "A message in the history",
                        )
                    })
                    .collect(),
                cx,
            );
            view.list = view.list.clone().measure_all();
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, window, cx| {
            view.list.pause_following_tail();
            view.list.scroll_to(ListOffset {
                item_ix: 35,
                offset_in_item: px(10.),
            });
            let before = view.list.scroll_px_offset_for_scrollbar().y;
            view.follow.offset = 40.;
            view.update_follow(window, cx);
            let handed_over = view.list.scroll_px_offset_for_scrollbar().y;
            assert_eq!(handed_over, before + px(40.));
            assert_eq!(view.follow.offset, 0.);
            assert!(!view.list.is_following_tail());
            view.update_follow(window, cx);
            assert_eq!(view.list.scroll_px_offset_for_scrollbar().y, handed_over);
        })
        .unwrap();
}

#[gpui::test]
fn closing_work_above_the_answer_moves_it_one_way(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            let mut source = vec![
                row("user", "Prompt"),
                row("assistant", &"Some text.\n\n".repeat(20)),
            ];
            source.extend((0..4).map(|ix| tool(ix, true)));
            source.push(row("assistant", "Still checking."));
            source.extend((4..8).map(|ix| tool(ix, true)));
            source.push(row("assistant", "Here is the answer."));
            view.patch(0, source, cx);
            view.work_folds.insert(1, true);
            view.rebuild(0, cx);
            view.streaming(true, cx);
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    render(window, cx);
    let answer = window
        .update(cx, |view, _, cx| {
            assert!(view.list.is_following_tail());
            // The work line's click handler.
            view.work_folds.insert(1, false);
            view.list.pause_following_tail();
            view.rebuild(1, cx);
            view.rows
                .iter()
                .rev()
                .find(|r| matches!(r.body, Body::Markdown(_)))
                .unwrap()
                .key
        })
        .unwrap();
    // The header stays under the pointer while the answer rises into the
    // space. Following again at the end must not ease that distance a second
    // time, overshooting and settling back down.
    let mut previous: Option<f32> = None;
    for _ in 0..24 {
        render(window, cx);
        let top = window
            .update(cx, |view, _, _| {
                let ix = view.rows.iter().position(|r| r.key == answer).unwrap();
                f32::from(view.list.bounds_for_item(ix).unwrap().top()) + view.follow.offset
            })
            .unwrap();
        if let Some(previous) = previous {
            assert!(
                top <= previous + 0.5,
                "the answer moved back down: {previous} -> {top}"
            );
        }
        previous = Some(top);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn tool(ix: usize, done: bool) -> Row {
    row(
        "tool",
        &serde_json::to_string(&ToolCall {
            id: ix.to_string(),
            name: "Bash".into(),
            input: format!(r#"{{"command":"rg pattern-{ix} src"}}"#),
            output: if done {
                "match\n".repeat(6)
            } else {
                String::new()
            },
            state: if done {
                ToolState::Completed
            } else {
                ToolState::Running
            },
            panel: None,
            display: None,
        })
        .unwrap(),
    )
}

#[gpui::test]
fn the_line_being_read_never_moves_back_down_as_work_folds(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.patch(
                0,
                vec![
                    row("user", "Earlier"),
                    row("assistant", &"Earlier answer paragraph.\n\n".repeat(30)),
                    row("user", "Fix the jitter"),
                ],
                cx,
            );
            view.streaming(true, cx);
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    let prose = |s: &str| row("assistant", s);
    let mut script: Vec<(u64, usize, Vec<Row>)> = Vec::new();
    // A turn streams prose, a group of tools, more prose and another group.
    // Each step is (delay in ms, source index, rows).
    let intro = "Looking at how the transcript scrolls when rows toggle and what React does there.";
    let words: Vec<&str> = intro.split(' ').collect();
    for n in (3..=words.len()).step_by(3) {
        script.push((60, 3, vec![prose(&words[..n].join(" "))]));
    }
    let mut at = 4;
    for ix in 0..3 {
        script.push((150, at, vec![tool(ix, false)]));
        script.push((200, at, vec![tool(ix, true)]));
        at += 1;
    }
    let mid = "Found the follow logic. Now checking how the list anchors while folds animate and whether the spring overshoots.";
    let words: Vec<&str> = mid.split(' ').collect();
    for n in (3..=words.len()).step_by(3) {
        script.push((60, at, vec![prose(&words[..n].join(" "))]));
    }
    at += 1;
    for ix in 3..6 {
        script.push((150, at, vec![tool(ix, false)]));
        script.push((200, at, vec![tool(ix, true)]));
        at += 1;
    }
    let fin = "Here is the final answer with a few sentences of explanation. ".repeat(4);
    let words: Vec<&str> = fin.split(' ').collect();
    for n in (4..=words.len()).step_by(4) {
        script.push((50, at, vec![prose(&words[..n].join(" "))]));
    }
    let mut t = 0u64;
    let mut events = script.into_iter().peekable();
    let mut next_at = 0u64;
    let mut prev: Option<((usize, usize), f32)> = None;
    while t < 9000 {
        if let Some((delay, _, _)) = events.peek() {
            if t >= next_at + delay {
                let (delay, start, rows) = events.next().unwrap();
                next_at += delay;
                window
                    .update(cx, |view, _, cx| view.patch(start, rows, cx))
                    .unwrap();
            }
        } else if t > next_at + 1500 {
            break;
        }
        render(window, cx);
        window
            .update(cx, |view, _, _| {
                // Each paragraph after a group folds the work above it while
                // the chat stays pinned to the end. Growth glides the newest
                // prose up; the fold must not drop it back down.
                let (ix, row) = view
                    .rows
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, r)| matches!(r.body, Body::Markdown(_) | Body::Plain(_)))
                    .unwrap();
                let top =
                    f32::from(view.list.bounds_for_item(ix).unwrap().top()) + view.follow.offset;
                if let Some((key, previous)) = prev
                    && key == row.key
                {
                    assert!(
                        top <= previous + 1.5,
                        "{key:?} moved back down at {t}ms: {previous} -> {top}"
                    );
                }
                prev = Some((row.key, top));
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
        t += 16;
    }
}

#[gpui::test]
fn long_user_messages_clamp_to_four_lines_until_shown_in_full(cx: &mut TestAppContext) {
    let lines = |n: usize| {
        (1..=n)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            // Use the storage writer's real line/byte chunking for the long
            // prompt; a single hand-built Row cannot exercise chunk expansion.
            let mut source = monocode_core::transcript::Transcript::default();
            for (role, text) in [
                ("user", "Short".to_owned()),
                ("assistant", "Ok.".to_owned()),
                ("user", lines(8)),
                ("assistant", "Ok.".to_owned()),
                ("user", lines(30)),
                ("assistant", "Ok.".to_owned()),
            ] {
                source.begin(role);
                source.append(&text);
            }
            view.patch(0, source.rows, cx);
            view.visible = true;
            view.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
        })
        .unwrap();
    render(window, cx);
    let height = |view: &TranscriptView, start: usize| -> (usize, f32) {
        let rows: Vec<_> = (0..view.rows.len())
            .filter(|&ix| {
                view.rows[ix].key.0 == start && matches!(view.rows[ix].body, Body::Plain(_))
            })
            .collect();
        let height = rows
            .iter()
            .map(|&ix| {
                f32::from(
                    view.list
                        .bounds_for_item(ix)
                        .map_or(px(0.), |b| b.size.height),
                )
            })
            .sum();
        (rows.len(), height)
    };
    let (short, clamped, chunked) = window
        .update(cx, |view, _, _| {
            (height(view, 0), height(view, 2), height(view, 4))
        })
        .unwrap();
    // Four 20px lines and the button under them; one chunk of a long message.
    assert_eq!(chunked.0, 1);
    assert_eq!(clamped.1, chunked.1);
    assert!(
        clamped.1 >= short.1 + 3. * 20. + 16.,
        "{short:?} {clamped:?}"
    );
    assert!(clamped.1 < short.1 + 5. * 20., "{short:?} {clamped:?}");
    window
        .update(cx, |view, _, cx| {
            view.toggle_prompt(2, cx);
            view.toggle_prompt(4, cx);
            assert!(
                view.motions.is_empty(),
                "Show more opens without a transition"
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| {
            let open = height(view, 2);
            assert!(open.1 >= clamped.1 + 4. * 20., "{clamped:?} {open:?}");
            assert!(height(view, 4).0 > 1);
        })
        .unwrap();
    window
        .update(cx, |view, _, cx| {
            view.toggle_prompt(4, cx);
            assert!(
                view.motions.is_empty(),
                "Show less closes without a transition"
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| assert_eq!(height(view, 4), chunked))
        .unwrap();
}

#[gpui::test]
fn an_anchored_prompt_stays_put_while_new_prose_folds_the_work_above(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.patch(
                0,
                vec![
                    row("user", "Earlier"),
                    row("assistant", &"Earlier answer paragraph.\n\n".repeat(30)),
                ],
                cx,
            );
            view.streaming(true, cx);
            view.visible = true;
            view.patch(2, vec![row("user", "Fix the jitter")], cx);
        })
        .unwrap();
    render(window, cx);
    let prose = |s: &str| row("assistant", s);
    let mut script: Vec<(u64, usize, Vec<Row>)> = Vec::new();
    let mut at = 3;
    for round in 0..3 {
        let text =
            format!("Round {round}: looking at how the transcript scrolls when rows toggle.");
        let words: Vec<String> = text.split(' ').map(String::from).collect();
        for n in (3..=words.len()).step_by(3) {
            script.push((60, at, vec![prose(&words[..n].join(" "))]));
        }
        at += 1;
        for ix in 0..3 {
            script.push((150, at, vec![tool(round * 10 + ix, false)]));
            script.push((200, at, vec![tool(round * 10 + ix, true)]));
            at += 1;
        }
    }
    let mut t = 0u64;
    let mut events = script.into_iter().peekable();
    let mut next_at = 0u64;
    let mut prev: Option<f32> = None;
    while t < 9000 {
        if let Some((delay, _, _)) = events.peek() {
            if t >= next_at + delay {
                let (delay, start, rows) = events.next().unwrap();
                next_at += delay;
                window
                    .update(cx, |view, _, cx| view.patch(start, rows, cx))
                    .unwrap();
            }
        } else if t > next_at + 1500 {
            break;
        }
        render(window, cx);
        window
            .update(cx, |view, _, _| {
                let ix = view.rows.iter().position(|r| r.key.0 == 2).unwrap();
                let top = view
                    .list
                    .bounds_for_item(ix)
                    .map(|b| f32::from(b.top()) + view.follow.offset);
                // The spacer below absorbs the fold, so the prompt never moves.
                if let (Some(top), Some(previous)) = (top, prev) {
                    assert!(
                        (top - previous).abs() <= 0.5,
                        "the prompt moved at {t}ms: {previous} -> {top}"
                    );
                }
                prev = top;
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
        t += 16;
    }
}

#[gpui::test]
fn replacing_work_near_the_viewport_limit_keeps_the_prompt_anchored(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.smooth_reveal = false;
            view.visible = false;
            view.patch(
                0,
                vec![
                    row("user", "Earlier"),
                    row("assistant", &"Earlier answer.\n\n".repeat(30)),
                ],
                cx,
            );
            view.streaming(true, cx);
            view.patch(2, vec![row("user", "Keep this prompt still")], cx);
            view.patch(
                3,
                vec![
                    row(
                        "assistant",
                        &"Checking the transcript layout.\n\n".repeat(6),
                    ),
                    tool(0, true),
                    tool(1, true),
                    tool(2, true),
                ],
                cx,
            );
            view.visible = true;
        })
        .unwrap();
    render(window, cx);
    let top = window
        .update(cx, |view, _, cx| {
            assert!(view.follow.measured.as_ref().unwrap().footer > BOTTOM_INSET);
            let ix = view.rows.iter().position(|r| r.key.0 == 2).unwrap();
            let top = f32::from(view.list.bounds_for_item(ix).unwrap().top());
            view.patch(
                7,
                vec![row(
                    "assistant",
                    &"Now checking the replacement.\n\n".repeat(6),
                )],
                cx,
            );
            top
        })
        .unwrap();
    for frame in 0..40 {
        render(window, cx);
        window
            .update(cx, |view, _, _| {
                let ix = view.rows.iter().position(|r| r.key.0 == 2).unwrap();
                let actual = view
                    .list
                    .bounds_for_item(ix)
                    .map(|b| f32::from(b.top()) + view.follow.offset);
                assert!(
                    actual.is_some_and(|actual| (actual - top).abs() < 0.5),
                    "prompt moved at frame {frame}: {top} -> {actual:?}, follow {}, footer {}",
                    view.follow.offset,
                    view.follow.measured.as_ref().unwrap().footer,
                );
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    window
        .update(cx, |view, _, cx| {
            view.patch(
                7,
                vec![row("assistant", &"A growing reply.\n\n".repeat(80))],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            assert!(view.list.is_following_tail());
            assert_eq!(view.follow.measured.as_ref().unwrap().footer, BOTTOM_INSET);
            assert!(
                view.follow.offset > 0.,
                "genuine overflow must still follow smoothly"
            );
            view.patch(8, vec![tool(3, true), tool(4, true), tool(5, true)], cx);
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, cx| {
            view.patch(
                11,
                vec![row(
                    "assistant",
                    "A short replacement after the long reply.",
                )],
                cx,
            );
        })
        .unwrap();
    render(window, cx);
    window
        .update(cx, |view, _, _| {
            let ix = view.rows.iter().position(|r| r.key.0 == 2).unwrap();
            assert!((f32::from(view.list.bounds_for_item(ix).unwrap().top()) - top).abs() < 0.5);
            assert_eq!(
                view.follow.offset, 0.,
                "a reply that fits must not inherit a scroll animation"
            );
        })
        .unwrap();
}

#[gpui::test]
fn empty_and_unrevealed_replacements_do_not_reopen_previous_work(cx: &mut TestAppContext) {
    let window = chat_mode(cx, false);
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            view.streaming(true, cx);
            view.patch(
                0,
                vec![
                    row("user", "Prompt"),
                    row("assistant", "First progress message."),
                    tool(0, true),
                    tool(1, true),
                    row("assistant", "Current progress message."),
                ],
                cx,
            );
            view.visible = true;
            assert_eq!(view.fold_targets[&1], Some(4));
            view.patch(5, vec![row("assistant", "")], cx);
            assert_eq!(
                view.fold_targets[&1],
                Some(4),
                "an empty placeholder must keep the current block"
            );
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|r| matches!(r.body, Body::Tool { .. }))
            );
            view.patch(5, vec![row("assistant", "The next progress message.")], cx);
            assert_eq!(
                view.fold_targets[&1],
                Some(4),
                "paced text must be visible before it replaces the current block"
            );
            view.reveals.get_mut(&5).unwrap().settle(Instant::now());
            view.rebuild(5, cx);
            assert_eq!(view.fold_targets[&1], Some(5));
            assert!(
                !view.rows.iter().any(|r| r.key == (4, 16)),
                "the old prose must leave layout when its replacement appears"
            );
        })
        .unwrap();
}

#[gpui::test]
fn a_mono_opening_sentence_already_shown_does_not_enter_again_when_the_run_ends(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| cx.set_reduce_motion(false));
    let window = chat(cx);
    window
        .update(cx, |view, _, cx| {
            view.smooth_reveal = false;
            view.streaming(true, cx);
            view.patch(
                0,
                vec![
                    row("user", "check the changes"),
                    row("assistant", "I'll start a review."),
                    row("tool", "{}"),
                ],
                cx,
            );
            assert!(
                !view.mono_prose_hidden(1),
                "the opening sentence is on screen"
            );
            view.patch(3, vec![row("assistant", "Started it.")], cx);
            view.entrances.clear();
            view.streaming(false, cx);
            assert!(
                !view.entrances.contains_key(&1),
                "the opening sentence must not flash in again"
            );
            assert!(
                view.entrances.contains_key(&3),
                "the reply held back while working still appears"
            );
        })
        .unwrap();
}
