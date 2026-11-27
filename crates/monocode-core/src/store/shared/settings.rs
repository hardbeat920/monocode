//! Native metadata lives separately from the desktop schema. Updating one key
//! must not clone, serialize and fsync every cached app result under the Store
//! mutex. SQLite commits only the changed key and retains durable receipts.
use super::{parse, sql};
use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};
use std::{collections::BTreeMap, path::Path};

type Snapshot = BTreeMap<String, String>;

fn legacy(path: &Path) -> Result<Snapshot, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => parse(&raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub(super) fn open(
    database: &Path,
    read_only: bool,
) -> Result<(Snapshot, Option<Connection>), String> {
    if database == Path::new(":memory:") {
        return Ok((BTreeMap::new(), None));
    }
    let name = database.file_name().unwrap_or_default().to_string_lossy();
    let path = database.with_file_name(format!("{name}.gpui.sqlite3"));
    let old_path = database.with_file_name(format!("{name}.gpui.json"));
    if read_only && !path.exists() {
        return Ok((legacy(&old_path)?, None));
    }
    if !read_only {
        // Match the old metadata file's permissions before storing any values.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.to_string()),
        }
    }
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let conn = sql(Connection::open_with_flags(&path, flags))?;
    sql(conn.busy_timeout(std::time::Duration::from_secs(5)))?;
    let version: i64 = sql(conn.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
    if version > 1 {
        return Err(format!("Unsupported native settings version: {version}"));
    }
    if read_only && version == 0 {
        // An interrupted initial import has not replaced the legacy snapshot.
        return Ok((legacy(&old_path)?, None));
    }
    if !read_only {
        sql(conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;"))?;
        if version == 0 {
            // Another native window may have finished the import while this
            // connection waited. Acquire the writer before checking its marker.
            let tx = sql(Transaction::new_unchecked(
                &conn,
                TransactionBehavior::Immediate,
            ))?;
            let version: i64 = sql(tx.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
            if version == 0 {
                let values = legacy(&old_path)?;
                sql(tx.execute_batch(
                    "CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
                ))?;
                {
                    let mut insert =
                        sql(tx.prepare("INSERT INTO settings(key,value) VALUES(?1,?2)"))?;
                    for (key, value) in &values {
                        sql(insert.execute(params![key, value]))?;
                    }
                }
                // The marker and all imported keys commit together. Keep the old
                // file untouched as a backup, but never re-import stale values.
                sql(tx.execute_batch("PRAGMA user_version=1;"))?;
                sql(tx.commit())?;
                return Ok((values, Some(conn)));
            }
        }
    }
    let values = {
        let mut query = sql(conn.prepare("SELECT key,value FROM settings"))?;
        let values = sql(query.query_map([], |row| Ok((row.get(0)?, row.get(1)?))))?;
        sql(values.collect())?
    };
    Ok((values, Some(conn)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn legacy_settings_and_large_receipts_import_once_without_changing_desktop_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        drop(Store::open_monocode(&path).unwrap());
        // Exercise a legacy installation, before the sidecar existed.
        std::fs::remove_file(dir.path().join("monocode.db.gpui.sqlite3")).unwrap();
        let old = dir.path().join("monocode.db.gpui.json");
        let receipt = "large result".repeat(100_000);
        let values = BTreeMap::from([
            ("workspace", "saved tabs"),
            ("mono-app-receipt:1:request", receipt.as_str()),
        ]);
        let original = serde_json::to_vec(&values).unwrap();
        std::fs::write(&old, &original).unwrap();
        let desktop = std::fs::read(&path).unwrap();
        let store = Store::open_monocode(&path).unwrap();
        assert_eq!(
            store.setting("workspace").unwrap().as_deref(),
            Some("saved tabs")
        );
        assert_eq!(
            store.setting("mono-app-receipt:1:request").unwrap(),
            Some(receipt.clone())
        );
        store.save_setting("workspace", "new tabs").unwrap();
        drop(store);
        assert_eq!(std::fs::read(&old).unwrap(), original);
        assert_eq!(std::fs::read(&path).unwrap(), desktop);
        let reopened = Store::open_monocode(&path).unwrap();
        assert_eq!(
            reopened.setting("workspace").unwrap().as_deref(),
            Some("new tabs")
        );
        assert_eq!(
            reopened.setting("mono-app-receipt:1:request").unwrap(),
            Some(receipt)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join("monocode.db.gpui.sqlite3"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn small_updates_do_not_rewrite_large_cached_results_and_unchanged_saves_do_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        store
            .save_setting("mono-app-receipt:1:large", &"x".repeat(10_000_000))
            .unwrap();
        let conn = store.shared.as_ref().unwrap().extras_db.as_ref().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let wal = dir.path().join("monocode.db.gpui.sqlite3-wal");
        for i in 0..10 {
            store.save_setting("workspace", &i.to_string()).unwrap();
        }
        let bytes = std::fs::metadata(&wal).unwrap().len();
        assert!(bytes < 100_000, "ten tiny updates wrote {bytes} bytes");
        let changes = conn.total_changes();
        store.save_setting("workspace", "9").unwrap();
        assert_eq!(conn.total_changes(), changes);
        assert_eq!(std::fs::metadata(&wal).unwrap().len(), bytes);
    }

    #[test]
    fn failed_save_preserves_the_committed_cache_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_monocode(&dir.path().join("monocode.db")).unwrap();
        store.save_setting("workspace", "old").unwrap();
        let conn = store.shared.as_ref().unwrap().extras_db.as_ref().unwrap();
        conn.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(store.save_setting("workspace", "new").is_err());
        assert_eq!(store.setting("workspace").unwrap().as_deref(), Some("old"));
        conn.execute_batch("PRAGMA query_only=OFF").unwrap();
        store.save_setting("workspace", "new").unwrap();
        assert_eq!(store.setting("workspace").unwrap().as_deref(), Some("new"));
    }

    #[test]
    fn read_only_legacy_access_does_not_create_or_import_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let old = dir.path().join("monocode.db.gpui.json");
        std::fs::write(&old, r#"{"workspace":"saved"}"#).unwrap();
        let (values, conn) = open(&path, true).unwrap();
        assert_eq!(values["workspace"], "saved");
        assert!(conn.is_none());
        assert!(!dir.path().join("monocode.db.gpui.sqlite3").exists());
        assert_eq!(
            std::fs::read_to_string(&old).unwrap(),
            r#"{"workspace":"saved"}"#
        );
    }

    #[test]
    fn read_only_reopen_sees_current_settings_and_refuses_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let store = Store::open_monocode(&path).unwrap();
        store.save_setting("workspace", "current").unwrap();
        let reader = Store::read_monocode(&path).unwrap();
        assert_eq!(
            reader.setting("workspace").unwrap().as_deref(),
            Some("current")
        );
        assert!(reader.save_setting("workspace", "wrong").is_err());
    }

    #[test]
    fn failed_import_is_retryable_without_losing_legacy_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monocode.db");
        let old = dir.path().join("monocode.db.gpui.json");
        std::fs::write(&old, "invalid JSON").unwrap();
        assert!(open(&path, false).is_err());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "invalid JSON");
        std::fs::write(&old, r#"{"workspace":"recovered"}"#).unwrap();
        assert_eq!(open(&path, false).unwrap().0["workspace"], "recovered");
    }
}
