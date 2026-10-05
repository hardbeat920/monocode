//! Release-note modal behavior over real GPUI windows.

use super::*;
use gpui::{
    InteractiveElement as _, ParentElement as _, Styled as _, TestAppContext, VisualTestContext,
    WindowHandle,
};
use std::cell::Cell;
use std::rc::Rc;

struct ModalHost {
    focus: FocusHandle,
    escaped: Rc<Cell<usize>>,
}

impl Render for ModalHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let escaped = self.escaped.clone();
        div()
            .id("release-notes-test-host")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .on_key_down(move |event, _, _| {
                if event.keystroke.key == "escape" {
                    escaped.set(escaped.get() + 1);
                }
            })
            .child(layer(window, cx))
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        monocode_markdown::init(cx);
    });
}

fn mount(cx: &mut TestAppContext) -> WindowHandle<ModalHost> {
    let window = cx.add_window(|window, cx| {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        ModalHost {
            focus,
            escaped: Rc::new(Cell::new(0)),
        }
    });
    draw(window, cx);
    window
}

fn draw(handle: WindowHandle<ModalHost>, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    for _ in 0..2 {
        visual.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear();
        });
        visual.run_until_parked();
    }
}

#[gpui::test]
fn release_notes_escape_closes_the_modal_and_restores_the_original_focus(cx: &mut TestAppContext) {
    init(cx);
    let handle = mount(cx);
    let dialog = handle
        .update(cx, |_, window, cx| {
            open("0.1.25", window, cx);
            ensure(window, cx)
        })
        .unwrap();
    draw(handle, cx);
    let viewport = handle
        .update(cx, |_, window, _| window.viewport_size())
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    let bounds = visual
        .debug_bounds("whats-new-overlay")
        .expect("the open dialog should draw its overlay");
    assert_eq!(bounds.size, viewport);
    assert_eq!(bounds.origin, gpui::point(gpui::px(0.), gpui::px(0.)));
    handle
        .update(cx, |host, window, cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.version.as_deref(), Some("0.1.25"));
            assert!(dialog.focus.is_focused(window));
            assert!(!host.focus.is_focused(window));
            let markdown = dialog.markdown.read(cx);
            assert!(!markdown.is_streaming());
            assert!(!markdown.text().is_empty());
            assert!(
                markdown
                    .text()
                    .contains("Inbox: comment on GitHub pull requests and issues")
            );
            assert!(!markdown.text().contains("## [0.1.25]"));
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "escape");
    draw(handle, cx);
    handle
        .update(cx, |host, window, cx| {
            assert!(dialog.read(cx).version.is_none());
            assert!(host.focus.is_focused(window));
            assert_eq!(host.escaped.get(), 0);
        })
        .unwrap();
}

#[gpui::test]
fn reopening_release_notes_keeps_the_original_focus_target(cx: &mut TestAppContext) {
    init(cx);
    let handle = mount(cx);
    handle
        .update(cx, |_, window, cx| open("0.1.25", window, cx))
        .unwrap();
    draw(handle, cx);
    let dialog = handle
        .update(cx, |_, window, cx| {
            open("0.0.0-unpublished", window, cx);
            ensure(window, cx)
        })
        .unwrap();
    draw(handle, cx);
    cx.read(|cx| {
        assert_eq!(
            dialog.read(cx).markdown.read(cx).text(),
            "Release notes for this version are not available in this build."
        );
    });
    cx.simulate_keystrokes(handle.into(), "escape");
    handle
        .update(cx, |host, window, cx| {
            assert!(dialog.read(cx).version.is_none());
            assert!(host.focus.is_focused(window));
        })
        .unwrap();
}

#[gpui::test]
fn release_notes_windows_keep_independent_open_state_and_focus(cx: &mut TestAppContext) {
    init(cx);
    let first = mount(cx);
    let second = mount(cx);
    let first_dialog = first
        .update(cx, |_, window, cx| {
            open("0.1.25", window, cx);
            ensure(window, cx)
        })
        .unwrap();
    let second_dialog = second
        .update(cx, |_, window, cx| {
            open("0.0.0-unpublished", window, cx);
            ensure(window, cx)
        })
        .unwrap();
    assert_ne!(first_dialog.entity_id(), second_dialog.entity_id());
    draw(first, cx);
    draw(second, cx);
    cx.simulate_keystrokes(first.into(), "escape");
    first
        .update(cx, |host, window, cx| {
            assert!(first_dialog.read(cx).version.is_none());
            assert!(host.focus.is_focused(window));
        })
        .unwrap();
    second
        .update(cx, |host, window, cx| {
            assert_eq!(
                second_dialog.read(cx).version.as_deref(),
                Some("0.0.0-unpublished")
            );
            assert!(second_dialog.read(cx).focus.is_focused(window));
            assert!(!host.focus.is_focused(window));
        })
        .unwrap();
    let first_id = first.window_id();
    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(!cx.global::<Dialogs>().0.contains_key(&first_id));
        assert_eq!(cx.global::<Dialogs>().0.len(), 1);
    });
    cx.simulate_keystrokes(second.into(), "escape");
    second
        .update(cx, |host, window, cx| {
            assert!(second_dialog.read(cx).version.is_none());
            assert!(host.focus.is_focused(window));
        })
        .unwrap();
}
