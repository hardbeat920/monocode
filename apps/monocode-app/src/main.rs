//! MonoCode, the native GPUI app.

mod cli;
mod gallery;
mod mock;
#[cfg(feature = "screenshot")]
mod screenshot;
mod shell;
mod views;

use gpui::{
    App, AppContext as _, Bounds, Styled as _, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, point, px, size,
};
use gpui_component::Root;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};

fn main() {
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

    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let mut appearance = AppearanceSettings::default();
            if let Some(theme) = &args.theme {
                appearance.theme_preference = ThemePreference::parse(Some(theme));
            }
            if let Some(scale) = args.ui_scale {
                appearance.ui_scale = scale;
            }
            monocode_ui::init(appearance, cx);

            let window = open_main_window(&args, entry, cx);
            #[cfg(feature = "screenshot")]
            if let Some(out) = args.screenshot.clone() {
                screenshot::capture_and_quit(window.into(), out, args.backdrop, cx);
            }
            #[cfg(not(feature = "screenshot"))]
            let _ = window;
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
        monocode_ui::sync_window(window, cx);
        window
            .observe_window_appearance(|window, cx| {
                monocode_ui::set_system_scheme(window.appearance(), cx);
                monocode_ui::sync_window(window, cx);
            })
            .detach();
        let view = (entry.build)(window, cx);
        // Root paints gpui-component's background by default. Our views paint
        // their own, translucent over the window glass.
        cx.new(|cx| Root::new(view, window, cx).bg(gpui::transparent_black()))
    })
    .expect("open the main window")
}
