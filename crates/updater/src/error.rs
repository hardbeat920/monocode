//! Updater errors. The messages match tauri-plugin-updater 2.10.1
//! (`src/error.rs`, Apache-2.0 OR MIT), because the update flow shows them to
//! the user and matches one of them by text.

use std::path::PathBuf;

/// Everything that can go wrong while checking, downloading, or installing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No endpoint was baked into this build.
    #[error("Updater does not have any endpoints set.")]
    EmptyEndpoints,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Semver(#[from] semver::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
    /// No endpoint answered with a release.
    #[error("Could not fetch a valid release JSON from the remote")]
    ReleaseNotFound,
    #[error(
        "Unsupported application architecture, expected one of `x86`, `x86_64`, `arm` or `aarch64`."
    )]
    UnsupportedArch,
    #[error("Unsupported OS, expected one of `linux`, `darwin` or `windows`.")]
    UnsupportedOs,
    #[error("Failed to determine updater package extract path.")]
    FailedToDetermineExtractPath,
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),
    /// A request that could not complete: DNS, connect, TLS, or a read error.
    #[error("{0}")]
    Http(String),
    #[error("the platform `{0}` was not found in the response `platforms` object")]
    TargetNotFound(String),
    #[error("None of the fallback platforms `{0:?}` were found in the response `platforms` object")]
    TargetsNotFound(Vec<String>),
    /// The download answered with a status other than 2xx.
    #[error("`{0}`")]
    Network(String),
    #[error(transparent)]
    Minisign(#[from] minisign_verify::Error),
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
    #[error(
        "The signature {0} could not be decoded, please check if it is a valid base64 string. The signature must be the contents of the `.sig` file from the release, as a string."
    )]
    SignatureUtf8(String),
    #[error("temp directory is not on the same mount point as the AppImage")]
    TempDirNotOnSameMountPoint,
    #[error("binary for the current target not found in the archive")]
    BinaryNotFoundInArchive,
    #[error("failed to create temporary directory")]
    TempDirNotFound,
    #[error("Authentication failed or was cancelled")]
    AuthenticationFailed,
    #[error("Failed to install package")]
    PackageInstallFailed,
    #[error("invalid updater binary format")]
    InvalidUpdaterFormat,
    #[error("The configured updater endpoint must use a secure protocol like `https`.")]
    InsecureTransportProtocol,
    /// On macOS the update replaces the running `.app`. A binary that is not
    /// inside one (a `cargo run` build) has nothing to replace. The plugin did
    /// not check this and would have replaced the binary's directory.
    #[error("{} is not inside an app bundle, so the update has nothing to replace", .0.display())]
    NotAppBundle(PathBuf),
    /// The worker thread stopped before it answered.
    #[error("the updater thread stopped unexpectedly")]
    WorkerStopped,
}

pub type Result<T> = std::result::Result<T, Error>;
