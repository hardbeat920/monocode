//! Port of host/child-backend.ts, the Node host's version of the Tauri
//! process commands.
//!
//! Most of it is gone: the harness already runs children through
//! `monocode_process::harness::HarnessHost`, wrapped by
//! `monocode_harness::core::child::HostChildBackend`. What remains is the
//! host's own policy on top: providers resolve with [`crate::process`],
//! named accounts are refused, OpenCode HTTP stays on loopback, transcript
//! reads are bounded, and nothing new starts once the host is stopping.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use monocode_core::HarnessId;
use monocode_harness::core::child::{
    ChildBackend, ChildFuture, ExecRequest, HostChildBackend, HttpRequest, HttpResponse,
    ResolvedHarnessBinary, SpawnRequest,
};
use monocode_remote::host::protocol::{RemoteProvider, provider_name};

use crate::process::resolve_provider;

fn ready<T: Send + 'static>(value: Result<T, String>) -> ChildFuture<T> {
    async move { value }.boxed()
}

/// `loopbackUrl`: OpenCode's server must be plain HTTP on this machine.
pub fn loopback_url(value: &str) -> Result<String, String> {
    let refused = || "OpenCode HTTP is limited to localhost".to_string();
    let url = url_parts(value).ok_or_else(refused)?;
    if url.scheme != "http"
        || !["127.0.0.1", "localhost"].contains(&url.host.as_str())
        || url.credentials
    {
        return Err(refused());
    }
    Ok(value.to_string())
}

struct UrlParts {
    scheme: String,
    host: String,
    credentials: bool,
}

/// The scheme, host, and whether credentials are present, without a URL
/// crate. Anything unusual fails, which refuses the request.
fn url_parts(value: &str) -> Option<UrlParts> {
    let (scheme, rest) = value.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (credentials, host_port) = match authority.rsplit_once('@') {
        Some((_, host_port)) => (true, host_port),
        None => (false, authority),
    };
    let host = match host_port.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => host_port,
    };
    Some(UrlParts {
        scheme: scheme.to_ascii_lowercase(),
        host: host.to_ascii_lowercase(),
        credentials,
    })
}

const MAX_TRANSCRIPT_BYTES: u64 = 8 * 1024 * 1024;

/// `harness_read_text_file`: provider transcripts the host's children wrote.
fn read_text_file(path: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.len() > MAX_TRANSCRIPT_BYTES {
        return Err("File is not a readable text file of at most 8 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Err("Binary file is not readable as text".into());
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// The host's [`ChildBackend`].
pub struct HeadlessChildBackend {
    inner: HostChildBackend,
    /// Fixed provider binaries, as the TypeScript constructor took them.
    binaries: HashMap<RemoteProvider, PathBuf>,
    closing: AtomicBool,
}

impl HeadlessChildBackend {
    pub fn new(inner: HostChildBackend, binaries: HashMap<RemoteProvider, PathBuf>) -> Self {
        Self {
            inner,
            binaries,
            closing: AtomicBool::new(false),
        }
    }

    /// `resolve`: the provider CLI this host launches.
    pub fn resolve(&self, provider: RemoteProvider) -> Result<PathBuf, String> {
        match self.binaries.get(&provider) {
            Some(path) => Ok(path.clone()),
            None => resolve_provider(provider),
        }
    }

    /// `close`: stop every child. Spawns fail from now on.
    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let _ = monocode_process::harness::harness_kill_all(self.inner.host());
    }

    fn stopping(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }
}

fn antigravity_args() -> Vec<String> {
    if cfg!(target_os = "linux") {
        vec!["--uid=".into()]
    } else {
        Vec::new()
    }
}

impl ChildBackend for HeadlessChildBackend {
    fn spawn(&self, mut request: SpawnRequest) -> ChildFuture<u32> {
        if self.stopping() {
            return ready(Err("Host is stopping".into()));
        }
        if let Some(account) = request.account.take()
            && account.id != "default"
        {
            return ready(Err(
                "Named provider accounts are not supported by this host yet".into(),
            ));
        }
        // TODO(port): the Node host started each child through
        // provider-guard.mjs and set MONOCODE_HOST=1. The process supervisor
        // here owns the process group and kills it on exit instead, and
        // nothing reads MONOCODE_HOST.
        self.inner.spawn(request)
    }

    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        self.inner.write(session_id, line)
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.inner.kill(session_id)
    }

    fn kill_all(&self) -> ChildFuture<()> {
        self.inner.kill_all()
    }

    /// The host has no configured paths; its resolved binary stands in for
    /// one, so the supervisor accepts exactly the binary resolved here.
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        self.resolve(provider)
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        ready(self.resolve(provider).map(|path| ResolvedHarnessBinary {
            path: path.to_string_lossy().into_owned(),
            args: (provider == HarnessId::Antigravity).then(antigravity_args),
        }))
    }

    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        self.inner.resolve_configured(provider, binary_path)
    }

    fn exec(&self, request: ExecRequest) -> ChildFuture<String> {
        let unsupported = || ready(Err("Unsupported headless catalog command".into()));
        let Some(provider) = request.binary_provider else {
            return unsupported();
        };
        match self.resolve(provider) {
            Ok(path) if path.to_string_lossy() == request.command => {}
            Ok(_) => return unsupported(),
            Err(error) => return ready(Err(error)),
        }
        if !matches!(
            request.args.join(" ").as_str(),
            "--version"
                | "--list-models"
                | "models --verbose"
                | "models --json"
                | "models"
                | "status --json"
                | "agent list"
        ) {
            return unsupported();
        }
        self.inner.exec(request)
    }

    fn free_port(&self) -> ChildFuture<u16> {
        self.inner.free_port()
    }

    fn http(&self, mut request: HttpRequest) -> ChildFuture<HttpResponse> {
        match loopback_url(&request.url) {
            Ok(url) => {
                request.url = url;
                self.inner.http(request)
            }
            Err(error) => ready(Err(error)),
        }
    }

    fn sse_open(
        &self,
        session_id: String,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        match loopback_url(&url) {
            Ok(url) => self.inner.sse_open(session_id, url, headers),
            Err(error) => ready(Err(error)),
        }
    }

    fn sse_close(&self, session_id: String) -> ChildFuture<()> {
        self.inner.sse_close(session_id)
    }

    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        smol::unblock(move || read_text_file(&path)).boxed()
    }

    fn update_cli(
        &self,
        _command: String,
        provider: HarnessId,
        _binary_path: Option<String>,
    ) -> ChildFuture<()> {
        ready(Err(format!(
            "Unsupported headless process operation: update {}",
            provider_name(provider)
        )))
    }

    fn home_dir(&self) -> ChildFuture<String> {
        self.inner.home_dir()
    }

    fn is_headless(&self) -> bool {
        true
    }
}

/// Builds the host's [`monocode_harness::Children`] over a fresh process
/// supervisor. Returns the backend too, for `resolve` and `close`.
pub fn host_children(
    data_dir: PathBuf,
    binaries: HashMap<RemoteProvider, PathBuf>,
    spawner: monocode_harness::core::task::SharedSpawner,
) -> (monocode_harness::Children, Arc<HeadlessChildBackend>) {
    use monocode_harness::core::child::{ChildRouter, HostChildOptions};
    let router = Arc::new(ChildRouter::new());
    let supervisor = monocode_process::harness::HarnessHost::new(router.clone());
    let inner = HostChildBackend::new(
        supervisor,
        HostChildOptions {
            data_dir,
            control: None,
            updater: None,
        },
    );
    let backend = Arc::new(HeadlessChildBackend::new(inner, binaries));
    let children = monocode_harness::Children::new(backend.clone(), router, spawner);
    (children, backend)
}

#[cfg(test)]
mod tests;
