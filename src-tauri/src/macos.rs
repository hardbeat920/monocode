//! macOS window chrome for Tauri windows. The native code lives in
//! `monocode_platform::macos`; this module wires it to Tauri window events and
//! keeps the dev bundle helpers that only `tauri dev` needs.

use std::ffi::OsStr;
use std::path::{Component, Path};

use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::NSApplication;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

use monocode_platform::macos as native;
pub(crate) use monocode_platform::macos::ns_window;

pub fn install(window: &WebviewWindow) {
    // Opaque for the dock bounce so the first frames are a solid field,
    // not a frosted desktop. Glass turns on after the first UI paint.
    native::install(window);

    let event_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(true) => {
            native::pin(&event_window);
        }
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            native::stretch_titlebar(&event_window);
        }
        WindowEvent::Destroyed => {
            set_window_badge(&event_window, 0);
            native::set_glass_enabled(event_window.label(), false);
        }
        _ => {}
    });
}

/// Slack-style red count on the Dock icon. `count` is this window's pending
/// approvals; the tile shows the sum across windows.
pub fn set_window_badge(window: &WebviewWindow, count: u32) {
    let label = window.label().to_string();
    let apply = move || native::paint_window_badge(&label, count);
    if MainThreadMarker::new().is_some() {
        apply();
        return;
    }
    let _ = window.app_handle().run_on_main_thread(apply);
}

pub fn set_visible(window: &WebviewWindow, visible: bool) {
    native::set_visible(window, visible);
}

pub fn set_background_blur_radius(window: &WebviewWindow, radius: u8) {
    native::set_background_blur_radius(window, window.label(), radius);
}

/// Turn on desktop blur after the first UI paint.
pub fn enable_glass(window: &WebviewWindow) {
    native::enable_glass(window, window.label());
}

/// Turn off the blur and fall back to an opaque window in the caller's colour.
pub fn disable_glass(window: &WebviewWindow, r: u8, g: u8, b: u8) {
    native::disable_glass(window, window.label(), r, g, b);
}

pub(crate) fn request_badge_authorization() {
    native::request_badge_authorization();
}

pub(crate) fn install_dock_menu(app: &AppHandle) {
    let app = app.clone();
    native::install_dock_menu(move || {
        let _ = crate::window::open_new_window(&app);
    });
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
        .and_then(|exe| existing_bundle_root_from_exe(&exe))
        .is_some()
}

#[cfg(debug_assertions)]
fn existing_bundle_root_from_exe(exe: &Path) -> Option<(std::path::PathBuf, String)> {
    let app = exe.parent()?.parent()?.parent()?.to_path_buf();
    let app_name = bundle_name_from_app_path(&app)?;
    app.join("Contents/Info.plist")
        .exists()
        .then_some((app, app_name))
}

#[cfg(debug_assertions)]
fn relaunch_from_dev_bundle() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some((app, app_name)) = existing_bundle_root_from_exe(&exe) {
        write_dev_bundle_icons(&app, &app_name)?;
        return Ok(());
    }
    let app_name = dev_bundle_name_from_env(DEV_BUNDLE_DEFAULT_NAME);

    let app = exe
        .parent()
        .ok_or("missing exe parent")?
        .join(dev_bundle_dir_name(&app_name));
    let macos_dir = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos_dir).map_err(|e| e.to_string())?;
    write_dev_bundle_icons(&app, &app_name)?;

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
fn write_dev_bundle_icons(app: &Path, app_name: &str) -> Result<(), String> {
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&resources).map_err(|e| e.to_string())?;
    std::fs::write(app.join("Contents/Info.plist"), dev_bundle_plist(app_name))
        .map_err(|e| e.to_string())?;
    std::fs::write(resources.join("AppIcon.icns"), DEV_ICNS).map_err(|e| e.to_string())?;
    std::fs::write(resources.join("Assets.car"), DEV_ASSETS_CAR).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("/usr/bin/touch")
        .arg(app)
        .status();
    Ok(())
}

/// Must match `CFBundleIdentifier` in the generated dev bundle plist and tauri.conf.json.
#[cfg(debug_assertions)]
const DEV_BUNDLE_DEFAULT_NAME: &str = "MonoCode";
#[cfg(debug_assertions)]
const DEV_BUNDLE_NAME_ENV: &str = "MONOCODE_DEV_APP_NAME";
#[cfg(debug_assertions)]
const DEV_BUNDLE_ID: &str = "com.monocode.desktop";
#[cfg(debug_assertions)]
const DEV_ICNS: &[u8] = include_bytes!("../icons/icon.icns");
#[cfg(debug_assertions)]
const DEV_ASSETS_CAR: &[u8] = include_bytes!("../macos/Assets.car");
#[cfg(debug_assertions)]
fn dev_bundle_dir_name(app_name: &str) -> String {
    format!("{app_name}.app")
}

#[cfg(debug_assertions)]
fn dev_bundle_name_from_env(fallback: &str) -> String {
    std::env::var(DEV_BUNDLE_NAME_ENV)
        .ok()
        .and_then(|value| sanitized_dev_bundle_name(&value))
        .unwrap_or_else(|| fallback.into())
}

#[cfg(debug_assertions)]
fn sanitized_dev_bundle_name(value: &str) -> Option<String> {
    let value = value.trim();
    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component == OsStr::new(value) => {
            Some(value.into())
        }
        _ => None,
    }
}

#[cfg(debug_assertions)]
fn bundle_name_from_app_path(app: &Path) -> Option<String> {
    (app.extension() == Some(OsStr::new("app")))
        .then(|| app.file_stem())
        .flatten()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|name| !name.is_empty())
}

#[cfg(debug_assertions)]
fn escape_plist_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(debug_assertions)]
fn dev_bundle_plist(app_name: &str) -> Vec<u8> {
    let app_name = escape_plist_text(app_name);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{app_name}</string>
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
	<string>{app_name}</string>
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
"#
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_bundle_exe_path(app_name: &str) -> (PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "monocode-macos-tests-{}-{nonce}",
            std::process::id()
        ));
        let app = root.join(app_name);
        let exe = app.join("Contents/MacOS/monocode");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), b"plist").unwrap();
        (root, exe)
    }

    #[test]
    fn sanitized_dev_bundle_name_accepts_single_component() {
        assert_eq!(
            sanitized_dev_bundle_name("  MonoCode Dev  "),
            Some("MonoCode Dev".into())
        );
    }

    #[test]
    fn sanitized_dev_bundle_name_rejects_invalid_components() {
        for invalid in ["", "   ", ".", "..", "../Other", "/tmp/Other", "Foo/Bar"] {
            assert_eq!(sanitized_dev_bundle_name(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn bundle_name_from_app_path_reads_existing_bundle_name() {
        assert_eq!(
            bundle_name_from_app_path(Path::new("/tmp/MonoCode Dev.app")),
            Some("MonoCode Dev".into())
        );
    }

    #[test]
    fn existing_bundle_root_from_exe_rejects_bundle_roots_without_a_usable_name() {
        let (root, exe) = test_bundle_exe_path(".app");
        assert_eq!(existing_bundle_root_from_exe(&exe), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dev_bundle_plist_uses_the_provided_app_name() {
        let plist = String::from_utf8(dev_bundle_plist("MonoCode Dev")).unwrap();
        assert!(plist.contains("<string>MonoCode Dev</string>"));
        assert!(!plist.contains("<string>MonoCode</string>"));
    }
}
