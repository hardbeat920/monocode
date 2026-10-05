//! The app data directory. Tauri's `app_data_dir()` for the
//! `com.monocode.desktop` identifier, so the native app opens the same
//! `monocode.db`, checkpoints, and provider account profiles.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use monocode_settings::APP_IDENTIFIER;

/// Overrides the data directory, for development and tests. The
/// `--data-dir` flag wins over it.
pub const DATA_DIR_ENV: &str = "MONOCODE_DATA_DIR";

/// Where the data directory came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirSource {
    /// `--data-dir`.
    Flag,
    /// `MONOCODE_DATA_DIR`.
    Env,
    /// The platform default that the Tauri app uses.
    Default,
}

/// The directory and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    pub path: PathBuf,
    pub source: DataDirSource,
}

impl DataDir {
    /// The user's real data directory, shared with the Tauri app.
    pub fn is_default(&self) -> bool {
        self.source == DataDirSource::Default
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Tauri's `app_data_dir()`: the platform data directory plus the bundle
/// identifier.
/// - macOS: `~/Library/Application Support/com.monocode.desktop`
/// - Linux: `$XDG_DATA_HOME/com.monocode.desktop`, else
///   `~/.local/share/com.monocode.desktop`
/// - Windows: `%APPDATA%\com.monocode.desktop` (the roaming folder)
pub fn default_data_dir() -> Result<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        env_path("HOME")
            .context("HOME is not set")?
            .join("Library")
            .join("Application Support")
    } else if cfg!(windows) {
        env_path("APPDATA").context("APPDATA is not set")?
    } else {
        env_path("XDG_DATA_HOME")
            .filter(|path| path.is_absolute())
            .or_else(|| env_path("HOME").map(|home| home.join(".local").join("share")))
            .context("HOME is not set")?
    };
    Ok(base.join(APP_IDENTIFIER))
}

/// The flag, else the environment variable, else the platform default.
pub fn resolve(flag: Option<&Path>) -> Result<DataDir> {
    if let Some(path) = flag {
        return Ok(DataDir {
            path: path.to_path_buf(),
            source: DataDirSource::Flag,
        });
    }
    if let Some(path) = env_path(DATA_DIR_ENV) {
        return Ok(DataDir {
            path,
            source: DataDirSource::Env,
        });
    }
    Ok(DataDir {
        path: default_data_dir()?,
        source: DataDirSource::Default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_wins() {
        let dir = resolve(Some(Path::new("/tmp/mc-flag"))).unwrap();
        assert_eq!(dir.path, PathBuf::from("/tmp/mc-flag"));
        assert_eq!(dir.source, DataDirSource::Flag);
        assert!(!dir.is_default());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_default_matches_tauri_on_macos() {
        let dir = default_data_dir().unwrap();
        assert!(dir.ends_with("Library/Application Support/com.monocode.desktop"));
    }
}
