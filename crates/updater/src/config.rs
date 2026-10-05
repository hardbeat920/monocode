//! Updater settings, the build-time feed config, and the platform names the
//! feed uses. Adapted from tauri-plugin-updater 2.10.1 (`src/updater.rs` and
//! `src/config.rs`, Apache-2.0 OR MIT).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};

/// The updater public key, baked in when the crate compiles. It is the base64
/// text of a minisign public key, the same value the Tauri release config
/// took from the `TAURI_UPDATER_PUBKEY` secret.
pub const BUILD_PUBKEY: Option<&str> = option_env!("TAURI_UPDATER_PUBKEY");

/// The `latest.json` URL, baked in when the crate compiles from
/// `TAURI_UPDATER_ENDPOINT`, as the Tauri release config did.
pub const BUILD_ENDPOINT: Option<&str> = option_env!("TAURI_UPDATER_ENDPOINT");

/// Product name, used in temp file names and the macOS bundle.
pub const APP_NAME: &str = "MonoCode";

/// Runs right before the process exits to start the Windows installer.
pub type OnBeforeExit = Arc<dyn Fn() + Send + Sync + 'static>;

/// How the running copy was installed. It picks the feed entry
/// (`{os}-{arch}-{installer}` before `{os}-{arch}`) and the install method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    AppImage,
    Deb,
    Rpm,
    App,
    Msi,
    Nsis,
}

impl Installer {
    /// The name the feed and the `{{bundle_type}}` placeholder use.
    pub fn name(self) -> &'static str {
        match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::App => "app",
            Self::Msi => "msi",
            Self::Nsis => "nsis",
        }
    }

    /// How this build was installed. Tauri's bundler patched the answer into
    /// each package's binary. The native packages share one binary, so this
    /// looks at the environment instead: macOS always runs from an `.app`,
    /// Windows from the NSIS installer, and Linux from an AppImage when the
    /// AppImage runtime set `APPIMAGE`, else from whichever package manager
    /// owns the executable.
    pub fn detect() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::App)
        } else if cfg!(windows) {
            Some(Self::Nsis)
        } else if cfg!(target_os = "linux") {
            if std::env::var_os("APPIMAGE").is_some() {
                return Some(Self::AppImage);
            }
            let exe = std::env::current_exe().ok()?;
            if owned_by("dpkg", &["-S"], &exe) {
                Some(Self::Deb)
            } else if owned_by("rpm", &["-qf"], &exe) {
                Some(Self::Rpm)
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// `true` when `tool args path` exits with success, meaning the package
/// manager knows the file.
fn owned_by(tool: &str, args: &[&str], path: &Path) -> bool {
    std::process::Command::new(tool)
        .args(args)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `updater_os`: the OS part of a feed key.
pub fn updater_os() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("linux")
    } else if cfg!(target_os = "macos") {
        Some("darwin")
    } else if cfg!(target_os = "windows") {
        Some("windows")
    } else {
        None
    }
}

/// `updater_arch`: the architecture part of a feed key. A universal macOS
/// binary answers for the slice that runs.
pub fn updater_arch() -> Option<&'static str> {
    if cfg!(target_arch = "x86") {
        Some("i686")
    } else if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else if cfg!(target_arch = "arm") {
        Some("armv7")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64")
    } else if cfg!(target_arch = "riscv64") {
        Some("riscv64")
    } else {
        None
    }
}

/// `target`: `{os}-{arch}`, the base feed key for this build.
pub fn target() -> Option<String> {
    Some(format!("{}-{}", updater_os()?, updater_arch()?))
}

/// The binary to replace or relaunch. Inside an AppImage that is the
/// AppImage file, not the binary mounted from it.
pub fn current_binary() -> std::io::Result<PathBuf> {
    if cfg!(target_os = "linux")
        && let Some(appimage) = std::env::var_os("APPIMAGE")
    {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe()
}

/// `extract_path_from_executable`: what the update replaces. On macOS that is
/// the `.app` around `Contents/MacOS/<binary>`, on Windows the install
/// directory. Linux replaces the executable itself and never calls this.
pub fn extract_path_from_executable(executable_path: &Path) -> Result<PathBuf> {
    let extract_path = executable_path
        .parent()
        .map(PathBuf::from)
        .ok_or(Error::FailedToDetermineExtractPath)?;

    if cfg!(target_os = "macos")
        && extract_path
            .display()
            .to_string()
            .contains("Contents/MacOS")
    {
        return extract_path
            .parent()
            .and_then(Path::parent)
            .map(PathBuf::from)
            .ok_or(Error::FailedToDetermineExtractPath);
    }

    Ok(extract_path)
}

/// Everything the updater needs. `from_build_env` fills it the way release
/// builds should run; tests and tools set the fields directly.
#[derive(Clone)]
pub struct UpdaterConfig {
    /// Feed URLs, tried in order. They may hold `{{current_version}}`,
    /// `{{target}}`, `{{arch}}`, and `{{bundle_type}}`.
    pub endpoints: Vec<String>,
    /// Base64 of the minisign public key text.
    pub pubkey: String,
    /// The running version, compared against the feed's `version`.
    pub current_version: String,
    /// Feed key to use instead of `{os}-{arch}[-{installer}]`.
    pub target: Option<String>,
    /// The binary to update. Defaults to `current_binary()`.
    pub executable_path: Option<PathBuf>,
    /// How this copy was installed. Defaults to `Installer::detect()`.
    pub installer: Option<Installer>,
    /// Overall timeout for the feed request and the download.
    pub timeout: Option<Duration>,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// Ignore `HTTPS_PROXY`, `HTTP_PROXY`, and `ALL_PROXY`.
    pub no_proxy: bool,
    /// Extra arguments for the Windows installer.
    pub installer_args: Vec<OsString>,
    /// This process's arguments, passed through the NSIS installer's `/ARGS`
    /// so the relaunched app gets them back. The first is the program.
    pub current_exe_args: Vec<OsString>,
    /// Accept `http` endpoints in release builds. Debug builds only warn.
    pub dangerous_insecure_transport_protocol: bool,
    /// Runs before the process exits to start the Windows installer.
    pub on_before_exit: Option<OnBeforeExit>,
}

impl fmt::Debug for UpdaterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdaterConfig")
            .field("endpoints", &self.endpoints)
            .field("current_version", &self.current_version)
            .field("target", &self.target)
            .field("executable_path", &self.executable_path)
            .field("installer", &self.installer)
            .finish_non_exhaustive()
    }
}

impl UpdaterConfig {
    /// A config with no endpoints and no key. Checking fails with
    /// `Error::EmptyEndpoints`, which the flow treats as "not configured".
    pub fn new(current_version: impl Into<String>) -> Self {
        Self {
            endpoints: Vec::new(),
            pubkey: String::new(),
            current_version: current_version.into(),
            target: None,
            executable_path: None,
            installer: None,
            timeout: None,
            headers: Vec::new(),
            no_proxy: false,
            installer_args: Vec::new(),
            current_exe_args: std::env::args_os().collect(),
            dangerous_insecure_transport_protocol: false,
            on_before_exit: None,
        }
    }

    /// The release config: the endpoint and key baked in at build time from
    /// `TAURI_UPDATER_ENDPOINT` and `TAURI_UPDATER_PUBKEY`. Builds without them
    /// have no endpoints, like a Tauri build without the release config.
    pub fn from_build_env(current_version: impl Into<String>) -> Self {
        let mut config = Self::new(current_version);
        config.endpoints = BUILD_ENDPOINT
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .map(|endpoint| vec![endpoint.to_string()])
            .unwrap_or_default();
        config.pubkey = BUILD_PUBKEY.unwrap_or_default().trim().to_string();
        config
    }

    /// `validate_endpoints`: release builds refuse plain `http` unless told to.
    pub(crate) fn validate_endpoints(&self, endpoints: &[url::Url]) -> Result<()> {
        if self.dangerous_insecure_transport_protocol {
            return Ok(());
        }
        for url in endpoints {
            if url.scheme() != "https" {
                if cfg!(debug_assertions) {
                    log::warn!(
                        "the updater endpoint {url} does not use https. Release builds refuse it."
                    );
                } else {
                    return Err(Error::InsecureTransportProtocol);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_joins_os_and_arch() {
        let target = target().expect("supported platform");
        assert_eq!(
            target,
            format!("{}-{}", updater_os().unwrap(), updater_arch().unwrap())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn extract_path_is_the_app_bundle_on_macos() {
        let exe = Path::new("/Applications/MonoCode.app/Contents/MacOS/MonoCode");
        assert_eq!(
            extract_path_from_executable(exe).unwrap(),
            PathBuf::from("/Applications/MonoCode.app")
        );
        let loose = Path::new("/tmp/target/release/monocode-app");
        assert_eq!(
            extract_path_from_executable(loose).unwrap(),
            PathBuf::from("/tmp/target/release")
        );
    }

    #[test]
    fn build_env_without_values_has_no_endpoints() {
        let config = UpdaterConfig::from_build_env("1.2.3");
        assert_eq!(config.current_version, "1.2.3");
        if BUILD_ENDPOINT.is_none_or(|endpoint| endpoint.trim().is_empty()) {
            assert!(config.endpoints.is_empty());
        }
    }
}
