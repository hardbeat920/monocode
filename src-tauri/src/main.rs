#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
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
    monocode_lib::run()
}
