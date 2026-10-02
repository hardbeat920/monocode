//! One-time import of the Tauri app's webview localStorage into `Kv`.
//!
//! The Tauri app kept every `monocode.*` key in its webview's localStorage:
//!
//! - macOS: WKWebView, under `~/Library/WebKit/<identifier>/WebsiteData`.
//! - Linux: webkit2gtk. Tauri points WebKit's data directory at
//!   `$XDG_DATA_HOME/<identifier>` (`~/.local/share/<identifier>`).
//! - Windows: WebView2, a Chromium LevelDB under
//!   `%LOCALAPPDATA%\<identifier>\EBWebView`.
//!
//! The import copies the old files and opens the copies, so it never opens a
//! WebKit or WebView2 file for writing. Each origin found is a candidate. The
//! production origin (`tauri://localhost`, or `http://tauri.localhost` on
//! Windows) wins when it holds any items, and otherwise the most recently
//! modified origin with items does, which is usually the dev server. Keys the
//! store already holds keep their value. The store records the import, so
//! later calls return `ImportStatus::AlreadyImported` without reading
//! anything.

mod webkit;
mod webview2;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use monocode_core::Platform;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::kv::{Kv, unix_millis};

/// The Tauri bundle identifier, which names the webview data folders.
pub const APP_IDENTIFIER: &str = "com.monocode.desktop";

/// The `Kv` metadata key that records the import.
pub const IMPORT_META_KEY: &str = "webviewImport";

/// Origins the packaged Tauri app served from. macOS and Linux use the
/// custom scheme; WebView2 serves custom schemes over http or https.
pub const PRODUCTION_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Whether `origin` is the packaged app rather than a dev server.
pub fn is_production_origin(origin: &str) -> bool {
    PRODUCTION_ORIGINS.contains(&origin)
}

/// Where the old webview kept localStorage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebviewData {
    /// WebKit: WKWebView on macOS, webkit2gtk on Linux.
    WebKit {
        /// Salted origin folders: `<hash>/<hash>/origin` next to
        /// `<hash>/<hash>/LocalStorage/localstorage.sqlite3`.
        origins_dir: PathBuf,
        /// `<scheme>_<host>_<port>.localstorage` files from WebKit versions
        /// before the salted layout.
        legacy_dir: PathBuf,
    },
    /// WebView2 on Windows: Chromium's localStorage LevelDB.
    WebView2 { leveldb_dir: PathBuf },
}

impl WebviewData {
    /// macOS: `<home>/Library/WebKit/<identifier>/WebsiteData`.
    pub fn macos(home: &Path, identifier: &str) -> Self {
        let root = home
            .join("Library")
            .join("WebKit")
            .join(identifier)
            .join("WebsiteData");
        WebviewData::WebKit {
            origins_dir: root.join("Default"),
            legacy_dir: root.join("LocalStorage"),
        }
    }

    /// Linux: `<data_home>/<identifier>`, where `data_home` is
    /// `$XDG_DATA_HOME` or `~/.local/share`. webkit2gtk names its general
    /// storage folder `storage` and its legacy folder `localstorage`.
    pub fn linux(data_home: &Path, identifier: &str) -> Self {
        let root = data_home.join(identifier);
        WebviewData::WebKit {
            origins_dir: root.join("storage"),
            legacy_dir: root.join("localstorage"),
        }
    }

    /// Windows: `<local_app_data>\<identifier>\EBWebView\Default\Local Storage\leveldb`.
    pub fn windows(local_app_data: &Path, identifier: &str) -> Self {
        WebviewData::WebView2 {
            leveldb_dir: local_app_data
                .join(identifier)
                .join("EBWebView")
                .join("Default")
                .join("Local Storage")
                .join("leveldb"),
        }
    }

    /// The location the Tauri app used on `platform`, from `HOME`,
    /// `XDG_DATA_HOME`, or `LOCALAPPDATA`. `None` when the variable is unset.
    pub fn for_platform(platform: Platform, identifier: &str) -> Option<Self> {
        match platform {
            Platform::Mac => env_path("HOME").map(|home| Self::macos(&home, identifier)),
            Platform::Linux => env_path("XDG_DATA_HOME")
                .filter(|path| path.is_absolute())
                .or_else(|| env_path("HOME").map(|home| home.join(".local").join("share")))
                .map(|data_home| Self::linux(&data_home, identifier)),
            Platform::Windows => {
                env_path("LOCALAPPDATA").map(|dir| Self::windows(&dir, identifier))
            }
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// How an import ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImportStatus {
    /// Items from one origin were copied into the store.
    #[serde(rename = "imported")]
    Imported,
    /// The store records an earlier import, so nothing was read.
    #[serde(rename = "alreadyImported")]
    AlreadyImported,
    /// No origin with items was found. The store records this too, so a
    /// fresh install does not scan again on every launch.
    #[default]
    #[serde(rename = "nothingFound")]
    NothingFound,
    /// A database that may hold the production items could not be read.
    /// Nothing was imported or recorded, so the next launch tries again.
    #[serde(rename = "failed")]
    Failed,
}

/// One origin found in the old storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// Such as `tauri://localhost` or `http://localhost:1420`.
    pub origin: String,
    /// The database file, or the LevelDB folder on Windows.
    pub path: PathBuf,
    /// When the origin's data last changed, if known.
    pub modified: Option<SystemTime>,
    /// How many items it holds.
    pub items: usize,
    pub production: bool,
}

/// An item the import could not decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItem {
    pub key: String,
    pub reason: String,
}

/// What `import_webkit_local_storage` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub status: ImportStatus,
    /// The origin whose items were imported.
    pub origin: Option<String>,
    /// The database the items came from.
    pub source: Option<PathBuf>,
    /// Every origin found, imported or not.
    pub candidates: Vec<ImportCandidate>,
    /// Items copied into the store.
    pub imported: usize,
    /// Items left out because the store already held the key.
    pub kept_existing: usize,
    /// Imported keys whose key or value held an unpaired UTF-16 surrogate,
    /// which became U+FFFD.
    pub lossy_keys: Vec<String>,
    /// Items in the chosen origin that could not be decoded at all.
    pub skipped: Vec<SkippedItem>,
    /// Problems that did not stop the import, such as an unreadable dev
    /// database or a failed write of the store.
    pub errors: Vec<String>,
}

/// What the store remembers about the import, under `IMPORT_META_KEY`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecord {
    /// When the import ran, in milliseconds since the Unix epoch.
    pub at: i64,
    pub status: ImportStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// Items the chosen origin held.
    #[serde(default)]
    pub items: i64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The import record, if the store has already run the import.
pub fn import_record(kv: &Kv) -> Option<ImportRecord> {
    serde_json::from_value(kv.meta(IMPORT_META_KEY)?).ok()
}

/// One origin's items, as read from the old storage.
#[derive(Debug, Default)]
pub(crate) struct OriginItems {
    pub origin: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub items: Vec<(String, String)>,
    pub lossy_keys: Vec<String>,
    pub skipped: Vec<SkippedItem>,
}

/// A database or folder that could not be read.
#[derive(Debug)]
pub(crate) struct ReadFailure {
    pub path: PathBuf,
    pub message: String,
    /// Whether it may hold the production items, which stops the import.
    pub blocks_import: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Found {
    pub origins: Vec<OriginItems>,
    pub failures: Vec<ReadFailure>,
}

/// Copy the old webview localStorage into `kv`, once. See the module docs
/// for how the origin is chosen. The store is flushed afterwards.
pub fn import_webkit_local_storage(kv: &Kv, data: &WebviewData) -> ImportReport {
    let mut report = ImportReport::default();
    if let Some(record) = import_record(kv) {
        report.status = ImportStatus::AlreadyImported;
        report.origin = record.origin;
        report.source = record.source;
        return report;
    }
    let found = read(data);
    report.candidates = found
        .origins
        .iter()
        .map(|origin| ImportCandidate {
            origin: origin.origin.clone(),
            path: origin.path.clone(),
            modified: origin.modified,
            items: origin.items.len(),
            production: is_production_origin(&origin.origin),
        })
        .collect();
    report.errors = found
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.path.display(), failure.message))
        .collect();
    if found.failures.iter().any(|failure| failure.blocks_import) {
        report.status = ImportStatus::Failed;
        return report;
    }

    let mut origins = found.origins;
    let record = |status: ImportStatus, chosen: Option<&OriginItems>| ImportRecord {
        at: unix_millis(SystemTime::now()),
        status,
        origin: chosen.map(|chosen| chosen.origin.clone()),
        source: chosen.map(|chosen| chosen.path.clone()),
        items: chosen.map_or(0, |chosen| chosen.items.len() as i64),
        extra: Map::new(),
    };
    let Some(index) = choose(&origins) else {
        let record = record(ImportStatus::NothingFound, None);
        kv.import_items(Vec::new(), IMPORT_META_KEY, to_value(&record));
        report.status = ImportStatus::NothingFound;
        flush(kv, &mut report);
        return report;
    };
    let chosen = origins.swap_remove(index);
    let record = record(ImportStatus::Imported, Some(&chosen));
    let total = chosen.items.len();
    report.imported = kv.import_items(chosen.items, IMPORT_META_KEY, to_value(&record));
    report.kept_existing = total - report.imported;
    report.status = ImportStatus::Imported;
    report.origin = Some(chosen.origin);
    report.source = Some(chosen.path);
    report.lossy_keys = chosen.lossy_keys;
    report.skipped = chosen.skipped;
    flush(kv, &mut report);
    report
}

fn read(data: &WebviewData) -> Found {
    let mut found = match data {
        WebviewData::WebKit {
            origins_dir,
            legacy_dir,
        } => webkit::read(origins_dir, legacy_dir),
        WebviewData::WebView2 { leveldb_dir } => webview2::read(leveldb_dir),
    };
    found
        .origins
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.origin.cmp(&b.origin)));
    found
}

/// The production origin with items, else the most recently modified
/// origin with items.
fn choose(origins: &[OriginItems]) -> Option<usize> {
    let newest = |production: bool| {
        origins
            .iter()
            .enumerate()
            .filter(|(_, origin)| {
                !origin.items.is_empty() && is_production_origin(&origin.origin) == production
            })
            .max_by_key(|(_, origin)| origin.modified)
            .map(|(index, _)| index)
    };
    newest(true).or_else(|| newest(false))
}

fn to_value(record: &ImportRecord) -> Value {
    serde_json::to_value(record).unwrap_or(Value::Null)
}

fn flush(kv: &Kv, report: &mut ImportReport) {
    if let Err(error) = kv.flush() {
        report
            .errors
            .push(format!("could not save the store: {error}"));
    }
}

/// Decode UTF-16LE code units. `None` for an odd byte count; the flag says
/// whether an unpaired surrogate became U+FFFD.
pub(crate) fn decode_utf16le(bytes: &[u8]) -> Option<(String, bool)> {
    let (pairs, []) = bytes.as_chunks::<2>() else {
        return None;
    };
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    Some(match String::from_utf16(&units) {
        Ok(text) => (text, false),
        Err(_) => (String::from_utf16_lossy(&units), true),
    })
}

/// Decode Latin-1 bytes, one char per byte.
pub(crate) fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// The newest modification time among `paths` that exist.
pub(crate) fn newest_mtime<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<SystemTime> {
    paths
        .into_iter()
        .filter_map(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
        })
        .max()
}

#[cfg(test)]
mod tests;
