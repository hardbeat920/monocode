//! Spotlight-style composer: a global shortcut floats a small panel over
//! whatever app is in front, and its prompt starts a session in a workspace
//! window without bringing that window forward.
//!
//! The panel is a WKWebView window re-classed as a non-activating `NSPanel`.
//! A plain `NSWindow` would have to activate MonoCode to take keys, which pulls
//! the workspace window over the browser and leaves focus with MonoCode after
//! the panel hides. A non-activating panel takes keys while the other app
//! stays active, and it can join a full-screen Space.

mod delivery;
pub mod git_popup;
pub mod screenshots;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};

use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSPanel, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::NSObjectProtocol;
use serde::{Deserialize, Serialize};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{
    GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::window::{workspace_windows, QUICK_COMPOSER_LABEL};

const WIDTH: f64 = 680.0;
const INITIAL_HEIGHT: f64 = 128.0;
/// Must match the card's CSS radius so the blur, border, and native shadow
/// share one outline.
const CORNER_RADIUS: f64 = 16.0;
const MAX_HEIGHT: f64 = 520.0;
/// Down from the top of the screen's work area, like Spotlight.
const TOP_FRACTION: f64 = 0.22;
const MAX_PROMPT_BYTES: usize = 256 * 1024;

/// Workspace windows listen for this, then take the request.
const LAUNCH: &str = "quick_composer_launch";
/// The panel refreshes its projects and focuses the prompt on every show.
const SHOWN: &str = "quick_composer_shown";
const DEFAULT_SHORTCUT: &str = "Command+Shift+Space";

fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    let shortcut = Shortcut::from_str(value).map_err(|err| err.to_string())?;
    if !shortcut
        .mods
        .intersects(Modifiers::SUPER | Modifiers::CONTROL)
    {
        return Err("Use Command or Control with another key.".into());
    }
    Ok(shortcut)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLaunch {
    prompt: String,
    cwd: String,
    harness: String,
    /// Absent means the harness's default model.
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    model_settings: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worktree_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worktree_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<QuickAttachment>,
    /// Bring the new session's window forward instead of starting it quietly.
    reveal: bool,
}

/// Only disk-backed attachments cross webviews, never webview-local blob URLs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuickAttachment {
    id: String,
    name: String,
    mime_type: String,
    kind: String,
    size: u64,
    path: String,
}

#[derive(Default)]
pub struct QuickComposerState {
    pending: Mutex<delivery::LaunchQueue>,
    capturing: AtomicBool,
    shortcut: Mutex<Option<Shortcut>>,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    app.manage(QuickComposerState::default());
    app.manage(git_popup::PopupState::default());
    app.manage(screenshots::Captures::default());
    std::thread::spawn(screenshots::cleanup_abandoned);
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event: ShortcutEvent| {
                if event.state == ShortcutState::Pressed {
                    // The handler runs inside a Carbon event callback, where
                    // `run_on_main_thread` would run inline and any panic
                    // aborts on the C frame. Hop off the thread so the work is
                    // queued onto the event loop like any other task.
                    let app = app.clone();
                    std::thread::spawn(move || toggle(&app));
                }
            })
            .build(),
    )?;
    Ok(())
}

/// The workspace calls this after painting, during idle time. Wry activates
/// the application on webview creation, so recheck native focus at execution
/// time: the user may have switched apps since the frontend scheduled it.
#[tauri::command]
pub async fn quick_composer_prepare(app: AppHandle, window: WebviewWindow) -> Result<bool, String> {
    if !crate::window::is_workspace_window(window.label()) {
        return Err("Only a workspace can prepare the composer.".into());
    }
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            if handle.get_webview_window(QUICK_COMPOSER_LABEL).is_some()
                && handle
                    .get_webview_window(crate::window::QUICK_COMPOSER_GIT_LABEL)
                    .is_some()
            {
                return Ok(true);
            }
            let Some(mtm) = MainThreadMarker::new() else {
                return Ok(false);
            };
            if !NSApplication::sharedApplication(mtm).isActive()
                || !window.is_visible().unwrap_or(false)
                || !window.is_focused().unwrap_or(false)
            {
                return Ok(false);
            }
            prepare(&handle)
                .map(|()| true)
                .map_err(|err| err.to_string())
        })();
        let _ = tx.try_send(result);
    })
    .map_err(|err| err.to_string())?;
    rx.recv()
        .await
        .ok_or_else(|| "Composer preparation was interrupted.".to_string())?
}

fn prepare(app: &AppHandle) -> tauri::Result<()> {
    let panel = match app.get_webview_window(QUICK_COMPOSER_LABEL) {
        Some(panel) => panel,
        None => build(app)?,
    };
    git_popup::prepare(app, &panel)?;
    Ok(())
}

/// Workspace windows own the setting, so the shortcut is only claimed once
/// one has read it. Idempotent: every window reports on boot.
#[tauri::command]
pub fn quick_composer_set_enabled(
    app: AppHandle,
    enabled: bool,
    shortcut: Option<String>,
) -> Result<(), String> {
    let next = parse_shortcut(shortcut.as_deref().unwrap_or(DEFAULT_SHORTCUT))?;
    let shortcuts = app.global_shortcut();
    let state = app.state::<QuickComposerState>();
    let mut current = state.shortcut.lock().map_err(|err| err.to_string())?;
    if enabled {
        if *current == Some(next) {
            return Ok(());
        }
        // Claim the replacement first. If the OS rejects it, keep the old
        // shortcut active so a failed edit cannot strand the composer.
        shortcuts
            .register(next)
            .map_err(|err| format!("Could not claim {next}: {err}"))?;
        if let Some(previous) = *current {
            if let Err(err) = shortcuts.unregister(previous) {
                let _ = shortcuts.unregister(next);
                return Err(format!("Could not release {previous}: {err}"));
            }
        }
        *current = Some(next);
    } else if let Some(previous) = *current {
        shortcuts
            .unregister(previous)
            .map_err(|err| err.to_string())?;
        *current = None;
        if let Some(panel) = app.get_webview_window(QUICK_COMPOSER_LABEL) {
            git_popup::dismiss(&app, false);
            let _ = panel.hide();
        }
    }
    Ok(())
}

/// Grow or shrink to the card, keeping the top edge where it was so the
/// prompt does not jump while the project list opens.
#[tauri::command]
pub fn quick_composer_fit(window: WebviewWindow, height: f64) -> Result<(), String> {
    if window.label() != QUICK_COMPOSER_LABEL || !height.is_finite() {
        return Ok(());
    }
    let panel = window.clone();
    window
        .run_on_main_thread(move || {
            let Some(ns_window) = crate::macos::ns_window(&panel) else {
                return;
            };
            // AppKit's origin is at the bottom left. Change size and origin in
            // one frame update so resizing never briefly moves the top edge.
            let mut frame = ns_window.frame();
            let height = height.clamp(1.0, MAX_HEIGHT);
            frame.origin.y += frame.size.height - height;
            frame.size.width = WIDTH;
            frame.size.height = height;
            ns_window.setFrame_display(frame, false);
            ns_window.invalidateShadow();
            ns_window.displayIfNeeded();
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn quick_composer_submit(
    app: AppHandle,
    window: WebviewWindow,
    mut request: QuickLaunch,
) -> Result<(), String> {
    if window.label() != QUICK_COMPOSER_LABEL {
        return Err("Only the quick composer can start quick sessions.".into());
    }
    if request.prompt.trim().is_empty() && request.attachments.is_empty() {
        return Err("Write a prompt first.".into());
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err("That prompt is too long for the quick composer.".into());
    }
    if request.cwd.trim().is_empty() {
        return Err("Pick a project first.".into());
    }

    validate_workspace(&request)?;
    validate_attachments(&request.attachments, &request.harness)?;
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let state = handle.state::<QuickComposerState>();
            if !state
                .pending
                .lock()
                .map_err(|err| err.to_string())?
                .has_capacity()
            {
                return Err(
                    "Too many sessions are waiting to start. Please try again shortly.".into(),
                );
            }
            // Window creation and enqueueing are serialized on the main thread:
            // a second submission sees the first submission's mounting window.
            let target = target_or_create(launch_target(&handle), request.reveal, |reveal| {
                crate::window::open_session_window(&handle, reveal)
            })?;
            screenshots::persist(&handle, &mut request.attachments)?;
            let reveal = request.reveal;
            state
                .pending
                .lock()
                .map_err(|err| err.to_string())?
                .push(request, target.label().to_string());
            git_popup::dismiss(&handle, false);
            let _ = window.hide();
            if reveal {
                let _ = target.unminimize();
                let _ = target.show();
                let _ = target.set_focus();
            }
            let _ = target.emit(LAUNCH, ());
            Ok(())
        })();
        let _ = tx.try_send(result);
    })
    .map_err(|err| err.to_string())?;
    rx.recv()
        .await
        .ok_or_else(|| "Session submission was interrupted.".to_string())?
}

fn validate_workspace(request: &QuickLaunch) -> Result<(), String> {
    if request
        .workspace_mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "current" | "worktree"))
        || request.worktree_base.as_deref().is_some_and(|base| {
            base.trim().is_empty() || request.workspace_mode.as_deref() != Some("worktree")
        })
        || (request.workspace_mode.as_deref() == Some("worktree") && request.worktree_cwd.is_some())
    {
        return Err("Select a valid workspace for this session.".into());
    }
    if let Some(path) = &request.worktree_cwd {
        if path.trim().is_empty() || !std::path::Path::new(path).is_dir() {
            return Err(
                "This worktree is no longer available. Select another working copy.".into(),
            );
        }
    }
    Ok(())
}

fn validate_attachments(files: &[QuickAttachment], harness: &str) -> Result<(), String> {
    if files.len() > 20 {
        return Err("You can attach up to 20 files.".into());
    }
    if !files.is_empty() && harness == "fx" {
        return Err("This provider does not support attachments.".into());
    }
    for file in files {
        if file.id.is_empty()
            || file.name.is_empty()
            || file.mime_type.is_empty()
            || !matches!(file.kind.as_str(), "image" | "audio" | "file")
            || !std::path::Path::new(&file.path).is_file()
        {
            return Err(format!("Could not read attachment: {}", file.name));
        }
    }
    Ok(())
}

/// User-initiated region/window capture. Cancellation leaves the draft intact.
#[tauri::command]
pub async fn quick_composer_capture(
    window: WebviewWindow,
    state: State<'_, QuickComposerState>,
) -> Result<Option<String>, String> {
    if window.label() != QUICK_COMPOSER_LABEL {
        return Err("Only the quick composer can capture an attachment.".into());
    }
    if state.capturing.swap(true, Ordering::SeqCst) {
        return Err("A screenshot is already in progress.".into());
    }
    git_popup::dismiss(window.app_handle(), false);
    if let Err(err) = window.hide() {
        state.capturing.store(false, Ordering::SeqCst);
        return Err(err.to_string());
    }
    let result = tauri::async_runtime::spawn_blocking(capture_screenshot)
        .await
        .map_err(|err| err.to_string())
        .and_then(|result| result);
    if let Ok(Some(path)) = &result {
        screenshots::register(window.app_handle(), path);
    }
    let panel = window.clone();
    let restored = window.run_on_main_thread(move || present(&panel));
    state.capturing.store(false, Ordering::SeqCst);
    if let Err(err) = restored {
        if let Ok(Some(path)) = &result {
            screenshots::discard(window.app_handle(), path);
        }
        return Err(err.to_string());
    }
    result
}

fn capture_screenshot() -> Result<Option<String>, String> {
    let path = screenshots::new_path()?;
    let dir = path.parent().ok_or("Missing capture directory")?;
    // Let WindowServer remove the panel before the system capture overlay appears.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let result = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-t", "png"])
        .arg(&path)
        .output();
    if path.is_file() {
        return Ok(Some(path.to_string_lossy().into_owned()));
    }
    let _ = std::fs::remove_dir(dir);
    let output = result.map_err(|err| format!("Could not take a screenshot: {err}"))?;
    let error = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && !error.trim().is_empty() {
        return Err(format!("Could not take a screenshot: {}", error.trim()));
    }
    Ok(None)
}

/// Reading claims a launch without removing it. Failed parsing or handoff can
/// retry the same ID; only the owning workspace's acknowledgement removes it.
#[tauri::command]
pub fn quick_composer_take(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, QuickComposerState>,
) -> Result<Option<delivery::Delivery>, String> {
    if !crate::window::is_workspace_window(window.label()) {
        return Ok(None);
    }
    Ok(state
        .pending
        .lock()
        .map_err(|err| err.to_string())?
        .claim(window.label(), |label| {
            app.get_webview_window(label).is_some()
        }))
}

#[tauri::command]
pub fn quick_composer_ack(
    window: WebviewWindow,
    state: State<'_, QuickComposerState>,
    id: String,
) -> Result<(), String> {
    if !crate::window::is_workspace_window(window.label()) {
        return Err("Only a workspace can acknowledge a session.".into());
    }
    state
        .pending
        .lock()
        .map_err(|err| err.to_string())?
        .acknowledge(window.label(), &id);
    Ok(())
}

fn target_or_create<T>(
    existing: Option<T>,
    reveal: bool,
    create: impl FnOnce(bool) -> Result<T, String>,
) -> Result<T, String> {
    match existing {
        Some(window) => Ok(window),
        None => create(reveal),
    }
}

/// The window the user last looked at, else the first one. Hidden windows
/// count: close-to-dock keeps them running.
fn launch_target(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = workspace_windows(app);
    windows
        .iter()
        .find(|window| window.is_focused().unwrap_or(false))
        .or_else(|| {
            windows
                .iter()
                .find(|window| window.is_visible().unwrap_or(false))
        })
        .or(windows.first())
        .cloned()
}

fn toggle(app: &AppHandle) {
    if app
        .state::<QuickComposerState>()
        .capturing
        .load(Ordering::SeqCst)
    {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(panel) = handle.get_webview_window(QUICK_COMPOSER_LABEL) {
            if panel.is_visible().unwrap_or(false) {
                git_popup::dismiss(&handle, false);
                let _ = panel.hide();
                return;
            }
            show(&handle, &panel);
            return;
        }
        match build(&handle) {
            Ok(panel) => show(&handle, &panel),
            Err(err) => eprintln!("monocode: quick composer: {err}"),
        }
    });
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let panel = WebviewWindowBuilder::new(
        app,
        QUICK_COMPOSER_LABEL,
        WebviewUrl::App("quick-composer.html".into()),
    )
    .title("MonoCode")
    .inner_size(WIDTH, INITIAL_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    // The window is exactly the card, so the native shadow follows the
    // rounded blur instead of a CSS shadow being clipped at the window edge.
    .shadow(true)
    // Popover follows the window's appearance, which the page sets to the
    // app theme. Active keeps it vibrant while another app is frontmost.
    .effects(
        EffectsBuilder::new()
            .effect(Effect::Popover)
            .state(EffectState::Active)
            .radius(CORNER_RADIUS)
            .build(),
    )
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .build()?;

    make_panel(&panel);

    // Keep the drop target visible when users switch apps to collect files.
    // Escape, the close button, the shortcut, and submission dismiss it.
    Ok(panel)
}

fn show(app: &AppHandle, panel: &WebviewWindow) {
    place(app, panel);
    present(panel);
    let _ = panel.emit(SHOWN, ());
    if let Err(err) = git_popup::prepare(app, panel) {
        // Opening the picker retries; a failed preload must not block drafting.
        eprintln!("monocode: prepare git picker: {err}");
    }
}

/// Restore focus without resetting a draft or moving the panel after capture.
fn present(panel: &WebviewWindow) {
    match crate::macos::ns_window(panel) {
        Some(ns_window) if is_panel(&ns_window) => {
            ns_window.orderFrontRegardless();
            ns_window.makeKeyWindow();
        }
        // Could not become a panel: an ordinary window still works, it just
        // brings MonoCode forward with it.
        _ => {
            let _ = panel.show();
            let _ = panel.set_focus();
        }
    }
}

/// Center on the screen under the pointer, since that is where the user is.
fn place(app: &AppHandle, panel: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|point| app.monitor_from_point(point.x, point.y).ok().flatten())
        .or_else(|| panel.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        let _ = panel.center();
        return;
    };
    let area = monitor.work_area();
    let width = (WIDTH * monitor.scale_factor()).round() as i32;
    let x = area.position.x + (area.size.width as i32 - width) / 2;
    let y = area.position.y + (area.size.height as f64 * TOP_FRACTION).round() as i32;
    let _ = panel.set_position(PhysicalPosition::new(x, y));
}

/// A runtime `NSPanel` subclass rather than `define_class!`, because it has to
/// carry Tao's `focusable` ivar: objc2 only re-classes an object into a class
/// of exactly the same instance size, and Tao reads that ivar by name.
fn panel_class() -> Option<&'static AnyClass> {
    static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
    *CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new(c"MonoCodeQuickComposerPanel", NSPanel::class())?;
        builder.add_ivar::<Bool>(c"focusable");
        unsafe {
            // Borderless windows refuse key status by default, and the prompt
            // needs keys.
            builder.add_method(
                sel!(canBecomeKeyWindow),
                can_become_key_window as extern "C-unwind" fn(_, _) -> _,
            );
            // Never main: that is what would pull the workspace window forward.
            builder.add_method(
                sel!(canBecomeMainWindow),
                can_become_main_window as extern "C-unwind" fn(_, _) -> _,
            );
        }
        Some(builder.register())
    })
}

extern "C-unwind" fn can_become_key_window(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

extern "C-unwind" fn can_become_main_window(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
}

fn is_panel(ns_window: &NSWindow) -> bool {
    panel_class().is_some_and(|class| ns_window.isKindOfClass(class))
}

/// Swap Tao's window class for a non-activating panel subclass in place, the
/// way tauri-nspanel does. Tao and wry keep their delegate and views.
fn make_panel(window: &WebviewWindow) {
    let Some(ns_window) = crate::macos::ns_window(window) else {
        return;
    };
    let Some(panel_class) = panel_class() else {
        return;
    };
    // The object was allocated for Tao's class. Re-classing into a different
    // size is undefined behavior (and objc2 panics on it), so fall back to an
    // ordinary window if a Tao update ever changes its layout.
    if panel_class.instance_size() != ns_window.class().instance_size() {
        eprintln!("monocode: quick composer: panel class does not match Tao's window size");
        return;
    }
    unsafe {
        let object: &AnyObject = ns_window.as_ref();
        AnyObject::set_class(object, panel_class);
    }
    let Some(panel) = ns_window.downcast_ref::<NSPanel>() else {
        return;
    };
    panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    // Setting the mask after creation does not update WindowServer's
    // activation tag on its own; this private setter does.
    let prevents = sel!(_setPreventsActivation:);
    let responds: bool = unsafe { msg_send![panel, respondsToSelector: prevents] };
    if responds {
        unsafe {
            let _: () = msg_send![panel, _setPreventsActivation: Bool::YES];
        }
    }
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setHidesOnDeactivate(false);
    panel.setLevel(NSStatusWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        parse_shortcut, target_or_create, validate_attachments, validate_workspace, QuickLaunch,
        DEFAULT_SHORTCUT,
    };
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    #[test]
    fn quick_composer_shortcut_requires_a_non_shift_modifier() {
        let original = parse_shortcut(DEFAULT_SHORTCUT).unwrap();
        assert_eq!(original.key, Code::Space);
        assert_eq!(original.mods, Modifiers::SUPER | Modifiers::SHIFT);
        let custom = parse_shortcut("Control+Option+KeyK").unwrap();
        assert_eq!(custom.key, Code::KeyK);
        assert_eq!(custom.mods, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(
            parse_shortcut("Command+KeyK").unwrap().mods,
            Modifiers::SUPER
        );
        assert_eq!(
            parse_shortcut("Control+KeyK").unwrap().mods,
            Modifiers::CONTROL
        );
        assert!(parse_shortcut("Shift+Space").is_err());
        assert!(parse_shortcut("Option+KeyK").is_err());
        assert!(parse_shortcut("Command+InvalidKey").is_err());
    }

    #[test]
    fn no_workspace_return_requests_hidden_creation_and_cmd_return_requests_reveal() {
        for reveal in [false, true] {
            let target = target_or_create(None, reveal, |visibility| {
                assert_eq!(visibility, reveal);
                Ok("new-workspace")
            })
            .unwrap();
            assert_eq!(target, "new-workspace");
        }
        let existing = target_or_create(Some("mounting-workspace"), false, |_| {
            panic!("A second submission must reuse the mounting workspace");
        })
        .unwrap();
        assert_eq!(existing, "mounting-workspace");
    }

    #[test]
    fn launch_preserves_model_settings_across_windows() {
        let input = serde_json::json!({
            "prompt": "fix the test",
            "cwd": "/tmp/project",
            "harness": "claude",
            "model": "claude-opus",
            "modelSettings": { "effort": "high", "fast": "true" },
            "runtimeMode": "auto-accept-edits",
            "reveal": false
        });
        let request: QuickLaunch = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(request.model_settings.get("effort").unwrap(), "high");
        assert_eq!(serde_json::to_value(request).unwrap(), input);
    }

    #[test]
    fn attachments_round_trip_without_webview_local_data() {
        let input = serde_json::json!({
            "prompt": "", "cwd": "/tmp/project", "harness": "codex", "model": null,
            "modelSettings": {}, "reveal": false,
            "attachments": [{ "id": "shot", "name": "Screenshot.png", "mimeType": "image/png",
                "kind": "image", "size": 4, "path": "/tmp/Screenshot.png" }]
        });
        let request: QuickLaunch = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(request.attachments.len(), 1);
        assert_eq!(serde_json::to_value(request).unwrap(), input);
    }

    #[test]
    fn missing_files_and_unsupported_providers_cannot_launch() {
        let request: QuickLaunch = serde_json::from_value(serde_json::json!({
            "prompt": "look", "cwd": "/tmp/project", "harness": "codex", "reveal": false,
            "attachments": [{ "id": "shot", "name": "Screenshot.png", "mimeType": "image/png",
                "kind": "image", "size": 4, "path": "/missing/monocode-test.png" }]
        }))
        .unwrap();
        assert!(validate_attachments(&request.attachments, "codex").is_err());
        assert!(validate_attachments(&request.attachments, "fx").is_err());
        assert!(validate_attachments(&vec![request.attachments[0].clone(); 21], "codex").is_err());
        assert!(validate_attachments(&[], "codex").is_ok());
    }

    #[test]
    fn workspace_choices_round_trip_and_conflicts_are_rejected() {
        let input = serde_json::json!({
            "prompt": "fix it", "cwd": "/tmp/project", "harness": "codex", "model": null,
            "modelSettings": {}, "reveal": false,
            "workspaceMode": "worktree", "worktreeBase": "origin/develop"
        });
        let mut request: QuickLaunch = serde_json::from_value(input.clone()).unwrap();
        assert!(validate_workspace(&request).is_ok());
        assert_eq!(serde_json::to_value(&request).unwrap(), input);
        request.worktree_cwd = Some("/tmp/existing".into());
        assert!(validate_workspace(&request).is_err());
        request.workspace_mode = None;
        assert!(validate_workspace(&request).is_err());
        request.worktree_base = None;
        request.worktree_cwd = Some("/missing/monocode-worktree-test".into());
        assert!(validate_workspace(&request).is_err());
        request.worktree_cwd = Some(std::env::temp_dir().to_string_lossy().into_owned());
        assert!(validate_workspace(&request).is_ok());
    }

    #[test]
    fn older_launches_without_settings_remain_valid() {
        let request: QuickLaunch = serde_json::from_value(serde_json::json!({
            "prompt": "fix the test", "cwd": "/tmp/project", "harness": "claude", "reveal": false
        }))
        .unwrap();
        assert!(request.model_settings.is_empty());
        assert!(request.runtime_mode.is_none());
    }
}
