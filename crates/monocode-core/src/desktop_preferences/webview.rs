//! Read desktop webview localStorage without writing WebKit or WebView2 files.
use super::DesktopPreferences;
#[cfg(any(target_os = "macos", target_os = "linux", test))]
use rusqlite::{types::ValueRef, Connection, OpenFlags};
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::{collections::BTreeMap, path::Path};

impl DesktopPreferences {
    pub(super) fn load_installed(data_dir: &Path) -> Self {
        #[cfg(target_os = "macos")]
        {
            let _ = data_dir;
            crate::accounts::home()
                .map(|home| {
                    Self::load_webkit(
                        &home.join("Library/WebKit/com.monocode.desktop/WebsiteData/Default"),
                    )
                })
                .unwrap_or_default()
        }
        #[cfg(target_os = "linux")]
        {
            Self::load_webkitgtk(&data_dir.join("localstorage"))
        }
        #[cfg(windows)]
        {
            let _ = data_dir;
            Self::load_webview2()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        {
            let _ = data_dir;
            Self::default()
        }
    }

    /// WebKit stores origins under two opaque directory components. Do not
    /// merge localhost development origins or Inspector storage into the app.
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn load_webkit(root: &Path) -> Self {
        let mut paths = Vec::new();
        if let Ok(first) = std::fs::read_dir(root) {
            for directory in first.flatten() {
                if let Ok(second) = std::fs::read_dir(directory.path()) {
                    for origin in second.flatten() {
                        let path = origin.path();
                        if std::fs::read(path.join("origin"))
                            .ok()
                            .is_some_and(|bytes| desktop_origin(&bytes))
                        {
                            let database = path.join("LocalStorage/localstorage.sqlite3");
                            if database.is_file() {
                                paths.push(database);
                            }
                        }
                    }
                }
            }
        }
        paths.sort();
        paths
            .into_iter()
            .find_map(|path| {
                Self::read_database(&path)
                    .ok()
                    .filter(|prefs| prefs.available)
            })
            .unwrap_or_default()
    }

    #[cfg(any(target_os = "linux", test))]
    pub(super) fn load_webkitgtk(directory: &Path) -> Self {
        let mut ranked = Vec::new();
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if let Some(rank) = webkitgtk_filename_rank(name) {
                    ranked.push((rank, path));
                }
            }
        }
        ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        ranked
            .into_iter()
            .find_map(|(_, path)| {
                Self::read_database(&path)
                    .ok()
                    .filter(|prefs| prefs.available)
            })
            .unwrap_or_default()
    }

    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    fn read_database(path: &Path) -> Result<Self, String> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_millis(150))
            .map_err(|e| e.to_string())?;
        let mut query = conn
            .prepare("SELECT key,value FROM ItemTable WHERE key LIKE 'monocode%' ")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value = match row.get_ref(1)? {
                    ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
                    ValueRef::Blob(bytes) => decode_webkit_value(bytes),
                    _ => None,
                };
                Ok((key, value))
            })
            .map_err(|e| e.to_string())?;
        let mut values = BTreeMap::new();
        for row in rows {
            let (key, value) = row.map_err(|e| e.to_string())?;
            if let Some(value) = value {
                values.insert(key, value);
            }
        }
        Ok(Self {
            available: !values.is_empty(),
            values,
        })
    }

    #[cfg(any(windows, test))]
    pub(super) fn load_webview2() -> Self {
        let Some(base) = std::env::var_os("LOCALAPPDATA") else {
            return Self::default();
        };
        let root = PathBuf::from(base).join("com.monocode.desktop");
        Self::load_webview2_from(&root)
    }

    #[cfg(any(windows, test))]
    fn load_webview2_from(root: &Path) -> Self {
        for relative in [
            "EBWebView/Default/Local Storage/leveldb",
            "Default/Local Storage/leveldb",
        ] {
            let path = root.join(relative);
            if !path.is_dir() {
                continue;
            }
            match Self::read_chromium_localstorage(&path) {
                Ok(loaded) if loaded.available => return loaded,
                Ok(_) => {}
                Err(error) => eprintln!(
                    "Could not read desktop WebView2 preferences from {}: {error}",
                    path.display()
                ),
            }
        }
        Self::default()
    }

    #[cfg(any(windows, test))]
    pub(super) fn read_chromium_localstorage(path: &Path) -> Result<Self, String> {
        // LevelDB opens can write recovery data and logs, even before taking its lock.
        let staging = tempfile::Builder::new()
            .prefix("monocode-webview-ls-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        copy_leveldb(path, staging.path())?;
        // The LevelDB reader asserts on malformed table footers and other corrupt
        // input. Keep the database and its iterators inside the unwind boundary:
        // a best-effort copy must fail like an I/O error, not terminate startup.
        std::panic::catch_unwind(|| Self::open_chromium_localstorage(staging.path()))
            .unwrap_or_else(|_| Err("WebView2 localStorage is damaged or incomplete".into()))
    }

    #[cfg(any(windows, test))]
    fn open_chromium_localstorage(path: &Path) -> Result<Self, String> {
        Self::open_chromium_with(path, rusty_leveldb::CompressionType::CompressionSnappy).or_else(
            |_| Self::open_chromium_with(path, rusty_leveldb::CompressionType::CompressionNone),
        )
    }

    #[cfg(any(windows, test))]
    fn open_chromium_with(
        path: &Path,
        compression_type: rusty_leveldb::CompressionType,
    ) -> Result<Self, String> {
        let options = rusty_leveldb::Options {
            create_if_missing: false,
            error_if_exists: false,
            compression_type,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(path, options).map_err(|e| e.to_string())?;
        let mut iter = db.new_iter().map_err(|e| e.to_string())?;
        let mut best_rank = u8::MAX;
        let mut values = BTreeMap::new();
        while let Some((key, value)) = rusty_leveldb::LdbIterator::next(&mut iter) {
            if let (Some(rank), Some((key, value))) = (
                chromium_item_rank(&key),
                chromium_monocode_entry(&key, &value),
            ) {
                if rank < best_rank {
                    values.clear();
                    best_rank = rank;
                }
                if rank == best_rank {
                    values.insert(key, value);
                }
            }
        }
        Ok(Self {
            available: !values.is_empty(),
            values,
        })
    }
}

fn origin_rank(scheme: &str, host: &str) -> Option<u8> {
    match (scheme, host) {
        ("tauri", "localhost") => Some(0),
        ("https", "tauri.localhost") => Some(1),
        ("http", "tauri.localhost") => Some(2),
        _ => None,
    }
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn webkitgtk_filename_rank(name: &str) -> Option<u8> {
    let stem = name.strip_suffix(".localstorage")?;
    let (rest, port) = stem.rsplit_once('_')?;
    let port: u16 = port.parse().ok()?;
    let (scheme, host) = rest.split_once('_')?;
    if port != 0 && !matches!((scheme, port), ("http", 80) | ("https", 443)) {
        return None;
    }
    origin_rank(scheme, host)
}

pub(super) fn decode_webkit_value(bytes: &[u8]) -> Option<String> {
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    String::from_utf16(
        &pairs
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect::<Vec<_>>(),
    )
    .ok()
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn desktop_origin(mut bytes: &[u8]) -> bool {
    fn string(bytes: &mut &[u8]) -> Option<String> {
        let length = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
        let ascii = *bytes.get(4)? == 1;
        *bytes = bytes.get(5..)?;
        if !ascii {
            return None;
        }
        let value = std::str::from_utf8(bytes.get(..length)?).ok()?.to_owned();
        *bytes = bytes.get(length..)?;
        Some(value)
    }
    let Some(scheme) = string(&mut bytes) else {
        return false;
    };
    let Some(host) = string(&mut bytes) else {
        return false;
    };
    origin_rank(&scheme, &host).is_some()
}

#[cfg(any(windows, test))]
fn copy_leveldb(src: &Path, dest: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == "LOCK" || name == "LOG" || name == "LOG.old" {
            continue;
        }
        std::fs::copy(entry.path(), dest.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(any(windows, test))]
pub(super) fn chromium_origin_rank(origin: &str) -> Option<u8> {
    let origin = origin.trim_end_matches('/');
    let (scheme, rest) = origin.split_once("://")?;
    let host = match rest.split_once(':') {
        Some((host, port)) if matches!((scheme, port), ("http", "80") | ("https", "443")) => host,
        Some(_) => return None,
        None => rest,
    };
    origin_rank(scheme, host)
}

#[cfg(any(windows, test))]
fn chromium_item_rank(key: &[u8]) -> Option<u8> {
    chromium_origin_rank(chromium_item_origin(key)?.0)
}

#[cfg(any(windows, test))]
fn chromium_item_origin(key: &[u8]) -> Option<(&str, &[u8])> {
    let key = key.strip_prefix(b"_")?;
    let split = key.iter().position(|&byte| byte == 0)?;
    let origin = std::str::from_utf8(key.get(..split)?)
        .ok()?
        .trim_end_matches('/');
    Some((origin, key.get(split + 1..)?))
}

#[cfg(any(windows, test))]
fn chromium_monocode_entry(key: &[u8], value: &[u8]) -> Option<(String, String)> {
    let script = chromium_item_origin(key)?.1;
    let name = decode_chromium_string(script)?;
    if !name.starts_with("monocode") {
        return None;
    }
    Some((name, decode_chromium_string(value)?))
}

#[cfg(any(windows, test))]
pub(super) fn decode_chromium_string(bytes: &[u8]) -> Option<String> {
    let (&prefix, rest) = bytes.split_first()?;
    match prefix {
        0 => decode_webkit_value(rest),
        1 => Some(rest.iter().map(|&byte| char::from(byte)).collect()),
        _ => std::str::from_utf8(bytes)
            .ok()
            .map(str::to_owned)
            .or_else(|| decode_webkit_value(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn utf16(value: &str) -> Vec<u8> {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn write_localstorage(path: &Path, key: &str, value: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE ItemTable(key TEXT,value BLOB)")
            .unwrap();
        conn.execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            rusqlite::params![key, utf16(value)],
        )
        .unwrap();
    }

    #[test]
    fn webkitgtk_filenames_keep_packaged_origins_and_drop_vite() {
        assert_eq!(
            webkitgtk_filename_rank("tauri_localhost_0.localstorage"),
            Some(0)
        );
        assert_eq!(
            webkitgtk_filename_rank("https_tauri.localhost_0.localstorage"),
            Some(1)
        );
        assert_eq!(
            webkitgtk_filename_rank("http_tauri.localhost_0.localstorage"),
            Some(2)
        );
        assert_eq!(
            webkitgtk_filename_rank("http_localhost_1420.localstorage"),
            None
        );
        assert_eq!(
            webkitgtk_filename_rank("http_localhost_0.localstorage"),
            None
        );
        assert_eq!(webkitgtk_filename_rank("localstorage.sqlite3"), None);
    }

    #[test]
    fn linux_localstorage_prefers_the_packaged_origin() {
        let root = tempfile::tempdir().unwrap();
        write_localstorage(
            &root.path().join("http_localhost_1420.localstorage"),
            "monocode.workspaceTheme",
            "ember",
        );
        write_localstorage(
            &root.path().join("tauri_localhost_0.localstorage"),
            "monocode.workspaceTheme",
            "tidepool",
        );
        let preferences = DesktopPreferences::load_webkitgtk(root.path());
        assert_eq!(preferences.get("monocode.workspaceTheme"), Some("tidepool"));
        assert!(preferences.available);
        let mut workspace = serde_json::json!({});
        preferences.apply_workspace(&mut workspace);
        assert_eq!(workspace["preferences"]["workspace_theme"], "tidepool");
    }

    #[test]
    fn webkitgtk_empty_origin_does_not_hide_saved_projects() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("tauri_localhost_0.localstorage");
        write_localstorage(&empty, "unrelated", "value");
        let unavailable = DesktopPreferences::read_database(&empty).unwrap();
        assert!(!unavailable.is_available());
        let mut workspace = serde_json::json!({"preferences": {"workspace_theme": "ember"}});
        let original = workspace.clone();
        unavailable.apply_workspace(&mut workspace);
        assert_eq!(workspace, original);

        let projects: Vec<_> = (0..8)
            .map(|i| serde_json::json!({"path": format!("/code/project-{i}")}))
            .collect();
        write_localstorage(
            &root.path().join("https_tauri.localhost_0.localstorage"),
            "monocode.recentProjects",
            &serde_json::to_string(&projects).unwrap(),
        );
        let loaded = DesktopPreferences::load_webkitgtk(root.path());
        assert_eq!(
            loaded.json("monocode.recentProjects"),
            Some(serde_json::json!(projects))
        );
        loaded.apply_workspace(&mut workspace);
        assert_eq!(workspace["projects"].as_array().unwrap().len(), 8);
    }

    #[test]
    fn chromium_origins_match_tauri_webview2() {
        assert_eq!(chromium_origin_rank("tauri://localhost"), Some(0));
        assert_eq!(chromium_origin_rank("https://tauri.localhost"), Some(1));
        assert_eq!(chromium_origin_rank("https://tauri.localhost/"), Some(1));
        assert_eq!(chromium_origin_rank("http://tauri.localhost"), Some(2));
        assert_eq!(chromium_origin_rank("http://localhost:1420"), None);
        assert_eq!(chromium_origin_rank("https://localhost"), None);
        assert_eq!(chromium_origin_rank("http://tauri.localhost:1234"), None);
        assert_eq!(chromium_origin_rank("https://tauri.localhost:443"), Some(1));
        assert_eq!(chromium_origin_rank("http://tauri.localhost:80"), Some(2));
        assert_eq!(chromium_origin_rank("https://tauri.localhost/path"), None);
        assert_eq!(
            webkitgtk_filename_rank("http_tauri.localhost_1234.localstorage"),
            None
        );
    }

    #[test]
    fn chromium_strings_accept_type_prefixes_and_plain_utf8() {
        let mut utf16_prefixed = vec![0];
        utf16_prefixed.extend(utf16("monocode.colorScheme"));
        assert_eq!(
            decode_chromium_string(&utf16_prefixed).as_deref(),
            Some("monocode.colorScheme")
        );
        let mut latin1 = vec![1];
        latin1.extend(b"tidepool");
        assert_eq!(decode_chromium_string(&latin1).as_deref(), Some("tidepool"));
        assert_eq!(
            decode_chromium_string(b"monocode.workspaceTheme").as_deref(),
            Some("monocode.workspaceTheme")
        );
    }

    #[test]
    fn webview2_loader_is_quiet_without_an_edge_profile() {
        let loaded = DesktopPreferences::load_webview2();
        if !loaded.available {
            assert!(loaded.values.is_empty());
        }
    }

    fn assert_webview2_read_preserves_source(keep_open: bool) {
        let root = tempfile::tempdir().unwrap();
        let options = rusty_leveldb::Options {
            create_if_missing: true,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(root.path(), options).unwrap();
        let mut vite_key = b"_http://localhost:1420\x00".to_vec();
        vite_key.extend(b"\x01monocode.workspaceTheme");
        db.put(&vite_key, b"\x01ember").unwrap();
        let mut key = b"_https://tauri.localhost\x00".to_vec();
        key.extend(b"\x01monocode.workspaceTheme");
        db.put(&key, b"\x01tidepool").unwrap();
        // An origin with no Monocode settings must not shadow the populated one.
        db.put(b"_tauri://localhost\x00\x01unrelated", b"\x01value")
            .unwrap();
        db.flush().unwrap();
        let source = keep_open.then_some(db);
        let before = snapshot_leveldb(root.path());
        let preferences = DesktopPreferences::read_chromium_localstorage(root.path()).unwrap();
        assert_eq!(preferences.get("monocode.workspaceTheme"), Some("tidepool"));
        assert!(preferences.available);
        assert_eq!(
            snapshot_leveldb(root.path()),
            before,
            "source WebView2 files changed"
        );
        let mut workspace = serde_json::json!({});
        preferences.apply_workspace(&mut workspace);
        assert_eq!(workspace["preferences"]["workspace_theme"], "tidepool");
        drop(source);
    }

    fn snapshot_leveldb(path: &Path) -> BTreeMap<std::ffi::OsString, Option<Vec<u8>>> {
        std::fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name();
                // Windows denies reads of the byte-range-locked LOCK file.
                let contents = (name != "LOCK").then(|| std::fs::read(entry.path()).unwrap());
                (name, contents)
            })
            .collect()
    }

    #[test]
    fn webview2_corrupt_tables_preserve_source_and_native_workspace() {
        for short_table in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("EBWebView/Default/Local Storage/leveldb");
            std::fs::create_dir_all(&path).unwrap();
            let mut db = rusty_leveldb::DB::open(&path, Default::default()).unwrap();
            db.put(
                b"_https://tauri.localhost\x00\x01monocode.workspaceTheme",
                b"\x01tidepool",
            )
            .unwrap();
            // Existing fixtures only exercise WAL recovery. Flush this setting
            // into a table so the importer must parse its footer.
            db.compact_range(b"", b"\xff").unwrap();
            db.close().unwrap();
            drop(db);
            let mut tables = 0;
            for entry in std::fs::read_dir(&path).unwrap() {
                let table = entry.unwrap().path();
                if table
                    .extension()
                    .is_some_and(|extension| extension == "ldb")
                {
                    let length = std::fs::metadata(&table).unwrap().len();
                    assert!(length > 56);
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(table)
                        .unwrap()
                        .set_len(if short_table { 17 } else { length - 8 })
                        .unwrap();
                    tables += 1;
                }
            }
            assert!(tables > 0);
            let before = snapshot_leveldb(&path);
            assert!(DesktopPreferences::read_chromium_localstorage(&path).is_err());

            let preferences = DesktopPreferences::load_webview2_from(root.path());
            assert!(!preferences.is_available());
            let mut workspace = serde_json::json!({
                "projects": ["/code/native"],
                "project": "/code/native",
                "preferences": {"workspace_theme": "ember"}
            });
            let original = workspace.clone();
            preferences.apply_workspace(&mut workspace);
            assert_eq!(workspace, original);
            assert_eq!(
                snapshot_leveldb(&path),
                before,
                "source WebView2 files changed"
            );
        }
    }

    #[test]
    fn webview2_empty_origin_is_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let mut db = rusty_leveldb::DB::open(root.path(), Default::default()).unwrap();
        db.put(b"_https://tauri.localhost\x00\x01unrelated", b"\x01value")
            .unwrap();
        db.flush().unwrap();
        drop(db);
        let loaded = DesktopPreferences::read_chromium_localstorage(root.path()).unwrap();
        assert!(!loaded.is_available());
        assert!(loaded.values.is_empty());
    }

    #[test]
    fn webview2_leveldb_preserves_unlocked_source_and_ignores_vite() {
        assert_webview2_read_preserves_source(false);
    }

    #[test]
    fn webview2_leveldb_preserves_locked_source_and_ignores_vite() {
        assert_webview2_read_preserves_source(true);
    }
}
