//! MonoCode, the native GPUI app.
//!
//! Startup follows src-tauri/src/main.rs and main.tsx: the `app` and
//! `control` subcommands run the agent CLI and exit; otherwise the app
//! resolves its data directory, boots the engine there
//! (`monocode_app::boot`), applies the stored appearance, and opens the
//! window.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod adapters;
mod cli;
mod composer_host;
mod file_pane;
mod format;
mod gallery;
mod glass;
mod pages;
mod panes;
mod quick;
mod remote_pane;
#[cfg(feature = "screenshot")]
mod screenshot;
mod session_cards;
mod session_empty;
mod session_navigation;
mod session_pane;
mod session_threads;
mod session_toolbar;
mod shell;
mod slots;
mod views;

use std::time::Duration;

use gpui::{App, AppContext as _, Styled as _, WindowHandle, px, size};
use gpui_component::Root;
use monocode_app::boot::{self, AppServices, BootOptions};
use monocode_app::data_dir;
use monocode_ui::{AppearanceSettings, ThemePreference};

/// `app` and `control` run the agent CLI against a running MonoCode, as the
/// Tauri binary did, so agents can call this binary as their CLI.
fn run_cli_subcommand() {
    let mut args = std::env::args().skip(1);
    let code = match args.next().as_deref() {
        Some("control") => monocode_process::control_cli::run(args.collect()),
        Some("app") => monocode_process::control_cli::run_app(args.collect()),
        Some("host") => monocode_host::run_host_cli(&args.collect::<Vec<_>>()),
        _ => return,
    };
    std::process::exit(code);
}

fn main() {
    #[cfg(windows)]
    if std::env::args().nth(1).is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "app" | "control" | "host" | "--list-views" | "--help" | "-h"
        )
    }) {
        monocode_platform::windows::attach_parent_console();
    }
    if let Some(code) = monocode_remote::ssh_askpass::maybe_run() {
        std::process::exit(code);
    }
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

    let application = gpui_platform::application().with_assets(monocode_ui::Assets);
    let (urls, opened_urls) = async_channel::unbounded::<Vec<String>>();
    application.on_open_urls(move |links| {
        let _ = urls.try_send(links);
    });
    if args.screenshot.is_none() {
        application.on_reopen(|cx| {
            if let Err(error) = shell::windows::reopen(cx) {
                log::error!("Could not reopen window: {error}");
            }
        });
    }
    application.run(move |cx: &mut App| {
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
        monocode_view_files::init(cx);
        monocode_view_pages::init(cx);
        monocode_view_inbox::init(cx);
        monocode_view_workbench::panes::init(cx);
        if AppServices::try_global(cx).is_some() {
            slots::install(cx);
        }
        cx.set_global(shell::StartupSession(args.open_session.clone()));
        shell::init(cx);
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
        if args.screenshot.is_none() {
            quick::init(cx);
            if AppServices::try_global(cx).is_some() {
                shell::windows::install_tray(cx);
                adapters::settings::check_for_updates(false, cx);
            }
            for link in &args.urls {
                if link.starts_with("monocode://pair?") {
                    adapters::remote::open_pairing_link(link, cx);
                }
            }
            cx.spawn(async move |cx| {
                while let Ok(links) = opened_urls.recv().await {
                    cx.update(|cx| {
                        for link in links {
                            if link.starts_with("monocode://pair?") {
                                adapters::remote::open_pairing_link(&link, cx);
                            }
                        }
                    });
                }
            })
            .detach();
        }
        cx.on_window_closed(|cx, id| {
            slots::forget_window(id, cx);
            shell::title_bar::forget_window_bounds(id, cx);
            if let Some(package) = monocode_engine::automations::AutomationsPackage::try_global(cx)
            {
                let (quick, reminders) = (package.quick_launch.clone(), package.reminders.clone());
                let label = shell::windows::window_label(id);
                quick.update(cx, |quick, _| quick.detach_window(&label));
                reminders.update(cx, |reminders, _| reminders.detach_window(&label));
            }
            if cx.windows().is_empty() {
                shell::keymap::request_quit(cx);
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
        // A screenshot run must not take focus from the user's windows.
        if args.screenshot.is_none() {
            cx.activate(true);
        }
    });
}

/// The main window: transparent and blurred on macOS in dark mode, with a
/// hidden title bar and the traffic lights inset into the 40px chrome.
fn open_main_window(
    args: &cli::Args,
    entry: &'static views::ViewEntry,
    cx: &mut App,
) -> WindowHandle<Root> {
    let (width, height) = args.size;
    let mut options = shell::windows::window_options(size(px(width), px(height)), cx);
    let save_window_state = args.screenshot.is_none() && entry.name == "shell";
    if save_window_state && !args.size_override {
        options.window_bounds = shell::windows::state::restore(cx).or(options.window_bounds);
    }
    options.focus = args.screenshot.is_none();
    let initial_bounds = options.window_bounds;
    cx.open_window(options, |window, cx| {
        glass::sync_window(window, cx);
        shell::windows::install_lifecycle(window, cx);
        if save_window_state {
            shell::windows::state::install(window, initial_bounds, cx);
        }
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
