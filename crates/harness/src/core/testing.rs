//! Test doubles shared by the framework tests: a fake [`ChildBackend`] that
//! records every call.

use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;

use monocode_core::harness::HarnessId;

use super::child::*;
use super::registry::{AcceptedHook, EventSink, HarnessAdapter};
use super::task::{BoxFuture, SmolSpawner};
use monocode_core::harness_event::{ApprovalDecision, SendTurnInput, SteerTurnInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Spawn(SpawnRequest),
    Write(String, String),
    Kill(String),
    KillAll,
    ResolveDefault(HarnessId),
    ResolveConfigured(HarnessId, String),
    Exec(ExecRequest),
    SseClose(String),
}

#[derive(Default)]
pub struct Fake {
    pub calls: Mutex<Vec<Call>>,
    pub runtime_paths: HashMap<HarnessId, String>,
    /// Resolved paths per provider. Others resolve to `/resolved`.
    pub resolved: HashMap<HarnessId, String>,
    /// Providers whose resolvers fail.
    pub missing: Vec<HarnessId>,
    /// Next pid for `spawn`. A receiver lets a test hold the spawn open.
    pub pids: Mutex<Option<async_channel::Receiver<u32>>>,
}

impl Fake {
    fn record(&self, call: Call) {
        self.calls.lock().push(call);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().clone()
    }
}

fn done<T: Send + 'static>(value: T) -> ChildFuture<T> {
    async move { Ok(value) }.boxed()
}

impl ChildBackend for Fake {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        self.record(Call::Spawn(request));
        match self.pids.lock().clone() {
            Some(pids) => async move { pids.recv().await.map_err(|e| e.to_string()) }.boxed(),
            None => done(7),
        }
    }
    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        self.record(Call::Write(session_id, line));
        done(())
    }
    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.record(Call::Kill(session_id));
        done(())
    }
    fn kill_all(&self) -> ChildFuture<()> {
        self.record(Call::KillAll);
        done(())
    }
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        self.runtime_paths.get(&provider).cloned()
    }
    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        self.record(Call::ResolveDefault(provider));
        if self.missing.contains(&provider) {
            return async move { Err(format!("{provider} CLI not found")) }.boxed();
        }
        done(ResolvedHarnessBinary {
            path: self
                .resolved
                .get(&provider)
                .cloned()
                .unwrap_or_else(|| "/resolved".into()),
            args: None,
        })
    }
    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        self.record(Call::ResolveConfigured(provider, binary_path));
        done(ResolvedHarnessBinary {
            path: "/resolved".into(),
            args: None,
        })
    }
    fn exec(&self, request: ExecRequest) -> ChildFuture<String> {
        self.record(Call::Exec(request));
        done("tool 1.2.3\n".into())
    }
    fn free_port(&self) -> ChildFuture<u16> {
        done(4100)
    }
    fn http(&self, _request: HttpRequest) -> ChildFuture<HttpResponse> {
        done(HttpResponse {
            status: 200,
            body: "{}".into(),
        })
    }
    fn sse_open(
        &self,
        _session_id: String,
        _url: String,
        _headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        done(())
    }
    fn sse_close(&self, session_id: String) -> ChildFuture<()> {
        self.record(Call::SseClose(session_id));
        done(())
    }
    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        done(format!("contents of {path}"))
    }
    fn update_cli(
        &self,
        _command: String,
        _provider: HarnessId,
        _binary_path: Option<String>,
    ) -> ChildFuture<()> {
        done(())
    }
    fn home_dir(&self) -> ChildFuture<String> {
        done("/home/alice".into())
    }
}

pub fn children(fake: Fake) -> (Children, Arc<Fake>) {
    let fake = Arc::new(fake);
    let children = Children::new(
        fake.clone(),
        Arc::new(ChildRouter::new()),
        Arc::new(SmolSpawner),
    );
    (children, fake)
}

/// An adapter that accepts everything and does nothing.
pub struct StubAdapter(pub HarnessId);

impl HarnessAdapter for StubAdapter {
    fn id(&self) -> HarnessId {
        self.0
    }
    fn send_turn(
        &self,
        _input: SendTurnInput,
        _on_event: EventSink,
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, anyhow::Result<()>> {
        async { Ok(()) }.boxed()
    }
    fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, anyhow::Result<()>> {
        async { Ok(()) }.boxed()
    }
    fn cancel_turn(&self, _session_id: String) -> BoxFuture<'_, anyhow::Result<()>> {
        async { Ok(()) }.boxed()
    }
    fn respond_approval(&self, _session_id: &str, _request_id: i64, _decision: ApprovalDecision) {}
    fn stop_session(&self, _session_id: String) -> BoxFuture<'_, anyhow::Result<()>> {
        async { Ok(()) }.boxed()
    }
    fn forget_session(&self, _session_id: String) -> BoxFuture<'_, anyhow::Result<()>> {
        async { Ok(()) }.boxed()
    }
    fn bind_session(&self, _: &str, _: &str, _: &str, _: Option<&str>) {}
}
