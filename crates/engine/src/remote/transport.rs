//! The Tauri commands `connections.ts`, `remoteAttachments.ts`, and the
//! connections settings called through `invoke`: `remote_machines`,
//! `remote_pair`, `remote_retry`, `remote_disconnect`, `remote_request`, the
//! SSH setup commands, and `read_file_base64`.
//!
//! `NativeTransport` runs them through `monocode_remote::remote` on a thread
//! per call, because a request blocks on the network and `changes.wait`
//! holds its request for up to 25 seconds. Tests use
//! `testing::FakeTransport`, which answers when the test says so.

use std::path::PathBuf;

use futures::channel::oneshot;
use futures::future::BoxFuture;
use monocode_remote::host::protocol::{RemoteMachine, SshSetup};
use monocode_remote::remote as client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// A transport call that finishes off the UI thread. Errors are the strings
/// the Tauri commands rejected with.
pub type RemoteFuture<T> = BoxFuture<'static, Result<T, String>>;

/// What the desktop asks of its saved connections. Every call returns a
/// future; none blocks the caller.
pub trait RemoteTransport: Send + Sync + 'static {
    /// `remote_machines`: the saved machines, without credentials.
    fn machines(&self) -> RemoteFuture<Vec<RemoteMachine>>;
    /// `remote_pair`: pair the machine in a `monocode://pair` link.
    fn pair(&self, link: String, name: String) -> RemoteFuture<RemoteMachine>;
    /// `remote_retry`: forget the machine's route and recent failure.
    fn retry(&self, machine_id: String) -> RemoteFuture<()>;
    /// `remote_disconnect`: delete the saved connection.
    fn disconnect(&self, machine_id: String) -> RemoteFuture<()>;
    /// `remote_request`: one host RPC.
    fn request(&self, machine_id: String, method: String, params: Value) -> RemoteFuture<Value>;
    /// `remote_ssh_begin`: start an SSH setup job and return its id.
    fn ssh_begin(
        &self,
        target: String,
        name: String,
        port: Option<u16>,
        upgrade: bool,
    ) -> RemoteFuture<String>;
    /// `remote_ssh_reconnect`: run `connect` again for a machine set up over
    /// SSH, optionally installing this desktop's host version.
    fn ssh_reconnect(&self, machine_id: String, upgrade: bool) -> RemoteFuture<String>;
    /// `remote_ssh_poll`.
    fn ssh_poll(&self, job_id: String) -> RemoteFuture<SshSetup>;
    /// `remote_ssh_answer`: a password, passphrase, or host key answer.
    fn ssh_answer(&self, job_id: String, prompt_id: String, answer: String) -> RemoteFuture<()>;
    /// `remote_ssh_cancel`.
    fn ssh_cancel(&self, job_id: String) -> RemoteFuture<()>;
    /// `read_file_base64`: a local attachment's bytes for upload.
    fn read_file_base64(&self, path: String) -> RemoteFuture<String>;
}

/// The saved connections in `<data_dir>/remote-machines.json`, reached over
/// pinned TLS or SSH forwards.
#[derive(Clone)]
pub struct NativeTransport {
    remote: client::Remote,
}

impl NativeTransport {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            remote: client::Remote::new(data_dir),
        }
    }

    /// Wrap a `Remote` the app already holds, so both share routes,
    /// tunnels, and SSH jobs.
    pub fn with_remote(remote: client::Remote) -> Self {
        Self { remote }
    }

    pub fn remote(&self) -> &client::Remote {
        &self.remote
    }
}

/// The remote crate serializes its public machine and job types in the
/// shapes the renderer read; read them back as the protocol types.
fn reshape<T: Serialize, U: DeserializeOwned>(value: T) -> Result<U, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| error.to_string())
}

/// Run a blocking call on its own thread. A request can hold its thread for
/// the 25 seconds of a long poll, which would starve a shared pool.
fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> RemoteFuture<T> {
    let (sender, receiver) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("remote-request".into())
        .spawn(move || {
            let _ = sender.send(call());
        });
    Box::pin(async move {
        if let Err(error) = spawned {
            return Err(error.to_string());
        }
        receiver.await.unwrap_or_else(|_| {
            Err("The host request did not complete. Retry to confirm its result.".into())
        })
    })
}

impl RemoteTransport for NativeTransport {
    fn machines(&self) -> RemoteFuture<Vec<RemoteMachine>> {
        let remote = self.remote.clone();
        blocking(move || reshape(client::remote_machines(&remote)?))
    }

    fn pair(&self, link: String, name: String) -> RemoteFuture<RemoteMachine> {
        let remote = self.remote.clone();
        blocking(move || reshape(client::remote_pair(&remote, link, name)?))
    }

    fn retry(&self, machine_id: String) -> RemoteFuture<()> {
        let remote = self.remote.clone();
        blocking(move || {
            client::remote_retry(&remote, machine_id);
            Ok(())
        })
    }

    fn disconnect(&self, machine_id: String) -> RemoteFuture<()> {
        let remote = self.remote.clone();
        blocking(move || client::remote_disconnect(&remote, machine_id))
    }

    fn request(&self, machine_id: String, method: String, params: Value) -> RemoteFuture<Value> {
        let remote = self.remote.clone();
        blocking(move || client::remote_request(&remote, machine_id, method, params))
    }

    fn ssh_begin(
        &self,
        target: String,
        name: String,
        port: Option<u16>,
        upgrade: bool,
    ) -> RemoteFuture<String> {
        let remote = self.remote.clone();
        blocking(move || client::remote_ssh_begin(&remote, target, name, port, Some(upgrade)))
    }

    fn ssh_reconnect(&self, machine_id: String, upgrade: bool) -> RemoteFuture<String> {
        let remote = self.remote.clone();
        blocking(move || client::remote_ssh_reconnect(&remote, machine_id, Some(upgrade)))
    }

    fn ssh_poll(&self, job_id: String) -> RemoteFuture<SshSetup> {
        let remote = self.remote.clone();
        blocking(move || reshape(client::remote_ssh_poll(&remote, job_id)?))
    }

    fn ssh_answer(&self, job_id: String, prompt_id: String, answer: String) -> RemoteFuture<()> {
        let remote = self.remote.clone();
        blocking(move || client::remote_ssh_answer(&remote, job_id, prompt_id, answer))
    }

    fn ssh_cancel(&self, job_id: String) -> RemoteFuture<()> {
        let remote = self.remote.clone();
        blocking(move || client::remote_ssh_cancel(&remote, job_id))
    }

    fn read_file_base64(&self, path: String) -> RemoteFuture<String> {
        blocking(move || monocode_git::fs::read_file_base64(path))
    }
}
