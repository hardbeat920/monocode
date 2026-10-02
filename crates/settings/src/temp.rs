//! A private scratch directory that deletes itself, for copying the old
//! webview databases before opening them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// A new directory under the system temp directory. On Unix only the
    /// owner can open it, because the copies can hold tokens.
    pub(crate) fn new(prefix: &str) -> io::Result<TempDir> {
        let base = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        for _ in 0..100 {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "monocode-{prefix}-{}-{nanos}-{n}",
                std::process::id()
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(TempDir(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not pick a temp directory name",
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
