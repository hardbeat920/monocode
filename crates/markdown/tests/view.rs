//! View tests in a headless window with the platform text system: selection
//! across blocks, copy, links, the copy button, and streaming.

#![cfg(target_os = "macos")]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, WindowHandle, point, px,
};
use monocode_markdown::{Block, MarkdownView, RenderedText};

use common::{Host, app, draw, open};

const DOC: &str = "First paragraph with a [link](https://example.com/a) inside.\n\n- item one\n- item two\n\n```rust\nfn main() {}\n```\n\nLast line.";

fn send(cx: &mut HeadlessAppContext, window: WindowHandle<Host>, event: PlatformInput) {
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_event(event, cx);
    })
    .expect("dispatch");
    draw(cx, window);
}

fn down(position: Point<Pixels>, click_count: usize) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count,
        first_mouse: false,
    })
}

fn drag(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    })
}

fn up(position: Point<Pixels>, click_count: usize) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count,
    })
}

/// The window position of byte `offset` in a rendered element, nudged into
/// the line box.
fn at(text: &RenderedText, offset: usize) -> Point<Pixels> {
    let p = text.layout.position_for_index(offset).expect("position");
    point(p.x + px(1.), p.y + text.layout.line_height() / 2.)
}

fn rendered(
    cx: &mut HeadlessAppContext,
    markdown: &gpui::Entity<MarkdownView>,
) -> Vec<RenderedText> {
    cx.read_entity(markdown, |view, _| view.rendered_text())
}

fn setup(
    doc: &'static str,
) -> (
    HeadlessAppContext,
    WindowHandle<Host>,
    gpui::Entity<MarkdownView>,
) {
    let mut cx = app();
    let (window, markdown) = open(&mut cx, 640., 640., move |cx| {
        MarkdownView::with_text(doc, cx)
    });
    draw(&mut cx, window);
    draw(&mut cx, window);
    (cx, window, markdown)
}

#[test]
fn every_block_paints_its_text() {
    let _serial = common::serial();
    let (mut cx, _window, markdown) = setup(DOC);
    let texts: Vec<String> = rendered(&mut cx, &markdown)
        .iter()
        .map(|t| t.text.to_string())
        .collect();
    assert_eq!(
        texts,
        vec![
            "First paragraph with a link inside.",
            "item one",
            "item two",
            "fn main() {}",
            "Last line."
        ]
    );
    // Document order is top to bottom.
    let tops: Vec<_> = rendered(&mut cx, &markdown)
        .iter()
        .map(|t| t.bounds.top())
        .collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "{tops:?}");
}

#[test]
fn drag_selects_across_blocks_and_copy_writes_it() {
    let _serial = common::serial();
    let (mut cx, window, markdown) = setup(DOC);
    let texts = rendered(&mut cx, &markdown);
    let start = at(&texts[0], "First ".len());
    let end = at(&texts[3], "fn main".len());
    send(&mut cx, window, down(start, 1));
    send(&mut cx, window, drag(at(&texts[1], 3)));
    send(&mut cx, window, drag(end));
    send(&mut cx, window, up(end, 1));
    let selected = cx
        .read_entity(&markdown, |view, _| view.selected_text())
        .expect("a selection");
    assert_eq!(
        selected,
        "paragraph with a link inside.\n\nitem one\nitem two\n\nfn main"
    );

    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_action(Box::new(monocode_markdown::Copy), cx);
    })
    .unwrap();
    let clipboard = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some(selected.as_str()));
}

#[test]
fn backwards_drag_selects_the_same_text() {
    let _serial = common::serial();
    let (mut cx, window, markdown) = setup(DOC);
    let texts = rendered(&mut cx, &markdown);
    let start = at(&texts[4], "Last".len());
    let end = at(&texts[2], "item ".len());
    send(&mut cx, window, down(start, 1));
    send(&mut cx, window, drag(end));
    send(&mut cx, window, up(end, 1));
    let selected = cx.read_entity(&markdown, |view, _| view.selected_text());
    assert_eq!(selected.as_deref(), Some("two\n\nfn main() {}\n\nLast"));
}

#[test]
fn double_and_triple_clicks() {
    let _serial = common::serial();
    let (mut cx, window, markdown) = setup(DOC);
    let texts = rendered(&mut cx, &markdown);
    let word = at(&texts[0], "First para".len());
    send(&mut cx, window, down(word, 1));
    send(&mut cx, window, up(word, 1));
    send(&mut cx, window, down(word, 2));
    send(&mut cx, window, up(word, 2));
    assert_eq!(
        cx.read_entity(&markdown, |view, _| view.selected_text())
            .as_deref(),
        Some("paragraph")
    );
    send(&mut cx, window, down(word, 3));
    send(&mut cx, window, up(word, 3));
    assert_eq!(
        cx.read_entity(&markdown, |view, _| view.selected_text())
            .as_deref(),
        Some("First paragraph with a link inside.")
    );
    // A plain click clears the selection.
    send(&mut cx, window, down(word, 1));
    send(&mut cx, window, up(word, 1));
    assert_eq!(
        cx.read_entity(&markdown, |view, _| view.selected_text()),
        None
    );
}

#[test]
fn select_all_copies_the_whole_message_without_code_padding() {
    let _serial = common::serial();
    let (mut cx, window, markdown) =
        setup("Run `cargo test` now.\n\n| a | b |\n|---|---|\n| 1 | 2 |");
    let texts = rendered(&mut cx, &markdown);
    send(&mut cx, window, down(at(&texts[0], 1), 1));
    send(&mut cx, window, up(at(&texts[0], 1), 1));
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_action(Box::new(monocode_markdown::SelectAll), cx);
    })
    .unwrap();
    draw(&mut cx, window);
    assert_eq!(
        cx.read_entity(&markdown, |view, _| view.selected_text())
            .as_deref(),
        Some("Run cargo test now.\n\na\tb\n1\t2")
    );
}

#[test]
fn clicking_a_link_calls_the_handler_and_dragging_does_not() {
    let _serial = common::serial();
    let clicks = Rc::new(RefCell::new(Vec::<String>::new()));
    let mut cx = app();
    let seen = clicks.clone();
    let (window, markdown) = open(&mut cx, 640., 400., move |cx| {
        let mut view = MarkdownView::with_text(DOC, cx);
        view.on_link_click(move |link, _, _| seen.borrow_mut().push(link.url.to_string()));
        view
    });
    draw(&mut cx, window);
    draw(&mut cx, window);
    let texts = rendered(&mut cx, &markdown);
    let link = at(&texts[0], "First paragraph with a l".len());
    send(&mut cx, window, down(link, 1));
    send(&mut cx, window, up(link, 1));
    assert_eq!(clicks.borrow().as_slice(), ["https://example.com/a"]);

    // A drag that starts on the link selects instead.
    send(&mut cx, window, down(link, 1));
    send(&mut cx, window, drag(at(&texts[0], 3)));
    send(&mut cx, window, up(at(&texts[0], 3), 1));
    assert_eq!(clicks.borrow().len(), 1);
    assert!(
        cx.read_entity(&markdown, |view, _| view.selected_text())
            .is_some()
    );
}

#[test]
fn copy_button_copies_the_code() {
    let _serial = common::serial();
    let (mut cx, window, markdown) = setup("```rust\nfn main() {}\nlet x = 1;\n```\n");
    let texts = rendered(&mut cx, &markdown);
    let code = &texts[0];
    // The button sits at the right of the header, above the code body
    // (header 36px, then a 1px rule and 10px of padding).
    let view_right = px(640. - 20.);
    let button = point(
        view_right - px(1. + 6. + 12.),
        code.bounds.top() - px(10. + 1. + 18.),
    );
    send(&mut cx, window, down(button, 1));
    send(&mut cx, window, up(button, 1));
    let clipboard = cx.update(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some("fn main() {}\nlet x = 1;"));
    // The press on the button did not start a selection.
    assert_eq!(
        cx.read_entity(&markdown, |view, _| view.selected_text()),
        None
    );
}

#[test]
fn streaming_reveals_words_then_settles_to_the_written_tree() {
    let _serial = common::serial();
    let mut cx = app();
    let (window, markdown) = open(&mut cx, 640., 400., MarkdownView::new);
    cx.update(|cx| {
        markdown.update(cx, |view, cx| {
            view.set_streaming(true, cx);
            view.push_str("Intro with **bold", cx);
        })
    });
    draw(&mut cx, window);
    let shown = cx.read_entity(&markdown, |view, _| view.document().clone());
    // The first word shows at once; the rest is paced.
    assert_eq!(shown.len(), 1);

    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(600) {
        draw(&mut cx, window);
        std::thread::sleep(Duration::from_millis(16));
    }
    let doc = cx.read_entity(&markdown, |view, _| view.document().clone());
    let Block::Paragraph(inline) = &doc.blocks[0].block else {
        panic!("expected a paragraph");
    };
    // Mid-stream the hanging bold is mended: no literal markers.
    assert_eq!(inline.text, "Intro with bold");
    assert!(inline.spans.iter().any(|s| s.style.strong));

    cx.update(|cx| markdown.update(cx, |view, cx| view.set_streaming(false, cx)));
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        draw(&mut cx, window);
        std::thread::sleep(Duration::from_millis(16));
    }
    let doc = cx.read_entity(&markdown, |view, _| view.document().clone());
    let Block::Paragraph(inline) = &doc.blocks[0].block else {
        panic!("expected a paragraph");
    };
    // Settled: the marker never closed, so it shows as written.
    assert_eq!(inline.text, "Intro with **bold");
    assert!(!cx.read_entity(&markdown, |view, _| view.is_animating()));
}

#[test]
fn reduced_motion_still_reveals_text() {
    let _serial = common::serial();
    let mut cx = app();
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, markdown) = open(&mut cx, 640., 400., MarkdownView::new);
    cx.update(|cx| {
        markdown.update(cx, |view, cx| {
            view.set_streaming(true, cx);
            view.push_str("one two three ", cx);
        })
    });
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(400) {
        draw(&mut cx, window);
        std::thread::sleep(Duration::from_millis(16));
    }
    let texts = rendered(&mut cx, &markdown);
    assert_eq!(texts[0].text.as_ref(), "one two three");
}
