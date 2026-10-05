//! TerminalView wired into a GPUI test window: keystrokes, IME, clipboard,
//! mouse, PTY output, resize, and exit, checked against a RecordingPty.

use gpui::{
    AppContext, ClipboardItem, Context, Entity, EntityInputHandler, Focusable, IntoElement,
    Modifiers, MouseButton, ParentElement, Render, Styled, TestAppContext, VisualTestContext,
    Window, div, point, px,
};
use monocode_terminal_view::{
    PtyEvent, RecordingPty, TerminalEvent, TerminalTheme, TerminalView,
    emulator::{SelectionType, Side},
};

struct Host {
    terminal: Entity<TerminalView>,
    events: Vec<TerminalEvent>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.terminal.clone())
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<Host>, RecordingPty, &mut VisualTestContext) {
    let pty = RecordingPty::new();
    let for_view = pty.clone();
    let (host, cx) = cx.add_window_view(|window, cx| {
        let terminal = cx.new(|cx| TerminalView::new(for_view, TerminalTheme::dark(), window, cx));
        cx.subscribe(&terminal, |host: &mut Host, _, event: &TerminalEvent, _| {
            host.events.push(event.clone())
        })
        .detach();
        Host {
            terminal,
            events: Vec::new(),
        }
    });
    let terminal = host.read_with(cx, |host, _| host.terminal.clone());
    cx.update(|window, cx| {
        let handle = terminal.read(cx).focus_handle(cx);
        handle.focus(window, cx);
        window.draw(cx).clear();
    });
    cx.run_until_parked();
    pty.take_written();
    (host, pty, cx)
}

fn terminal(host: &Entity<Host>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    host.read_with(cx, |host, _| host.terminal.clone())
}

fn output(pty: &RecordingPty, bytes: &[u8], cx: &mut VisualTestContext) {
    pty.sender()
        .send_blocking(PtyEvent::Output(bytes.to_vec()))
        .unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
}

#[gpui::test]
fn typing_reaches_the_pty(cx: &mut TestAppContext) {
    let (_host, pty, cx) = setup(cx);
    cx.simulate_keystrokes("l s space - l enter");
    assert_eq!(pty.take_written(), b"ls -l\r".to_vec());
    cx.simulate_keystrokes("ctrl-c up tab shift-tab escape backspace");
    assert_eq!(pty.take_written(), b"\x03\x1b[A\t\x1b[Z\x1b\x7f".to_vec());
    cx.simulate_keystrokes("alt-b ctrl-a f5");
    assert_eq!(pty.take_written(), b"\x1bb\x01\x1b[15~".to_vec());
}

#[gpui::test]
fn application_cursor_mode_changes_arrows(cx: &mut TestAppContext) {
    let (_host, pty, cx) = setup(cx);
    output(&pty, b"\x1b[?1h", cx);
    cx.simulate_keystrokes("up left");
    assert_eq!(pty.take_written(), b"\x1bOA\x1bOD".to_vec());
}

#[gpui::test]
fn ime_preedit_waits_for_commit(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    let terminal = terminal(&host, cx);
    cx.update(|window, cx| {
        terminal.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            assert_eq!(view.marked_text_range(window, cx), Some(0..2));
        })
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(pty.take_written().is_empty(), "preedit is not sent");
    cx.update(|window, cx| {
        terminal.update(cx, |view, cx| {
            view.replace_text_in_range(None, "你", window, cx);
            assert_eq!(view.marked_text_range(window, cx), None);
        })
    });
    assert_eq!(pty.take_written(), "你".as_bytes().to_vec());
}

#[gpui::test]
fn paste_uses_bracketed_mode(cx: &mut TestAppContext) {
    let (_host, pty, cx) = setup(cx);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("a\nb".into())));
    cx.simulate_keystrokes("ctrl-v");
    assert_eq!(pty.take_written(), b"a\rb".to_vec());
    output(&pty, b"\x1b[?2004h", cx);
    cx.simulate_keystrokes("ctrl-v");
    assert_eq!(pty.take_written(), b"\x1b[200~a\rb\x1b[201~".to_vec());
}

#[gpui::test]
fn ctrl_c_copies_a_selection_instead_of_interrupting(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    output(&pty, b"hello world", cx);
    let terminal = terminal(&host, cx);
    terminal.update(cx, |view, _| {
        let emulator = view.emulator_mut();
        emulator.start_selection(SelectionType::Simple, emulator.grid_point(0, 0), Side::Left);
        emulator.update_selection(emulator.grid_point(0, 4), Side::Right);
    });
    cx.simulate_keystrokes("ctrl-c");
    assert!(pty.take_written().is_empty());
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("hello"));
    // Typing clears the selection, so the next Ctrl+C interrupts.
    cx.simulate_keystrokes("x ctrl-c");
    assert_eq!(pty.take_written(), b"x\x03".to_vec());
}

#[gpui::test]
fn output_replies_title_and_bell(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    output(&pty, b"hi\x1b[6n", cx);
    let terminal = terminal(&host, cx);
    terminal.read_with(cx, |view, _| assert_eq!(view.emulator().row_text(0), "hi"));
    assert_eq!(pty.take_written(), b"\x1b[1;3R".to_vec());
    output(&pty, b"\x1b]0;build\x07\x07", cx);
    host.read_with(cx, |host, _| {
        assert!(
            host.events
                .contains(&TerminalEvent::TitleChanged(Some("build".into())))
        );
        assert!(host.events.contains(&TerminalEvent::Bell));
    });
}

#[gpui::test]
fn layout_resizes_the_grid_and_the_pty(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    let terminal = terminal(&host, cx);
    let size = terminal.read_with(cx, |view, _| view.grid_size());
    let resized = pty.sizes();
    let last = resized
        .last()
        .copied()
        .expect("the first layout resizes the PTY");
    assert_eq!((last.cols, last.rows), (size.cols, size.rows));
    assert_ne!((size.cols, size.rows), (80, 24));
    host.read_with(cx, |host, _| {
        assert!(host.events.contains(&TerminalEvent::Resized(last)));
    });
}

#[gpui::test]
fn exit_prints_the_notice_and_stops_input(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    pty.sender()
        .send_blocking(PtyEvent::Exited(Some(0)))
        .unwrap();
    cx.run_until_parked();
    let terminal = terminal(&host, cx);
    terminal.read_with(cx, |view, _| {
        assert!(view.has_exited());
        assert!(
            view.emulator()
                .screen_text()
                .contains("[process exited (0)]")
        );
    });
    host.read_with(cx, |host, _| {
        assert!(host.events.contains(&TerminalEvent::Exited(Some(0))))
    });
    cx.simulate_keystrokes("a enter");
    assert!(pty.take_written().is_empty());
}

#[gpui::test]
fn mouse_drag_selects_and_reporting_sends_sgr(cx: &mut TestAppContext) {
    let (host, pty, cx) = setup(cx);
    output(&pty, b"select me please", cx);
    let terminal = terminal(&host, cx);
    let layout = terminal.read_with(cx, |view, _| view.layout().unwrap());
    let at = |col: usize, row: usize| {
        let cell = layout.cell_bounds(row, col, 1);
        point(cell.left() + px(1.0), cell.top() + px(1.0))
    };
    cx.simulate_mouse_down(at(0, 0), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(at(6, 0), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(at(6, 0), MouseButton::Left, Modifiers::default());
    let selected = terminal.read_with(cx, |view, _| view.emulator().selection_text());
    assert_eq!(selected.as_deref(), Some("select"));
    assert!(pty.take_written().is_empty());

    output(&pty, b"\x1b[?1000h\x1b[?1006h", cx);
    cx.simulate_mouse_down(at(3, 1), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(at(3, 1), MouseButton::Left, Modifiers::default());
    assert_eq!(pty.take_written(), b"\x1b[<0;4;2M\x1b[<0;4;2m".to_vec());
}
