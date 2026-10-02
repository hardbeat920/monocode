//! Runs the user's login shell in a window through portable-pty.
//!
//! ```sh
//! cargo run -p monocode-terminal-view --example shell
//! cargo run -p monocode-terminal-view --example shell -- --light
//! ```
//!
//! With the `screenshot` feature, `--screenshot <path.png>` types the
//! `--command` text into the shell, waits for output, writes what the window
//! draws to the PNG, and quits:
//!
//! ```sh
//! cargo run -p monocode-terminal-view --example shell --features screenshot -- \
//!     --screenshot /tmp/terminal.png --command 'ls -G'
//! ```
//!
//! `--select r0,c0,r1,c1` drags a selection and `--hover row,col` moves the
//! pointer there with Cmd held, both before the capture. `TERMINAL_TRACE=1`
//! prints every write to the PTY.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Context as _;
use gpui::{
    App, AppContext, Bounds, Context, Entity, Focusable, IntoElement, KeyBinding, ParentElement,
    Render, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div, px, size,
};
use monocode_terminal_view::theme::hsla;
use monocode_terminal_view::{Pty, PtyEvent, PtySize, TerminalEvent, TerminalTheme, TerminalView};
use portable_pty::{CommandBuilder, MasterPty, native_pty_system};

actions!(shell_example, [Quit]);

/// A [`Pty`] over portable-pty. A reader thread forwards output; a writer
/// thread keeps a large paste from blocking the UI thread.
struct PortablePty {
    master: Box<dyn MasterPty + Send>,
    writer: mpsc::Sender<Vec<u8>>,
    events: Option<async_channel::Receiver<PtyEvent>>,
}

impl PortablePty {
    fn spawn_login_shell(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let pair = native_pty_system()
            .openpty(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open pty")?;
        let mut command = CommandBuilder::new_default_prog();
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        if let Some(home) = std::env::var_os("HOME") {
            command.cwd(home);
        }
        let mut child = pair.slave.spawn_command(command).context("spawn shell")?;
        drop(pair.slave);

        let (sender, events) = async_channel::unbounded();
        let mut reader = pair.master.try_clone_reader().context("clone reader")?;
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sender
                            .send_blocking(PtyEvent::Output(buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let code = child.wait().ok().map(|status| status.exit_code() as i32);
            let _ = sender.send_blocking(PtyEvent::Exited(code));
        });

        let mut writer = pair.master.take_writer().context("take writer")?;
        let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for bytes in writer_rx {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(Self {
            master: pair.master,
            writer: writer_tx,
            events: Some(events),
        })
    }
}

impl Pty for PortablePty {
    fn take_events(&mut self) -> Option<async_channel::Receiver<PtyEvent>> {
        self.events.take()
    }

    fn write(&mut self, bytes: &[u8]) {
        if std::env::var_os("TERMINAL_TRACE").is_some() {
            eprintln!("pty write {:?}", String::from_utf8_lossy(bytes));
        }
        let _ = self.writer.send(bytes.to_vec());
    }

    fn resize(&mut self, size: PtySize) {
        let _ = self.master.resize(portable_pty::PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        });
    }
}

struct Shell {
    terminal: Entity<TerminalView>,
}

impl Shell {
    fn new(theme: TerminalTheme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pty = PortablePty::spawn_login_shell(80, 24).expect("spawn login shell");
        let terminal = cx.new(|cx| TerminalView::new(pty, theme, window, cx));
        cx.subscribe_in(&terminal, window, |_, _, event, window, cx| match event {
            TerminalEvent::TitleChanged(title) => {
                window.set_window_title(title.as_deref().unwrap_or("Terminal"))
            }
            TerminalEvent::OpenUrl(url) => cx.open_url(url),
            _ => {}
        })
        .detach();
        let handle = terminal.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        Self { terminal }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // TerminalView.tsx draws on a transparent background over the pane,
        // which is `--color-background-base`.
        let base = self.terminal.read(cx).theme().base_background;
        div()
            .size_full()
            .bg(hsla(base))
            .child(self.terminal.clone())
    }
}

struct Args {
    light: bool,
    screenshot: Option<String>,
    command: Option<String>,
    /// Drag-select from (row, col) to (row, col) before the screenshot.
    select: Option<[usize; 4]>,
    /// Hover (row, col) with Cmd held before the screenshot.
    hover: Option<[usize; 2]>,
}

fn numbers<const N: usize>(text: Option<String>) -> Option<[usize; N]> {
    let values: Vec<usize> = text?.split(',').filter_map(|v| v.parse().ok()).collect();
    values.try_into().ok()
}

fn parse_args() -> Args {
    let mut args = Args {
        light: false,
        screenshot: None,
        command: None,
        select: None,
        hover: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--light" => args.light = true,
            "--screenshot" => args.screenshot = iter.next(),
            "--command" => args.command = iter.next(),
            "--select" => args.select = numbers(iter.next()),
            "--hover" => args.hover = numbers(iter.next()),
            other => eprintln!("ignoring argument {other}"),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let theme = if args.light {
        TerminalTheme::light()
    } else {
        TerminalTheme::dark()
    };
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(900.0), px(560.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Terminal".into()),
                        ..Default::default()
                    }),
                    focus: true,
                    show: true,
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Shell::new(theme, window, cx)),
            )
            .expect("open window");
        cx.activate(true);

        if let Some(path) = args.screenshot {
            let command = args.command;
            let (select, hover) = (args.select, args.hover);
            cx.spawn(async move |cx| {
                let executor = cx.background_executor().clone();
                let pause = |ms| executor.timer(Duration::from_millis(ms));
                pause(1500).await;
                if let Some(command) = command {
                    let _ = window.update(cx, |shell, _, cx| {
                        shell.terminal.update(cx, |view, cx| {
                            view.input(format!("{command}\r").as_bytes(), cx)
                        })
                    });
                    pause(2000).await;
                }
                let any_window: gpui::AnyWindowHandle = window.into();
                let _ = any_window.update(cx, |_, window, cx| {
                    window.draw(cx).clear();
                    simulate_pointer(window, cx, select, hover);
                });
                let _ = window.update(cx, |shell, _, cx| {
                    shell
                        .terminal
                        .update(cx, |view, cx| view.restart_cursor_blink(cx))
                });
                pause(100).await;
                // The untyped handle does not lease the root view, so drawing
                // inside the update can render it.
                let result = any_window.update(cx, |_, window, cx| capture(window, cx, &path));
                match result {
                    Ok(Ok(())) => eprintln!("wrote {path}"),
                    Ok(Err(error)) => eprintln!("screenshot failed: {error:#}"),
                    Err(error) => eprintln!("screenshot failed: {error:#}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
}

/// Drive the real mouse path: press, drag, and release for a selection, and
/// a Cmd-held move for link hover.
fn simulate_pointer(
    window: &mut Window,
    cx: &mut App,
    select: Option<[usize; 4]>,
    hover: Option<[usize; 2]>,
) {
    use gpui::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    };
    let Some(root) = window.root::<Shell>().flatten() else {
        return;
    };
    let Some(layout) = root.read(cx).terminal.read(cx).layout() else {
        return;
    };
    let at = |row: usize, col: usize| {
        let cell = layout.cell_bounds(row, col, 1);
        gpui::point(cell.left() + px(1.0), cell.top() + px(2.0))
    };
    if let Some([r0, c0, r1, c1]) = select {
        let modifiers = Modifiers::default();
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: at(r0, c0),
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at(r1, c1),
                pressed_button: Some(MouseButton::Left),
                modifiers,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: at(r1, c1),
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    }
    if let Some([row, col]) = hover {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at(row, col),
                pressed_button: None,
                modifiers: Modifiers {
                    platform: cfg!(target_os = "macos"),
                    control: !cfg!(target_os = "macos"),
                    ..Modifiers::default()
                },
            }),
            cx,
        );
    }
}

#[cfg(feature = "screenshot")]
fn capture(window: &mut Window, cx: &mut App, path: &str) -> anyhow::Result<()> {
    window.draw(cx).clear();
    window.render_to_image()?.save(path)?;
    Ok(())
}

#[cfg(not(feature = "screenshot"))]
fn capture(_window: &mut Window, _cx: &mut App, _path: &str) -> anyhow::Result<()> {
    anyhow::bail!("build with --features screenshot to write PNGs")
}
