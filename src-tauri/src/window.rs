use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::window::Color;
#[cfg(target_os = "windows")]
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WebviewWindowBuilder};

static WINDOW_COUNTER: AtomicU32 = AtomicU32::new(1);
static ALLOW_EXIT: AtomicBool = AtomicBool::new(false);

const QUIT_REQUESTED: &str = "quit_requested";

pub fn open_new_window(app: &AppHandle) -> Result<(), String> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("missing main window config")?
        .clone();

    let id = WINDOW_COUNTER.fetch_add(1, Ordering::Relaxed);
    config.label = format!("window-{id}");

    let window = WebviewWindowBuilder::from_config(app, &config)
        .map_err(|err| err.to_string())?
        .build()
        .map_err(|err| err.to_string())?;

    #[cfg(target_os = "macos")]
    crate::macos::install(&window);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.set_decorations(false);
        let _ = window.set_shadow(true);
    }

    let _ = window.set_focus();
    Ok(())
}

/// Desktop blur goes on after the first UI paint, not during the dock bounce.
#[tauri::command]
pub fn enable_window_glass(window: WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_background_color(Some(Color(0, 0, 0, 3)));
        crate::macos::enable_glass(&window);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
        let _ = window.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).build());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = window;
    }
}

/// Close with a running chat hides the webview so the harness child keeps going.
#[tauri::command]
pub fn hide_window(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|err| err.to_string())
}

/// Finish an idle close. `destroy` skips CloseRequested so the JS handler
/// does not loop; `close` would fire it again.
#[tauri::command]
pub fn destroy_window(window: WebviewWindow) -> Result<(), String> {
    window.destroy().map_err(|err| err.to_string())
}

/// Dock click / Cmd-click with no visible windows: bring hidden ones back.
pub fn show_hidden_or_open_new(app: &AppHandle) -> Result<(), String> {
    let mut windows: Vec<WebviewWindow> = app.webview_windows().into_values().collect();
    if windows.is_empty() {
        return open_new_window(app);
    }
    windows.sort_by(|a, b| a.label().cmp(b.label()));
    for window in &windows {
        let _ = window.unminimize();
        let _ = window.show();
    }
    windows
        .first()
        .ok_or_else(|| "missing window".to_string())?
        .set_focus()
        .map_err(|err| err.to_string())
}

/// window-state can restore a window as hidden after a quit-while-hidden.
pub fn ensure_launch_window_visible(app: &AppHandle) {
    let windows: Vec<WebviewWindow> = app.webview_windows().into_values().collect();
    if windows.is_empty() {
        return;
    }
    let any_visible = windows
        .iter()
        .any(|window| window.is_visible().unwrap_or(false));
    if any_visible {
        return;
    }
    let _ = show_hidden_or_open_new(app);
}

pub fn allow_exit() -> bool {
    ALLOW_EXIT.load(Ordering::SeqCst)
}

/// Ask the UI to persist in-flight chats, then call `confirm_quit`.
pub fn request_quit(app: &AppHandle) {
    let windows = app.webview_windows();
    let target = windows
        .values()
        .find(|window| window.is_focused().unwrap_or(false))
        .cloned()
        .or_else(|| windows.get("main").cloned())
        .or_else(|| windows.values().next().cloned());
    match target {
        Some(window) => {
            if window.emit(QUIT_REQUESTED, ()).is_err() {
                confirm_quit(app.clone());
            }
        }
        None => confirm_quit(app.clone()),
    }
}

/// Persist already happened in JS. Show windows so window-state doesn't save hidden.
#[tauri::command]
pub fn confirm_quit(app: AppHandle) {
    ALLOW_EXIT.store(true, Ordering::SeqCst);
    for window in app.webview_windows().values() {
        let _ = window.show();
    }
    // Belt and braces. `RunEvent::Exit` reaps too, and it also runs before the
    // process is gone, but a macOS terminate that skips the run loop would not
    // reach it — and `kill_all`'s SIGKILL wait only works while we're alive.
    if let Some(host) = app.try_state::<crate::harness::HarnessHost>() {
        host.kill_all();
    }
    if let Some(host) = app.try_state::<crate::pty::PtyHost>() {
        host.kill_all();
    }
    app.exit(0);
}
