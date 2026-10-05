//! macOS chrome: traffic lights and WindowServer background blur.
//!
//! The overlay titlebar is ~28pt. Our HTML tab bar is 40px (`h-10`), so the
//! native `NSTitlebarContainerView` has to be stretched to match or the
//! traffic-light strip looks shorter than the rest of the chrome.
//!
//! Tao's `trafficLightPosition` re-runs `setFrame` on the titlebar *every
//! drawRect* using `window.frame().height`. That is why the buttons jumped
//! during live resize. We never set that option. Buttons are Auto Layout
//! pinned once. The container is `setFrame`'d to 40px on install,
//! resize, and focus — not from `drawRect`.
//!
//! Sidebar glass uses a transparent NSWindow plus
//! `CGSSetWindowBackgroundBlurRadius` (private WindowServer API). That
//! blurs the desktop behind the window; CSS only tints the sidebar on top.
//! A nearly transparent AppKit visual-effect view behind the WKWebView keeps
//! CSS backdrop filters stable during hover repaints and window capture.
//!
//! Fully clear `NSColor.clearColor` (alpha 0) plus a native shadow makes
//! macOS draw a chamfered gap at the corners. Tiny alpha (0.01) keeps the
//! shadow without that outline.
//!
//! Moved from src-tauri/src/macos.rs. Functions take any window that exposes
//! an AppKit raw window handle (raw-window-handle 0.6), so Tauri and GPUI
//! windows share them. Per-window state (badge count, glass) is keyed by a
//! caller-chosen string such as the window label.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSColor, NSMenu, NSMenuItem,
    NSRequestUserAttentionType, NSTitlebarSeparatorStyle, NSUserInterfaceItemIdentification,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowOrderingMode,
};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Must match the HTML title bar (`h-10` = 40px).
const TAB_BAR_HEIGHT: f64 = 40.0;
const BUTTON_SIZE: f64 = 14.0;
const LEFT_MARGIN: f64 = 12.0;
const BUTTON_SPACING: f64 = 6.0;
/// Vertically center 14pt buttons in the tab bar: (40 - 14) / 2.
const TOP_INSET: f64 = (TAB_BAR_HEIGHT - BUTTON_SIZE) / 2.0;

pub const BLUR_MIN: u8 = 1;
pub const BLUR_MAX: u8 = 64;
pub const BLUR_DEFAULT: u8 = 24;

const GLASS_BACKING_ID: &str = "monocode.webview-glass-backing";

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

static PINNED: AtomicBool = AtomicBool::new(false);
static BLUR_RADIUS: AtomicU8 = AtomicU8::new(BLUR_DEFAULT);
static WINDOW_BADGES: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
static GLASS_WINDOWS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

type CgsConnection = usize;
type SetBlurFn = unsafe extern "C" fn(CgsConnection, c_int, c_int) -> c_int;
type ConnectionFn = unsafe extern "C" fn() -> CgsConnection;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// First setup of a new window. Opaque for the dock bounce so the first
/// frames are a solid field, not a frosted desktop. Glass turns on after the
/// first UI paint. Callers then call `pin` on focus, `stretch_titlebar` on
/// resize, and `window_destroyed` when the window goes away.
pub fn install(window: &impl HasWindowHandle) {
    prepare_launch(window);
    let _ = pin(window);
}

/// Clear the badge and glass state a destroyed window left behind. Call on
/// the main thread.
pub fn window_destroyed(key: &str) {
    paint_window_badge(key, 0);
    set_glass_enabled(key, false);
}

fn window_badges() -> &'static Mutex<HashMap<String, u32>> {
    WINDOW_BADGES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Slack-style red count on the Dock icon. `count` is this window's pending
/// approvals; the tile shows the sum across windows. Does nothing off the main
/// thread, so callers dispatch it there.
pub fn paint_window_badge(label: &str, count: u32) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let mut map = window_badges()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let previous: u32 = map.values().copied().sum();
    if count == 0 {
        map.remove(label);
    } else {
        map.insert(label.to_string(), count);
    }
    let total: u32 = map.values().copied().sum();
    drop(map);

    let ns_app = NSApplication::sharedApplication(mtm);
    let tile = ns_app.dockTile();
    tile.setShowsApplicationBadge(total > 0);
    if total == 0 {
        tile.setBadgeLabel(None);
    } else {
        let text = if total > 99 {
            "99+".to_string()
        } else {
            total.to_string()
        };
        tile.setBadgeLabel(Some(&NSString::from_str(&text)));
    }
    tile.display();

    if total > previous && !ns_app.isActive() {
        ns_app.requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
    }
}

pub fn set_visible(window: &impl HasWindowHandle, visible: bool) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    for kind in button_kinds() {
        if let Some(button) = ns_window.standardWindowButton(kind) {
            button.setHidden(!visible);
        }
    }
}

pub fn set_background_blur_radius(window: &impl HasWindowHandle, key: &str, radius: u8) {
    let radius = radius.clamp(BLUR_MIN, BLUR_MAX);
    BLUR_RADIUS.store(radius, Ordering::Relaxed);
    if glass_enabled(key) {
        apply_blur(window, radius);
    }
}

fn glass_windows() -> &'static Mutex<HashSet<String>> {
    GLASS_WINDOWS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn glass_enabled(key: &str) -> bool {
    glass_windows()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains(key)
}

pub fn set_glass_enabled(key: &str, enabled: bool) {
    let mut windows = glass_windows()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    if enabled {
        windows.insert(key.to_string());
    } else {
        windows.remove(key);
    }
}

/// Solid field behind the dock bounce. Same colour as the HTML sheet.
pub fn prepare_launch(window: &impl HasWindowHandle) {
    set_launch_background(window, 23, 23, 23);
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setHasShadow(true);
    ns_window.invalidateShadow();
    ns_window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
}

fn set_launch_background(window: &impl HasWindowHandle, r: u8, g: u8, b: u8) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    set_glass_backing(&ns_window, false);
    ns_window.setOpaque(true);
    ns_window.setBackgroundColor(Some(&NSColor::colorWithRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        1.0,
    )));
}

/// Turn on desktop blur after the first UI paint.
pub fn enable_glass(window: &impl HasWindowHandle, key: &str) {
    set_glass_enabled(key, true);
    prepare_glass(window);
    apply_blur(window, BLUR_RADIUS.load(Ordering::Relaxed));
}

/// Turn off the blur and fall back to an opaque window in the caller's colour.
pub fn disable_glass(window: &impl HasWindowHandle, key: &str, r: u8, g: u8, b: u8) {
    set_glass_enabled(key, false);
    apply_blur(window, 0);
    set_launch_background(window, r, g, b);
}

fn prepare_glass(window: &impl HasWindowHandle) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    set_glass_backing(&ns_window, true);
    ns_window.setOpaque(false);
    // Fully clear + shadow leaves a jagged gap at the corners.
    ns_window.setBackgroundColor(Some(&NSColor::clearColor().colorWithAlphaComponent(0.01)));
    ns_window.setHasShadow(true);
    ns_window.invalidateShadow();
    ns_window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
}

/// Keep an AppKit backdrop surface below the transparent WKWebView. With only
/// WindowServer blur, native hover/capture tests expose unfiltered web content
/// for individual frames. A visual-effect backing prevents that without
/// removing CSS blur or making the page opaque; an ordinary layer-backed
/// NSView did not. Keep a nonzero alpha so AppKit retains the effect, but only
/// tint at 1% so the existing glass appearance and blur-radius control remain.
fn set_glass_backing(window: &NSWindow, enabled: bool) {
    let Some(content) = window.contentView() else {
        return;
    };
    let identifier = NSString::from_str(GLASS_BACKING_ID);
    if let Some(backing) = content
        .subviews()
        .iter()
        .find(|view| view.identifier().as_deref() == Some(&identifier))
    {
        backing.setHidden(!enabled);
        return;
    }
    if !enabled {
        return;
    }

    let backing = NSVisualEffectView::initWithFrame(
        NSVisualEffectView::alloc(window.mtm()),
        content.bounds(),
    );
    backing.setIdentifier(Some(&identifier));
    backing.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    backing.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    backing.setState(NSVisualEffectState::Active);
    backing.setAlphaValue(0.01);
    backing.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    // Wry owns the parent and WKWebView. Insert a sibling below the webview;
    // do not replace its parent, first responder, or event-handling view.
    content.addSubview_positioned_relativeTo(&backing, NSWindowOrderingMode::Below, None);
}

fn apply_blur(window: &impl HasWindowHandle, radius: u8) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    let Some(set_blur) = set_blur_fn() else {
        return;
    };
    let Some(connection) = cgs_connection() else {
        return;
    };
    let window_number = ns_window.windowNumber();
    if window_number <= 0 {
        return;
    }
    unsafe {
        set_blur(connection, window_number as c_int, radius as c_int);
    }
}

/// Pin the traffic lights into the 40px tab bar. Call on focus.
pub fn pin(window: &impl HasWindowHandle) -> bool {
    let Some(ns_window) = ns_window(window) else {
        return PINNED.load(Ordering::Relaxed);
    };
    unsafe {
        if !PINNED.load(Ordering::Relaxed) && !pin_ns_window(&ns_window) {
            return false;
        }
        stretch_ns_window(&ns_window);
    }
    true
}

/// Keep the titlebar container 40px tall. Call on resize and scale changes.
pub fn stretch_titlebar(window: &impl HasWindowHandle) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    unsafe { stretch_ns_window(&ns_window) }
}

pub fn ns_window(window: &impl HasWindowHandle) -> Option<objc2::rc::Retained<NSWindow>> {
    let Ok(handle) = window.window_handle() else {
        return None;
    };
    ns_window_from_raw(handle.as_raw())
}

/// The `NSWindow` behind an AppKit raw window handle.
pub fn ns_window_from_raw(handle: RawWindowHandle) -> Option<objc2::rc::Retained<NSWindow>> {
    let RawWindowHandle::AppKit(appkit) = handle else {
        return None;
    };
    let ns_view: *mut objc2::runtime::AnyObject = appkit.ns_view.as_ptr().cast();
    if ns_view.is_null() {
        return None;
    }
    let view = unsafe { &*ns_view.cast::<objc2_app_kit::NSView>() };
    view.window()
}

fn button_kinds() -> [objc2_app_kit::NSWindowButton; 3] {
    use objc2_app_kit::NSWindowButton;
    [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
}

unsafe fn pin_ns_window(window: &NSWindow) -> bool {
    let kinds = button_kinds();
    let Some(close) = window.standardWindowButton(kinds[0]) else {
        return false;
    };
    // SAFETY: the caller holds the window on the main thread, as AppKit requires.
    let Some(titlebar) = (unsafe { close.superview() }) else {
        return false;
    };

    titlebar.setClipsToBounds(false);
    if let Some(container) = unsafe { titlebar.superview() } {
        container.setClipsToBounds(false);
    }

    for (i, kind) in kinds.iter().enumerate() {
        let Some(button) = window.standardWindowButton(*kind) else {
            continue;
        };
        button.setTranslatesAutoresizingMaskIntoConstraints(false);
        let x = LEFT_MARGIN + i as f64 * (BUTTON_SIZE + BUTTON_SPACING);
        let w = button.widthAnchor().constraintEqualToConstant(BUTTON_SIZE);
        let h = button.heightAnchor().constraintEqualToConstant(BUTTON_SIZE);
        let leading = button
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&titlebar.leadingAnchor(), x);
        let top = button
            .topAnchor()
            .constraintEqualToAnchor_constant(&titlebar.topAnchor(), TOP_INSET);
        w.setActive(true);
        h.setActive(true);
        leading.setActive(true);
        top.setActive(true);
    }

    PINNED.store(true, Ordering::Relaxed);
    true
}

unsafe fn stretch_ns_window(window: &NSWindow) {
    let kinds = button_kinds();
    let Some(close) = window.standardWindowButton(kinds[0]) else {
        return;
    };
    // SAFETY: the caller holds the window on the main thread, as AppKit requires.
    let Some(titlebar) = (unsafe { close.superview() }) else {
        return;
    };
    let Some(container) = (unsafe { titlebar.superview() }) else {
        return;
    };

    let parent_height = unsafe { container.superview() }
        .map(|parent| parent.frame().size.height)
        .unwrap_or_else(|| window.frame().size.height);

    let mut frame = container.frame();
    frame.size.height = TAB_BAR_HEIGHT;
    frame.origin.y = parent_height - TAB_BAR_HEIGHT;
    container.setFrame(frame);

    let mut inner = titlebar.frame();
    inner.origin.y = 0.0;
    inner.size.height = TAB_BAR_HEIGHT;
    inner.size.width = frame.size.width;
    titlebar.setFrame(inner);
}

fn set_blur_fn() -> Option<SetBlurFn> {
    static FN: OnceLock<Option<SetBlurFn>> = OnceLock::new();
    *FN.get_or_init(|| dlsym_fn(b"CGSSetWindowBackgroundBlurRadius\0"))
}

fn cgs_connection() -> Option<CgsConnection> {
    static FN: OnceLock<Option<ConnectionFn>> = OnceLock::new();
    let function = (*FN.get_or_init(|| {
        dlsym_fn(b"CGSDefaultConnectionForThread\0").or_else(|| dlsym_fn(b"CGSMainConnectionID\0"))
    }))?;
    let connection = unsafe { function() };
    (connection != 0).then_some(connection)
}

fn dlsym_fn<T>(symbol: &[u8]) -> Option<T> {
    unsafe {
        let ptr = dlsym(RTLD_DEFAULT, symbol.as_ptr().cast());
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&ptr))
        }
    }
}

struct DockMenuTargetIvars {
    new_window: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MonoCodeDockMenuTarget"]
    #[ivars = DockMenuTargetIvars]
    struct DockMenuTarget;

    impl DockMenuTarget {
        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&NSMenuItem>) {
            (self.ivars().new_window)();
        }
    }
);

thread_local! {
    static DOCK_MENU_TARGET: RefCell<Option<Retained<DockMenuTarget>>> =
        const { RefCell::new(None) };
}

/// Since macOS 12, `NSDockTile` badge updates are ignored unless the app has
/// requested `UNUserNotificationCenter` authorization with the badge option.
/// Must run on the main thread after launch (`RunEvent::Ready`), not in setup.
///
/// Only re-requests once the user has already answered the prompt: the
/// one-time system dialog is reserved for the Notifications toggle, so a
/// badge-only request at startup must not consume it. Until then the badge
/// stays off.
pub fn request_badge_authorization() {
    if MainThreadMarker::new().is_none() {
        return;
    }

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNNotificationSettings,
        UNUserNotificationCenter,
    };
    use std::ptr::NonNull;

    let center = UNUserNotificationCenter::currentNotificationCenter();
    let handler = RcBlock::new(|settings: NonNull<UNNotificationSettings>| {
        let settings = unsafe { settings.as_ref() };
        if settings.authorizationStatus() == UNAuthorizationStatus::NotDetermined {
            return;
        }
        let done = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Badge,
                &done,
            );
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
}

/// Add a "New Window" item to the Dock menu. Call on the main thread.
pub fn install_dock_menu(new_window: impl Fn() + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let target = DockMenuTarget::alloc().set_ivars(DockMenuTargetIvars {
        new_window: Box::new(new_window),
    });
    let target: Retained<DockMenuTarget> = unsafe { msg_send![super(target), init] };
    DOCK_MENU_TARGET.with(|slot| {
        *slot.borrow_mut() = Some(target.clone());
    });

    let menu = NSMenu::new(mtm);
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("New Window"),
            Some(sel!(newWindow:)),
            &NSString::new(),
        )
    };
    unsafe {
        item.setTarget(Some(&target));
    }
    menu.addItem(&item);

    let ns_app = NSApplication::sharedApplication(mtm);
    unsafe {
        let _: () = msg_send![&*ns_app, setDockMenu: Some(&*menu)];
    }
}
