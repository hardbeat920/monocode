//! src-tauri/src/macos.rs's traffic lights: 14 pt buttons, 6 pt apart, 12 pt
//! from the left edge. GPUI's `traffic_light_position` only moves the row and
//! keeps AppKit's own spacing, which on recent macOS crowds the sidebar
//! toggle. Auto Layout pins the buttons, so GPUI's frame updates on resize,
//! focus and fullscreen don't move them back.
//!
//! The buttons hang from the titlebar container's top, which GPUI keeps at the
//! window's top. GPUI sizes the container from the live button frame plus its
//! own inset, so on macOS versions whose buttons report a larger frame the
//! container grows and anything measured from its bottom sinks below the bar.

use gpui::Window;
use objc2_app_kit::{NSLayoutAttribute, NSView, NSWindow, NSWindowButton};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const BUTTON_SIZE: f64 = 14.;
const LEFT_MARGIN: f64 = 12.;
const BUTTON_SPACING: f64 = 6.;

/// Pins the buttons, or moves already pinned ones; AppKit can recreate them,
/// so call again whenever the window may have rebuilt its titlebar, and after
/// the interface zoom changes the title bar's height.
pub(crate) fn pin(window: &Window, title_bar_height: f32) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    // The buttons keep their native size; the bar they sit in and the gaps
    // around them follow the interface zoom.
    let zoom = f64::from(window.content_zoom());
    let top = (f64::from(title_bar_height) * zoom - BUTTON_SIZE) / 2.;
    let kinds = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let Some(button) = ns_window.standardWindowButton(kind) else {
            continue;
        };
        let Some(titlebar) = (unsafe { button.superview() }) else {
            continue;
        };
        let x = LEFT_MARGIN * zoom + i as f64 * (BUTTON_SIZE + BUTTON_SPACING * zoom);
        // Already pinned: move the constraints, since a second set would only
        // conflict.
        if !button.translatesAutoresizingMaskIntoConstraints() {
            let target = objc2::rc::Retained::as_ptr(&button).cast::<objc2::runtime::AnyObject>();
            for constraint in titlebar.constraints().iter() {
                let first = unsafe { constraint.firstItem() };
                if !first.is_some_and(|item| std::ptr::eq(&*item, target)) {
                    continue;
                }
                match constraint.firstAttribute() {
                    NSLayoutAttribute::Leading => constraint.setConstant(x),
                    NSLayoutAttribute::Top => constraint.setConstant(top),
                    _ => {}
                }
            }
            continue;
        }
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        for constraint in [
            button.widthAnchor().constraintEqualToConstant(BUTTON_SIZE),
            button.heightAnchor().constraintEqualToConstant(BUTTON_SIZE),
            button
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&titlebar.leadingAnchor(), x),
            button
                .topAnchor()
                .constraintEqualToAnchor_constant(&titlebar.topAnchor(), top),
        ] {
            constraint.setActive(true);
        }
    }
}

/// GPUI's own position for the buttons, which sizes the titlebar container:
/// scaled with the interface zoom so the container always spans the bar and
/// never clips the pinned buttons.
pub(crate) fn position(zoom: f32, title_bar_height: f32) -> gpui::Point<gpui::Pixels> {
    gpui::point(
        gpui::px(LEFT_MARGIN as f32 * zoom),
        gpui::px((title_bar_height * zoom - BUTTON_SIZE as f32) / 2.),
    )
}

fn ns_window(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI's AppKit handle points at its live content view.
    let view = unsafe { &*appkit.ns_view.as_ptr().cast::<NSView>() };
    view.window()
}
/// Return keyboard input to GPUI after interacting with an embedded native view.
pub(crate) fn focus_content(window: &Window) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    let view = unsafe { &*appkit.ns_view.as_ptr().cast::<NSView>() };
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(view));
    }
}
#[cfg(not(test))]
pub(crate) fn hide(window: &Window) {
    if let Some(window) = ns_window(window) {
        window.orderOut(None);
    }
}
/// mono_chat.rs's floating panel: GPUI's pop-up is already a non-activating
/// panel on every Space. Like Tauri's, it hides the window buttons, resizes,
/// and floats at the status level instead of above menus.
#[cfg(not(test))]
pub(crate) fn floating_panel(window: &Window, resizable: bool) {
    use objc2_app_kit::{NSWindowCollectionBehavior, NSWindowStyleMask};
    // NSStatusWindowLevel.
    const STATUS_LEVEL: isize = 25;
    let Some(window) = ns_window(window) else {
        return;
    };
    if resizable {
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::Resizable);
    }
    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        if let Some(button) = window.standardWindowButton(kind) {
            button.setHidden(true);
        }
    }
    window.setLevel(STATUS_LEVEL);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}
/// quick_composer.rs's `quick_composer_fit`: grow or shrink to the card in
/// one frame update, keeping the top edge where it was so the prompt never
/// moves while a picker opens below it.
///
/// Called while rendering, so the resize waits for the frame to finish: GPUI
/// reports AppKit's new size by updating the window, which fails mid-render
/// and would leave the enlarged panel showing a stale, unpainted frame.
#[cfg(not(test))]
pub(crate) fn fit_height(window: &Window, cx: &gpui::App, height: f64) {
    let Some(window) = ns_window(window) else {
        return;
    };
    if (window.frame().size.height - height).abs() < 0.5 {
        return;
    }
    cx.foreground_executor()
        .spawn(async move {
            let mut frame = window.frame();
            if (frame.size.height - height).abs() < 0.5 {
                return;
            }
            // AppKit's origin is the bottom left.
            frame.origin.y += frame.size.height - height;
            frame.size.height = height;
            window.setFrame_display(frame, true);
            window.invalidateShadow();
        })
        .detach();
}
