use std::sync::Mutex;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventType};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde::{Deserialize, Serialize};
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use crate::window::{QUICK_COMPOSER_GIT_LABEL, QUICK_COMPOSER_LABEL};

const WIDTH: f64 = 320.0;
const MAX_HEIGHT: f64 = 520.0;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PickerKind {
    Workspace,
    Base,
    Branch,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Anchor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Request {
    id: String,
    kind: PickerKind,
    choice: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branches: Option<serde_json::Value>,
    anchor: Anchor,
}

#[derive(Default)]
pub struct PopupState(Mutex<Option<Request>>, Mutex<Option<String>>);

#[tauri::command]
pub async fn quick_git_open(
    app: AppHandle,
    window: WebviewWindow,
    request: Request,
) -> Result<(), String> {
    if window.label() != QUICK_COMPOSER_LABEL {
        return Err("Only the quick composer can open this picker.".into());
    }
    if request.id.is_empty()
        || ![
            request.anchor.x,
            request.anchor.y,
            request.anchor.width,
            request.anchor.height,
        ]
        .iter()
        .all(|n| n.is_finite())
        || request
            .choice
            .get("cwd")
            .and_then(|v| v.as_str())
            .is_none_or(str::is_empty)
    {
        return Err("Invalid picker request.".into());
    }
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            if !window.is_visible().unwrap_or(false) {
                return Err("The composer is no longer visible.".into());
            }
            let popup = prepare(&handle, &window).map_err(|err| err.to_string())?;
            *handle
                .state::<PopupState>()
                .0
                .lock()
                .map_err(|err| err.to_string())? = Some(request.clone());
            *handle
                .state::<PopupState>()
                .1
                .lock()
                .map_err(|err| err.to_string())? = None;
            // A newly booting webview retrieves the same request with quick_git_state.
            popup
                .emit("quick_git_request", request)
                .map_err(|err| err.to_string())
        })();
        let _ = tx.try_send(result);
    })
    .map_err(|err| err.to_string())?;
    rx.recv()
        .await
        .ok_or_else(|| "Picker window did not open.".to_string())?
}

#[tauri::command]
pub fn quick_git_state(
    window: WebviewWindow,
    state: State<'_, PopupState>,
) -> Result<Option<Request>, String> {
    if window.label() != QUICK_COMPOSER_GIT_LABEL {
        return Err("Only the picker can read its state.".into());
    }
    Ok(state.0.lock().map_err(|err| err.to_string())?.clone())
}

/// Resize only the popup. The composer's frame and native glass never grow.
#[tauri::command]
pub fn quick_git_fit(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    height: f64,
) -> Result<(), String> {
    if window.label() != QUICK_COMPOSER_GIT_LABEL || !height.is_finite() {
        return Ok(());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let state = handle.state::<PopupState>();
        let request = state.0.lock().ok().and_then(|state| state.clone());
        let Some(request) = request.filter(|request| request.id == id) else {
            return;
        };
        let Some(parent) = handle.get_webview_window(QUICK_COMPOSER_LABEL) else {
            return;
        };
        if !parent.is_visible().unwrap_or(false) {
            dismiss(&handle, false);
            return;
        }
        let (Some(parent_ns), Some(popup_ns)) = (
            crate::macos::ns_window(&parent),
            crate::macos::ns_window(&window),
        ) else {
            return;
        };
        let parent_frame = parent_ns.frame();
        let screen = parent_ns
            .screen()
            .map(|screen| screen.visibleFrame())
            .unwrap_or(parent_frame);
        let frame = popup_frame(parent_frame, screen, &request.anchor, height);
        popup_ns.setFrame_display(frame, false);
        popup_ns.invalidateShadow();
        popup_ns.displayIfNeeded();
        // Focus once for each opening, including reuse of an already visible panel.
        // Later content resizes must not steal focus.
        let should_present = state.1.lock().is_ok_and(|mut presented| {
            if presented.as_deref() == Some(id.as_str()) {
                return false;
            }
            *presented = Some(id);
            true
        });
        if should_present {
            super::present(&window);
        }
    })
    .map_err(|err| err.to_string())
}

fn popup_frame(parent: NSRect, screen: NSRect, anchor: &Anchor, height: f64) -> NSRect {
    let width = WIDTH.min(screen.size.width);
    let height = height.clamp(1.0, MAX_HEIGHT).min(screen.size.height);
    let x = (parent.origin.x + anchor.x)
        .clamp(screen.origin.x, screen.origin.x + screen.size.width - width);
    let anchor_top = parent.origin.y + parent.size.height - anchor.y;
    let below = anchor_top - anchor.height - 6.0 - height;
    let above = anchor_top + 6.0;
    let y = if below >= screen.origin.y {
        below
    } else {
        above
    };
    let y = y.clamp(
        screen.origin.y,
        screen.origin.y + screen.size.height - height,
    );
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

#[tauri::command]
pub fn quick_git_complete(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    choice: Option<serde_json::Value>,
    restore_focus: bool,
) -> Result<(), String> {
    if ![QUICK_COMPOSER_LABEL, QUICK_COMPOSER_GIT_LABEL].contains(&window.label()) {
        return Err("Invalid picker window.".into());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        complete(&handle, Some(&id), choice, restore_focus, false);
    })
    .map_err(|err| err.to_string())
}

fn complete(
    app: &AppHandle,
    id: Option<&str>,
    choice: Option<serde_json::Value>,
    restore_focus: bool,
    blur: bool,
) {
    let state = app.state::<PopupState>();
    let request = {
        let Ok(mut state) = state.0.lock() else {
            return;
        };
        if id.is_some_and(|id| state.as_ref().is_none_or(|request| request.id != id)) {
            return;
        }
        state.take()
    };
    if let Ok(mut presented) = state.1.lock() {
        *presented = None;
    }
    if let Some(popup) = app.get_webview_window(QUICK_COMPOSER_GIT_LABEL) {
        let _ = popup.hide();
    }
    if let (Some(request), Some(parent)) = (request, app.get_webview_window(QUICK_COMPOSER_LABEL)) {
        if restore_focus && parent.is_visible().unwrap_or(false) {
            super::present(&parent);
        }
        let trigger_click = blur
            && MainThreadMarker::new().is_some_and(|mtm| {
                NSApplication::sharedApplication(mtm)
                    .currentEvent()
                    .is_some_and(|event| event.r#type() == NSEventType::LeftMouseDown)
                    && crate::macos::ns_window(&parent).is_some_and(|window| {
                        over_trigger(window.frame(), &request.anchor, NSEvent::mouseLocation())
                    })
            });
        let _ = parent.emit(
            "quick_git_result",
            serde_json::json!({ "id": request.id, "choice": choice, "restoreFocus": restore_focus,
            "triggerKind": if trigger_click { Some(request.kind) } else { None } }),
        );
    }
}

fn over_trigger(parent: NSRect, anchor: &Anchor, point: NSPoint) -> bool {
    let left = parent.origin.x + anchor.x;
    let top = parent.origin.y + parent.size.height - anchor.y;
    point.x >= left
        && point.x <= left + anchor.width
        && point.y <= top
        && point.y >= top - anchor.height
}

pub(super) fn dismiss(app: &AppHandle, restore_focus: bool) {
    complete(app, None, None, restore_focus, false);
}

#[tauri::command]
pub fn quick_composer_dismiss(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.label() != QUICK_COMPOSER_LABEL {
        return Err("Invalid composer window.".into());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        dismiss(&handle, false);
        let _ = window.hide();
    })
    .map_err(|err| err.to_string())
}

/// Boot the hidden webview after the workspace paints, before the first picker click.
/// The nonactivating panel is presented only after content is measured.
pub(super) fn prepare(app: &AppHandle, parent: &WebviewWindow) -> tauri::Result<WebviewWindow> {
    if let Some(popup) = app.get_webview_window(QUICK_COMPOSER_GIT_LABEL) {
        return Ok(popup);
    }
    let popup = WebviewWindowBuilder::new(
        app,
        QUICK_COMPOSER_GIT_LABEL,
        WebviewUrl::App("quick-composer.html?popup=git".into()),
    )
    .title("Choose workspace")
    .inner_size(WIDTH, 1.0)
    .decorations(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .effects(
        EffectsBuilder::new()
            .effect(Effect::Popover)
            .state(EffectState::Active)
            .radius(12.0)
            .build(),
    )
    .parent(parent)?
    .build()?;
    super::make_panel(&popup);
    // Attaching an AppKit child can affect its ordering even when the builder
    // requested invisibility. Keep the preloaded panel out until its first fit.
    popup.hide()?;
    let handle = app.clone();
    let blurred = popup.clone();
    popup.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(false))
            && handle
                .state::<PopupState>()
                .1
                .lock()
                .is_ok_and(|presented| presented.is_some())
            && blurred.is_visible().unwrap_or(false)
            && !blurred.is_focused().unwrap_or(false)
        {
            complete(&handle, None, None, false, true);
        }
    });
    Ok(popup)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blur_only_marks_a_click_inside_the_current_trigger() {
        let parent = NSRect::new(NSPoint::new(-600.0, 400.0), NSSize::new(680.0, 140.0));
        let anchor = Anchor {
            x: 30.0,
            y: 10.0,
            width: 100.0,
            height: 24.0,
        };
        assert!(over_trigger(parent, &anchor, NSPoint::new(-550.0, 520.0)));
        assert!(!over_trigger(parent, &anchor, NSPoint::new(-400.0, 520.0)));
        assert!(!over_trigger(parent, &anchor, NSPoint::new(-550.0, 490.0)));
    }

    #[test]
    fn request_preserves_the_composers_branch_snapshot() {
        let value = serde_json::json!({
            "id": "first-open", "kind": "workspace",
            "choice": { "cwd": "/repo", "mode": "current" },
            "branches": { "current": "main", "detached": false,
                "branches": [{ "name": "main", "current": true, "remote": null }] },
            "anchor": { "x": 10.0, "y": 10.0, "width": 100.0, "height": 24.0 }
        });
        let request: Request = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }
    #[test]
    fn menu_is_anchored_without_changing_the_composer_frame() {
        let parent = NSRect::new(NSPoint::new(200.0, 600.0), NSSize::new(680.0, 140.0));
        let screen = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));
        let anchor = Anchor {
            x: 120.0,
            y: 16.0,
            width: 70.0,
            height: 24.0,
        };
        let frame = popup_frame(parent, screen, &anchor, 280.0);
        assert_eq!(frame.origin, NSPoint::new(320.0, 414.0));
        assert_eq!(frame.size, NSSize::new(320.0, 280.0));
        assert_eq!(parent.size.height, 140.0);
    }
    #[test]
    fn popup_is_never_a_workspace_launch_target() {
        assert!(!crate::window::is_workspace_window(
            QUICK_COMPOSER_GIT_LABEL
        ));
        assert!(!crate::window::is_workspace_window(QUICK_COMPOSER_LABEL));
        assert!(crate::window::is_workspace_window("main"));
    }

    #[test]
    fn menu_flips_and_clamps_at_screen_edges() {
        let parent = NSRect::new(NSPoint::new(1000.0, 0.0), NSSize::new(680.0, 140.0));
        let screen = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));
        let frame = popup_frame(
            parent,
            screen,
            &Anchor {
                x: 500.0,
                y: 16.0,
                width: 70.0,
                height: 24.0,
            },
            300.0,
        );
        assert_eq!(frame.origin, NSPoint::new(1120.0, 130.0));
        let small = NSRect::new(NSPoint::new(-800.0, -400.0), NSSize::new(250.0, 200.0));
        let frame = popup_frame(
            parent,
            small,
            &Anchor {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            700.0,
        );
        assert_eq!(frame, small);
    }
}
