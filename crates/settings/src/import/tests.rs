use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use monocode_core::{AppSettings, Platform};
use rusqlite::Connection;
use rusqlite::config::DbConfig;

use super::webkit::{legacy_origin, parse_origin_file};
use super::webview2::fixture;
use super::*;
use crate::kv::Kv;
use crate::temp::TempDir;

/// The `origin` files this machine's WebKit wrote for the packaged app and
/// the dev server.
const PRODUCTION_ORIGIN_FILE: [u8; 50] = [
    0x05, 0x00, 0x00, 0x00, 0x01, b't', b'a', b'u', b'r', b'i', 0x09, 0x00, 0x00, 0x00, 0x01, b'l',
    b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x00, 0x05, 0x00, 0x00, 0x00, 0x01, b't', b'a',
    b'u', b'r', b'i', 0x09, 0x00, 0x00, 0x00, 0x01, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's',
    b't', 0x00,
];
const DEV_ORIGIN_FILE: [u8; 52] = [
    0x04, 0x00, 0x00, 0x00, 0x01, b'h', b't', b't', b'p', 0x09, 0x00, 0x00, 0x00, 0x01, b'l', b'o',
    b'c', b'a', b'l', b'h', b'o', b's', b't', 0x01, 0x8c, 0x05, 0x04, 0x00, 0x00, 0x00, 0x01, b'h',
    b't', b't', b'p', 0x09, 0x00, 0x00, 0x00, 0x01, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's',
    b't', 0x01, 0x8c, 0x05,
];

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn encode_string(out: &mut Vec<u8>, text: &str) {
    out.extend((text.len() as u32).to_le_bytes());
    out.push(1);
    out.extend(text.as_bytes());
}

fn encode_origin(out: &mut Vec<u8>, protocol: &str, host: &str, port: Option<u16>) {
    encode_string(out, protocol);
    encode_string(out, host);
    match port {
        Some(port) => {
            out.push(1);
            out.extend(port.to_le_bytes());
        }
        None => out.push(0),
    }
}

fn origin_file(protocol: &str, host: &str, port: Option<u16>) -> Vec<u8> {
    let mut out = Vec::new();
    encode_origin(&mut out, protocol, host, port);
    encode_origin(&mut out, protocol, host, port);
    out
}

/// A WebKit database in WAL mode. The connection closes without a
/// checkpoint, so the items stay in the `-wal` file as they do while the old
/// app runs.
fn write_database(path: &Path, items: &[(&str, Vec<u8>)]) {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);",
        )
        .unwrap();
    for (key, value) in items {
        connection
            .execute("INSERT INTO ItemTable VALUES (?1, ?2)", (key, value))
            .unwrap();
    }
}

fn set_modified(path: &Path, unix_seconds: u64) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        file.set_modified(UNIX_EPOCH + Duration::from_secs(unix_seconds))
            .unwrap();
    }
}

/// One salted origin folder. Returns the database path.
fn salted(
    origins_dir: &Path,
    salt: &str,
    origin: &[u8],
    items: &[(&str, Vec<u8>)],
    unix_seconds: u64,
) -> PathBuf {
    let frame = origins_dir.join(salt).join(salt);
    fs::create_dir_all(frame.join("LocalStorage")).unwrap();
    fs::write(frame.join("origin"), origin).unwrap();
    let database = frame.join("LocalStorage").join("localstorage.sqlite3");
    write_database(&database, items);
    for suffix in ["", "-wal", "-shm"] {
        set_modified(
            Path::new(&format!("{}{suffix}", database.display())),
            unix_seconds,
        );
    }
    database
}

struct WebKitFixture {
    dir: TempDir,
}

impl WebKitFixture {
    fn new() -> Self {
        WebKitFixture {
            dir: TempDir::new("webkit-fixture").unwrap(),
        }
    }

    fn origins_dir(&self) -> PathBuf {
        self.dir.path().join("Default")
    }

    fn legacy_dir(&self) -> PathBuf {
        self.dir.path().join("LocalStorage")
    }

    fn data(&self) -> WebviewData {
        WebviewData::WebKit {
            origins_dir: self.origins_dir(),
            legacy_dir: self.legacy_dir(),
        }
    }
}

/// Every file under `dir` with its bytes and modification time.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.insert(path.clone(), (fs::read(&path).unwrap(), modified));
            }
        }
    }
    files
}

#[test]
fn parses_the_origin_files_this_machine_wrote() {
    assert_eq!(
        parse_origin_file(&PRODUCTION_ORIGIN_FILE),
        Ok(("tauri://localhost".into(), "tauri://localhost".into()))
    );
    assert_eq!(
        parse_origin_file(&DEV_ORIGIN_FILE),
        Ok((
            "http://localhost:1420".into(),
            "http://localhost:1420".into()
        ))
    );
    assert_eq!(
        parse_origin_file(&origin_file("tauri", "localhost", None))
            .unwrap()
            .0,
        "tauri://localhost"
    );
}

#[test]
fn parses_utf16_origin_strings_and_rejects_truncated_files() {
    let mut bytes = Vec::new();
    for _ in 0..2 {
        bytes.extend(5u32.to_le_bytes());
        bytes.push(0);
        bytes.extend(utf16("https"));
        bytes.extend(u32::MAX.to_le_bytes());
        bytes.push(0);
    }
    assert_eq!(
        parse_origin_file(&bytes),
        Ok(("https://".into(), "https://".into()))
    );
    assert!(parse_origin_file(&PRODUCTION_ORIGIN_FILE[..20]).is_err());
    assert!(parse_origin_file(&[]).is_err());
}

#[test]
fn turns_legacy_file_names_into_origins() {
    assert_eq!(
        legacy_origin("tauri_localhost_0").as_deref(),
        Some("tauri://localhost")
    );
    assert_eq!(
        legacy_origin("http_localhost_1420").as_deref(),
        Some("http://localhost:1420")
    );
    assert_eq!(legacy_origin("localhost"), None);
    assert_eq!(legacy_origin("http_localhost_port"), None);
    assert_eq!(legacy_origin("_localhost_0"), None);
}

#[test]
fn imports_the_production_origin_over_a_newer_dev_server() {
    let fixture = WebKitFixture::new();
    let production = salted(
        &fixture.origins_dir(),
        "prod-salt",
        &PRODUCTION_ORIGIN_FILE,
        &[
            ("monocode.themeHue", utf16("120")),
            (
                "monocode.lastModel",
                utf16(r#"{"harness":"codex","model":"gpt-5"}"#),
            ),
            (
                "monocode.chatBackgroundPath",
                utf16("/Users/me/Pictures/caf\u{e9} \u{1f600}.png"),
            ),
            ("monocode:tab-group:key-version", utf16("2")),
        ],
        1_000,
    );
    salted(
        &fixture.origins_dir(),
        "dev-salt",
        &DEV_ORIGIN_FILE,
        &[
            ("monocode.themeHue", utf16("300")),
            ("monocode.dev", utf16("1")),
        ],
        2_000,
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.status, ImportStatus::Imported);
    assert_eq!(report.origin.as_deref(), Some("tauri://localhost"));
    assert_eq!(report.source.as_deref(), Some(production.as_path()));
    assert_eq!(report.imported, 4);
    assert_eq!(report.kept_existing, 0);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.candidates.len(), 2);
    assert!(
        report
            .candidates
            .iter()
            .any(|c| c.production && c.items == 4)
    );
    assert!(
        report
            .candidates
            .iter()
            .any(|c| !c.production && c.items == 2)
    );
    assert_eq!(kv.len(), 4);
    assert_eq!(
        kv.get_item("monocode.chatBackgroundPath").as_deref(),
        Some("/Users/me/Pictures/caf\u{e9} \u{1f600}.png")
    );
    assert_eq!(kv.get_item("monocode.dev"), None);
    let settings = AppSettings::from_local_storage(|key| kv.get_item(key), Platform::Mac);
    assert_eq!(settings.appearance.theme_hue, 120);
    assert_eq!(settings.models.last_model.unwrap().model, "gpt-5");
}

#[test]
fn falls_back_to_the_most_recently_modified_origin() {
    let fixture = WebKitFixture::new();
    salted(
        &fixture.origins_dir(),
        "old",
        &origin_file("http", "localhost", Some(1420)),
        &[("monocode.themeHue", utf16("10"))],
        1_000,
    );
    salted(
        &fixture.origins_dir(),
        "new",
        &origin_file("http", "localhost", Some(5173)),
        &[("monocode.themeHue", utf16("20"))],
        3_000,
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.origin.as_deref(), Some("http://localhost:5173"));
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("20"));
}

#[test]
fn passes_over_an_empty_production_origin() {
    let fixture = WebKitFixture::new();
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[],
        3_000,
    );
    salted(
        &fixture.origins_dir(),
        "dev",
        &DEV_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("20"))],
        1_000,
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.origin.as_deref(), Some("http://localhost:1420"));
    assert_eq!(report.imported, 1);
}

#[test]
fn never_changes_the_webkit_files() {
    let fixture = WebKitFixture::new();
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("120"))],
        1_000,
    );
    salted(
        &fixture.origins_dir(),
        "dev",
        &DEV_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("300"))],
        2_000,
    );
    let before = snapshot(fixture.dir.path());
    assert!(
        before
            .keys()
            .any(|path| path.to_string_lossy().ends_with("-wal"))
    );
    let report = import_webkit_local_storage(&Kv::in_memory(), &fixture.data());
    assert_eq!(report.status, ImportStatus::Imported);
    assert_eq!(snapshot(fixture.dir.path()), before);
}

#[test]
fn runs_once_and_remembers_it_across_launches() {
    let fixture = WebKitFixture::new();
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("120"))],
        1_000,
    );
    let store = TempDir::new("import-once").unwrap();
    let kv = Kv::open(store.path()).unwrap();
    assert_eq!(
        import_webkit_local_storage(&kv, &fixture.data()).status,
        ImportStatus::Imported
    );
    kv.set_item("monocode.themeHue", "200");
    drop(kv);

    let kv = Kv::open(store.path()).unwrap();
    let record = import_record(&kv).unwrap();
    assert_eq!(record.status, ImportStatus::Imported);
    assert_eq!(record.origin.as_deref(), Some("tauri://localhost"));
    assert_eq!(record.items, 1);
    let again = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(again.status, ImportStatus::AlreadyImported);
    assert_eq!(again.origin.as_deref(), Some("tauri://localhost"));
    assert!(again.candidates.is_empty());
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("200"));
}

#[test]
fn keeps_values_the_store_already_has() {
    let fixture = WebKitFixture::new();
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[
            ("monocode.themeHue", utf16("120")),
            ("monocode.themeSaturation", utf16("40")),
        ],
        1_000,
    );
    let kv = Kv::in_memory();
    kv.set_item("monocode.themeHue", "200");
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.imported, 1);
    assert_eq!(report.kept_existing, 1);
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("200"));
    assert_eq!(
        kv.get_item("monocode.themeSaturation").as_deref(),
        Some("40")
    );
}

#[test]
fn records_a_fresh_install_so_it_does_not_scan_again() {
    let fixture = WebKitFixture::new();
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.status, ImportStatus::NothingFound);
    assert!(report.candidates.is_empty());
    assert_eq!(
        import_record(&kv).unwrap().status,
        ImportStatus::NothingFound
    );
    assert!(kv.is_empty());
    assert_eq!(
        import_webkit_local_storage(&kv, &fixture.data()).status,
        ImportStatus::AlreadyImported
    );
}

#[test]
fn reports_undecodable_and_lossy_items() {
    let fixture = WebKitFixture::new();
    let mut lone_surrogate = utf16("a");
    lone_surrogate.extend(0xd800u16.to_le_bytes());
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[
            ("monocode.odd", vec![0x41, 0x00, 0x42]),
            ("monocode.lone", lone_surrogate),
            ("monocode.fine", utf16("ok")),
        ],
        1_000,
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.imported, 2);
    assert_eq!(report.lossy_keys, ["monocode.lone"]);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].key, "monocode.odd");
    assert_eq!(kv.get_item("monocode.lone").as_deref(), Some("a\u{fffd}"));
    assert_eq!(kv.get_item("monocode.odd"), None);
}

#[test]
fn fails_without_recording_when_the_production_database_is_unreadable() {
    let fixture = WebKitFixture::new();
    let production = salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[],
        1_000,
    );
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", production.display()));
    }
    fs::write(&production, b"this is not a database at all").unwrap();
    salted(
        &fixture.origins_dir(),
        "dev",
        &DEV_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("20"))],
        2_000,
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.status, ImportStatus::Failed);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].contains("tauri://localhost"));
    assert!(kv.is_empty());
    assert!(import_record(&kv).is_none());
}

#[test]
fn skips_third_party_frames_and_unreadable_origin_files() {
    let fixture = WebKitFixture::new();
    let mut third_party = Vec::new();
    encode_origin(&mut third_party, "tauri", "localhost", None);
    encode_origin(&mut third_party, "https", "example.com", None);
    salted(
        &fixture.origins_dir(),
        "frame",
        &third_party,
        &[("x", utf16("1"))],
        5_000,
    );
    salted(
        &fixture.origins_dir(),
        "broken",
        &[1, 2, 3],
        &[("x", utf16("1"))],
        5_000,
    );
    salted(
        &fixture.origins_dir(),
        "prod",
        &PRODUCTION_ORIGIN_FILE,
        &[("monocode.themeHue", utf16("120"))],
        1_000,
    );
    let report = import_webkit_local_storage(&Kv::in_memory(), &fixture.data());
    assert_eq!(report.status, ImportStatus::Imported);
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert_eq!(report.origin.as_deref(), Some("tauri://localhost"));
}

#[test]
fn reads_legacy_localstorage_files() {
    let fixture = WebKitFixture::new();
    fs::create_dir_all(fixture.legacy_dir()).unwrap();
    write_database(
        &fixture.legacy_dir().join("tauri_localhost_0.localstorage"),
        &[("monocode.themeHue", utf16("77"))],
    );
    fs::write(fixture.legacy_dir().join("StorageTracker.db"), b"").unwrap();
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &fixture.data());
    assert_eq!(report.status, ImportStatus::Imported);
    assert_eq!(report.origin.as_deref(), Some("tauri://localhost"));
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("77"));
}

fn webview2_data(dir: &Path) -> WebviewData {
    WebviewData::WebView2 {
        leveldb_dir: dir.to_path_buf(),
    }
}

#[test]
fn imports_the_production_origin_from_webview2() {
    let dir = TempDir::new("webview2").unwrap();
    let prod = "http://tauri.localhost";
    let dev = "http://localhost:1420";
    fixture::write(
        dir.path(),
        &[
            fixture::meta(prod, 1_000),
            fixture::meta(dev, 2_000),
            (
                fixture::data_key(prod, &fixture::utf16("monocode.themeHue")),
                fixture::utf16("120"),
            ),
            (
                fixture::data_key(prod, &fixture::latin1("monocode.lastModel")),
                fixture::latin1(r#"{"harness":"claude","model":"opus"}"#),
            ),
            (
                fixture::data_key(dev, &fixture::utf16("monocode.themeHue")),
                fixture::utf16("300"),
            ),
            (
                fixture::data_key(
                    "https://site.example/^0https://other.example",
                    &fixture::utf16("x"),
                ),
                fixture::utf16("1"),
            ),
        ],
        &[
            (
                fixture::data_key(prod, &fixture::utf16("monocode.chatBackgroundPath")),
                fixture::utf16("C:\\Users\\me\\caf\u{e9} \u{1f600}.png"),
            ),
            (
                fixture::data_key(prod, &fixture::utf16("monocode.gone")),
                fixture::utf16("1"),
            ),
            (
                fixture::data_key(prod, &fixture::utf16("monocode.bad")),
                vec![7, 7],
            ),
        ],
        &[fixture::data_key(prod, &fixture::utf16("monocode.gone"))],
    );
    fs::write(dir.path().join("LOCK"), b"held by WebView2").unwrap();
    let before = snapshot(dir.path());
    assert!(
        before
            .keys()
            .any(|path| path.extension().is_some_and(|e| e == "ldb"))
    );
    assert!(
        before
            .keys()
            .any(|path| path.extension().is_some_and(|e| e == "log"))
    );

    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &webview2_data(dir.path()));
    assert_eq!(report.status, ImportStatus::Imported);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.origin.as_deref(), Some(prod));
    assert_eq!(report.candidates.len(), 2);
    assert_eq!(report.imported, 3);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].key, "monocode.bad");
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("120"));
    assert_eq!(
        kv.get_item("monocode.lastModel").as_deref(),
        Some(r#"{"harness":"claude","model":"opus"}"#)
    );
    assert_eq!(
        kv.get_item("monocode.chatBackgroundPath").as_deref(),
        Some("C:\\Users\\me\\caf\u{e9} \u{1f600}.png")
    );
    assert_eq!(kv.get_item("monocode.gone"), None);
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn webview2_falls_back_to_the_newest_meta_time() {
    let dir = TempDir::new("webview2-dev").unwrap();
    let old = "http://localhost:1420";
    let new = "http://localhost:5173";
    fixture::write(
        dir.path(),
        &[
            fixture::meta(old, 1_000),
            fixture::meta(new, 2_000),
            (
                fixture::data_key(new, &fixture::utf16("monocode.themeHue")),
                fixture::utf16("20"),
            ),
            (
                fixture::data_key(old, &fixture::utf16("monocode.themeHue")),
                fixture::utf16("10"),
            ),
        ],
        &[],
        &[],
    );
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &webview2_data(dir.path()));
    assert_eq!(report.origin.as_deref(), Some(new));
    assert_eq!(
        report
            .candidates
            .iter()
            .find(|c| c.origin == new)
            .unwrap()
            .modified,
        Some(UNIX_EPOCH + Duration::from_secs(2_000))
    );
    assert_eq!(kv.get_item("monocode.themeHue").as_deref(), Some("20"));
}

#[test]
fn an_unreadable_webview2_database_stops_the_import() {
    let dir = TempDir::new("webview2-broken").unwrap();
    fs::write(dir.path().join("CURRENT"), b"MANIFEST-000099\n").unwrap();
    let kv = Kv::in_memory();
    let report = import_webkit_local_storage(&kv, &webview2_data(dir.path()));
    assert_eq!(report.status, ImportStatus::Failed);
    assert_eq!(report.errors.len(), 1);
    assert!(import_record(&kv).is_none());
}

#[test]
fn a_missing_webview2_folder_is_a_fresh_install() {
    let dir = TempDir::new("webview2-missing").unwrap();
    let report = import_webkit_local_storage(
        &Kv::in_memory(),
        &webview2_data(&dir.path().join("leveldb")),
    );
    assert_eq!(report.status, ImportStatus::NothingFound);
}

#[test]
fn builds_each_platforms_location() {
    assert_eq!(
        WebviewData::macos(Path::new("/Users/me"), APP_IDENTIFIER),
        WebviewData::WebKit {
            origins_dir: "/Users/me/Library/WebKit/com.monocode.desktop/WebsiteData/Default".into(),
            legacy_dir: "/Users/me/Library/WebKit/com.monocode.desktop/WebsiteData/LocalStorage"
                .into(),
        }
    );
    assert_eq!(
        WebviewData::linux(Path::new("/home/me/.local/share"), APP_IDENTIFIER),
        WebviewData::WebKit {
            origins_dir: "/home/me/.local/share/com.monocode.desktop/storage".into(),
            legacy_dir: "/home/me/.local/share/com.monocode.desktop/localstorage".into(),
        }
    );
    let WebviewData::WebView2 { leveldb_dir } =
        WebviewData::windows(Path::new("C:/Users/me/AppData/Local"), APP_IDENTIFIER)
    else {
        panic!("expected WebView2");
    };
    let parts: Vec<String> = leveldb_dir
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    assert!(parts.ends_with(&[
        "com.monocode.desktop".to_string(),
        "EBWebView".into(),
        "Default".into(),
        "Local Storage".into(),
        "leveldb".into(),
    ]));
    assert!(is_production_origin("tauri://localhost"));
    assert!(is_production_origin("http://tauri.localhost"));
    assert!(!is_production_origin("http://localhost:1420"));
}

/// The real-data check: import this machine's old webview storage into a
/// temp store. Prints counts, origins, and key names, never values.
#[test]
#[ignore = "reads this machine's webview data; run with --ignored --nocapture"]
fn imports_this_machines_webview_data() {
    let platform = Platform::current();
    let data = WebviewData::for_platform(platform, APP_IDENTIFIER).expect("no home folder");
    let roots: Vec<PathBuf> = match &data {
        WebviewData::WebKit {
            origins_dir,
            legacy_dir,
        } => vec![origins_dir.clone(), legacy_dir.clone()],
        WebviewData::WebView2 { leveldb_dir } => vec![leveldb_dir.clone()],
    };
    let before: Vec<_> = roots.iter().map(|root| snapshot(root)).collect();

    let store = TempDir::new("real-import").unwrap();
    let kv = Kv::open(store.path()).unwrap();
    let report = import_webkit_local_storage(&kv, &data);
    println!("status: {:?}", report.status);
    println!("chosen origin: {:?}", report.origin);
    println!("source: {:?}", report.source);
    for candidate in &report.candidates {
        println!(
            "candidate: {} items={} production={} modified={:?}",
            candidate.origin,
            candidate.items,
            candidate.production,
            candidate.modified.map(crate::kv::unix_millis)
        );
    }
    println!(
        "imported={} kept_existing={} lossy={} skipped={} errors={:?}",
        report.imported,
        report.kept_existing,
        report.lossy_keys.len(),
        report.skipped.len(),
        report.errors
    );
    println!("keys: {:?}", kv.keys());

    let parsed = AppSettings::from_local_storage(|key| kv.get_item(key), platform);
    let known: Vec<&str> = crate::settings_store::APP_SETTINGS_KEYS
        .iter()
        .copied()
        .filter(|key| kv.get_item(key).is_some())
        .collect();
    let effective: Vec<&str> = known
        .iter()
        .copied()
        .filter(|hidden| {
            AppSettings::from_local_storage(
                |key| (key != *hidden).then(|| kv.get_item(key)).flatten(),
                platform,
            ) != parsed
        })
        .collect();
    println!("AppSettings keys present: {} {:?}", known.len(), known);
    println!(
        "keys that change the parsed AppSettings: {} {:?}",
        effective.len(),
        effective
    );
    println!(
        "parsed differs from defaults: {}",
        parsed != AppSettings::default()
    );
    let json = serde_json::to_string(&parsed).unwrap();
    assert_eq!(serde_json::from_str::<AppSettings>(&json).unwrap(), parsed);

    drop(kv);
    let reopened = Kv::open(store.path()).unwrap();
    assert_eq!(
        AppSettings::from_local_storage(|key| reopened.get_item(key), platform),
        parsed
    );
    let after: Vec<_> = roots.iter().map(|root| snapshot(root)).collect();
    // The old app may be running and writing, so only compare files the
    // import could have touched if it opened them directly.
    for (before, after) in before.iter().zip(&after) {
        for (path, (bytes, _)) in before {
            if path.to_string_lossy().ends_with("-wal") || path.to_string_lossy().ends_with("-shm")
            {
                continue;
            }
            if let Some((now, _)) = after.get(path) {
                assert_eq!(now, bytes, "{} changed", path.display());
            }
        }
    }
    assert_eq!(report.status, ImportStatus::Imported);
}
