use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, State};

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_SESSION_BYTES: u64 = 64 * 1024 * 1024;
static ASSET_WRITES: Mutex<()> = Mutex::new(());

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssetSource {
    id: String,
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssetSnapshot {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<String>,
}

#[tauri::command]
pub async fn session_context_assets(
    app: AppHandle,
    store: State<'_, crate::session_store::SessionStore>,
    session_id: String,
    attachments: Vec<ContextAssetSource>,
) -> Result<Vec<ContextAssetSnapshot>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let connection = store.shared_conn();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = connection.lock().map_err(|_| "Session store is locked")?;
        snapshot_stored_assets(&conn, &root, &session_id, attachments)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn snapshot_stored_assets(
    conn: &rusqlite::Connection,
    root: &Path,
    session_id: &str,
    attachments: Vec<ContextAssetSource>,
) -> Result<Vec<ContextAssetSnapshot>, String> {
    crate::session_store::ensure_context_writable(conn, session_id)
        .map_err(|error| error.to_string())?;
    snapshot_assets(root, session_id, attachments)
}

fn snapshot_assets(
    root: &Path,
    session_id: &str,
    attachments: Vec<ContextAssetSource>,
) -> Result<Vec<ContextAssetSnapshot>, String> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Invalid session id for historical attachment storage".into());
    }
    let _guard = ASSET_WRITES
        .lock()
        .map_err(|_| "Historical attachment storage is locked")?;
    let assets = root.join("context-history").join(session_id).join("assets");
    fs::create_dir_all(&assets).map_err(|error| error.to_string())?;
    let mut total = saved_asset_bytes(&assets)?;
    let index_path = assets.with_file_name("assets.index.json");
    let mut by_id = read_asset_index(&index_path)?;
    let mut results = Vec::with_capacity(attachments.len());
    for source in attachments {
        if let Some(saved) = by_id
            .get(&source.id)
            .filter(|saved| saved.id == source.id && valid_cached_asset(&assets, saved))
        {
            results.push(saved.clone());
            continue;
        }
        let saved = match snapshot_one(&assets, &source, &mut total) {
            Ok((path, hash)) => ContextAssetSnapshot {
                id: source.id.clone(),
                path: Some(path.to_string_lossy().into_owned()),
                sha256: Some(hash),
                unavailable_reason: None,
            },
            Err(reason) => ContextAssetSnapshot {
                id: source.id.clone(),
                path: None,
                sha256: None,
                unavailable_reason: Some(reason),
            },
        };
        by_id.insert(source.id, saved.clone());
        results.push(saved);
    }
    write_asset_index(&index_path, &by_id)?;
    Ok(results)
}

fn read_asset_index(path: &Path) -> Result<HashMap<String, ContextAssetSnapshot>, String> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("Historical attachment index is not a bounded regular file".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Historical attachment index is invalid".into())
}

fn valid_cached_asset(directory: &Path, snapshot: &ContextAssetSnapshot) -> bool {
    let (Some(path), Some(hash)) = (&snapshot.path, &snapshot.sha256) else {
        return false;
    };
    let path = Path::new(path);
    hash.len() == 64
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && path.parent() == Some(directory)
        && path.file_stem().and_then(|stem| stem.to_str()) == Some(hash.as_str())
        && verify_saved_asset(path, hash).is_ok()
}

fn write_asset_index(
    path: &Path,
    index: &HashMap<String, ContextAssetSnapshot>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(index).map_err(|error| error.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Historical attachment index exceeds the 4 MiB metadata limit".into());
    }
    let temporary = path.with_file_name(format!(".{}.index.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), String> {
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn saved_asset_bytes(directory: &Path) -> Result<u64, String> {
    let mut total = 0u64;
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn snapshot_one(
    directory: &Path,
    source: &ContextAssetSource,
    total: &mut u64,
) -> Result<(PathBuf, String), String> {
    let bytes = read_source(source)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let extension = Path::new(&source.name)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 12
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let target = directory.join(format!("{hash}.{extension}"));
    if target.exists() {
        verify_saved_asset(&target, &hash)?;
        return Ok((target, hash));
    }
    if total.saturating_add(bytes.len() as u64) > MAX_SESSION_BYTES {
        return Err("Historical attachment storage exceeds the 64 MiB session limit".into());
    }
    let temporary = directory.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = write_immutable_asset(&temporary, &target, &bytes, &hash);
    let _ = fs::remove_file(&temporary);
    result?;
    *total = total.saturating_add(bytes.len() as u64);
    Ok((target, hash))
}

fn read_source(source: &ContextAssetSource) -> Result<Vec<u8>, String> {
    if source.data.is_none() {
        if let Some(path) = &source.path {
            let path = Path::new(path);
            if !path.is_absolute() {
                return Err("Historical attachment path is not absolute".into());
            }
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| format!("Historical attachment is unavailable. {error}"))?;
            if !metadata.is_file() {
                return Err("Historical attachment path is not a regular file".into());
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err("Historical attachment exceeds the 20 MiB file limit".into());
            }
            let file = File::open(path)
                .map_err(|error| format!("Historical attachment is unavailable. {error}"))?;
            let mut bytes = Vec::with_capacity(metadata.len() as usize);
            file.take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err("Historical attachment exceeds the 20 MiB file limit".into());
            }
            return Ok(bytes);
        }
    }
    let Some(data) = &source.data else {
        return Err("Historical attachment has no accessible file or saved bytes".into());
    };
    let encoded = if data.starts_with("data:") {
        data.split_once(',')
            .filter(|(header, _)| header.ends_with(";base64"))
            .map(|(_, encoded)| encoded)
            .ok_or("Historical attachment data URL is not base64")?
    } else {
        data.as_str()
    };
    if encoded.len() as u64 > MAX_FILE_BYTES.div_ceil(3) * 4 {
        return Err("Historical attachment exceeds the 20 MiB file limit".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Historical attachment data is not valid base64".to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("Historical attachment exceeds the 20 MiB file limit".into());
    }
    Ok(bytes)
}

fn verify_saved_asset(path: &Path, expected_hash: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err("Saved historical attachment is not a bounded regular file".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if format!("{:x}", Sha256::digest(bytes)) != expected_hash {
        return Err("Saved historical attachment content does not match its hash".into());
    }
    Ok(())
}

fn write_immutable_asset(
    temporary: &Path,
    target: &Path,
    bytes: &[u8],
    expected_hash: &str,
) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temporary).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    match fs::hard_link(temporary, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_saved_asset(target, expected_hash)
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("monocode-context-assets-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn inline(id: &str, bytes: &[u8]) -> ContextAssetSource {
        ContextAssetSource {
            id: id.into(),
            name: "image.png".into(),
            path: None,
            data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        }
    }

    #[test]
    fn deleted_session_cannot_recreate_historical_assets() {
        let fixture = Fixture::new();
        let store = crate::session_store::SessionStore::open_in_memory().unwrap();
        let conn = store.lock_conn().unwrap();
        conn.execute("INSERT INTO context_history_cleanup (session_id, pending) VALUES ('deleted-session', 0)", []).unwrap();
        assert!(snapshot_stored_assets(
            &conn,
            &fixture.0,
            "deleted-session",
            vec![inline("late", b"Late asset")]
        )
        .is_err());
        assert!(!fixture.0.join("context-history/deleted-session").exists());
    }

    #[test]
    fn snapshots_content_by_hash_and_survives_source_deletion() {
        let fixture = Fixture::new();
        let source_path = fixture.0.join("original.txt");
        fs::write(&source_path, b"Exact original bytes").unwrap();
        let source = ContextAssetSource {
            id: "attachment-1".into(),
            name: "original.txt".into(),
            path: Some(source_path.to_string_lossy().into_owned()),
            data: None,
        };
        let snapshot = snapshot_assets(&fixture.0, "session-1", vec![source.clone()])
            .unwrap()
            .remove(0);
        fs::remove_file(&source_path).unwrap();
        let saved_path = Path::new(snapshot.path.as_ref().unwrap());
        assert_eq!(fs::read(saved_path).unwrap(), b"Exact original bytes");
        assert_eq!(
            snapshot.sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest(b"Exact original bytes")).as_str())
        );
        assert!(saved_path.starts_with(fixture.0.join("context-history/session-1/assets")));
        let recovered = snapshot_assets(&fixture.0, "session-1", vec![source])
            .unwrap()
            .remove(0);
        assert_eq!(recovered.path, snapshot.path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(saved_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn deduplicates_identical_content_and_keeps_original_source() {
        let fixture = Fixture::new();
        let first =
            snapshot_assets(&fixture.0, "session-1", vec![inline("a", b"same bytes")]).unwrap();
        let second =
            snapshot_assets(&fixture.0, "session-1", vec![inline("b", b"same bytes")]).unwrap();
        assert_eq!(first[0].path, second[0].path);
        assert_eq!(
            fs::read_dir(fixture.0.join("context-history/session-1/assets"))
                .unwrap()
                .count(),
            1
        );
        let changed = snapshot_assets(
            &fixture.0,
            "session-1",
            vec![inline("a", b"new original bytes")],
        )
        .unwrap();
        assert_eq!(changed[0].path, first[0].path);
    }

    #[test]
    fn reports_missing_non_file_and_invalid_data_without_inventing_a_path() {
        let fixture = Fixture::new();
        let sources = vec![
            ContextAssetSource {
                id: "missing".into(),
                name: "missing.png".into(),
                path: Some(fixture.0.join("missing.png").to_string_lossy().into_owned()),
                data: None,
            },
            ContextAssetSource {
                id: "directory".into(),
                name: "folder".into(),
                path: Some(fixture.0.to_string_lossy().into_owned()),
                data: None,
            },
            ContextAssetSource {
                id: "relative".into(),
                name: "relative.txt".into(),
                path: Some("relative.txt".into()),
                data: None,
            },
            ContextAssetSource {
                id: "invalid".into(),
                name: "invalid.png".into(),
                path: None,
                data: Some("not base64!".into()),
            },
        ];
        for snapshot in snapshot_assets(&fixture.0, "session-1", sources).unwrap() {
            assert!(snapshot.path.is_none());
            assert!(snapshot.sha256.is_none());
            assert!(snapshot.unavailable_reason.is_some());
        }
    }

    #[test]
    fn rejects_traversal_session_ids_before_creating_any_files() {
        let fixture = Fixture::new();
        assert!(snapshot_assets(&fixture.0, "../outside", vec![inline("a", b"bytes")]).is_err());
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }

    #[test]
    fn enforces_the_file_limit_and_session_total_limit() {
        let fixture = Fixture::new();
        let large = fixture.0.join("large.bin");
        File::create(&large)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let snapshot = snapshot_assets(
            &fixture.0,
            "session-1",
            vec![ContextAssetSource {
                id: "large".into(),
                name: "large.bin".into(),
                path: Some(large.to_string_lossy().into_owned()),
                data: None,
            }],
        )
        .unwrap()
        .remove(0);
        assert!(snapshot.unavailable_reason.unwrap().contains("20 MiB"));
        let directory = fixture.0.join("context-history/session-1/assets");
        File::create(directory.join("existing.bin"))
            .unwrap()
            .set_len(MAX_SESSION_BYTES)
            .unwrap();
        let snapshot = snapshot_assets(
            &fixture.0,
            "session-1",
            vec![inline("another", b"new bytes")],
        )
        .unwrap()
        .remove(0);
        assert!(snapshot.unavailable_reason.unwrap().contains("64 MiB"));
        assert!(snapshot.path.is_none());
    }

    #[test]
    fn refuses_to_replace_a_corrupted_immutable_asset() {
        let fixture = Fixture::new();
        let saved = snapshot_assets(&fixture.0, "session-1", vec![inline("a", b"good bytes")])
            .unwrap()
            .remove(0);
        fs::write(saved.path.as_ref().unwrap(), b"corrupted").unwrap();
        let retried = snapshot_assets(&fixture.0, "session-1", vec![inline("b", b"good bytes")])
            .unwrap()
            .remove(0);
        assert!(retried.unavailable_reason.unwrap().contains("hash"));
        assert_eq!(fs::read(saved.path.unwrap()).unwrap(), b"corrupted");
    }
}
