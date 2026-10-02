//! Reads WebKit's localStorage: WKWebView on macOS, webkit2gtk on Linux.
//!
//! Current WebKit keeps one SQLite database per origin in salted folders,
//! `<origins_dir>/<top hash>/<frame hash>/LocalStorage/localstorage.sqlite3`,
//! with an `origin` file in the frame folder that names both origins. Older
//! WebKit kept `<scheme>_<host>_<port>.localstorage` files in one folder.
//! Both use `ItemTable (key TEXT, value BLOB)`, where the value holds the
//! string's UTF-16LE code units.
//!
//! The databases run in WAL mode, and even a read-only SQLite connection
//! writes to the `-shm` file. So each database and its `-wal` and `-journal`
//! files are copied into a private temp folder and the copy is opened.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use super::{
    Found, OriginItems, ReadFailure, SkippedItem, decode_latin1, decode_utf16le,
    is_production_origin, newest_mtime,
};
use crate::temp::TempDir;

const DATABASE_NAME: &str = "localstorage.sqlite3";
const LEGACY_SUFFIX: &str = ".localstorage";
/// Files SQLite may keep next to a database.
const SIDE_FILES: [&str; 2] = ["-wal", "-journal"];

pub(super) fn read(origins_dir: &Path, legacy_dir: &Path) -> Found {
    let mut found = Found::default();
    read_salted(origins_dir, &mut found);
    read_legacy(legacy_dir, &mut found);
    found
}

fn read_salted(dir: &Path, found: &mut Found) {
    for top in subdirs(dir, found) {
        for frame in subdirs(&top, found) {
            let database = frame.join("LocalStorage").join(DATABASE_NAME);
            if !database.is_file() {
                continue;
            }
            let origin = fs::read(frame.join("origin"))
                .map_err(|error| format!("could not read the origin file: {error}"))
                .and_then(|bytes| parse_origin_file(&bytes));
            match origin {
                // Storage a third-party frame kept under another top origin.
                Ok((top_origin, frame_origin)) if top_origin != frame_origin => {}
                Ok((_, origin)) => push_database(found, origin, database),
                Err(message) => found.failures.push(ReadFailure {
                    path: frame,
                    message,
                    blocks_import: false,
                }),
            }
        }
    }
}

fn read_legacy(dir: &Path, found: &mut Found) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    for path in files {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(identifier) = name.strip_suffix(LEGACY_SUFFIX) else {
            continue;
        };
        let Some(origin) = legacy_origin(identifier) else {
            continue;
        };
        push_database(found, origin, path);
    }
}

/// Sorted child folders. A missing folder has none; any other error is
/// recorded but does not stop the import.
fn subdirs(dir: &Path, found: &mut Found) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            found.failures.push(ReadFailure {
                path: dir.to_path_buf(),
                message: format!("could not list the folder: {error}"),
                blocks_import: false,
            });
            return Vec::new();
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn push_database(found: &mut Found, origin: String, database: PathBuf) {
    match read_database(&database) {
        Ok(mut items) => {
            items.origin = origin;
            found.origins.push(items);
        }
        Err(message) => found.failures.push(ReadFailure {
            blocks_import: is_production_origin(&origin),
            path: database,
            message: format!("{origin}: {message}"),
        }),
    }
}

fn side_file(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Read every item from a copy of `database`.
pub(super) fn read_database(database: &Path) -> Result<OriginItems, String> {
    let side_files: Vec<PathBuf> = SIDE_FILES
        .iter()
        .map(|suffix| side_file(database, suffix))
        .collect();
    let modified =
        newest_mtime(std::iter::once(database).chain(side_files.iter().map(PathBuf::as_path)));
    let temp = TempDir::new("webkit-import").map_err(|error| error.to_string())?;
    let copy = temp.path().join(DATABASE_NAME);
    fs::copy(database, &copy).map_err(|error| format!("could not copy the database: {error}"))?;
    for (suffix, side) in SIDE_FILES.iter().zip(&side_files) {
        if side.is_file() {
            fs::copy(side, side_file(&copy, suffix))
                .map_err(|error| format!("could not copy {}: {error}", side.display()))?;
        }
    }
    let connection = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("could not open the database: {error}"))?;
    let mut items = OriginItems {
        path: database.to_path_buf(),
        modified,
        ..OriginItems::default()
    };
    let mut statement = connection
        .prepare("SELECT key, value FROM ItemTable")
        .map_err(|error| format!("could not read ItemTable: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("could not read ItemTable: {error}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("could not read ItemTable: {error}"))?
    {
        let key = row.get_ref(0).map_err(|error| error.to_string())?;
        let value = row.get_ref(1).map_err(|error| error.to_string())?;
        let (key, key_lossy) = match decode_column(key) {
            Ok(key) => key,
            Err(reason) => {
                items.skipped.push(SkippedItem {
                    key: String::new(),
                    reason: format!("key: {reason}"),
                });
                continue;
            }
        };
        match decode_column(value) {
            Ok((value, value_lossy)) => {
                if key_lossy || value_lossy {
                    items.lossy_keys.push(key.clone());
                }
                items.items.push((key, value));
            }
            Err(reason) => items.skipped.push(SkippedItem {
                key,
                reason: format!("value: {reason}"),
            }),
        }
    }
    Ok(items)
}

/// Text columns are UTF-8 (SQLite converts on read). Blob columns hold
/// UTF-16LE code units.
fn decode_column(value: ValueRef<'_>) -> Result<(String, bool), String> {
    match value {
        ValueRef::Text(text) => match std::str::from_utf8(text) {
            Ok(text) => Ok((text.to_string(), false)),
            Err(_) => Ok((String::from_utf8_lossy(text).into_owned(), true)),
        },
        ValueRef::Blob(bytes) => decode_utf16le(bytes)
            .ok_or_else(|| format!("odd byte count ({}) for UTF-16", bytes.len())),
        ValueRef::Null => Err("null".into()),
        ValueRef::Integer(_) | ValueRef::Real(_) => Err("a number, not a string".into()),
    }
}

/// `<scheme>_<host>_<port>`, WebKit's `databaseIdentifier`, where port 0
/// means the scheme's default.
pub(super) fn legacy_origin(identifier: &str) -> Option<String> {
    let (rest, port) = identifier.rsplit_once('_')?;
    let port: u16 = port.parse().ok()?;
    let (scheme, host) = rest.split_once('_')?;
    if scheme.is_empty() {
        return None;
    }
    Some(if port == 0 {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    })
}

/// The `origin` file: the top origin, then the frame origin, each written by
/// WebKit's persistence encoder as protocol, host, and an optional port.
pub(super) fn parse_origin_file(bytes: &[u8]) -> Result<(String, String), String> {
    let mut reader = Reader { bytes, pos: 0 };
    let top = reader.origin()?;
    let frame = reader.origin()?;
    Ok((top, frame))
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("the origin file ends early")?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A length in code units (`u32::MAX` for a null string), an is-8-bit
    /// flag, then Latin-1 bytes or UTF-16LE code units.
    fn string(&mut self) -> Result<String, String> {
        let len = self.u32()?;
        if len == u32::MAX {
            return Ok(String::new());
        }
        let len = len as usize;
        if self.u8()? != 0 {
            return Ok(decode_latin1(self.take(len)?));
        }
        let bytes = self.take(len.checked_mul(2).ok_or("the origin file is malformed")?)?;
        decode_utf16le(bytes)
            .map(|(text, _)| text)
            .ok_or_else(|| "the origin file is malformed".to_string())
    }

    fn origin(&mut self) -> Result<String, String> {
        let protocol = self.string()?;
        let host = self.string()?;
        let port = match self.u8()? {
            0 => None,
            _ => Some(self.u16()?),
        };
        if protocol.is_empty() {
            return Err("the origin file names no protocol".into());
        }
        Ok(match port {
            Some(port) => format!("{protocol}://{host}:{port}"),
            None => format!("{protocol}://{host}"),
        })
    }
}
