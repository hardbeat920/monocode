//! Reads WebView2's localStorage on Windows: Chromium's LevelDB in
//! `EBWebView\Default\Local Storage\leveldb`.
//!
//! Chromium stores each item under `_<origin>\0<key>`. `META:<origin>` holds
//! a protobuf whose field 1 is the origin's last change, in microseconds
//! since 1601. Item keys and values start with an encoding byte: 0 for
//! UTF-16LE, 1 for Latin-1.
//!
//! The files are copied into rusty-leveldb's in-memory environment and the
//! database is opened there, so nothing on disk is opened for writing. The
//! `LOCK` file is skipped, because a running WebView2 holds it.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusty_leveldb::env::Env;
use rusty_leveldb::{DB, LdbIterator, MemEnv, Options};

use super::{Found, OriginItems, ReadFailure, SkippedItem, decode_latin1, decode_utf16le};

const DATA_PREFIX: &[u8] = b"_";
const META_PREFIX: &[u8] = b"META:";
/// Microseconds from 1601-01-01 to 1970-01-01.
const WINDOWS_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;

pub(super) fn read(dir: &Path) -> Found {
    let mut found = Found::default();
    if !dir.is_dir() {
        return found;
    }
    match read_database(dir) {
        Ok(origins) => found.origins = origins,
        // One database holds every origin, the production one included.
        Err(message) => found.failures.push(ReadFailure {
            path: dir.to_path_buf(),
            message,
            blocks_import: true,
        }),
    }
    found
}

fn read_database(dir: &Path) -> Result<Vec<OriginItems>, String> {
    let env = MemEnv::new();
    let name = Path::new("leveldb");
    let mut newest_file = None;
    let entries =
        fs::read_dir(dir).map_err(|error| format!("could not list the folder: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file() || entry.file_name() == "LOCK" {
            continue;
        }
        let bytes = fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        newest_file = newest_file.max(modified);
        let mut file = env
            .open_writable_file(&name.join(entry.file_name()))
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
    }
    let options = Options {
        env: Rc::new(Box::new(env)),
        create_if_missing: false,
        ..Options::default()
    };
    let mut db = DB::open(name, options).map_err(|error| format!("could not open: {error}"))?;
    let mut iter = db.new_iter().map_err(|error| error.to_string())?;
    let mut origins: BTreeMap<String, OriginItems> = BTreeMap::new();
    let mut modified: BTreeMap<String, SystemTime> = BTreeMap::new();
    while let Some((key, value)) = iter.next() {
        if let Some(rest) = key.strip_prefix(DATA_PREFIX) {
            let Some(separator) = rest.iter().position(|&byte| byte == 0) else {
                continue;
            };
            let origin = String::from_utf8_lossy(&rest[..separator]).into_owned();
            // A third-party frame's storage, partitioned under a top-level site.
            if origin.contains('^') {
                continue;
            }
            let entry = origins
                .entry(origin.clone())
                .or_insert_with(|| OriginItems {
                    origin,
                    path: dir.to_path_buf(),
                    ..OriginItems::default()
                });
            push_item(entry, &rest[separator + 1..], &value);
        } else if let Some(origin) = key.strip_prefix(META_PREFIX)
            && let Some(time) = last_modified(&value)
        {
            modified.insert(String::from_utf8_lossy(origin).into_owned(), time);
        }
    }
    Ok(origins
        .into_values()
        .map(|mut origin| {
            origin.modified = modified.get(&origin.origin).copied().or(newest_file);
            origin
        })
        .collect())
}

fn push_item(origin: &mut OriginItems, key: &[u8], value: &[u8]) {
    let Some((key, key_lossy)) = decode_prefixed(key) else {
        origin.skipped.push(SkippedItem {
            key: String::from_utf8_lossy(key).into_owned(),
            reason: "key: unknown encoding".into(),
        });
        return;
    };
    match decode_prefixed(value) {
        Some((value, value_lossy)) => {
            if key_lossy || value_lossy {
                origin.lossy_keys.push(key.clone());
            }
            origin.items.push((key, value));
        }
        None => origin.skipped.push(SkippedItem {
            key,
            reason: "value: unknown encoding".into(),
        }),
    }
}

/// A Chromium localStorage string: 0 then UTF-16LE, or 1 then Latin-1.
pub(super) fn decode_prefixed(bytes: &[u8]) -> Option<(String, bool)> {
    match bytes.split_first()? {
        (0, rest) => decode_utf16le(rest),
        (1, rest) => Some((decode_latin1(rest), false)),
        _ => None,
    }
}

/// Field 1 of `LocalStorageOriginMetaData`, a varint of Chromium's
/// `base::Time` (microseconds since 1601).
fn last_modified(proto: &[u8]) -> Option<SystemTime> {
    let mut pos = 0;
    while pos < proto.len() {
        let tag = varint(proto, &mut pos)?;
        match (tag >> 3, tag & 7) {
            (1, 0) => {
                let micros = varint(proto, &mut pos)? as i64 - WINDOWS_EPOCH_OFFSET_MICROS;
                return u64::try_from(micros)
                    .ok()
                    .map(|micros| UNIX_EPOCH + Duration::from_micros(micros));
            }
            (_, 0) => {
                varint(proto, &mut pos)?;
            }
            (_, 1) => pos += 8,
            (_, 2) => {
                let len = varint(proto, &mut pos)? as usize;
                pos = pos.checked_add(len)?;
            }
            (_, 5) => pos += 4,
            _ => return None,
        }
    }
    None
}

fn varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
pub(super) mod fixture {
    //! Writes a LevelDB in Chromium's localStorage layout, for tests.

    use super::*;

    pub(crate) fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    pub(crate) fn latin1(text: &str) -> Vec<u8> {
        let mut bytes = vec![1];
        bytes.extend(text.chars().map(|c| c as u8));
        bytes
    }

    pub(crate) fn data_key(origin: &str, key: &[u8]) -> Vec<u8> {
        let mut bytes = DATA_PREFIX.to_vec();
        bytes.extend_from_slice(origin.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(key);
        bytes
    }

    /// `META:<origin>` with field 1 set to `unix_seconds`, plus a size field.
    pub(crate) fn meta(origin: &str, unix_seconds: i64) -> (Vec<u8>, Vec<u8>) {
        let mut key = META_PREFIX.to_vec();
        key.extend_from_slice(origin.as_bytes());
        let mut value = vec![0x08];
        put_varint(
            &mut value,
            (unix_seconds * 1_000_000 + WINDOWS_EPOCH_OFFSET_MICROS) as u64,
        );
        value.push(0x10);
        put_varint(&mut value, 1234);
        (key, value)
    }

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    /// Build the database in memory, then copy its files into `dir`.
    /// `tabled` records are compacted into a snappy table, `logged` ones stay
    /// in the write-ahead log, and `deleted` keys leave tombstones.
    pub(crate) fn write(
        dir: &Path,
        tabled: &[(Vec<u8>, Vec<u8>)],
        logged: &[(Vec<u8>, Vec<u8>)],
        deleted: &[Vec<u8>],
    ) {
        let env = Rc::new(Box::new(MemEnv::new()) as Box<dyn Env>);
        let options = Options {
            env: Rc::clone(&env),
            compressor: 1,
            ..Options::default()
        };
        let name = Path::new("leveldb");
        let mut db = DB::open(name, options).unwrap();
        db.put(b"VERSION", b"1").unwrap();
        for (key, value) in tabled {
            db.put(key, value).unwrap();
        }
        db.compact_range(b"\x00", b"\xff\xff").unwrap();
        for (key, value) in logged {
            db.put(key, value).unwrap();
        }
        for key in deleted {
            db.delete(key).unwrap();
        }
        db.flush().unwrap();
        db.close().unwrap();
        drop(db);
        fs::create_dir_all(dir).unwrap();
        for child in env.children(name).unwrap() {
            let mut file = env.open_sequential_file(&name.join(&child)).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
            fs::write(dir.join(&child), bytes).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_both_string_encodings() {
        assert_eq!(
            decode_prefixed(&fixture::utf16("caf\u{e9} \u{1f600}")),
            Some(("caf\u{e9} \u{1f600}".to_string(), false))
        );
        assert_eq!(
            decode_prefixed(&fixture::latin1("caf\u{e9}")),
            Some(("caf\u{e9}".to_string(), false))
        );
        assert_eq!(decode_prefixed(&[2, 0x41]), None);
        assert_eq!(decode_prefixed(&[]), None);
        assert_eq!(decode_prefixed(&[0, 0x41]), None);
    }

    #[test]
    fn reads_the_last_modified_time_from_meta() {
        let (_, value) = fixture::meta("http://tauri.localhost", 1_790_000_000);
        assert_eq!(
            last_modified(&value),
            Some(UNIX_EPOCH + Duration::from_secs(1_790_000_000))
        );
        assert_eq!(last_modified(&[0x10, 0x01]), None);
        assert_eq!(last_modified(&[0x08]), None);
    }
}
