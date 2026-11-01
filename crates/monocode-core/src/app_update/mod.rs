//! Native app updates. Uses the same Minisign keys and package signatures as
//! Tauri. The native feed is additionally signed, binding version, channel and
//! platform to the package; it cannot dispatch a production Tauri update.
mod install;
#[cfg(test)]
mod tests;

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const APP_ID: &str = "dev.usemono.gpui";
pub const CHANNEL: &str = "native-preview";
const MAX_FEED: u64 = 2 * 1024 * 1024;
const MAX_PACKAGE: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    App,
    Appimage,
    Deb,
    Rpm,
    Nsis,
}

impl PackageKind {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Appimage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Nsis => "nsis",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Installation {
    pub kind: PackageKind,
    pub path: PathBuf,
}

impl Installation {
    pub fn detect() -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        {
            let app = executable
                .ancestors()
                .nth(3)
                .ok_or("Cannot locate the app bundle")?;
            let plist = std::fs::read_to_string(app.join("Contents/Info.plist"))
                .map_err(|_| "Updates require an installed Monocode Native app bundle")?;
            if !plist.contains(&format!("<string>{APP_ID}</string>")) {
                return Err("This is not a Monocode Native bundle".into());
            }
            Ok(Self {
                kind: PackageKind::App,
                path: app.to_path_buf(),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(target_os = "linux")]
            if let Some(image) = std::env::var_os("APPIMAGE") {
                let path = PathBuf::from(image);
                if path.is_absolute() && path.is_file() {
                    return Ok(Self {
                        kind: PackageKind::Appimage,
                        path,
                    });
                }
            }
            let marker = executable
                .parent()
                .ok_or("Cannot locate the installation")?
                .join("native-install.json");
            #[derive(Deserialize)]
            struct Marker {
                app_id: String,
                kind: PackageKind,
            }
            let marker: Marker = serde_json::from_slice(
                &std::fs::read(marker)
                    .map_err(|_| "Updates require an installed Monocode Native package")?,
            )
            .map_err(|e| e.to_string())?;
            let supported = if cfg!(windows) {
                marker.kind == PackageKind::Nsis
            } else {
                matches!(marker.kind, PackageKind::Deb | PackageKind::Rpm)
            };
            if marker.app_id != APP_ID || !supported {
                return Err("Unsupported native installation".into());
            }
            Ok(Self {
                kind: marker.kind,
                path: executable,
            })
        }
    }

    pub fn target(&self) -> Result<String, String> {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            "linux" => "linux",
            "windows" => "windows",
            _ => return Err("Updates are unavailable on this platform".into()),
        };
        Ok(format!(
            "{os}-{}-{}",
            std::env::consts::ARCH,
            self.kind.suffix()
        ))
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: String,
    pub public_key: String,
    pub current: Version,
}
impl Config {
    pub fn new(endpoint: &str, public_key: &str, current: &str) -> Result<Self, String> {
        https_url(endpoint)?;
        decode_key(public_key)?;
        Ok(Self {
            endpoint: endpoint.into(),
            public_key: public_key.into(),
            current: Version::parse(current).map_err(|e| e.to_string())?,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Artifact {
    url: String,
    signature: String,
}
#[derive(Deserialize)]
struct Manifest {
    app_id: String,
    channel: String,
    version: Version,
    #[serde(default)]
    notes: String,
    platforms: BTreeMap<String, Artifact>,
}
#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub version: Version,
    pub notes: String,
    artifact: Artifact,
    public_key: String,
}
pub struct Download {
    _directory: tempfile::TempDir,
    path: PathBuf,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    Relaunch(PathBuf),
    InstallerStarted,
}

/// Launch after the old process has completed its normal persistence/shutdown
/// hooks. Values are positional arguments, never interpolated into shell code.
pub fn schedule_relaunch(executable: &Path) -> Result<(), String> {
    if !executable.is_file() {
        return Err("The updated executable could not be found".into());
    }
    #[cfg(unix)]
    {
        use std::{
            os::unix::process::CommandExt,
            process::{Command, Stdio},
        };
        #[cfg(target_os = "macos")]
        let (script, path) = (
            "pid=$1; app=$2; shift 2; while kill -0 \"$pid\" 2>/dev/null; do sleep 0.1; done; exec /usr/bin/open -n \"$app\" --args \"$@\"",
            executable.ancestors().nth(3).ok_or("Cannot locate the updated app bundle")?,
        );
        #[cfg(not(target_os = "macos"))]
        let (script, path) = (
            "pid=$1; executable=$2; shift 2; while kill -0 \"$pid\" 2>/dev/null; do sleep 0.1; done; exec \"$executable\" \"$@\"",
            executable,
        );
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .arg("monocode-native-relaunch")
            .arg(std::process::id().to_string())
            .arg(path)
            .args(std::env::args_os().skip(1))
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        command.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = executable;
        Err("The native installer is responsible for relaunching this app".into())
    }
}

fn https_url(value: &str) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|e| e.to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Update URLs must use HTTPS without embedded credentials".into());
    }
    Ok(())
}
fn decoded(value: &str) -> Result<String, String> {
    String::from_utf8(STANDARD.decode(value.trim()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn decode_key(value: &str) -> Result<PublicKey, String> {
    PublicKey::decode(&decoded(value)?).map_err(|e| format!("Invalid update public key: {e}"))
}
fn verify(bytes: &[u8], signature: &str, key: &str) -> Result<(), String> {
    let signature = Signature::decode(&decoded(signature)?)
        .map_err(|e| format!("Invalid update signature: {e}"))?;
    decode_key(key)?
        .verify(bytes, &signature, true)
        .map_err(|e| format!("Update signature verification failed: {e}"))
}

fn select(manifest: Manifest, config: &Config, target: &str) -> Result<Option<Update>, String> {
    if manifest.app_id != APP_ID || manifest.channel != CHANNEL {
        return Err("The update feed belongs to a different application or release channel".into());
    }
    if manifest.version <= config.current {
        return Ok(None);
    }
    let artifact = manifest
        .platforms
        .get(target)
        .ok_or_else(|| format!("No native update package is available for {target}"))?
        .clone();
    https_url(&artifact.url)?;
    Signature::decode(&decoded(&artifact.signature)?)
        .map_err(|e| format!("Invalid package signature: {e}"))?;
    Ok(Some(Update {
        version: manifest.version,
        notes: manifest.notes,
        artifact,
        public_key: config.public_key.clone(),
    }))
}

pub fn verify_feed(bytes: &[u8], config: &Config, target: &str) -> Result<Option<Update>, String> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid native update feed: {e}"))?;
    let payload = STANDARD
        .decode(envelope.payload)
        .map_err(|e| e.to_string())?;
    verify(&payload, &envelope.signature, &config.public_key)?;
    select(
        serde_json::from_slice(&payload).map_err(|e| e.to_string())?,
        config,
        target,
    )
}

fn request(url: &str) -> Result<ureq::Response, String> {
    https_url(url)?;
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
        .get(url)
        .set("User-Agent", "Monocode-Native-Updater")
        .set("Accept-Encoding", "identity")
        .call()
        .map_err(|e| e.to_string())?;
    https_url(response.get_url())?;
    Ok(response)
}

pub fn check(config: &Config, installation: &Installation) -> Result<Option<Update>, String> {
    let response = request(&config.endpoint)?;
    if response.status() == 204 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_FEED + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FEED {
        return Err("Native update feed exceeds its size limit".into());
    }
    verify_feed(&bytes, config, &installation.target()?)
}

impl Update {
    pub fn download(&self, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Download, String> {
        let response = request(&self.artifact.url)?;
        let total = response
            .header("Content-Length")
            .and_then(|n| n.parse::<u64>().ok());
        if total.is_some_and(|n| n > MAX_PACKAGE) {
            return Err("Update package exceeds its size limit".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("monocode-native-update-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let path = directory.path().join("package");
        let mut file = File::create(&path).map_err(|e| e.to_string())?;
        let mut reader = response.into_reader();
        let mut downloaded = 0;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            downloaded += n as u64;
            if downloaded > MAX_PACKAGE {
                return Err("Update package exceeds its size limit".into());
            }
            file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            progress(downloaded, total);
        }
        if total.is_some_and(|n| n != downloaded) {
            return Err("Update download was incomplete".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        let download = Download {
            _directory: directory,
            path,
        };
        self.verify_file(&download.path)?;
        Ok(download)
    }
    pub fn verify_file(&self, path: &Path) -> Result<(), String> {
        let signature =
            Signature::decode(&decoded(&self.artifact.signature)?).map_err(|e| e.to_string())?;
        let key = decode_key(&self.public_key)?;
        // Tauri's signer emits prehashed Minisign signatures, allowing bounded
        // memory verification even for large app bundles.
        let mut verifier = key.verify_stream(&signature).map_err(|e| e.to_string())?;
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            verifier.update(&buffer[..n]);
        }
        verifier
            .finalize()
            .map_err(|e| format!("Update signature verification failed: {e}"))
    }
    pub fn install(
        &self,
        download: Download,
        installation: &Installation,
    ) -> Result<Installed, String> {
        // Reverify immediately before installation, including cached downloads.
        self.verify_file(&download.path)?;
        install::install(self, download, installation)
    }
}
