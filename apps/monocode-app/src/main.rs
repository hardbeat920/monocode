//! MonoCode, the native GPUI app.
//!
//! Startup follows src-tauri/src/main.rs and main.tsx: the `app` and
//! `control` subcommands run the agent CLI and exit; otherwise the app
//! resolves its data directory, boots the engine there
//! (`monocode_app::boot`), applies the stored appearance, and opens the
//! window.

mod cli;
mod composer_host;
mod file_pane;
mod format;
mod gallery;
mod glass;
#[cfg(feature = "screenshot")]
mod screenshot;
mod session_pane;
mod shell;
mod view_data;
mod views;

use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Styled as _, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions, actions, point, px, size,
};
use gpui_component::Root;
use monocode_app::boot::{self, AppServices, BootOptions};
use monocode_app::data_dir;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};

actions!(monocode, [Quit]);

/// `app` and `control` run the agent CLI against a running MonoCode, as the
/// Tauri binary did, so agents can call this binary as their CLI.
fn run_cli_subcommand() {
    let mut args = std::env::args().skip(1);
    let code = match args.next().as_deref() {
        Some("control") => monocode_process::control_cli::run(args.collect()),
        Some("app") => monocode_process::control_cli::run_app(args.collect()),
        _ => return,
    };
    std::process::exit(code);
}

fn main() {
    run_cli_subcommand();
    let args = match cli::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err:#}");
            std::process::exit(2);
        }
    };
    if args.list_views {
        for view in views::VIEWS {
            println!("{:<14} {}", view.name, view.description);
        }
        return;
    }
    let Some(entry) = views::find(&args.view) else {
        eprintln!("unknown view {:?}. Run with --list-views.", args.view);
        std::process::exit(2);
    };
    if args.screenshot.is_some() && !cfg!(feature = "screenshot") {
        eprintln!("--screenshot needs a build with `--features screenshot`");
        std::process::exit(2);
    }
    let data_dir = if entry.engine {
        match data_dir::resolve(args.data_dir.as_deref()) {
            Ok(dir) => {
                eprintln!("data dir: {}", dir.path.display());
                Some(dir)
            }
            Err(err) => {
                eprintln!("{err:#}");
                std::process::exit(2);
            }
        }
    } else {
        None
    };

    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            if let Some(dir) = data_dir.clone()
                && let Err(err) = boot::boot(BootOptions::app(dir), cx)
            {
                eprintln!("could not start: {err:#}");
                std::process::exit(1);
            }
            let mut appearance = AppServices::try_global(cx)
                .map(|services| glass::appearance_from_settings(&services.settings.appearance))
                .unwrap_or_else(|| AppearanceSettings {
                    theme_preference: ThemePreference::Dark,
                    ..AppearanceSettings::default()
                });
            if let Some(theme) = &args.theme {
                appearance.theme_preference = ThemePreference::parse(Some(theme));
            }
            if let Some(scale) = args.ui_scale {
                appearance.ui_scale = scale;
            }
            monocode_ui::init(appearance, cx);
            monocode_view_transcript::transcript::init(cx);
            monocode_editor::init(cx);
            monocode_view_composer::composer::init(cx);
            monocode_view_composer::pickers::init(cx);
            cx.set_global(shell::StartupSession(args.open_session.clone()));
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                // `onNew` (⌘T): a new chat in a new tab.
                KeyBinding::new(
                    if cfg!(target_os = "macos") {
                        "cmd-t"
                    } else {
                        "ctrl-t"
                    },
                    shell::NewSession,
                    None,
                ),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_app_quit(|cx| {
                let shutdown = AppServices::try_global(cx).map(|_| boot::shutdown(cx));
                async move {
                    if let Some(shutdown) = shutdown {
                        shutdown.await;
                    }
                }
            })
            .detach();

            let window = open_main_window(&args, entry, cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            #[cfg(feature = "screenshot")]
            if let Some(out) = args.screenshot.clone() {
                let settle = args
                    .settle_ms
                    .unwrap_or(if entry.engine { 2500 } else { 900 });
                screenshot::capture_and_quit(
                    window.into(),
                    out,
                    args.backdrop,
                    Duration::from_millis(settle),
                    cx,
                );
            }
            #[cfg(not(feature = "screenshot"))]
            let _ = (window, Duration::ZERO);
            cx.activate(true);
        });
}

/// The main window: transparent and blurred on macOS in dark mode, with a
/// hidden title bar and the traffic lights inset into the 40px chrome.
fn open_main_window(
    args: &cli::Args,
    entry: &'static views::ViewEntry,
    cx: &mut App,
) -> WindowHandle<Root> {
    let theme = Theme::of(cx);
    let metrics = theme.metrics;
    let (width, height) = args.size;
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(800.), px(520.))),
        titlebar: Some(TitlebarOptions {
            title: Some("MonoCode".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(metrics.traffic_light_x),
                px(metrics.traffic_light_y),
            )),
        }),
        app_owns_titlebar_drag: true,
        window_background: theme.window_background(),
        app_id: Some("com.monocode.desktop".into()),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        glass::sync_window(window, cx);
        window
            .observe_window_appearance(|window, cx| {
                monocode_ui::set_system_scheme(window.appearance(), cx);
                glass::sync_window(window, cx);
            })
            .detach();
        let view = (entry.build)(window, cx);
        // Root paints gpui-component's background by default. Our views paint
        // their own, translucent over the window glass.
        cx.new(|cx| Root::new(view, window, cx).bg(gpui::transparent_black()))
    })
    .expect("open the main window")
}
