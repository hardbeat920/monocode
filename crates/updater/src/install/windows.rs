//! Windows install: write the downloaded installer to a temp file, start it,
//! and exit so it can replace the binary. Adapted from tauri-plugin-updater
//! 2.10.1 (`src/updater.rs`, Apache-2.0 OR MIT), in its default passive mode.
//!
//! The NSIS installer gets `/P /R /UPDATE /ARGS <this process's arguments>`:
//! passive progress, restart the app when done, update mode (no new
//! shortcuts), and the arguments for the restarted app. The feed only ships
//! the bare `-setup.exe`, so the plugin's `.zip` handling is left out.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::PathBuf;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

use crate::config::APP_NAME;
use crate::error::{Error, Result};
use crate::install::formats::{is_exe, is_msi};
use crate::install::windows_args::{escape_msi_property_arg, escape_nsis_current_exe_arg};
use crate::updater::Update;

/// Passive mode's switches (`WindowsUpdateInstallMode::Passive`).
const NSIS_PASSIVE_ARGS: [&str; 2] = ["/P", "/R"];
const MSIEXEC_PASSIVE_ARGS: [&str; 1] = ["/passive"];

enum WindowsUpdaterType {
    Nsis {
        path: PathBuf,
        _temp: tempfile::TempPath,
    },
    Msi {
        path: PathBuf,
        _temp: tempfile::TempPath,
    },
}

pub(super) fn install(update: &Update, bytes: &[u8]) -> Result<()> {
    let updater_type = extract(update, bytes)?;

    let current_args: Vec<&OsStr> = update
        .current_exe_args
        .iter()
        .skip(1)
        .map(OsString::as_os_str)
        .collect();
    let extra_args = update.installer_args.iter().map(OsString::as_os_str);

    let (file, parameters): (OsString, Vec<OsString>) = match &updater_type {
        WindowsUpdaterType::Nsis { path, .. } => {
            let mut args: Vec<OsString> = NSIS_PASSIVE_ARGS.iter().map(OsString::from).collect();
            args.push("/UPDATE".into());
            args.push("/ARGS".into());
            args.extend(
                current_args
                    .iter()
                    .map(|arg| OsString::from(escape_nsis_current_exe_arg(arg))),
            );
            args.extend(extra_args.map(OsString::from));
            (path.as_os_str().to_os_string(), args)
        }
        WindowsUpdaterType::Msi { path, .. } => {
            let escaped_args = current_args
                .iter()
                .map(escape_msi_property_arg)
                .collect::<Vec<_>>()
                .join(" ");
            let mut quoted_path = OsString::from("\"");
            quoted_path.push(path.as_os_str());
            quoted_path.push("\"");
            let mut args: Vec<OsString> = vec!["/i".into(), quoted_path];
            args.extend(MSIEXEC_PASSIVE_ARGS.iter().map(OsString::from));
            args.push("/promptrestart".into());
            args.extend(extra_args.map(OsString::from));
            args.push("AUTOLAUNCHAPP=True".into());
            args.push(format!("LAUNCHAPPARGS=\"{escaped_args}\"").into());
            let msiexec = std::env::var("SYSTEMROOT").map_or_else(
                |_| OsString::from("msiexec.exe"),
                |root| OsString::from(format!("{root}\\System32\\msiexec.exe")),
            );
            (msiexec, args)
        }
    };

    if let Some(on_before_exit) = update.on_before_exit.as_ref() {
        log::debug!("running on_before_exit hook");
        on_before_exit();
    }

    let file = encode_wide(&file);
    let parameters = encode_wide(parameters.join(OsStr::new(" ")));
    let operation = encode_wide("open");
    // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives
    // the call, and a null window and directory are allowed.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOW,
        )
    };
    if result as isize <= 32 {
        return Err(Error::PackageInstallFailed);
    }

    std::process::exit(0);
}

fn extract(update: &Update, bytes: &[u8]) -> Result<WindowsUpdaterType> {
    if is_exe(bytes) {
        let (path, temp) = write_to_temp(update, bytes, ".exe")?;
        Ok(WindowsUpdaterType::Nsis { path, _temp: temp })
    } else if is_msi(bytes) {
        let (path, temp) = write_to_temp(update, bytes, ".msi")?;
        Ok(WindowsUpdaterType::Msi { path, _temp: temp })
    } else {
        Err(Error::InvalidUpdaterFormat)
    }
}

fn write_to_temp(
    update: &Update,
    bytes: &[u8],
    ext: &str,
) -> Result<(PathBuf, tempfile::TempPath)> {
    let temp_dir = tempfile::Builder::new()
        .prefix(&format!("{APP_NAME}-{}-updater-", update.version))
        .tempdir()?
        .keep();
    let mut temp_file = tempfile::Builder::new()
        .prefix(&format!("{APP_NAME}-{}-installer", update.version))
        .suffix(ext)
        .rand_bytes(0)
        .tempfile_in(temp_dir)?;
    temp_file.write_all(bytes)?;
    let temp = temp_file.into_temp_path();
    Ok((temp.to_path_buf(), temp))
}

fn encode_wide(string: impl AsRef<OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}
