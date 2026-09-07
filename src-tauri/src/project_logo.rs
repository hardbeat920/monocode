use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::fs::expand_home;

const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;
const ALLOWED_EXT: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];

fn project_logos_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("project-logos");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Projects are keyed by their full path, which sanitizing alone cannot keep
/// apart: `a-b/c` and `a/b/c` both collapse to `a-b-c`, and a deep checkout can
/// outgrow the filesystem's name limit. So every stem carries a hash of the
/// whole key and keeps only as much of its readable tail as fits.
const MAX_STEM_CHARS: usize = 96;
/// `-` plus the hex hash.
const HASH_CHARS: usize = 17;

fn fnv1a(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn sanitize_project_key(project: &str) -> String {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return "project".into();
    }
    let safe: String = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let head = MAX_STEM_CHARS - HASH_CHARS;
    let readable: String = if safe.chars().count() <= head {
        safe
    } else {
        safe.chars().skip(safe.chars().count() - head).collect()
    };
    format!("{readable}-{:016x}", fnv1a(trimmed))
}

fn logo_stem(project: &str) -> String {
    sanitize_project_key(project)
}

fn remove_existing_logos(dir: &Path, project: &str) -> Result<(), String> {
    let stem = logo_stem(project);
    let prefix = format!("{stem}.");
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == stem || name.starts_with(&prefix) {
            std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn save_project_logo_sync(
    app: &AppHandle,
    project: &str,
    source_path: &str,
) -> Result<String, String> {
    let source = expand_home(source_path);
    let meta = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > MAX_LOGO_BYTES {
        return Err(format!(
            "Logo is too large (maximum {} MB).",
            MAX_LOGO_BYTES / 1024 / 1024
        ));
    }

    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        return Err("Logo must be a PNG, JPG, GIF, WebP, or SVG image.".into());
    }

    let dir = project_logos_dir(app)?;
    let stem = logo_stem(project);
    let dest = dir.join(format!("{stem}.{ext}"));
    let temp = dir.join(format!(".{stem}-upload"));

    // Copy before removing the old logo so re-selecting the saved file still works.
    std::fs::copy(&source, &temp).map_err(|e| format!("{}: {e}", temp.display()))?;
    remove_existing_logos(&dir, project)?;
    std::fs::rename(&temp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest.to_string_lossy().into_owned())
}

fn remove_project_logo_sync(app: &AppHandle, project: &str) -> Result<(), String> {
    let dir = project_logos_dir(app)?;
    remove_existing_logos(&dir, project)
}

#[tauri::command]
pub async fn save_project_logo(
    app: AppHandle,
    project: String,
    source_path: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_project_logo_sync(&app, &project, &source_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Drops a logo file the app itself copied in. Logos saved before the key scheme
/// changed live under a stem we can no longer derive, so the caller hands us the
/// path it had stored; anything outside the logo directory is ignored.
fn forget_logo_file_sync(app: &AppHandle, path: &str) -> Result<(), String> {
    let dir = project_logos_dir(app)?;
    let file = expand_home(path);
    if file.parent() != Some(dir.as_path()) || !file.is_file() {
        return Ok(());
    }
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn forget_logo_file(app: AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || forget_logo_file_sync(&app, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remove_project_logo(app: AppHandle, project: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || remove_project_logo_sync(&app, &project))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_project_key_replaces_unsafe_characters() {
        assert!(sanitize_project_key("agent-terminal").starts_with("agent-terminal-"));
        assert!(sanitize_project_key("foo/bar").starts_with("foo-bar-"));
        assert_eq!(sanitize_project_key("  "), "project");
    }

    #[test]
    fn sanitize_project_key_separates_paths_that_sanitize_alike() {
        // `-` survives and `/` becomes `-`, so only the hash tells these apart.
        assert_ne!(
            sanitize_project_key("/Users/me/cortex-finance/agentbase"),
            sanitize_project_key("/Users/me/cortex/finance/agentbase"),
        );
        // A sibling folder cannot be a filename prefix of its neighbour either.
        let stem = sanitize_project_key("/Users/me/proj");
        assert!(!sanitize_project_key("/Users/me/proj.old").starts_with(&format!("{stem}.")));
    }

    #[test]
    fn sanitize_project_key_shortens_long_paths_without_colliding() {
        let base = "/Users/me/".to_string() + &"deep/".repeat(40);
        let a = sanitize_project_key(&format!("{base}agentbase"));
        let b = sanitize_project_key(&format!("{base}other/agentbase"));
        assert!(a.chars().count() <= MAX_STEM_CHARS);
        assert!(b.chars().count() <= MAX_STEM_CHARS);
        assert_ne!(a, b);
        assert_eq!(a, sanitize_project_key(&format!("{base}agentbase")));
    }
}
