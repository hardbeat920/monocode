//! macOS install: replace the running `.app` with the one in the update's
//! `.app.tar.gz`. Adapted from tauri-plugin-updater 2.10.1 (`src/updater.rs`,
//! Apache-2.0 OR MIT). The archive holds one top-level `MonoCode.app/`
//! directory, and its first path component is dropped, so the bundle lands
//! wherever the running one lives whatever it is called.

use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;

use crate::error::{Error, Result};
use crate::updater::Update;

pub(super) fn install(update: &Update, bytes: &[u8]) -> Result<()> {
    install_bundle(&update.extract_path, bytes)
}

/// Replaces the bundle at `extract_path` with the archive's bundle.
pub(super) fn install_bundle(extract_path: &Path, bytes: &[u8]) -> Result<()> {
    if extract_path.extension().and_then(|ext| ext.to_str()) != Some("app") {
        return Err(Error::NotAppBundle(extract_path.to_path_buf()));
    }

    let parent = extract_path
        .parent()
        .ok_or(Error::FailedToDetermineExtractPath)?;
    let tmp_backup_dir = tempfile::Builder::new()
        .prefix("monocode_current_app")
        .tempdir_in(parent)
        .or_else(|_| {
            tempfile::Builder::new()
                .prefix("monocode_current_app")
                .tempdir()
        })?;
    let tmp_extract_dir = tempfile::Builder::new()
        .prefix("monocode_updated_app")
        .tempdir_in(parent)
        .or_else(|_| {
            tempfile::Builder::new()
                .prefix("monocode_updated_app")
                .tempdir()
        })?;

    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(bytes)));
    let mut root = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let mut parts = path.components();
        let Some(Component::Normal(first)) = parts.next() else {
            return Err(Error::InvalidUpdaterFormat);
        };
        if Path::new(first).extension().and_then(|ext| ext.to_str()) != Some("app")
            || root.as_deref().is_some_and(|root| root != first)
            || !(entry.header().entry_type().is_file() || entry.header().entry_type().is_dir())
        {
            return Err(Error::InvalidUpdaterFormat);
        }
        root = Some(first.to_os_string());
        let mut collected_path = PathBuf::new();
        for part in parts {
            let Component::Normal(part) = part else {
                return Err(Error::InvalidUpdaterFormat);
            };
            collected_path.push(part);
        }
        if collected_path.as_os_str().is_empty() {
            continue;
        }
        let extraction_path = tmp_extract_dir.path().join(&collected_path);

        if let Some(parent) = extraction_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Err(err) = entry.unpack(&extraction_path) {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(err.into());
        }
    }
    if !tmp_extract_dir.path().join("Contents/Info.plist").is_file()
        || !tmp_extract_dir.path().join("Contents/MacOS").is_dir()
    {
        return Err(Error::InvalidUpdaterFormat);
    }

    // Move the running app aside. A copy in /Applications owned by another
    // user needs an administrator to replace it.
    let move_result = std::fs::rename(extract_path, tmp_backup_dir.path().join("current_app"));
    let need_authorization = match move_result {
        Ok(()) => false,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => true,
        Err(err) => {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(err.into());
        }
    };

    if need_authorization {
        log::debug!("app installation needs admin privileges");
        let new = parent.join(format!(".monocode-new-{}", std::process::id()));
        let backup = parent.join(format!(".monocode-backup-{}", std::process::id()));
        let script = format!(
            "do shell script \"set -e; /usr/bin/ditto {staged} {new}; mv {src} {backup}; if mv {new} {src}; then rm -rf {backup}; else mv {backup} {src}; exit 1; fi\" with administrator privileges",
            staged = applescript_shell_quote(tmp_extract_dir.path()),
            src = applescript_shell_quote(extract_path),
            new = applescript_shell_quote(&new),
            backup = applescript_shell_quote(&backup),
        );
        let status = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .status();
        if !status.is_ok_and(|status| status.success()) {
            std::fs::remove_dir_all(tmp_extract_dir.path()).ok();
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Failed to move the new app into place",
            )));
        }
    } else {
        if let Err(error) = std::fs::rename(tmp_extract_dir.path(), extract_path) {
            std::fs::rename(tmp_backup_dir.path().join("current_app"), extract_path)?;
            return Err(error.into());
        }
    }

    // Tell Launch Services the bundle changed.
    let _ = Command::new("touch").arg(extract_path).status();
    Ok(())
}

/// A path as a single-quoted shell word inside an AppleScript string. The
/// plugin pasted the path between single quotes unescaped.
fn applescript_shell_quote(path: &Path) -> String {
    let shell = format!("'{}'", path.display().to_string().replace('\'', r"'\''"));
    shell.replace('\\', r"\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_for_the_admin_script() {
        assert_eq!(
            applescript_shell_quote(Path::new("/Applications/MonoCode.app")),
            "'/Applications/MonoCode.app'"
        );
        assert_eq!(
            applescript_shell_quote(Path::new("/tmp/it's \"here\"")),
            r#"'/tmp/it'\\''s \"here\"'"#
        );
    }

    #[test]
    fn refuses_a_path_that_is_not_a_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let err = install_bundle(dir.path(), &[]).unwrap_err();
        assert!(matches!(err, Error::NotAppBundle(_)));
        assert!(dir.path().exists());
    }

    #[test]
    fn malformed_archive_preserves_the_installed_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("MonoCode.app");
        std::fs::create_dir(&bundle).unwrap();
        std::fs::write(bundle.join("old"), "original").unwrap();
        let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(gzip);
        tar.append_dir("WrongRoot", dir.path()).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        assert!(matches!(
            install_bundle(&bundle, &bytes),
            Err(Error::InvalidUpdaterFormat)
        ));
        assert_eq!(
            std::fs::read_to_string(bundle.join("old")).unwrap(),
            "original"
        );
    }
}
