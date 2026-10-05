//! The transcript view in a headless window with the platform text system:
//! virtualization, streaming into a reused markdown view, folding work on a
//! click, opening a subagent, and the events the view reports.

#![cfg(target_os = "macos")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyWindowHandle, AppContext as _, Context, Entity, HeadlessAppContext, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, PlatformInput, Point, Render,
    Styled as _, Window, WindowHandle, div, point, px, size,
};
use monocode_core::block::AgentStepKind;
use monocode_core::transcript::fixtures::*;
use monocode_core::{Block, HarnessId, Session};
use monocode_view_transcript::transcript::model::plan::RowKind;
use monocode_view_transcript::transcript::{TranscriptConfig, TranscriptEvent, TranscriptView};

struct Host {
    transcript: Entity<TranscriptView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.transcript.clone())
    }
}

/// AppKit is not safe to set up from several test threads at once.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn app() -> HeadlessAppContext {
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(platform.text_system(), Arc::new(()), || None);
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        monocode_view_transcript::transcript::init(cx);
    });
    cx
}

type Events = Rc<RefCell<Vec<TranscriptEvent>>>;

fn open(
    cx: &mut HeadlessAppContext,
    session: Session,
) -> (WindowHandle<Host>, Entity<TranscriptView>, Events) {
    let events: Events = Rc::default();
    let sink = events.clone();
    let session = Arc::new(session);
    let window = cx
        .open_window(size(px(800.), px(700.)), move |_, cx| {
            let transcript = cx.new(|cx| {
                let mut view = TranscriptView::new(cx);
                view.set_config(TranscriptConfig::default(), cx);
                view.set_session(session, cx);
                view
            });
            cx.subscribe(&transcript, move |_, event: &TranscriptEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
            .detach();
            cx.new(|_| Host { transcript })
        })
        .expect("open window");
    let transcript = cx
        .read_window(&window, |host, cx| host.read(cx).transcript.clone())
        .expect("host");
    draw(cx, window);
    (window, transcript, events)
}

fn draw(cx: &mut HeadlessAppContext, window: impl Into<AnyWindowHandle>) {
    let window = window.into();
    for _ in 0..2 {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    }
}

fn click(cx: &mut HeadlessAppContext, window: WindowHandle<Host>, position: Point<gpui::Pixels>) {
    for event in [
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
    ] {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(event, cx);
        })
        .expect("dispatch");
    }
    draw(cx, window);
}

fn session(blocks: Vec<Block>) -> Session {
    let mut session = Session::blank("s", HarnessId::Claude, "claude:opus-4.6", "/repo");
    session.blocks = blocks;
    session
}

/// A settled session of `turns` turns, each a prompt, a tool call, and a reply.
fn long_session(turns: usize) -> Session {
    let mut blocks = Vec::new();
    for turn in 0..turns {
        blocks.push(timed_user(
            &format!("u{turn}"),
            &format!("Question {turn}"),
            1_000,
            2_000,
        ));
        blocks.push(shell(&format!("t{turn}")));
        blocks.push(note(
            &format!("a{turn}"),
            &format!("Answer {turn} with **some** markdown."),
        ));
    }
    session(blocks)
}

#[test]
fn lays_out_only_the_rows_on_screen() {
    let _guard = serial();
    let mut cx = app();
    let (_window, transcript, _) = open(&mut cx, long_session(2_000));
    cx.update(|cx| {
        let view = transcript.read(cx);
        // Prompt, fold line, answer, and action row for each turn.
        assert_eq!(view.rows().len(), 8_000);
        let drawn = view.markdown_view_count();
        assert!(drawn > 0, "the answers on screen have markdown views");
        assert!(drawn < 40, "only visible answers were built, got {drawn}");
        // Pinned to the bottom: the last answer is drawn, the first is not.
        assert!(view.markdown_for("a1999").is_some());
        assert!(view.markdown_for("a0").is_none());
        assert!(!view.is_scrolled_away());
    });
}

#[test]
fn streams_into_the_same_markdown_view() {
    let _guard = serial();
    let mut cx = app();
    let mut streaming = note("a", "Partial");
    streaming.streaming = Some(true);
    let mut live = session(vec![user("u", "Go"), shell("t"), streaming]);
    live.busy = Some(true);
    let (window, transcript, _) = open(&mut cx, live.clone());
    let first = cx
        .update(|cx| transcript.read(cx).markdown_for("a"))
        .expect("drawn");
    let rows_before = cx.update(|cx| transcript.read(cx).rows().to_vec());

    live.blocks[2].text.push_str(" answer, now longer.");
    cx.update(|cx| transcript.update(cx, |view, cx| view.set_session(Arc::new(live.clone()), cx)));
    draw(&mut cx, window);
    let second = cx
        .update(|cx| transcript.read(cx).markdown_for("a"))
        .expect("drawn");
    assert_eq!(first.entity_id(), second.entity_id());
    cx.update(|cx| assert_eq!(second.read(cx).text(), "Partial answer, now longer."));
    // The prompt row is the same row: only the streaming item changed.
    let rows_after = cx.update(|cx| transcript.read(cx).rows().to_vec());
    assert!(rows_before[0].same_as(&rows_after[0]));
}

#[test]
fn clicking_the_fold_line_shows_and_hides_the_work() {
    let _guard = serial();
    let mut cx = app();
    let blocks = vec![
        timed_user("u", "Check it", 1_000, 2_000),
        shell("t1"),
        note("mid", "Halfway."),
        shell("t2"),
        note("done", "All set."),
    ];
    let (window, transcript, _) = open(&mut cx, session(blocks));
    let fold_row = cx.update(|cx| {
        transcript
            .read(cx)
            .rows()
            .iter()
            .position(|row| matches!(row.kind, RowKind::FoldLine(_)))
            .expect("a fold line")
    });
    let count = |cx: &mut HeadlessAppContext| cx.update(|cx| transcript.read(cx).rows().len());
    assert_eq!(count(&mut cx), 4);
    let bounds = cx
        .update(|cx| transcript.read(cx).row_bounds(fold_row))
        .expect("the fold line is on screen");
    click(
        &mut cx,
        window,
        point(bounds.left() + px(60.), bounds.top() + px(12.)),
    );
    // The fold opened: its three entries now have rows.
    assert_eq!(count(&mut cx), 7);
    click(
        &mut cx,
        window,
        point(bounds.left() + px(60.), bounds.top() + px(12.)),
    );
    assert_eq!(count(&mut cx), 4);
}

#[test]
fn a_failed_subagent_opens_on_its_reason() {
    let _guard = serial();
    let mut cx = app();
    let mut failed = agent_run(
        "ag",
        "Inspect auth",
        "failed",
        vec![step(
            "s1",
            AgentStepKind::Tool,
            "Read src/auth.ts",
            Some("completed"),
        )],
    );
    failed.tool.as_mut().unwrap().detail = Some("Child process disconnected".into());
    let mut blocks = vec![
        user("u", "Delegate"),
        failed,
        note("a", "I could not finish."),
    ];
    blocks[0].started_at = Some(1_000);
    let (_window, transcript, _) = open(&mut cx, session(blocks));
    // The view built the run's trail, so the reason is on screen.
    cx.update(|cx| {
        let view = transcript.read(cx);
        assert!(view.rows().iter().any(|row| matches!(
            &row.kind,
            RowKind::Item { item, .. } if item.blocks().iter().any(|block| block.id == "ag")
        )));
    });
}

#[test]
fn reports_jumping_away_and_back() {
    let _guard = serial();
    let mut cx = app();
    let (window, transcript, events) = open(&mut cx, long_session(50));
    cx.update(|cx| transcript.update(cx, |view, cx| view.scroll_to_top(cx)));
    draw(&mut cx, window);
    cx.update(|cx| transcript.update(cx, |view, cx| view.jump_to_bottom(cx)));
    draw(&mut cx, window);
    let events = events.borrow();
    assert!(events.contains(&TranscriptEvent::JumpToBottomChanged { show: true }));
    assert_eq!(
        events.last(),
        Some(&TranscriptEvent::JumpToBottomChanged { show: false })
    );
    cx.update(|cx| assert!(transcript.read(cx).markdown_for("a49").is_some()));
}

#[test]
fn navigating_to_a_folded_result_opens_its_fold() {
    let _guard = serial();
    let mut cx = app();
    let blocks = vec![
        timed_user("u", "Check it", 1_000, 2_000),
        read("t1", "src/needle.ts"),
        note("done", "All set."),
    ];
    let (window, transcript, _) = open(&mut cx, session(blocks));
    let found = cx.update(|cx| {
        transcript.update(cx, |view, cx| {
            view.navigate_to_block(Some("t1"), "needle", cx)
        })
    });
    draw(&mut cx, window);
    assert!(found);
    cx.update(|cx| {
        let view = transcript.read(cx);
        let current: Vec<_> = view
            .rows()
            .iter()
            .filter(|row| row.search_current)
            .collect();
        assert_eq!(current.len(), 1);
        assert_eq!(view.search_query(), "needle");
    });
    assert!(!cx.update(
        |cx| transcript.update(cx, |view, cx| view.navigate_to_block(
            Some("missing"),
            "",
            cx
        ))
    ));
}

/// A pane that can be hidden and resized, like a pooled session tab.
struct Pane {
    transcript: Entity<TranscriptView>,
    width: gpui::Pixels,
    shown: bool,
}

impl Render for Pane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(self.width)
            .h_full()
            .when(self.shown, |el| el.child(self.transcript.clone()))
    }
}

#[test]
fn remeasures_prompt_corners_when_a_pooled_tab_is_shown_at_a_new_width() {
    let _guard = serial();
    let mut cx = app();
    let session = Arc::new(session(vec![user(
        "prompt",
        "A prompt that fits one wide line",
    )]));
    let window = cx
        .open_window(size(px(800.), px(700.)), move |_, cx| {
            let transcript = cx.new(|cx| {
                let mut view = TranscriptView::new(cx);
                view.set_config(TranscriptConfig::default(), cx);
                view.set_session(session, cx);
                view
            });
            cx.new(|_| Pane {
                transcript,
                width: px(800.),
                shown: true,
            })
        })
        .expect("open window");
    let transcript = cx
        .read_window(&window, |pane, cx| pane.read(cx).transcript.clone())
        .expect("pane");
    draw(&mut cx, window);
    let single_line = |cx: &mut HeadlessAppContext| {
        cx.update(|cx| transcript.read(cx).prompt_is_single_line("prompt"))
    };
    assert_eq!(single_line(&mut cx), Some(true));

    // Hidden, then shown again in a narrower pane: the same prompt wraps,
    // and the first frame back measures it at the new width.
    let show_at = |cx: &mut HeadlessAppContext, width: f32| {
        window
            .update(cx, |pane, _, cx| {
                pane.shown = false;
                cx.notify();
            })
            .expect("hide");
        draw(cx, window);
        window
            .update(cx, |pane, _, cx| {
                pane.width = px(width);
                pane.shown = true;
                cx.notify();
            })
            .expect("show");
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
    };
    show_at(&mut cx, 300.);
    assert_eq!(single_line(&mut cx), Some(false));

    // And back to one line when the pane widens again.
    show_at(&mut cx, 800.);
    assert_eq!(single_line(&mut cx), Some(true));
}
