#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// The portable AppImage bundles WebKitGTK and starts through linuxdeploy's
/// X11 hook. On Mesa/Wayland systems its DMA-BUF path can abort before the
/// WebView is created, so opt out before Tauri initializes GTK. Respect an
/// explicit user setting and leave native `.deb`/`.rpm` builds untouched.
#[cfg(target_os = "linux")]
fn configure_appimage_graphics() {
    let running_from_appimage =
        std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some();
    if running_from_appimage && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

fn main() {
    if let Some(code) = monocode_lib::ssh_askpass::maybe_run() {
        std::process::exit(code);
    }
    if std::env::args().nth(1).as_deref() == Some("control") {
        std::process::exit(monocode_lib::control_cli::run(
            std::env::args().skip(2).collect(),
        ));
    }
    if std::env::args().nth(1).as_deref() == Some("app") {
        std::process::exit(monocode_lib::control_cli::run_app(
            std::env::args().skip(2).collect(),
        ));
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    monocode_lib::ensure_macos_dev_bundle();
    #[cfg(target_os = "linux")]
    configure_appimage_graphics();
    monocode_lib::run()
}
