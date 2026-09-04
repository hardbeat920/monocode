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
//!
//! Fully clear `NSColor.clearColor` (alpha 0) plus a native shadow makes
//! macOS draw a chamfered gap at the corners. Tiny alpha (0.01) keeps the
//! shadow without that outline.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{
    define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSColor, NSMenu, NSMenuItem, NSRequestUserAttentionType,
    NSTitlebarSeparatorStyle, NSWindow,
};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

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

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

static PINNED: AtomicBool = AtomicBool::new(false);
static BLUR_RADIUS: AtomicU8 = AtomicU8::new(BLUR_DEFAULT);
static WINDOW_BADGES: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();

type CgsConnection = usize;
type SetBlurFn = unsafe extern "C" fn(CgsConnection, c_int, c_int) -> c_int;
type ConnectionFn = unsafe extern "C" fn() -> CgsConnection;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

pub fn install(window: &WebviewWindow) {
    // Opaque for the dock bounce so the first frames are a solid field,
    // not a frosted desktop. Glass turns on after the first UI paint.
    prepare_launch(window);
    let _ = pin(window);

    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            pin(&event_window);
        }
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            stretch_titlebar(&event_window);
        }
        WindowEvent::Destroyed => set_window_badge(&event_window, 0),
        _ => {}
    });
}

/// Slack-style red count on the Dock icon. `count` is this window's pending
/// approvals; the tile shows the sum across windows.
pub fn set_window_badge(window: &WebviewWindow, count: u32) {
    let label = window.label().to_string();
    let apply = move || paint_window_badge(&label, count);
    if MainThreadMarker::new().is_some() {
        apply();
        return;
    }
    let _ = window.app_handle().run_on_main_thread(apply);
}

fn window_badges() -> &'static Mutex<HashMap<String, u32>> {
    WINDOW_BADGES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn paint_window_badge(label: &str, count: u32) {
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

pub fn set_visible(window: &WebviewWindow, visible: bool) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    for kind in button_kinds() {
        if let Some(button) = ns_window.standardWindowButton(kind) {
            button.setHidden(!visible);
        }
    }
}

pub fn set_background_blur_radius(window: &WebviewWindow, radius: u8) {
    let radius = radius.clamp(BLUR_MIN, BLUR_MAX);
    BLUR_RADIUS.store(radius, Ordering::Relaxed);
    apply_blur(window, radius);
}

/// Solid field behind the dock bounce. Same colour as the HTML sheet.
fn prepare_launch(window: &WebviewWindow) {
    set_launch_background(window, 23, 23, 23);
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setHasShadow(true);
    ns_window.invalidateShadow();
    ns_window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
}

fn set_launch_background(window: &WebviewWindow, r: u8, g: u8, b: u8) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setOpaque(true);
    ns_window.setBackgroundColor(Some(&NSColor::colorWithRed_green_blue_alpha(
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        1.0,
    )));
}

/// Turn on desktop blur after the first UI paint.
pub fn enable_glass(window: &WebviewWindow) {
    prepare_glass(window);
    apply_blur(window, BLUR_RADIUS.load(Ordering::Relaxed));
}

fn prepare_glass(window: &WebviewWindow) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setOpaque(false);
    // Fully clear + shadow leaves a jagged gap at the corners.
    ns_window.setBackgroundColor(Some(&NSColor::clearColor().colorWithAlphaComponent(0.01)));
    ns_window.setHasShadow(true);
    ns_window.invalidateShadow();
    ns_window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
}

fn apply_blur(window: &WebviewWindow, radius: u8) {
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
        set_blur(
            connection,
            window_number as c_int,
            radius.max(BLUR_MIN) as c_int,
        );
    }
}

fn pin(window: &WebviewWindow) -> bool {
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

fn stretch_titlebar(window: &WebviewWindow) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    unsafe { stretch_ns_window(&ns_window) }
}

pub(crate) fn ns_window(window: &WebviewWindow) -> Option<objc2::rc::Retained<NSWindow>> {
    let Ok(handle) = window.window_handle() else {
        return None;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
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
    let Some(titlebar) = close.superview() else {
        return false;
    };

    titlebar.setClipsToBounds(false);
    if let Some(container) = titlebar.superview() {
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
    let Some(titlebar) = close.superview() else {
        return;
    };
    let Some(container) = titlebar.superview() else {
        return;
    };

    let parent_height = container
        .superview()
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
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MonoCodeDockMenuTarget"]
    #[ivars = DockMenuTargetIvars]
    struct DockMenuTarget;

    impl DockMenuTarget {
        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&NSMenuItem>) {
            let _ = crate::window::open_new_window(&self.ivars().app);
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
pub(crate) fn request_badge_authorization() {
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

pub(crate) fn install_dock_menu(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let target = DockMenuTarget::alloc().set_ivars(DockMenuTargetIvars { app: app.clone() });
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

/// `tauri dev` launches a raw binary. The Dock then skips Icon Services and
/// paints Tauri's embedded icns edge-to-edge. Wrap that binary in a real
/// `.app` so macOS applies the plate, mask, and padding.
#[cfg(debug_assertions)]
pub(crate) fn ensure_dev_bundle() {
    if let Err(err) = relaunch_from_dev_bundle() {
        eprintln!("monocode: macos dev bundle: {err}");
    }
}

/// Tauri sets `applicationIconImage` on Ready in dev, which undoes the bundle
/// icon. Clearing it restores Icon Services.
#[cfg(debug_assertions)]
pub(crate) fn prefer_bundle_dock_icon() {
    if !current_exe_is_bundled() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    unsafe { app.setApplicationIconImage(None) };
    app.dockTile().display();
    // Tauri assigns the embedded bitmap after Ready. Clear again so Icon
    // Services keeps the composed AppIcon (squircle fill + artwork).
    unsafe {
        let _: () = msg_send![
            &app,
            performSelector: sel!(setApplicationIconImage:),
            withObject: None::<&objc2::runtime::AnyObject>,
            afterDelay: 0.3_f64
        ];
    }
}

#[cfg(debug_assertions)]
fn current_exe_is_bundled() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("..").join("Info.plist")))
        .is_some_and(|plist| plist.exists())
}

#[cfg(debug_assertions)]
fn relaunch_from_dev_bundle() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if current_exe_is_bundled() {
        let app = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or("missing bundle root")?
            .to_path_buf();
        write_dev_bundle_icons(&app)?;
        return Ok(());
    }

    let app = exe
        .parent()
        .ok_or("missing exe parent")?
        .join("MonoCode.app");
    let macos_dir = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos_dir).map_err(|e| e.to_string())?;
    write_dev_bundle_icons(&app)?;

    let bundled = macos_dir.join("monocode");
    let _ = std::fs::remove_file(&bundled);
    // A copy, not a hard link: re-signing below rewrites the file, and the
    // linked original is the executable running this code.
    std::fs::copy(&exe, &bundled).map_err(|e| e.to_string())?;
    let mut perms = std::fs::metadata(&bundled)
        .map_err(|e| e.to_string())?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bundled, perms).map_err(|e| e.to_string())?;

    // The linker's ad-hoc signature carries a `monocode-<hash>` identifier.
    // UNUserNotificationCenter refuses authorization, without prompting,
    // unless the signing identifier matches CFBundleIdentifier.
    let signed = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier", DEV_BUNDLE_ID])
        .arg(&app)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !signed {
        eprintln!("monocode: macos dev bundle: codesign failed; notifications stay off");
    }

    let err = Command::new(&bundled)
        .args(std::env::args_os().skip(1))
        .exec();
    Err(err.to_string())
}

#[cfg(debug_assertions)]
fn write_dev_bundle_icons(app: &std::path::Path) -> Result<(), String> {
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&resources).map_err(|e| e.to_string())?;
    std::fs::write(app.join("Contents/Info.plist"), DEV_BUNDLE_PLIST).map_err(|e| e.to_string())?;
    std::fs::write(resources.join("AppIcon.icns"), DEV_ICNS).map_err(|e| e.to_string())?;
    std::fs::write(resources.join("Assets.car"), DEV_ASSETS_CAR).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("/usr/bin/touch")
        .arg(app)
        .status();
    Ok(())
}

/// Must match `CFBundleIdentifier` in `DEV_BUNDLE_PLIST` and tauri.conf.json.
#[cfg(debug_assertions)]
const DEV_BUNDLE_ID: &str = "com.monocode.desktop";
#[cfg(debug_assertions)]
const DEV_ICNS: &[u8] = include_bytes!("../icons/icon.icns");
#[cfg(debug_assertions)]
const DEV_ASSETS_CAR: &[u8] = include_bytes!("../macos/Assets.car");
#[cfg(debug_assertions)]
const DEV_BUNDLE_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>MonoCode</string>
	<key>CFBundleExecutable</key>
	<string>monocode</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundleIconName</key>
	<string>AppIcon</string>
	<key>CFBundleIdentifier</key>
	<string>com.monocode.desktop</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>MonoCode</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1.75</string>
	<key>CFBundleVersion</key>
	<string>0.1.75.5</string>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
"#;
