//! Non-activating floating panels on macOS: the quick composer and its git
//! popup. Port of `make_panel`, `present`, `place`, `quick_composer_fit`,
//! and the git popup frame code in src-tauri/src/quick_composer.rs and
//! src-tauri/src/quick_composer/git_popup.rs.
//!
//! A plain window would have to activate MonoCode to take keys, which pulls
//! the workspace window over the app in front and leaves focus with MonoCode
//! after the panel hides. A non-activating panel takes keys while the other
//! app stays active, and it can join a full-screen Space.
//!
//! Open the GPUI window with `WindowKind::PopUp`, `focus: false`, and
//! `show: false`. That kind is already an `NSPanel` (zui's `GPUIPanel`) with
//! the non-activating style, the pop-up menu level, and the all-Spaces
//! behavior. [`make_panel`] adds what it lacks: it re-classes the window
//! into a layout-identical subclass that refuses main status (main status
//! is what would bring the workspace window forward), keeps the panel
//! visible when MonoCode deactivates, sets the level, and marks it
//! transient so it stays out of window cycling. `WindowKind::Floating` is
//! an activating panel, so it is not enough.
//!
//! Every function here must run on the main thread. Each one returns an
//! error or does nothing otherwise.

use std::ffi::CString;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSPanel, NSUserInterfaceItemIdentification,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::HasWindowHandle;

use crate::macos::ns_window;
use crate::panel_geometry::{
    PanelAnchor, PanelPoint, PanelRect, fit_frame, over_trigger, place_frame, popup_frame,
    screen_at,
};

/// Class names start with this, so a second call finds the subclass.
const CLASS_PREFIX: &str = "MonoCodePanel_";
const VIBRANCY_ID: &str = "monocode.panel-vibrancy";

/// AppKit window levels (`NSWindowLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelLevel {
    /// `NSFloatingWindowLevel` (3).
    Floating,
    /// `NSStatusWindowLevel` (25): above other apps' windows. The Tauri quick
    /// composer used it.
    Status,
    /// `NSPopUpMenuWindowLevel` (101): what zui gives `WindowKind::PopUp`.
    PopUpMenu,
}

impl PanelLevel {
    pub fn raw(self) -> isize {
        match self {
            PanelLevel::Floating => 3,
            PanelLevel::Status => 25,
            PanelLevel::PopUpMenu => 101,
        }
    }
}

/// How [`make_panel`] sets the panel up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelStyle {
    pub level: PanelLevel,
    /// Join every Space, including full-screen ones.
    pub all_spaces: bool,
    /// A Popover-material blur behind the content with this corner radius,
    /// like Tauri's `Effect::Popover` with `EffectState::Active`.
    pub vibrancy_radius: Option<f64>,
}

impl PanelStyle {
    /// The quick composer: status level, every Space, a 16pt blur.
    pub fn quick_composer() -> Self {
        Self {
            level: PanelLevel::Status,
            all_spaces: true,
            vibrancy_radius: Some(crate::panel_geometry::QUICK_COMPOSER_CORNER_RADIUS),
        }
    }

    /// The git popup: above the composer, a 12pt blur.
    pub fn git_popup() -> Self {
        Self {
            level: PanelLevel::Status,
            all_spaces: true,
            vibrancy_radius: Some(crate::panel_geometry::GIT_POPUP_CORNER_RADIUS),
        }
    }
}

fn main_thread() -> Result<MainThreadMarker, String> {
    MainThreadMarker::new().ok_or_else(|| "Panels must be changed on the main thread.".to_string())
}

fn window_of(window: &impl HasWindowHandle) -> Result<Retained<NSWindow>, String> {
    main_thread()?;
    ns_window(window).ok_or_else(|| "The window has no AppKit handle.".to_string())
}

extern "C-unwind" fn yes(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::YES
}

extern "C-unwind" fn no(_this: &AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
}

/// The subclass already installed on `object`, if any.
fn installed_class(object: &AnyObject) -> Option<&'static AnyClass> {
    let mut class = Some(object.class());
    while let Some(current) = class {
        if current
            .name()
            .to_bytes()
            .starts_with(CLASS_PREFIX.as_bytes())
        {
            return Some(current);
        }
        class = current.superclass();
    }
    None
}

/// A subclass of `original` with no ivars: key status yes, main status no.
fn panel_class(original: &'static AnyClass) -> Result<&'static AnyClass, String> {
    let name = CString::new(format!(
        "{CLASS_PREFIX}{}",
        original.name().to_string_lossy()
    ))
    .map_err(|err| err.to_string())?;
    if let Some(class) = AnyClass::get(&name) {
        return Ok(class);
    }
    let mut builder =
        ClassBuilder::new(&name, original).ok_or("Could not declare the panel class.")?;
    unsafe {
        // Borderless windows refuse key status by default, and the prompt
        // needs keys.
        builder.add_method(
            sel!(canBecomeKeyWindow),
            yes as extern "C-unwind" fn(_, _) -> _,
        );
        // Never main: that is what would pull the workspace window forward.
        builder.add_method(
            sel!(canBecomeMainWindow),
            no as extern "C-unwind" fn(_, _) -> _,
        );
    }
    Ok(builder.register())
}

/// Turn a GPUI `WindowKind::PopUp` window into the non-activating floating
/// panel the quick composer needs. Safe to call more than once.
pub fn make_panel(window: &impl HasWindowHandle, style: &PanelStyle) -> Result<(), String> {
    make_ns_panel(&*window_of(window)?, style)
}

/// [`make_panel`] on an `NSWindow`.
pub fn make_ns_panel(window: &NSWindow, style: &PanelStyle) -> Result<(), String> {
    main_thread()?;
    let Some(panel) = window.downcast_ref::<NSPanel>() else {
        return Err("Open the window with WindowKind::PopUp so it is an NSPanel.".into());
    };
    let object: &AnyObject = panel.as_ref();
    if installed_class(object).is_none() {
        let original = object.class();
        let class = panel_class(original)?;
        // Re-classing into a different size is undefined behavior, so keep
        // the window as it is if a GPUI update ever changes its layout.
        if class.instance_size() != original.instance_size() {
            return Err("The panel class does not match the window's layout.".into());
        }
        unsafe {
            AnyObject::set_class(object, class);
        }
    }
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
    panel.setLevel(style.level.raw());
    let mut behavior = NSWindowCollectionBehavior::Transient
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::FullScreenAuxiliary;
    if style.all_spaces {
        behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
    }
    panel.setCollectionBehavior(behavior);
    panel.setHasShadow(true);
    if let Some(radius) = style.vibrancy_radius {
        add_vibrancy(window, radius);
    }
    Ok(())
}

/// The window is one of our panels.
pub fn is_panel(window: &NSWindow) -> bool {
    let object: &AnyObject = window.as_ref();
    installed_class(object).is_some() && window.downcast_ref::<NSPanel>().is_some()
}

/// A Popover-material blur under the content, rounded to `radius`. The
/// window must be transparent (`WindowBackgroundAppearance::Transparent`).
fn add_vibrancy(window: &NSWindow, radius: f64) {
    let Some(content) = window.contentView() else {
        return;
    };
    let identifier = NSString::from_str(VIBRANCY_ID);
    if content
        .subviews()
        .iter()
        .any(|view| view.identifier().as_deref() == Some(&identifier))
    {
        return;
    }
    let view = NSVisualEffectView::initWithFrame(
        NSVisualEffectView::alloc(window.mtm()),
        content.bounds(),
    );
    view.setIdentifier(Some(&identifier));
    view.setMaterial(NSVisualEffectMaterial::Popover);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    // Active keeps it vibrant while another app is frontmost.
    view.setState(NSVisualEffectState::Active);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    view.setWantsLayer(true);
    unsafe {
        let layer: *mut AnyObject = msg_send![&*view, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
    // GPUI owns the content view and its Metal view. Insert a sibling below
    // it; keep its first responder and event handling.
    content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
    window.invalidateShadow();
}

/// Match the blur to the app theme rather than the system one: the native
/// material follows the window's appearance.
pub fn set_dark(window: &impl HasWindowHandle, dark: bool) -> Result<(), String> {
    let window = window_of(window)?;
    let name = NSString::from_str(if dark {
        "NSAppearanceNameDarkAqua"
    } else {
        "NSAppearanceNameAqua"
    });
    unsafe {
        let appearance: *mut AnyObject =
            msg_send![objc2::class!(NSAppearance), appearanceNamed: &*name];
        let _: () = msg_send![&*window, setAppearance: appearance];
    }
    Ok(())
}

/// `present`: bring the panel forward and give it keys without activating
/// MonoCode. Restores focus after a capture without moving the panel.
pub fn present(window: &impl HasWindowHandle) -> Result<(), String> {
    present_ns(&*window_of(window)?)
}

/// Present a promoted panel without activating MonoCode. A window that
/// could not become a panel still opens and takes keys as an ordinary window.
pub fn present_with_fallback(window: &impl HasWindowHandle) -> Result<(), String> {
    let window = window_of(window)?;
    if is_panel(&window) {
        present_ns(&window)
    } else {
        window.makeKeyAndOrderFront(None);
        Ok(())
    }
}

/// [`present`] on an `NSWindow`. Refuses a window that is not one of our
/// panels, because ordering it front would activate the app.
pub fn present_ns(window: &NSWindow) -> Result<(), String> {
    main_thread()?;
    if !is_panel(window) {
        return Err("Only a panel can come forward without activating MonoCode.".into());
    }
    window.orderFrontRegardless();
    window.makeKeyWindow();
    Ok(())
}

/// Hide the panel.
pub fn hide(window: &impl HasWindowHandle) -> Result<(), String> {
    let window = window_of(window)?;
    window.orderOut(None);
    Ok(())
}

/// Reveal a workspace window after a Dock reopen or a reminder.
pub fn show_workspace_window(window: &impl HasWindowHandle) -> Result<(), String> {
    let window = window_of(window)?;
    if window.isMiniaturized() {
        window.deminiaturize(None);
    }
    window.orderFront(None);
    Ok(())
}

/// The panel is on screen.
pub fn is_visible(window: &impl HasWindowHandle) -> bool {
    workspace_is_visible(window).unwrap_or(false)
}

/// Minimized windows are hidden; windows behind another app remain visible.
pub fn workspace_is_visible(window: &impl HasWindowHandle) -> Result<bool, String> {
    let window = window_of(window)?;
    Ok(window.isVisible() && !window.isMiniaturized())
}

/// Read the current native visibility after the GPUI window borrow ends.
/// The returned reader must run on the main thread.
pub fn visibility_reader(window: &impl HasWindowHandle) -> Result<Rc<dyn Fn() -> bool>, String> {
    let window = window_of(window)?;
    Ok(Rc::new(move || {
        MainThreadMarker::new().is_some_and(|_| window.isVisible() && !window.isMiniaturized())
    }))
}

/// Read whether the native window has keys after its GPUI borrow ends.
/// The returned reader must run on the main thread.
pub fn focus_reader(window: &impl HasWindowHandle) -> Result<Rc<dyn Fn() -> bool>, String> {
    let window = window_of(window)?;
    Ok(Rc::new(move || {
        MainThreadMarker::new().is_some_and(|_| window.isKeyWindow())
    }))
}

/// The panel has keys.
pub fn is_key(window: &impl HasWindowHandle) -> bool {
    window_of(window).is_ok_and(|window| window.isKeyWindow())
}

fn rect(frame: NSRect) -> PanelRect {
    PanelRect::new(
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
    )
}

fn ns_rect(frame: PanelRect) -> NSRect {
    NSRect::new(
        NSPoint::new(frame.x, frame.y),
        NSSize::new(frame.width, frame.height),
    )
}

/// The panel's frame in screen coordinates.
pub fn frame(window: &impl HasWindowHandle) -> Result<PanelRect, String> {
    Ok(rect(window_of(window)?.frame()))
}

/// Set the frame, redraw the shadow, and paint at once, in one update so a
/// resize never moves the top edge for a frame.
fn set_frame(window: &NSWindow, frame: PanelRect) {
    window.setFrame_display(ns_rect(frame), false);
    window.invalidateShadow();
    window.displayIfNeeded();
}

/// `quick_composer_fit`: change the height, keep the top edge.
pub fn fit_height(
    window: &impl HasWindowHandle,
    width: f64,
    height: f64,
    max_height: f64,
) -> Result<(), String> {
    fit_ns_height(&*window_of(window)?, width, height, max_height);
    Ok(())
}

/// [`fit_height`] on an `NSWindow`.
pub fn fit_ns_height(window: &NSWindow, width: f64, height: f64, max_height: f64) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    let next = fit_frame(rect(window.frame()), width, height, max_height);
    set_frame(window, next);
}

/// Every screen as (frame, visible frame).
fn screens() -> Vec<(PanelRect, PanelRect)> {
    let mut out = Vec::new();
    unsafe {
        let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
        if screens.is_null() {
            return out;
        }
        let count: usize = msg_send![screens, count];
        for index in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: index];
            if screen.is_null() {
                continue;
            }
            let frame: NSRect = msg_send![screen, frame];
            let visible: NSRect = msg_send![screen, visibleFrame];
            out.push((rect(frame), rect(visible)));
        }
    }
    out
}

/// The pointer, in screen coordinates.
pub fn pointer() -> PanelPoint {
    let point: NSPoint = unsafe { msg_send![objc2::class!(NSEvent), mouseLocation] };
    PanelPoint::new(point.x, point.y)
}

/// `place`: center the panel on the screen under the pointer, since that is
/// where the user is.
pub fn place_on_pointer_screen(
    window: &impl HasWindowHandle,
    width: f64,
    top_fraction: f64,
) -> Result<(), String> {
    let window = window_of(window)?;
    let Some(area) = screen_at(&screens(), pointer()) else {
        return Err("No screen is available.".into());
    };
    let current = rect(window.frame());
    set_frame(
        &window,
        place_frame(area, width, current.height, top_fraction),
    );
    Ok(())
}

/// The visible frame of the screen the window is on, else the main screen.
fn visible_frame_of(window: &NSWindow) -> Option<PanelRect> {
    unsafe {
        let screen: *mut AnyObject = msg_send![window, screen];
        let screen = if screen.is_null() {
            msg_send![objc2::class!(NSScreen), mainScreen]
        } else {
            screen
        };
        if screen.is_null() {
            return None;
        }
        let visible: NSRect = msg_send![screen, visibleFrame];
        Some(rect(visible))
    }
}

/// Convert saved Tauri physical window coordinates to AppKit points.
pub fn primary_scale_factor() -> f32 {
    if MainThreadMarker::new().is_none() {
        return 1.;
    }
    unsafe {
        let screen: *mut AnyObject = msg_send![objc2::class!(NSScreen), mainScreen];
        if screen.is_null() {
            return 1.;
        }
        let scale: f64 = msg_send![screen, backingScaleFactor];
        scale as f32
    }
}

/// `quick_git_fit`: size the popup and hang it off the anchor control. The
/// composer's frame never changes.
pub fn place_popup(
    parent: &impl HasWindowHandle,
    popup: &impl HasWindowHandle,
    anchor: &PanelAnchor,
    height: f64,
    width: f64,
    max_height: f64,
) -> Result<(), String> {
    let parent = window_of(parent)?;
    let popup = window_of(popup)?;
    let parent_frame = rect(parent.frame());
    let screen = visible_frame_of(&parent).unwrap_or(parent_frame);
    set_frame(
        &popup,
        popup_frame(parent_frame, screen, anchor, height, width, max_height),
    );
    Ok(())
}

/// `NSEventTypeLeftMouseDown`.
const LEFT_MOUSE_DOWN: usize = 1;

/// The popup lost focus because of a press on the control that opened it.
/// The composer then treats the click as "close", not "open again".
pub fn blurred_by_trigger(parent: &impl HasWindowHandle, anchor: &PanelAnchor) -> bool {
    let Ok(parent) = window_of(parent) else {
        return false;
    };
    let pressing = unsafe {
        let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
        let event: *mut AnyObject = msg_send![app, currentEvent];
        if event.is_null() {
            false
        } else {
            let kind: usize = msg_send![event, type];
            kind == LEFT_MOUSE_DOWN
        }
    };
    pressing && over_trigger(rect(parent.frame()), anchor, pointer())
}
