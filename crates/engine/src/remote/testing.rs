//! A scriptable `RemoteTransport` for tests (the TypeScript tests' `invoke`
//! mock): it records every call, answers from queued replies or a handler,
//! and can hold a call until the test releases it. A call with no reply
//! waits forever, like `new Promise(() => {})`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use futures::channel::oneshot;
use monocode_remote::host::protocol::{RemoteMachine, SshSetup};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::transport::{RemoteFuture, RemoteTransport};

/// One answer to a request.
#[derive(Debug, Clone)]
pub enum Reply {
    Value(Value),
    Error(String),
    /// Wait until `FakeTransport::release` answers it.
    Hold,
}

type Handler = Arc<dyn Fn(&str, &str, &Value) -> Option<Reply> + Send + Sync>;

#[derive(Default)]
struct State {
    machines: Vec<RemoteMachine>,
    machines_error: Option<String>,
    paired: Option<RemoteMachine>,
    calls: Vec<(String, String, Value)>,
    queued: HashMap<String, VecDeque<Reply>>,
    handler: Option<Handler>,
    held: Vec<(String, oneshot::Sender<Result<Value, String>>)>,
    /// Calls with no reply, kept so their futures stay pending.
    waiting: Vec<oneshot::Sender<Result<Value, String>>>,
    files: HashMap<String, String>,
    commands: Vec<String>,
}

/// The test transport. Clones share state.
#[derive(Clone, Default)]
pub struct FakeTransport {
    state: Arc<Mutex<State>>,
}

/// A paired machine for tests.
pub fn machine(id: &str, environment_id: &str) -> RemoteMachine {
    RemoteMachine {
        id: id.into(),
        name: "Home".into(),
        endpoint: String::new(),
        endpoints: None,
        environment_id: environment_id.into(),
        ssh: None,
    }
}

impl FakeTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// What `remote_machines` answers.
    pub fn set_machines(&self, machines: Vec<RemoteMachine>) {
        let mut state = self.state.lock();
        state.machines = machines;
        state.machines_error = None;
    }

    /// Make `remote_machines` fail.
    pub fn fail_machines(&self, error: &str) {
        self.state.lock().machines_error = Some(error.into());
    }

    /// What `remote_pair` answers.
    pub fn set_paired(&self, machine: RemoteMachine) {
        self.state.lock().paired = Some(machine);
    }

    /// Queue one answer for the next call of `method`.
    pub fn respond(&self, method: &str, value: Value) {
        self.queue(method, Reply::Value(value));
    }

    /// Queue one failure for the next call of `method`.
    pub fn fail(&self, method: &str, error: &str) {
        self.queue(method, Reply::Error(error.into()));
    }

    /// Hold the next call of `method` until `release`.
    pub fn hold(&self, method: &str) {
        self.queue(method, Reply::Hold);
    }

    pub fn queue(&self, method: &str, reply: Reply) {
        self.state
            .lock()
            .queued
            .entry(method.into())
            .or_default()
            .push_back(reply);
    }

    /// Answer calls that have no queued reply. `None` leaves a call waiting.
    pub fn set_handler(
        &self,
        handler: impl Fn(&str, &str, &Value) -> Option<Reply> + Send + Sync + 'static,
    ) {
        self.state.lock().handler = Some(Arc::new(handler));
    }

    /// Answer the oldest held call of `method`. `false` when none is held.
    pub fn release(&self, method: &str, result: Result<Value, String>) -> bool {
        let mut state = self.state.lock();
        let Some(index) = state.held.iter().position(|(held, _)| held == method) else {
            return false;
        };
        let (_, sender) = state.held.remove(index);
        sender.send(result).is_ok()
    }

    /// How many calls of `method` are held.
    pub fn held(&self, method: &str) -> usize {
        self.state
            .lock()
            .held
            .iter()
            .filter(|(held, _)| held == method)
            .count()
    }

    /// A local file for `read_file_base64`.
    pub fn set_file(&self, path: &str, base64: &str) {
        self.state.lock().files.insert(path.into(), base64.into());
    }

    /// Every request as `(machine, method, params)`.
    pub fn calls(&self) -> Vec<(String, String, Value)> {
        self.state.lock().calls.clone()
    }

    /// The params of every call of `method`.
    pub fn calls_for(&self, method: &str) -> Vec<Value> {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|(_, called, _)| called == method)
            .map(|(_, _, params)| params.clone())
            .collect()
    }

    /// Transport commands other than `remote_request`, in call order.
    pub fn commands(&self) -> Vec<String> {
        self.state.lock().commands.clone()
    }

    pub fn clear_calls(&self) {
        let mut state = self.state.lock();
        state.calls.clear();
        state.commands.clear();
    }

    fn command(&self, name: &str) {
        self.state.lock().commands.push(name.into());
    }
}

impl RemoteTransport for FakeTransport {
    fn machines(&self) -> RemoteFuture<Vec<RemoteMachine>> {
        self.command("remote_machines");
        let state = self.state.lock();
        let result = match &state.machines_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.machines.clone()),
        };
        Box::pin(async move { result })
    }

    fn pair(&self, _link: String, _name: String) -> RemoteFuture<RemoteMachine> {
        self.command("remote_pair");
        let mut state = self.state.lock();
        let result = state
            .paired
            .clone()
            .ok_or_else(|| "Paste the whole pairing link".to_string());
        // A paired machine is saved, so the next list read includes it.
        if let Ok(machine) = &result {
            state.machines.retain(|entry| entry.id != machine.id);
            state.machines.push(machine.clone());
        }
        Box::pin(async move { result })
    }

    fn retry(&self, _machine_id: String) -> RemoteFuture<()> {
        self.command("remote_retry");
        Box::pin(async { Ok(()) })
    }

    fn disconnect(&self, machine_id: String) -> RemoteFuture<()> {
        self.command("remote_disconnect");
        self.state
            .lock()
            .machines
            .retain(|machine| machine.id != machine_id);
        Box::pin(async { Ok(()) })
    }

    fn request(&self, machine_id: String, method: String, params: Value) -> RemoteFuture<Value> {
        let mut state = self.state.lock();
        state
            .calls
            .push((machine_id.clone(), method.clone(), params.clone()));
        let reply = match state.queued.get_mut(&method).and_then(VecDeque::pop_front) {
            Some(reply) => Some(reply),
            None => state
                .handler
                .clone()
                .and_then(|handler| handler(&machine_id, &method, &params)),
        };
        match reply {
            Some(Reply::Value(value)) => Box::pin(async move { Ok(value) }),
            Some(Reply::Error(error)) => Box::pin(async move { Err(error) }),
            Some(Reply::Hold) => {
                let (sender, receiver) = oneshot::channel();
                state.held.push((method, sender));
                Box::pin(async move { receiver.await.unwrap_or_else(|_| Err("released".into())) })
            }
            None => {
                let (sender, receiver) = oneshot::channel();
                state.waiting.push(sender);
                Box::pin(async move { receiver.await.unwrap_or_else(|_| Err("released".into())) })
            }
        }
    }

    fn ssh_begin(
        &self,
        _target: String,
        _name: String,
        _port: Option<u16>,
        _upgrade: bool,
    ) -> RemoteFuture<String> {
        self.command("remote_ssh_begin");
        Box::pin(async { Ok("job".to_string()) })
    }

    fn ssh_reconnect(&self, _machine_id: String, _upgrade: bool) -> RemoteFuture<String> {
        self.command("remote_ssh_reconnect");
        Box::pin(async { Ok("job".to_string()) })
    }

    fn ssh_poll(&self, job_id: String) -> RemoteFuture<SshSetup> {
        self.command("remote_ssh_poll");
        Box::pin(async move {
            serde_json::from_value(json!({ "id": job_id, "message": "", "done": true }))
                .map_err(|error| error.to_string())
        })
    }

    fn ssh_answer(&self, _job_id: String, _prompt_id: String, _answer: String) -> RemoteFuture<()> {
        self.command("remote_ssh_answer");
        Box::pin(async { Ok(()) })
    }

    fn ssh_cancel(&self, _job_id: String) -> RemoteFuture<()> {
        self.command("remote_ssh_cancel");
        Box::pin(async { Ok(()) })
    }

    fn read_file_base64(&self, path: String) -> RemoteFuture<String> {
        self.command("read_file_base64");
        let result = self
            .state
            .lock()
            .files
            .get(&path)
            .cloned()
            .ok_or_else(|| format!("{path}: not found"));
        Box::pin(async move { result })
    }
}
