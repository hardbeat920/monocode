//! Trackpad pinch for in-app zoom surfaces such as the image lightbox.
//!
//! WKWebView keeps `allowsMagnification` off, and WebKit does not reliably turn a
//! pinch into DOM events for the page, so a surface that wants pinch-to-zoom asks
//! for it here. While a window has it enabled, its magnify events are swallowed
//! and forwarded to that window as `trackpad_magnify` with AppKit's incremental
//! magnification (`scale *= 1 + delta`).

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::{Mutex, Once, OnceLock};

use block2::RcBlock;
use objc2_app_kit::{NSEvent, NSEventMask};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

static APP: OnceLock<AppHandle> = OnceLock::new();
static INSTALL: Once = Once::new();
/// AppKit window number -> webview window label.
static ENABLED: Mutex<Option<HashMap<isize, String>>> = Mutex::new(None);

pub fn set_enabled(window: &WebviewWindow, enabled: bool) {
    let Some(ns_window) = crate::macos::ns_window(window) else {
        return;
    };
    let number = ns_window.windowNumber();
    let mut map = ENABLED.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    if enabled {
        map.insert(number, window.label().to_string());
    } else {
        map.remove(&number);
    }
    if enabled {
        let _ = APP.set(window.app_handle().clone());
        // AppKit monitors must be added on the main thread.
        let _ = window.run_on_main_thread(|| INSTALL.call_once(install_monitor));
    }
}

fn install_monitor() {
    let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        let ns_event = unsafe { event.as_ref() };
        let label = ENABLED
            .lock()
            .ok()
            .and_then(|map| map.as_ref()?.get(&ns_event.windowNumber()).cloned());
        let (Some(label), Some(app)) = (label, APP.get()) else {
            return event.as_ptr();
        };
        let _ = app.emit_to(label.as_str(), "trackpad_magnify", ns_event.magnification());
        std::ptr::null_mut()
    });
    // The monitor lives for the rest of the process, so the token is leaked.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::Magnify, &handler)
    };
    std::mem::forget(monitor);
}
