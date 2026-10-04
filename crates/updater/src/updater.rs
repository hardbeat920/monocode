//! Check the feed, download a package, verify it, and install it. Adapted
//! from tauri-plugin-updater 2.10.1 (`src/updater.rs`, Apache-2.0 OR MIT),
//! with ureq in place of reqwest. The calls block; `check_async` and
//! `download_and_install_async` run them on a worker thread under any
//! executor.

use std::ffi::OsString;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::Duration;

use semver::Version;
use time::OffsetDateTime;
use url::Url;

use crate::config::{
    Installer, OnBeforeExit, UpdaterConfig, current_binary, extract_path_from_executable,
    updater_arch, updater_os,
};
use crate::error::{Error, Result};
use crate::manifest::{RemoteRelease, expand_endpoint};
use crate::verify::verify_signature;

const UPDATER_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Bytes read per progress event.
const CHUNK: usize = 64 * 1024;

/// Download progress, the events `downloadAndInstall` reported to the
/// TypeScript flow. `Started` comes with the first chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

/// Checks one feed for this build. Cheap to clone.
#[derive(Clone)]
pub struct Updater {
    config: UpdaterConfig,
    current_version: Version,
    endpoints: Vec<Url>,
    arch: &'static str,
    installer: Option<Installer>,
    extract_path: PathBuf,
}

impl std::fmt::Debug for Updater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater")
            .field("current_version", &self.current_version)
            .field("endpoints", &self.endpoints)
            .field("installer", &self.installer)
            .field("extract_path", &self.extract_path)
            .finish_non_exhaustive()
    }
}

impl Updater {
    /// `UpdaterBuilder::build`. Fails with `Error::EmptyEndpoints` when the
    /// build has no feed, which the flow reports as "not configured".
    pub fn new(config: UpdaterConfig) -> Result<Self> {
        let endpoints = config
            .endpoints
            .iter()
            .map(|endpoint| endpoint.parse::<Url>())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        config.validate_endpoints(&endpoints)?;
        if endpoints.is_empty() {
            return Err(Error::EmptyEndpoints);
        }

        let arch = updater_arch().ok_or(Error::UnsupportedArch)?;
        let current_version = Version::parse(&config.current_version)?;

        let executable_path = match &config.executable_path {
            Some(path) => path.clone(),
            None => current_binary()?,
        };
        let extract_path = if cfg!(target_os = "linux") {
            executable_path
        } else {
            extract_path_from_executable(&executable_path)?
        };
        let installer = config.installer.or_else(Installer::detect);

        Ok(Self {
            config,
            current_version,
            endpoints,
            arch,
            installer,
            extract_path,
        })
    }

    /// The version this updater compares against.
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    /// What an install replaces: the `.app` on macOS, the install directory on
    /// Windows, the executable or AppImage on Linux.
    pub fn extract_path(&self) -> &std::path::Path {
        &self.extract_path
    }

    /// Asks each endpoint in turn for the latest release. `Ok(None)` means
    /// this build is current. A 204 answer also means current.
    pub fn check(&self) -> Result<Option<Update>> {
        let target = match &self.config.target {
            Some(target) => target.clone(),
            None => updater_os().ok_or(Error::UnsupportedOs)?.to_string(),
        };
        let bundle_type = self.installer.map_or("unknown", Installer::name);

        let mut remote_release: Option<RemoteRelease> = None;
        let mut raw_json: Option<serde_json::Value> = None;
        let mut last_error: Option<Error> = None;
        for endpoint in &self.endpoints {
            let url = expand_endpoint(
                endpoint,
                &self.current_version,
                &target,
                self.arch,
                bundle_type,
            )?;
            log::debug!("checking for updates {url}");

            let request = self.request(&url, "application/json");
            match request.call() {
                Ok(response) => {
                    if response.status() == 204 {
                        log::debug!("update endpoint returned 204 No Content");
                        return Ok(None);
                    }
                    let body = response
                        .into_string()
                        .map_err(|e| Error::Http(e.to_string()))?;
                    let update_response: serde_json::Value = serde_json::from_str(&body)
                        .map_err(|e| Error::Http(format!("error decoding response body: {e}")))?;
                    raw_json = Some(update_response.clone());
                    match serde_json::from_value::<RemoteRelease>(update_response) {
                        Ok(release) => {
                            last_error = None;
                            remote_release = Some(release);
                            break;
                        }
                        Err(err) => {
                            log::error!("failed to deserialize update response: {err}");
                            last_error = Some(err.into());
                        }
                    }
                }
                Err(ureq::Error::Status(code, _)) => {
                    log::error!("update endpoint answered with status {code}");
                }
                Err(err @ ureq::Error::Transport(_)) => {
                    log::error!("failed to check for updates: {err}");
                    last_error = Some(Error::Http(err.to_string()));
                }
            }
        }

        if let Some(error) = last_error {
            return Err(error);
        }
        let release = remote_release.ok_or(Error::ReleaseNotFound)?;
        let should_update = release.version > self.current_version;
        let (download_url, signature) = self.get_urls(&release)?;

        if !should_update {
            return Ok(None);
        }
        Ok(Some(Update {
            body: release.notes.clone(),
            current_version: self.current_version.to_string(),
            version: release.version.to_string(),
            date: release.pub_date,
            target,
            download_url: download_url.clone(),
            signature: signature.clone(),
            raw_json: raw_json.unwrap_or_default(),
            pubkey: self.config.pubkey.clone(),
            timeout: self.config.timeout,
            headers: self.config.headers.clone(),
            no_proxy: self.config.no_proxy,
            extract_path: self.extract_path.clone(),
            installer: self.installer,
            installer_args: self.config.installer_args.clone(),
            current_exe_args: self.config.current_exe_args.clone(),
            on_before_exit: self.config.on_before_exit.clone(),
        }))
    }

    /// `check` on a worker thread. The future needs no particular executor.
    pub async fn check_async(&self) -> Result<Option<Update>> {
        let updater = self.clone();
        crate::background::run_blocking(move || updater.check()).await?
    }

    /// The configured target, else `{os}-{arch}-{installer}` and then
    /// `{os}-{arch}`.
    fn get_urls<'a>(&self, release: &'a RemoteRelease) -> Result<(&'a Url, &'a String)> {
        if let Some(target) = &self.config.target {
            return Ok((release.download_url(target)?, release.signature(target)?));
        }

        let os = updater_os().ok_or(Error::UnsupportedOs)?;
        let arch = self.arch;
        let mut targets = Vec::new();
        if let Some(installer) = self.installer {
            targets.push(format!("{os}-{arch}-{}", installer.name()));
        }
        targets.push(format!("{os}-{arch}"));

        for target in &targets {
            log::debug!("searching for updater target '{target}' in release data");
            if let (Ok(download_url), Ok(signature)) =
                (release.download_url(target), release.signature(target))
            {
                return Ok((download_url, signature));
            }
        }
        Err(Error::TargetsNotFound(targets))
    }

    fn request(&self, url: &Url, accept: &str) -> ureq::Request {
        request(
            url,
            accept,
            self.config.timeout,
            &self.config.headers,
            self.config.no_proxy,
        )
    }
}

fn request(
    url: &Url,
    accept: &str,
    timeout: Option<Duration>,
    headers: &[(String, String)],
    no_proxy: bool,
) -> ureq::Request {
    let mut agent = ureq::AgentBuilder::new()
        .user_agent(UPDATER_USER_AGENT)
        .try_proxy_from_env(!no_proxy)
        .timeout_connect(Duration::from_secs(30));
    if let Some(timeout) = timeout {
        agent = agent.timeout(timeout);
    }
    let mut request = agent.build().get(url.as_str());
    let mut has_accept = false;
    for (name, value) in headers {
        has_accept |= name.eq_ignore_ascii_case("accept");
        request = request.set(name, value);
    }
    if !has_accept {
        request = request.set("Accept", accept);
    }
    request
}

/// A release newer than the running build.
#[derive(Clone)]
pub struct Update {
    /// Release notes from the feed.
    pub body: Option<String>,
    /// The version that checked.
    pub current_version: String,
    /// The version on offer.
    pub version: String,
    /// When the release was published.
    pub date: Option<OffsetDateTime>,
    /// The `{{target}}` value: the configured target or the OS name.
    pub target: String,
    pub download_url: Url,
    /// Base64 of the package's `.sig` file.
    pub signature: String,
    /// The whole feed response, for fields this crate does not read.
    pub raw_json: serde_json::Value,
    pub(crate) pubkey: String,
    pub(crate) timeout: Option<Duration>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) no_proxy: bool,
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) extract_path: PathBuf,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) installer: Option<Installer>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) installer_args: Vec<OsString>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) current_exe_args: Vec<OsString>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) on_before_exit: Option<OnBeforeExit>,
}

impl std::fmt::Debug for Update {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Update")
            .field("current_version", &self.current_version)
            .field("version", &self.version)
            .field("target", &self.target)
            .field("download_url", &self.download_url.as_str())
            .finish_non_exhaustive()
    }
}

impl Update {
    /// Downloads the package and verifies its signature against the build's
    /// public key. Nothing is written to disk.
    pub fn download(&self, mut on_event: impl FnMut(DownloadEvent)) -> Result<Vec<u8>> {
        let response = match request(
            &self.download_url,
            "application/octet-stream",
            self.timeout,
            &self.headers,
            self.no_proxy,
        )
        .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(code, response)) => {
                return Err(Error::Network(format!(
                    "Download request failed with status: {code} {}",
                    response.status_text()
                )));
            }
            Err(err @ ureq::Error::Transport(_)) => return Err(Error::Http(err.to_string())),
        };

        let content_length: Option<u64> = response
            .header("Content-Length")
            .and_then(|value| value.trim().parse().ok());
        let mut buffer = Vec::with_capacity(
            content_length
                .and_then(|len| usize::try_from(len).ok())
                .unwrap_or(0)
                .min(512 * 1024 * 1024),
        );
        let mut reader = response.into_reader();
        let mut chunk = vec![0u8; CHUNK];
        let mut first_chunk = true;
        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            if first_chunk {
                first_chunk = false;
                on_event(DownloadEvent::Started { content_length });
            }
            on_event(DownloadEvent::Progress { chunk_length: read });
            buffer.extend_from_slice(&chunk[..read]);
        }
        on_event(DownloadEvent::Finished);

        verify_signature(&buffer, &self.signature, &self.pubkey)?;
        Ok(buffer)
    }

    /// Installs a package that `download` returned. On Windows this starts the
    /// installer and exits the process.
    pub fn install(&self, bytes: impl AsRef<[u8]>) -> Result<()> {
        crate::install::install(self, bytes.as_ref())
    }

    /// `download`, then `install`.
    pub fn download_and_install(&self, on_event: impl FnMut(DownloadEvent)) -> Result<()> {
        let bytes = self.download(on_event)?;
        self.install(bytes)
    }

    /// `download_and_install` on a worker thread. `on_event` runs on the
    /// task that awaits, so it may touch UI state. The future needs no
    /// particular executor.
    pub async fn download_and_install_async(
        &self,
        mut on_event: impl FnMut(DownloadEvent),
    ) -> Result<()> {
        enum Message {
            Event(DownloadEvent),
            Done(Result<()>),
        }
        let update = self.clone();
        let (tx, rx) = async_channel::unbounded();
        crate::background::spawn(move || {
            let events = tx.clone();
            let result = update.download_and_install(|event| {
                let _ = events.send_blocking(Message::Event(event));
            });
            let _ = tx.send_blocking(Message::Done(result));
        })?;
        while let Ok(message) = rx.recv().await {
            match message {
                Message::Event(event) => on_event(event),
                Message::Done(result) => return result,
            }
        }
        Err(Error::WorkerStopped)
    }
}

#[cfg(test)]
impl Update {
    /// An update with only the fields the flow reads, for flow tests.
    pub(crate) fn for_test(version: &str, body: Option<&str>) -> Self {
        Self {
            body: body.map(str::to_string),
            current_version: "0.0.0".into(),
            version: version.into(),
            date: None,
            target: "test".into(),
            download_url: "https://example.invalid/update".parse().unwrap(),
            signature: String::new(),
            raw_json: serde_json::Value::Null,
            pubkey: String::new(),
            timeout: None,
            headers: Vec::new(),
            no_proxy: true,
            extract_path: PathBuf::new(),
            installer: None,
            installer_args: Vec::new(),
            current_exe_args: Vec::new(),
            on_before_exit: None,
        }
    }
}
