//! Tauri glue over `monocode_platform::screenshots`: the managed capture
//! registry and the release command.
use tauri::{AppHandle, Manager, State, WebviewWindow};

pub use monocode_platform::screenshots::{capture_screenshot, cleanup_abandoned, Captures};

use super::QuickAttachment;

pub fn register(app: &AppHandle, path: &str) {
    app.state::<Captures>().register(path);
}

pub(super) fn discard(app: &AppHandle, path: &str) {
    app.state::<Captures>().discard(path);
}

#[tauri::command]
pub fn quick_composer_release_capture(
    window: WebviewWindow,
    state: State<'_, Captures>,
    paths: Vec<String>,
) -> Result<(), String> {
    if window.label() != crate::window::QUICK_COMPOSER_LABEL {
        return Err("Invalid capture owner.".into());
    }
    state.release(&paths)
}

pub(super) fn persist(app: &AppHandle, files: &mut [QuickAttachment]) -> Result<(), String> {
    let data_dir = crate::app_data_dir(app)?;
    let mut paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    app.state::<Captures>().persist(&data_dir, &mut paths)?;
    for (file, path) in files.iter_mut().zip(paths) {
        file.path = path;
    }
    Ok(())
}
