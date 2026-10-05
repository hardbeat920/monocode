//! Installs a downloaded, verified package the way tauri-plugin-updater did
//! on each platform.

pub(crate) mod formats;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
pub(crate) mod windows_args;

use crate::error::Result;
use crate::updater::Update;

pub(crate) fn install(update: &Update, bytes: &[u8]) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::install(update, bytes)
    }
    #[cfg(target_os = "linux")]
    {
        linux::install(update, bytes)
    }
    #[cfg(windows)]
    {
        windows::install(update, bytes)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (update, bytes);
        Err(crate::error::Error::UnsupportedOs)
    }
}
