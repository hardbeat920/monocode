//! An optional `theme.json` in the app config directory lets a desktop theme
//! switcher (Omarchy's `theme-set` hook, a dotfiles script) retint MonoCode.
//! The file is polled rather than watched: it changes a few times a day, and a
//! metadata check once a second costs less than another watcher dependency.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Emitter, Manager};

pub(crate) const CHANGED: &str = "monocode:external-theme-changed";
const FILE_NAME: &str = "theme.json";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_THEME_BYTES: u64 = 64 * 1024;

fn theme_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(FILE_NAME))
}

/// Raw file contents, or `None` when the file is missing, unreadable, or too
/// large to be a theme. The page owns parsing so it can ignore bad input.
fn read_theme(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_THEME_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let metadata = std::fs::metadata(path).ok()?;
    Some((
        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        metadata.len(),
    ))
}

#[tauri::command]
pub(crate) fn read_external_theme(app: AppHandle) -> Option<String> {
    read_theme(&theme_path(&app)?)
}

/// Emits [`CHANGED`] with the new contents (or `null` once removed) to every
/// window whenever the file is written, replaced, or deleted.
pub(crate) fn init(app: &AppHandle) {
    let Some(path) = theme_path(app) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last = stamp(&path);
        loop {
            std::thread::sleep(POLL_INTERVAL);
            let next = stamp(&path);
            if next == last {
                continue;
            }
            last = next;
            let _ = app.emit(CHANGED, read_theme(&path));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "monocode-external-theme-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_theme_returns_contents() {
        let dir = temp_dir("read");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, r##"{"background":"#111c18"}"##).unwrap();
        assert_eq!(
            read_theme(&path).as_deref(),
            Some(r##"{"background":"#111c18"}"##)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_theme_ignores_missing_and_oversized_files() {
        let dir = temp_dir("limits");
        let path = dir.join(FILE_NAME);
        assert_eq!(read_theme(&path), None);
        std::fs::write(&path, vec![b' '; MAX_THEME_BYTES as usize + 1]).unwrap();
        assert_eq!(read_theme(&path), None);
        assert_eq!(read_theme(&dir), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamp_changes_when_the_file_is_replaced() {
        let dir = temp_dir("stamp");
        let path = dir.join(FILE_NAME);
        assert_eq!(stamp(&path), None);
        std::fs::write(&path, "{}").unwrap();
        let first = stamp(&path);
        assert!(first.is_some());
        std::fs::write(&path, r#"{"appearance":"light"}"#).unwrap();
        assert_ne!(stamp(&path), first);
        let _ = std::fs::remove_dir_all(dir);
    }
}
