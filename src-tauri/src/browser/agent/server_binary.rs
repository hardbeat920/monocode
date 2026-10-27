//! Which executable a provider's `monocode-browser` entry starts.
//!
//! Providers keep `browser-mcp` running for as long as a session is open.
//! Windows locks a running executable, so registering MonoCode's own exe
//! made `tauri dev` fail to relink it ("Access is denied") and would make the
//! updater fail to replace `MonoCode.exe` while any agent session is open.
//! On Windows the entry therefore starts a copy in the app data directory,
//! named after the build it was copied from: a new build gets a new name,
//! so a copy still locked by an old session is never overwritten, and old
//! copies are deleted once nothing holds them.
//!
//! macOS and Linux do not lock running executables, and there the copy would
//! do harm: a binary copied out of the signed `.app` bundle no longer matches
//! the bundle's signature seal, and an AppImage's binary needs the libraries
//! inside the mounted image. Those register the executable itself; for an
//! AppImage that is the `.AppImage` file, since the mount point disappears
//! when the app quits.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

/// File name prefix of the copies; anything else in the directory is left alone.
const PREFIX: &str = "monocode-browser-mcp-";

/// The executable the running app would register, before any copy.
pub fn source_executable() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(image));
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

/// Whether this platform registers a copy instead of the executable itself.
pub fn uses_copy() -> bool {
    cfg!(windows)
}

/// Name of the copy of `source`: changes with every build (version, size and
/// modification time), without reading the whole executable.
pub fn copy_name(source: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(source).map_err(|e| e.to_string())?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_nanos())
        .unwrap_or_default();
    let identity = format!(
        "{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        metadata.len(),
        modified
    );
    let digest = Sha256::digest(identity.as_bytes());
    let short: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{PREFIX}{}-{short}{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::EXE_SUFFIX
    ))
}

/// Path the entry should name, without creating anything (for status).
pub fn planned(source: &Path, dir: &Path, copy: bool) -> Result<PathBuf, String> {
    if copy {
        Ok(dir.join(copy_name(source)?))
    } else {
        Ok(source.to_path_buf())
    }
}

/// Make sure the planned executable exists and return it. Copies through a
/// temporary file and a rename, so a half-written copy is never registered,
/// then deletes older copies that nothing holds open anymore.
pub fn install(source: &Path, dir: &Path, copy: bool) -> Result<PathBuf, String> {
    let target = planned(source, dir, copy)?;
    if !copy {
        return Ok(target);
    }
    let expected = std::fs::metadata(source).map_err(|e| e.to_string())?.len();
    let complete = std::fs::metadata(&target).is_ok_and(|m| m.len() == expected);
    if !complete {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let temporary = dir.join(format!(".{PREFIX}{}.tmp", uuid::Uuid::new_v4().simple()));
        let copied = std::fs::copy(source, &temporary).and_then(|_| {
            match std::fs::metadata(&target) {
                // A concurrent registration finished the same copy first.
                Ok(existing) if existing.len() == expected => {
                    return std::fs::remove_file(&temporary);
                }
                // Left by an interrupted attempt; on Windows a rename cannot
                // replace it, so remove it first.
                Ok(_) => std::fs::remove_file(&target)?,
                Err(_) => {}
            }
            std::fs::rename(&temporary, &target)
        });
        if let Err(error) = copied {
            let _ = std::fs::remove_file(&temporary);
            // Another registration may have finished the same copy meanwhile.
            if !std::fs::metadata(&target).is_ok_and(|m| m.len() == expected) {
                return Err(format!(
                    "Cannot copy MonoCode's browser server to {}: {error}",
                    target.display()
                ));
            }
        }
    }
    remove_old_copies(dir, &target);
    Ok(target)
}

/// Delete every other copy; a copy a running session still holds is locked
/// on Windows and stays until a later registration.
fn remove_old_copies(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stale = if name.starts_with(PREFIX) {
            path != keep
        } else if name.starts_with(&format!(".{PREFIX}")) {
            // A temporary file may belong to a registration still copying.
            entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 3600)
        } else {
            false
        };
        if stale {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> PathBuf {
        let root = std::env::temp_dir().join(format!("monocode-bin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn copies_once_per_build_and_drops_older_copies() {
        let root = sandbox();
        let source = root.join("monocode.exe");
        std::fs::write(&source, b"build one").unwrap();
        let bin = root.join("bin");

        let first = install(&source, &bin, true).unwrap();
        assert_eq!(first.parent(), Some(bin.as_path()));
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(PREFIX));
        assert_eq!(std::fs::read(&first).unwrap(), b"build one");
        assert_eq!(planned(&source, &bin, true).unwrap(), first);
        // Same build: same copy, nothing rewritten.
        assert_eq!(install(&source, &bin, true).unwrap(), first);

        // A rebuild (new size and time) gets a new name; the old copy, no
        // longer held by anything here, is removed. Unrelated files stay.
        std::fs::write(bin.join("notes.txt"), b"keep").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&source, b"build two, larger").unwrap();
        let second = install(&source, &bin, true).unwrap();
        assert_ne!(second, first);
        assert!(!first.exists());
        assert!(bin.join("notes.txt").exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"build two, larger");

        // A truncated copy under the right name is replaced, not trusted.
        std::fs::write(&second, b"part").unwrap();
        assert_eq!(install(&source, &bin, true).unwrap(), second);
        assert_eq!(std::fs::read(&second).unwrap(), b"build two, larger");
        let leftovers: Vec<_> = std::fs::read_dir(&bin)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn without_a_copy_the_executable_itself_is_registered() {
        let root = sandbox();
        let source = root.join("monocode");
        std::fs::write(&source, b"x").unwrap();
        let bin = root.join("bin");
        assert_eq!(install(&source, &bin, false).unwrap(), source);
        assert!(!bin.exists(), "nothing is copied");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copies_are_used_exactly_where_executables_are_locked() {
        assert_eq!(uses_copy(), cfg!(windows));
        let name = copy_name(&std::env::current_exe().unwrap()).unwrap();
        assert!(name.ends_with(std::env::consts::EXE_SUFFIX));
        assert!(name.contains(env!("CARGO_PKG_VERSION")));
    }
}
