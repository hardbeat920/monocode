//! Native windows and the New Window action.

use super::{Shell, ShellOptions, WorkspaceStart, keymap::NewWindow};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Global, Pixels, Size, Styled as _,
    TitlebarOptions, Window, WindowBounds, WindowId, WindowOptions, point, px, size,
};
use gpui_component::Root;
use monocode_ui::Theme;
use std::collections::HashSet;

#[path = "window_state.rs"]
pub mod state;

#[derive(Default)]
struct WorkspaceWindows(HashSet<WindowId>);
impl Global for WorkspaceWindows {}

#[cfg(all(windows, not(test)))]
struct AppTray {
    _tray: monocode_platform::tray::Tray,
}
#[cfg(all(windows, not(test)))]
impl Global for AppTray {}

pub fn window_options(window_size: Size<Pixels>, cx: &App) -> WindowOptions {
    let theme = Theme::of(cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        window_min_size: Some(size(px(800.), px(520.))),
        titlebar: Some(TitlebarOptions {
            title: Some("MonoCode".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(theme.metrics.traffic_light_x),
                px(theme.metrics.traffic_light_y),
            )),
        }),
        app_owns_titlebar_drag: true,
        window_background: theme.window_background(),
        app_id: Some("com.monocode.desktop".into()),
        ..Default::default()
    }
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &NewWindow, cx| {
        if let Err(error) = open_workspace_window(true, cx) {
            log::error!("Could not open window: {error}");
        }
    });
    cx.on_window_closed(|cx, id| {
        if cx.has_global::<WorkspaceWindows>() {
            cx.global_mut::<WorkspaceWindows>().0.remove(&id);
        }
    })
    .detach();
}

/// Workspace windows exclude the retained quick composer and git panels.
fn workspace_windows(cx: &App) -> Vec<AnyWindowHandle> {
    let mut windows = cx.windows();
    windows.retain(|window| {
        cx.try_global::<WorkspaceWindows>()
            .is_some_and(|workspaces| workspaces.0.contains(&window.window_id()))
    });
    windows.sort_by_key(|window| window.window_id().as_u64());
    windows
}

fn show_window(window: &Window, cx: &mut App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        monocode_platform::macos_panel::show_workspace_window(window)?;
    }
    #[cfg(all(windows, not(test)))]
    {
        monocode_platform::windows::show_native_window(window)?;
    }
    #[cfg(any(not(any(target_os = "macos", windows)), all(windows, test)))]
    {
        let _ = window;
    }
    record_window_visibility(window, true, cx);
    Ok(())
}

pub fn bring_forward(window: &mut Window, cx: &mut App) {
    if let Err(error) = show_window(window, cx) {
        log::error!("Could not show window: {error}");
    }
    window.activate_window();
    cx.activate(true);
}

pub fn hide_window(window: &Window, cx: &mut App) {
    record_window_visibility(window, false, cx);
    #[cfg(all(windows, not(test)))]
    {
        if let Err(error) = monocode_platform::windows::hide_native_window(window) {
            log::error!("Could not hide window: {error}");
        }
        let _ = cx;
    }
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = monocode_platform::macos_panel::hide(window) {
            log::error!("Could not hide window: {error}");
        }
    }
    #[cfg(any(not(any(target_os = "macos", windows)), all(windows, test)))]
    {
        let _ = window;
        cx.hide();
    }
}

fn record_window_visibility(window: &Window, visible: bool, cx: &mut App) {
    if let Some(workspace) = crate::slots::window_workspace_for(window, cx) {
        workspace.update(cx, |workspace, cx| {
            workspace.set_window_hidden(!visible, cx)
        });
    }
}

/// Update engine polling from native visibility, independently of focus.
pub fn sync_window_visibility(window: &Window, cx: &mut App) {
    #[cfg(target_os = "macos")]
    let visible = monocode_platform::macos_panel::workspace_is_visible(window).ok();
    #[cfg(all(windows, not(test)))]
    let visible = monocode_platform::windows::workspace_is_visible(window).ok();
    #[cfg(any(not(any(target_os = "macos", windows)), all(windows, test)))]
    let visible = None;
    if let Some(visible) = visible {
        record_window_visibility(window, visible, cx);
    }
}

/// Dock and tray reopen restores existing workspaces before creating one.
pub fn reopen(cx: &mut App) -> Result<(), String> {
    let windows = workspace_windows(cx);
    if windows.is_empty() {
        open_workspace_window(true, cx)?;
        cx.activate(true);
        return Ok(());
    }
    for handle in &windows {
        handle
            .update(cx, |_, window, cx| show_window(window, cx))
            .map_err(|error| error.to_string())??;
    }
    windows[0]
        .update(cx, |_, window, cx| bring_forward(window, cx))
        .map_err(|error| error.to_string())
}

pub fn install_tray(_cx: &mut App) {
    #[cfg(all(windows, not(test)))]
    {
        use monocode_platform::tray::{TrayCommand, TrayImage, TrayOptions};
        let cx = _cx;
        if cx.has_global::<AppTray>() {
            return;
        }
        let icon = match TrayImage::from_png(include_bytes!("../../../packaging/assets/icon.png")) {
            Ok(icon) => icon,
            Err(error) => {
                log::error!("Could not load tray icon: {error}");
                return;
            }
        };
        let (commands, received) = async_channel::unbounded();
        match monocode_platform::tray::install(
            TrayOptions {
                icon: Some(icon),
                ..TrayOptions::default()
            },
            move |command| {
                let _ = commands.try_send(command);
            },
        ) {
            Ok(Some(tray)) => cx.set_global(AppTray { _tray: tray }),
            Ok(None) => return,
            Err(error) => {
                log::error!("Could not install tray: {error}");
                return;
            }
        }
        cx.spawn(async move |cx| {
            while let Ok(command) = received.recv().await {
                cx.update(|cx| match command {
                    TrayCommand::Show => {
                        if let Err(error) = reopen(cx) {
                            log::error!("Could not reopen window: {error}");
                        }
                    }
                    TrayCommand::Quit => super::keymap::request_quit(cx),
                });
            }
        })
        .detach();
    }
}

pub fn window_label(id: gpui::WindowId) -> String {
    format!("native-{id:?}")
}

pub fn open_workspace_window(reveal: bool, cx: &mut App) -> Result<String, String> {
    let options = session_window_options(reveal, cx);
    let window = cx
        .open_window(options, |window, cx| {
            crate::glass::sync_window(window, cx);
            install_lifecycle(window, cx);
            let mut options = ShellOptions::saved(cx);
            options.start = WorkspaceStart::Fresh { project: None };
            let shell = cx.new(|cx| Shell::new(options, window, cx));
            cx.new(|cx| Root::new(shell, window, cx).bg(gpui::transparent_black()))
        })
        .map_err(|error| error.to_string())?;
    Ok(window_label(window.window_id()))
}

fn session_window_options(reveal: bool, cx: &App) -> WindowOptions {
    let mut options = window_options(size(px(1280.), px(800.)), cx);
    options.focus = reveal;
    options.show = reveal;
    options
}

pub fn install_lifecycle(window: &Window, cx: &mut App) {
    cx.default_global::<WorkspaceWindows>()
        .0
        .insert(window.window_handle().window_id());
    if monocode_engine::runtime::Engine::try_global(cx).is_none() {
        return;
    }
    window.on_window_should_close(cx, |_, cx| {
        let lifecycle = monocode_engine::runtime::Engine::lifecycle(cx);
        if lifecycle.read(cx).is_quitting() {
            return true;
        }
        let close_to_tray =
            monocode_app::boot::AppServices::try_global(cx).is_some_and(|services| {
                monocode_settings::settings_store::load_close_to_tray(
                    &services.kv,
                    monocode_core::Platform::current(),
                )
            });
        lifecycle
            .update(cx, |lifecycle, cx| {
                lifecycle.handle_close_requested(close_to_tray, cx)
            })
            .detach();
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, IntoElement, Render, TestAppContext, div};

    struct TestWindow;
    impl Render for TestWindow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn reopen_targets_workspaces_and_excludes_auxiliary_windows(cx: &mut TestAppContext) {
        cx.skip_drawing();
        cx.update(init);
        let first = cx.add_window(|window, cx| {
            install_lifecycle(window, cx);
            TestWindow
        });
        let panel = cx.add_window(|_, _| TestWindow);
        let second = cx.add_window(|window, cx| {
            install_lifecycle(window, cx);
            TestWindow
        });
        let ids = cx.update(|cx| {
            workspace_windows(cx)
                .iter()
                .map(|window| window.window_id())
                .collect::<Vec<_>>()
        });
        assert_eq!(ids, vec![first.window_id(), second.window_id()]);
        assert!(!ids.contains(&panel.window_id()));
        first
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        second
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        assert!(cx.update(|cx| workspace_windows(cx).is_empty()));
        assert!(cx.update(|cx| {
            cx.windows()
                .iter()
                .any(|window| window.window_id() == panel.window_id())
        }));
    }

    #[gpui::test]
    fn background_launch_windows_start_hidden_and_unfocused(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            let background = session_window_options(false, cx);
            assert!(!background.show);
            assert!(!background.focus);
            let foreground = session_window_options(true, cx);
            assert!(foreground.show);
            assert!(foreground.focus);
        });
    }

    #[gpui::test]
    fn visibility_updates_foreground_and_all_window_polling(cx: &mut TestAppContext) {
        use monocode_engine::runtime::{Engine, testing::init_test_engine};
        use monocode_engine::workspace::{Workspace, WorkspaceConfig, WorkspaceSetup};

        cx.skip_drawing();
        init_test_engine(cx);
        cx.update(|cx| {
            monocode_engine::workspace::init(
                WorkspaceSetup {
                    terminals: false,
                    ..WorkspaceSetup::default()
                },
                cx,
            );
        });
        let first_workspace =
            cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/first")), cx));
        let second_workspace =
            cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/second")), cx));
        let session = first_workspace.read_with(cx, |workspace, _| {
            workspace.active_tab().unwrap().focused_id.clone()
        });
        let first = cx.add_window(|window, cx| {
            crate::slots::register_workspace(first_workspace.clone(), window, cx);
            TestWindow
        });
        let second = cx.add_window(|window, cx| {
            crate::slots::register_workspace(second_workspace.clone(), window, cx);
            TestWindow
        });
        let hooks = cx.update(|cx| Engine::hooks(cx).workspace.clone());
        assert!(cx.update(|cx| hooks.is_foreground(&session, cx)));
        assert!(!cx.update(|cx| hooks.window_hidden(cx)));
        first
            .update(cx, |_, window, cx| {
                record_window_visibility(window, false, cx)
            })
            .unwrap();
        assert!(!cx.update(|cx| hooks.is_foreground(&session, cx)));
        assert!(!cx.update(|cx| hooks.window_hidden(cx)));
        second
            .update(cx, |_, window, cx| {
                record_window_visibility(window, false, cx)
            })
            .unwrap();
        assert!(cx.update(|cx| hooks.window_hidden(cx)));
        first
            .update(cx, |_, window, cx| {
                record_window_visibility(window, true, cx)
            })
            .unwrap();
        assert!(cx.update(|cx| hooks.is_foreground(&session, cx)));
        assert!(!cx.update(|cx| hooks.window_hidden(cx)));
    }
}
