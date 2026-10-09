use super::QuickAttachment;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[derive(Default)]
pub struct Captures(pub Mutex<HashSet<PathBuf>>);

fn root() -> PathBuf {
    std::env::temp_dir().join("monocode-captures")
}

pub fn new_path() -> Result<PathBuf, String> {
    let dir = root().join(format!("{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir.join("Screenshot.png"))
}

fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir(dir);
    }
}

pub fn register(app: &AppHandle, path: &str) {
    if let Ok(mut owned) = app.state::<Captures>().0.lock() {
        owned.insert(path.into());
    }
}

fn release(owned: &mut HashSet<PathBuf>, paths: &[String]) {
    for path in paths {
        let path = PathBuf::from(path);
        // Never unlink uploads, persistent session attachments, or arbitrary IPC paths.
        if owned.remove(&path) {
            remove(&path);
        }
    }
}

pub(super) fn discard(app: &AppHandle, path: &str) {
    if let Ok(mut owned) = app.state::<Captures>().0.lock() {
        release(&mut owned, &[path.to_owned()]);
    }
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
    release(&mut *state.0.lock().map_err(|err| err.to_string())?, &paths);
    Ok(())
}

// Stage copies first. If any copy fails the draft's originals remain usable.
fn persist_to(
    owned: &mut HashSet<PathBuf>,
    files: &mut [QuickAttachment],
    destination: &Path,
) -> Result<(), String> {
    let mut copies = Vec::new();
    let result = (|| {
        for (index, file) in files.iter().enumerate() {
            if !owned.contains(Path::new(&file.path)) {
                continue;
            }
            let dir = destination.join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
            let path = dir.join("Screenshot.png");
            copies.push((index, path.clone()));
            std::fs::copy(&file.path, &path).map_err(|err| err.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        for (_, path) in copies {
            remove(&path);
        }
        return result;
    }
    for (index, path) in copies {
        let old = std::mem::replace(&mut files[index].path, path.to_string_lossy().into_owned());
        release(owned, &[old]);
    }
    Ok(())
}

pub(super) fn persist(app: &AppHandle, files: &mut [QuickAttachment]) -> Result<(), String> {
    let destination = app
        .path()
        .app_data_dir()
        .map_err(|err| err.to_string())?
        .join("attachments/quick-captures");
    persist_to(
        &mut *app
            .state::<Captures>()
            .0
            .lock()
            .map_err(|err| err.to_string())?,
        files,
        &destination,
    )
}

pub fn cleanup_abandoned() {
    // Discard captures left by crashed/quit processes after 24 hours. A live
    // process owns its drafts regardless of their age. Never follow symlinks.
    sweep(&root(), SystemTime::now(), |pid| unsafe {
        libc::kill(pid as i32, 0) == 0
    });
}

fn sweep(root: &Path, now: SystemTime, alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((pid, id)) = name.to_str().and_then(|name| name.split_once('-')) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() || alive(pid) {
            continue;
        }
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if !meta.is_dir() || meta.is_symlink() {
            continue;
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age >= Duration::from_secs(24 * 60 * 60));
        if old {
            remove(&entry.path().join("Screenshot.png"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!("capture-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
    fn attachment(path: &Path) -> QuickAttachment {
        QuickAttachment {
            id: "shot".into(),
            name: "Screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: "image".into(),
            size: 4,
            path: path.to_string_lossy().into_owned(),
        }
    }
    #[test]
    fn release_only_deletes_owned_captures_and_promotion_retains_session_copy() {
        let root = temp();
        let capture = root.join("capture/Screenshot.png");
        std::fs::create_dir_all(capture.parent().unwrap()).unwrap();
        std::fs::write(&capture, b"test").unwrap();
        let upload = root.join("uploaded.png");
        std::fs::write(&upload, b"user").unwrap();
        let mut owned = HashSet::from([capture.clone()]);
        release(&mut owned, &[upload.to_string_lossy().into_owned()]);
        assert!(upload.exists());
        let mut files = vec![attachment(&capture), attachment(&upload)];
        persist_to(&mut owned, &mut files, &root.join("persistent")).unwrap();
        assert!(!capture.exists());
        assert_eq!(std::fs::read(&files[0].path).unwrap(), b"test");
        release(&mut owned, &[files[0].path.clone()]);
        assert!(Path::new(&files[0].path).exists());
        assert_eq!(files[1].path, upload.to_string_lossy());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_promotion_keeps_the_original_for_retry() {
        let root = temp();
        let capture = root.join("Screenshot.png");
        std::fs::write(&capture, b"test").unwrap();
        let destination = root.join("not-a-directory");
        std::fs::write(&destination, b"x").unwrap();
        let mut owned = HashSet::from([capture.clone()]);
        let mut files = vec![attachment(&capture)];
        assert!(persist_to(&mut owned, &mut files, &destination).is_err());
        assert!(owned.contains(&capture));
        assert!(capture.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sweep_retains_recent_live_and_unrecognized_files() {
        let root = temp();
        let dead = root.join(format!("1-{}", uuid::Uuid::new_v4()));
        let live = root.join(format!("2-{}", uuid::Uuid::new_v4()));
        let other = root.join("user-folder");
        for dir in [&dead, &live, &other] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("Screenshot.png"), b"test").unwrap();
        }
        sweep(&root, SystemTime::now(), |_| false);
        assert!(dead.exists());
        sweep(
            &root,
            SystemTime::now() + Duration::from_secs(48 * 60 * 60),
            |pid| pid == 2,
        );
        assert!(!dead.exists());
        assert!(live.exists());
        assert!(other.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
