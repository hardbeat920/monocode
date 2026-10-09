//! Read-only access to SQLite stores owned by other tools. MonoCode must
//! never write to them, including WAL/SHM sidecars.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub(crate) fn open(path: &Path) -> Result<Connection, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        // Live WAL files must stay visible. Never use immutable for a live store.
        return Connection::open_with_flags(path, flags).map_err(|e| e.to_string());
    }
    // A checkpointed WAL-mode database otherwise tries to create a new -shm
    // file even on a read-only connection. No sidecars are needed to read it.
    let name = path
        .to_str()
        .ok_or("Invalid store path")?
        .replace('%', "%25")
        .replace('?', "%3F")
        .replace('#', "%23");
    Connection::open_with_flags(
        format!("file:{name}?immutable=1"),
        flags | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| e.to_string())
}
