//! The terminal dock and the empty-session arcade, for checking them by eye
//! or by screenshot.
//!
//! ```text
//! cargo run -p monocode-view-workbench --example terminal_dock_gallery -- [options]
//!   --view <name>        bottom (default), top, left, right: the dock on that
//!                        edge with two terminals, beside an empty session.
//!                        menu: the bottom dock with its side menu open.
//!                        arcade: the idle arcade band mid-game.
//!                        playing: a game under control, with the HUD.
//!   --game <id>          pacman (default) or snake, for arcade and playing
//!   --hover              rest the pointer on the arcade band
//!   --light              light theme
//!   --size <w>x<h>       window size in points
//!   --screenshot <png>   write what the window draws, then quit. The window
//!                        opens without focus and the app is not activated.
//! ```
//!
//! The terminals are views over a recording PTY fed canned output, so the
//! gallery runs no shell.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_component::Root;
use monocode_layout::project_terminal::{add_terminal_to_dock, create_project_terminal};
use monocode_layout::{DockSide, FilePaneTab, new_terminal_file};
use monocode_terminal_view::{RecordingPty, TerminalTheme, TerminalView};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_workbench::panes::empty_session::{EmptySession, EmptySessionProps};
use monocode_view_workbench::panes::surface_tabs::ClipboardOnlyActions;
use monocode_view_workbench::terminal_dock::arcade::ArcadeRng;
use monocode_view_workbench::terminal_dock::{
    DockTerminals, TerminalDock, TerminalDockEvent, TerminalGridBackground, dock_grid,
};

struct Args {
    view: String,
    game: String,
    hover: bool,
    light: bool,
    size: (f32, f32),
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        view: "bottom".into(),
        game: "pacman".into(),
        hover: false,
        light: false,
        size: (1100., 720.),
        screenshot: None,
    };
    let mut size_set = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--view" => parsed.view = args.next().expect("--view <name>"),
            "--game" => parsed.game = args.next().expect("--game <id>"),
            "--hover" => parsed.hover = true,
            "--light" => parsed.light = true,
            "--size" => {
                let value = args.next().expect("--size <w>x<h>");
                let (w, h) = value.split_once('x').expect("--size <w>x<h>");
                parsed.size = (w.parse().expect("width"), h.parse().expect("height"));
                size_set = true;
            }
            "--screenshot" => parsed.screenshot = args.next().map(PathBuf::from),
            other => eprintln!("unknown argument {other}"),
        }
    }
    if !size_set && matches!(parsed.view.as_str(), "arcade" | "playing") {
        parsed.size = (1000., 560.);
    }
    parsed
}

/// A colored `ls` and a prompt, as a shell would draw them.
const SHELL_OUTPUT: &str = "\x1b[1;32mgian@mbp\x1b[0m \x1b[1;34m~/code/monocode\x1b[0m % ls -G\r\n\
\x1b[1;34mapps\x1b[0m        Cargo.lock  \x1b[1;34mcrates\x1b[0m      \x1b[1;34mdocs\x1b[0m        package.json\r\n\
AGENTS.md   Cargo.toml  \x1b[1;34mhost\x1b[0m        README.md   \x1b[1;34msrc\x1b[0m\r\n\
\x1b[1;32mgian@mbp\x1b[0m \x1b[1;34m~/code/monocode\x1b[0m % cargo test -p monocode-view-workbench\r\n\
\x1b[1;32m   Compiling\x1b[0m monocode-view-workbench v0.6.0\r\n\
\x1b[1;32m    Finished\x1b[0m `test` profile [unoptimized + debuginfo] target(s) in 41.2s\r\n\
test result: \x1b[32mok\x1b[0m. 171 passed; 0 failed; 0 ignored\r\n\
\x1b[1;32mgian@mbp\x1b[0m \x1b[1;34m~/code/monocode\x1b[0m % ";

const SERVER_OUTPUT: &str = "\x1b[1;32mgian@mbp\x1b[0m \x1b[1;34m~/code/monocode\x1b[0m % npm run dev\r\n\r\n\
  \x1b[1;32mVITE\x1b[0m v7.1.2  ready in 412 ms\r\n\r\n\
  \x1b[32m➜\x1b[0m  Local:   \x1b[36mhttp://localhost:1420/\x1b[0m\r\n";

/// Terminals over a recording PTY, fed canned output by title.
struct CannedTerminals {
    light: bool,
}

impl DockTerminals for CannedTerminals {
    fn open(&self, file: &FilePaneTab, window: &mut Window, cx: &mut App) -> Entity<TerminalView> {
        let theme = if self.light {
            TerminalTheme::light()
        } else {
            TerminalTheme::dark()
        };
        let output = if file.path == "dev server" {
            SERVER_OUTPUT
        } else {
            SHELL_OUTPUT
        };
        cx.new(|cx| {
            let mut view = TerminalView::new(RecordingPty::new(), theme, window, cx);
            view.feed(output.as_bytes(), cx);
            view
        })
    }
}

fn terminal(id: &str, title: &str) -> FilePaneTab {
    let mut file = new_terminal_file("/Users/gian/code/monocode", Some(title), None);
    file.id = id.into();
    file
}

/// The workspace: an empty session with the dock on one edge.
struct Workspace {
    dock: Entity<TerminalDock>,
    main: AnyView,
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock = self.dock.read(cx);
        let side = dock.dock().side;
        let size = dock.display_size() as f64;
        dock_grid(
            Some(side),
            size,
            Some(self.dock.clone().into_any_element()),
            self.main.clone().into_any_element(),
        )
    }
}

struct Gallery {
    content: AnyView,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .child(self.content.clone())
    }
}

/// An empty session with the arcade behind it, the game well under way.
fn empty_session(game: usize, cx: &mut App) -> (AnyView, Entity<TerminalGridBackground>) {
    let arcade = cx.new(|cx| {
        let mut arcade = TerminalGridBackground::with_rng(ArcadeRng::seeded(0x5eed), cx);
        arcade.show_game(game, cx);
        arcade
    });
    let session = cx.new(|cx| {
        let mut screen = EmptySession::new(EmptySessionProps {
            cwd: "/Users/gian/code/monocode".into(),
            project: Some("monocode".into()),
            has_chat_background: false,
            arcade_enabled: true,
        });
        screen.set_arcade(Some(arcade.clone().into()), cx);
        screen
    });
    (session.into(), arcade)
}

struct Built {
    content: AnyView,
    arcade: Entity<TerminalGridBackground>,
    dock: Option<Entity<TerminalDock>>,
}

fn build(args: &Args, window: &mut Window, cx: &mut App) -> Built {
    let game = usize::from(args.game == "snake");
    let side = match args.view.as_str() {
        "top" => Some(DockSide::Top),
        "left" => Some(DockSide::Left),
        "right" => Some(DockSide::Right),
        "bottom" | "menu" => Some(DockSide::Bottom),
        _ => None,
    };
    let (main, arcade) = empty_session(game, cx);
    let Some(side) = side else {
        return Built {
            content: main,
            arcade,
            dock: None,
        };
    };

    let model = add_terminal_to_dock(
        &create_project_terminal(
            "/Users/gian/code/monocode",
            terminal("server", "dev server"),
            Some(side),
        ),
        terminal("shell", "zsh"),
    );
    let terminals: Rc<dyn DockTerminals> = Rc::new(CannedTerminals { light: args.light });
    let dock = cx.new(|cx| {
        let mut dock =
            TerminalDock::new(model, terminals, Rc::new(ClipboardOnlyActions), window, cx);
        dock.set_focused(true, window, cx);
        dock
    });
    let workspace = cx.new(|cx| {
        cx.observe(&dock, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&dock, |_, _, event: &TerminalDockEvent, _| {
            eprintln!("dock: {event:?}")
        })
        .detach();
        Workspace {
            dock: dock.clone(),
            main,
        }
    });
    Built {
        content: workspace.into(),
        arcade,
        dock: Some(dock),
    }
}

fn main() {
    let args = parse_args();
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let mut appearance = AppearanceSettings::default();
            if args.light {
                appearance.theme_preference = ThemePreference::parse(Some("light"));
            }
            monocode_ui::init(appearance, cx);
            monocode_view_workbench::panes::init(cx);
            let screenshot = args.screenshot.clone();
            let bounds = Bounds::centered(None, size(px(args.size.0), px(args.size.1)), cx);
            let mut built = None;
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        // A screenshot run must never take focus.
                        focus: screenshot.is_none(),
                        ..Default::default()
                    },
                    |window, cx| {
                        monocode_ui::sync_window(window, cx);
                        let parts = build(&args, window, cx);
                        let gallery = cx.new(|_| Gallery {
                            content: parts.content.clone(),
                        });
                        built = Some(parts);
                        cx.new(|cx| Root::new(gallery, window, cx))
                    },
                )
                .expect("open window");

            let view = args.view.clone();
            let hover = args.hover;
            let playing = view == "playing";
            let open_menu = view == "menu";
            let Built { arcade, dock, .. } = built.expect("the gallery");
            cx.spawn(async move |cx| {
                // Let the first layout size the boards, then play a while.
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let any: gpui::AnyWindowHandle = window.into();
                let _ = any.update(cx, |_, window, cx| {
                    arcade.update(cx, |arcade, cx| {
                        if playing {
                            arcade.take_control(window, cx);
                        } else {
                            arcade.fast_forward(12_000.0, cx);
                        }
                    });
                });
                if playing {
                    // Steer through a few turns so the game is under way.
                    for (x, y) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 0), (0, 1)] {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let _ = any.update(cx, |_, _, cx| {
                            arcade.update(cx, |arcade, cx| {
                                arcade.steer(x, y);
                                arcade.fast_forward(900.0, cx);
                            });
                        });
                    }
                }
                if open_menu && let Some(dock) = &dock {
                    let _ = any.update(cx, |_, _, cx| {
                        dock.update(cx, |dock, cx| dock.open_side_menu(cx));
                    });
                }
                if hover {
                    let _ = any.update(cx, |_, window, cx| {
                        let width = window.viewport_size().width;
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: point(width / 2.0, px(90.)),
                                ..Default::default()
                            }),
                            cx,
                        );
                    });
                }
                let Some(out) = screenshot else {
                    cx.update(|cx| cx.activate(true));
                    return;
                };
                // Let images load, the hover fade finish, and the tabs settle.
                for _ in 0..10 {
                    cx.background_executor()
                        .timer(Duration::from_millis(60))
                        .await;
                    let _ = any.update(cx, |_, window, _| window.refresh());
                }
                let result = any.update(cx, |_, window, cx| {
                    if !hover {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: point(px(-1000.), px(-1000.)),
                                ..Default::default()
                            }),
                            cx,
                        );
                    }
                    window.draw(cx).clear();
                    window.render_to_image().map(|image| image.save(&out))
                });
                match result {
                    Ok(Ok(Ok(()))) => eprintln!("wrote {}", out.display()),
                    other => eprintln!("screenshot failed: {other:?}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
