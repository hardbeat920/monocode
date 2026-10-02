use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::fs::expand_home;

const MAX_BACKGROUND_BYTES: u64 = 25 * 1024 * 1024;
const ALLOWED_EXT: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

fn backgrounds_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("backgrounds");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn remove_existing_backgrounds(dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == "chat-background" || name.starts_with("chat-background.") {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(())
}

fn project_background_stem(project: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in project.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("project-{hash:016x}")
}

fn remove_project_background(dir: &Path, project: &str) -> Result<(), String> {
    let stem = project_background_stem(project);
    let prefix = format!("{stem}.");
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == stem || name.starts_with(&prefix) {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(())
}

fn background_extension(source: &Path) -> Result<String, String> {
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ALLOWED_EXT.contains(&ext.as_str()) {
        Ok(ext)
    } else {
        Err("Background must be a PNG, JPG, GIF, or WebP image.".into())
    }
}

fn save_chat_background_sync(app: &AppHandle, source_path: &str) -> Result<String, String> {
    let source = expand_home(source_path);
    let meta = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > MAX_BACKGROUND_BYTES {
        return Err(format!(
            "Background is too large (maximum {} MB).",
            MAX_BACKGROUND_BYTES / 1024 / 1024
        ));
    }
    let ext = background_extension(&source)?;
    let dir = backgrounds_dir(app)?;
    let dest = dir.join(format!("chat-background.{ext}"));
    let temp = dir.join(".chat-background-upload");

    // Copy first so choosing the currently saved image remains safe.
    std::fs::copy(&source, &temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    remove_existing_backgrounds(&dir)?;
    std::fs::rename(&temp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest.to_string_lossy().into_owned())
}

fn save_project_chat_background_sync(
    app: &AppHandle,
    project: &str,
    source_path: &str,
) -> Result<String, String> {
    if project.trim().is_empty() {
        return Err("Project is required".into());
    }
    let source = expand_home(source_path);
    let meta = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > MAX_BACKGROUND_BYTES {
        return Err(format!(
            "Background is too large (maximum {} MB).",
            MAX_BACKGROUND_BYTES / 1024 / 1024
        ));
    }
    let ext = background_extension(&source)?;
    let dir = backgrounds_dir(app)?;
    let stem = project_background_stem(project);
    let dest = dir.join(format!("{stem}.{ext}"));
    let temp = dir.join(format!(".{stem}-upload"));

    std::fs::copy(&source, &temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    remove_project_background(&dir, project)?;
    std::fs::rename(&temp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn save_chat_background(app: AppHandle, source_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || save_chat_background_sync(&app, &source_path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_chat_background(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = backgrounds_dir(&app)?;
        remove_existing_backgrounds(&dir)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_project_chat_background(
    app: AppHandle,
    project: String,
    source_path: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_project_chat_background_sync(&app, &project, &source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_project_chat_background(app: AppHandle, project: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = backgrounds_dir(&app)?;
        remove_project_background(&dir, &project)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_extensions_case_insensitively() {
        assert_eq!(
            background_extension(Path::new("wallpaper.JPEG")).unwrap(),
            "jpeg"
        );
        assert_eq!(
            background_extension(Path::new("wallpaper.webp")).unwrap(),
            "webp"
        );
    }

    #[test]
    fn rejects_files_the_webview_cannot_render_as_backgrounds() {
        assert!(background_extension(Path::new("wallpaper.txt")).is_err());
        assert!(background_extension(Path::new("wallpaper")).is_err());
    }

    #[test]
    fn project_background_stems_are_stable_and_distinct() {
        assert_eq!(
            project_background_stem("/Users/me/agent"),
            project_background_stem("/Users/me/agent")
        );
        assert_ne!(
            project_background_stem("/Users/me/agent"),
            project_background_stem("/Users/other/agent")
        );
    }
}
