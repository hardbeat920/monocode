//! Linux install. Adapted from tauri-plugin-updater 2.10.1 (`src/updater.rs`,
//! Apache-2.0 OR MIT):
//!
//! - AppImage: swap the AppImage file in place, from a raw AppImage or an
//!   `.AppImage.tar.gz`, keeping a backup until the new file is written.
//! - deb and rpm: hand the package to `dpkg -i` or `rpm -U` through pkexec,
//!   then a graphical sudo prompt, then terminal sudo.

use std::ffi::OsStr;
use std::io::{Cursor, Write as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::Installer;
use crate::error::{Error, Result};
use crate::install::formats::{is_deb, is_gz, is_rpm};
use crate::updater::Update;

pub(super) fn install(update: &Update, bytes: &[u8]) -> Result<()> {
    match update.installer {
        Some(Installer::Deb) => install_deb(&update.extract_path, bytes),
        Some(Installer::Rpm) => install_rpm(&update.extract_path, bytes),
        _ => install_appimage(&update.extract_path, bytes),
    }
}

/// `dirs::cache_dir` without the crate.
fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
}

/// Where a temporary copy may go: the temp dir, the cache dir, then beside
/// the executable.
fn tmp_dir_locations(extract_path: &Path) -> Vec<PathBuf> {
    let mut locations = vec![std::env::temp_dir()];
    locations.extend(cache_dir());
    locations.extend(extract_path.parent().map(Path::to_path_buf));
    locations
}

pub(super) fn install_appimage(extract_path: &Path, bytes: &[u8]) -> Result<()> {
    let metadata = extract_path.metadata()?;
    for location in tmp_dir_locations(extract_path) {
        let Ok(tmp_dir) = tempfile::Builder::new()
            .prefix("monocode_appimage_update")
            .tempdir_in(&location)
        else {
            continue;
        };
        if metadata.dev() != tmp_dir.path().metadata()?.dev() {
            continue;
        }
        std::fs::set_permissions(tmp_dir.path(), std::fs::Permissions::from_mode(0o700))?;
        let backup = tmp_dir.path().join("previous.AppImage");
        let staged = tmp_dir.path().join("new.AppImage");
        if is_gz(bytes) {
            let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
            let mut archive = tar::Archive::new(decoder);
            let mut found = false;
            for entry in archive.entries()? {
                let mut entry = entry?;
                let is_appimage = entry.path()?.extension() == Some(OsStr::new("AppImage"));
                if is_appimage {
                    if found || !entry.header().entry_type().is_file() {
                        return Err(Error::InvalidUpdaterFormat);
                    }
                    entry.unpack(&staged)?;
                    found = true;
                }
            }
            if !found {
                return Err(Error::BinaryNotFoundInArchive);
            }
        } else {
            std::fs::write(&staged, bytes)?;
        }
        // Prepare the complete replacement before moving the installed copy.
        std::fs::set_permissions(&staged, metadata.permissions())?;
        std::fs::rename(extract_path, &backup)?;
        if let Err(error) = std::fs::rename(&staged, extract_path) {
            std::fs::rename(&backup, extract_path)?;
            return Err(error.into());
        }
        return Ok(());
    }
    Err(Error::TempDirNotOnSameMountPoint)
}

fn install_deb(extract_path: &Path, bytes: &[u8]) -> Result<()> {
    if !is_deb(bytes) {
        log::warn!("update is not a valid deb package");
        return Err(Error::InvalidUpdaterFormat);
    }
    try_tmp_locations(extract_path, bytes, "dpkg", "-i", "deb")
}

fn install_rpm(extract_path: &Path, bytes: &[u8]) -> Result<()> {
    if !is_rpm(bytes) {
        return Err(Error::InvalidUpdaterFormat);
    }
    try_tmp_locations(extract_path, bytes, "rpm", "-U", "rpm")
}

fn try_tmp_locations(
    extract_path: &Path,
    bytes: &[u8],
    install_cmd: &str,
    install_arg: &str,
    package_extension: &str,
) -> Result<()> {
    for location in tmp_dir_locations(extract_path) {
        let prefix = format!("monocode_{package_extension}_update");
        let Ok(tmp_dir) = tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir_in(location)
        else {
            continue;
        };
        let pkg_path = tmp_dir.path().join(format!("package.{package_extension}"));
        if std::fs::write(&pkg_path, bytes).is_ok() {
            return try_install_with_privileges(&pkg_path, install_cmd, install_arg);
        }
    }
    Err(Error::TempDirNotFound)
}

fn try_install_with_privileges(
    pkg_path: &Path,
    install_cmd: &str,
    install_arg: &str,
) -> Result<()> {
    // 1. pkexec, the graphical sudo prompt.
    if let Ok(status) = Command::new("pkexec")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .status()
        && status.success()
    {
        log::debug!("installed {} with pkexec", pkg_path.display());
        return Ok(());
    }

    // 2. A zenity or kdialog password prompt fed to sudo.
    if let Ok(password) = get_password_graphically()
        && install_with_sudo(pkg_path, &password, install_cmd, install_arg)?
    {
        log::debug!("installed {} with GUI sudo", pkg_path.display());
        return Ok(());
    }

    // 3. Terminal sudo.
    let status = Command::new("sudo")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .status()?;
    if status.success() {
        log::debug!("installed {} with sudo", pkg_path.display());
        Ok(())
    } else {
        Err(Error::PackageInstallFailed)
    }
}

fn get_password_graphically() -> Result<String> {
    let zenity = Command::new("zenity")
        .args([
            "--password",
            "--title=Authentication Required",
            "--text=Enter your password to install the update:",
        ])
        .output();
    if let Ok(output) = zenity
        && output.status.success()
    {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let kdialog = Command::new("kdialog")
        .args(["--password", "Enter your password to install the update:"])
        .output();
    if let Ok(output) = kdialog
        && output.status.success()
    {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    Err(Error::AuthenticationFailed)
}

fn install_with_sudo(
    pkg_path: &Path,
    password: &str,
    install_cmd: &str,
    install_arg: &str,
) -> Result<bool> {
    let mut child = Command::new("sudo")
        .arg("-S")
        .arg(install_cmd)
        .arg(install_arg)
        .arg(pkg_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{password}")?;
    }
    Ok(child.wait()?.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invalid_archive_preserves_the_existing_appimage() {
        let directory = tempfile::tempdir().unwrap();
        let appimage = directory.path().join("MonoCode.AppImage");
        std::fs::write(&appimage, "existing").unwrap();
        let bytes = [0x1f, 0x8b, 0x08, 0xff];
        assert!(install_appimage(&appimage, &bytes).is_err());
        assert_eq!(std::fs::read_to_string(&appimage).unwrap(), "existing");
    }

    #[test]
    fn a_raw_replacement_keeps_the_executable_mode() {
        let directory = tempfile::tempdir().unwrap();
        let appimage = directory.path().join("MonoCode.AppImage");
        std::fs::write(&appimage, "existing").unwrap();
        std::fs::set_permissions(&appimage, std::fs::Permissions::from_mode(0o755)).unwrap();
        install_appimage(&appimage, b"updated").unwrap();
        assert_eq!(std::fs::read_to_string(&appimage).unwrap(), "updated");
        assert_eq!(
            appimage.metadata().unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
