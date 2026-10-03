//! A [`RemoteHost`] for the tests: canned machines and answers, and a log of
//! every call, standing in for the mocked `invoke` in the React tests.

use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{App, AppContext as _, Task};
use monocode_remote::host::protocol::{RemoteMachine, RemoteMachineSsh, SshSetup};
use serde_json::{Value, json};

use crate::host::{HostTask, RemoteHost, SshBegin};

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Machines,
    Request {
        machine_id: String,
        method: String,
        params: Value,
    },
    Pair {
        link: String,
        name: String,
    },
    Retry(String),
    Disconnect(String),
    SshBegin(SshBegin),
    SshReconnect {
        machine_id: String,
        upgrade: bool,
    },
    SshPoll(String),
    SshAnswer {
        job_id: String,
        prompt_id: String,
        answer: String,
    },
    SshCancel(String),
}

type Responder = Box<dyn Fn(&str, &str, &Value) -> Result<Value, String>>;

pub struct FakeState {
    pub machines: Vec<RemoteMachine>,
    pub setup: SshSetup,
    pub paired: RemoteMachine,
    pub calls: Vec<Call>,
    pub version: Option<String>,
    /// Answers `remote_request`.
    pub respond: Responder,
    /// Requests for these methods wait until the test sends a value.
    pub held: Vec<(String, oneshot::Sender<Result<Value, String>>)>,
    pub hold: Vec<String>,
}

#[derive(Clone)]
pub struct FakeRemote(pub Rc<RefCell<FakeState>>);

pub fn ssh_machine() -> RemoteMachine {
    RemoteMachine {
        id: "machine".into(),
        name: "Home Mac".into(),
        endpoint: "ssh://me@home".into(),
        endpoints: None,
        environment_id: "env".into(),
        ssh: Some(RemoteMachineSsh {
            target: "me@home".into(),
            port: None,
            remote_port: 3774,
        }),
    }
}

pub fn direct_machine() -> RemoteMachine {
    RemoteMachine {
        id: "direct".into(),
        name: "Studio".into(),
        endpoint: "10.0.0.2:3774".into(),
        endpoints: Some(vec![
            "https://10.0.0.2:3774".into(),
            "https://100.64.0.9:3774".into(),
        ]),
        environment_id: "env-2".into(),
        ssh: None,
    }
}

impl FakeRemote {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(FakeState {
            machines: Vec::new(),
            setup: SshSetup {
                id: "setup".into(),
                message: "Installing host…".into(),
                prompt: None,
                done: false,
                error: None,
                machine: None,
            },
            paired: direct_machine(),
            calls: Vec::new(),
            version: Some("1.2.3".into()),
            respond: Box::new(|_, _, _| {
                Ok(json!({ "environmentId": "env", "providers": ["codex"] }))
            }),
            held: Vec::new(),
            hold: Vec::new(),
        })))
    }

    pub fn calls(&self) -> Vec<Call> {
        self.0.borrow().calls.clone()
    }

    pub fn called(&self, call: &Call) -> bool {
        self.0.borrow().calls.contains(call)
    }

    /// Whether any `remote_request` asked for `method`.
    pub fn requested(&self, method: &str) -> bool {
        self.0
            .borrow()
            .calls
            .iter()
            .any(|call| matches!(call, Call::Request { method: m, .. } if m == method))
    }

    /// The order of calls, with requests named by their method.
    pub fn sequence(&self) -> Vec<String> {
        self.0
            .borrow()
            .calls
            .iter()
            .map(|call| match call {
                Call::Request { method, .. } => method.clone(),
                Call::Disconnect(_) => "remote_disconnect".into(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    pub fn set_respond(
        &self,
        respond: impl Fn(&str, &str, &Value) -> Result<Value, String> + 'static,
    ) {
        self.0.borrow_mut().respond = Box::new(respond);
    }

    /// Requests for `method` wait for [`FakeRemote::finish`].
    pub fn hold(&self, method: &str) {
        self.0.borrow_mut().hold.push(method.into());
    }

    /// Answers the oldest held request for `method`.
    pub fn finish(&self, method: &str, value: Result<Value, String>) {
        let mut state = self.0.borrow_mut();
        if let Some(index) = state.held.iter().position(|(held, _)| held == method) {
            let (_, sender) = state.held.remove(index);
            let _ = sender.send(value);
        }
    }

    fn log(&self, call: Call) {
        self.0.borrow_mut().calls.push(call);
    }
}

impl RemoteHost for FakeRemote {
    fn machines(&self, _: &mut App) -> HostTask<Vec<RemoteMachine>> {
        self.log(Call::Machines);
        Task::ready(Ok(self.0.borrow().machines.clone()))
    }

    fn request(
        &self,
        machine_id: &str,
        method: &str,
        params: Value,
        cx: &mut App,
    ) -> HostTask<Value> {
        self.log(Call::Request {
            machine_id: machine_id.into(),
            method: method.into(),
            params: params.clone(),
        });
        let held = self.0.borrow().hold.iter().any(|held| held == method);
        if held {
            let (sender, receiver) = oneshot::channel();
            self.0.borrow_mut().held.push((method.into(), sender));
            return cx.background_spawn(async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("Request dropped".into()))
            });
        }
        let answer = (self.0.borrow().respond)(machine_id, method, &params);
        Task::ready(answer)
    }

    fn app_version(&self, _: &mut App) -> HostTask<String> {
        match self.0.borrow().version.clone() {
            Some(version) => Task::ready(Ok(version)),
            None => Task::ready(Err("No version".into())),
        }
    }

    fn pair(&self, link: &str, name: &str, _: &mut App) -> HostTask<RemoteMachine> {
        self.log(Call::Pair {
            link: link.into(),
            name: name.into(),
        });
        let machine = self.0.borrow().paired.clone();
        self.0.borrow_mut().machines.push(machine.clone());
        Task::ready(Ok(machine))
    }

    fn retry(&self, machine_id: &str, _: &mut App) -> HostTask<()> {
        self.log(Call::Retry(machine_id.into()));
        Task::ready(Ok(()))
    }

    fn disconnect(&self, machine_id: &str, _: &mut App) -> HostTask<()> {
        self.log(Call::Disconnect(machine_id.into()));
        self.0.borrow_mut().machines.clear();
        Task::ready(Ok(()))
    }

    fn ssh_begin(&self, request: SshBegin, _: &mut App) -> HostTask<String> {
        self.log(Call::SshBegin(request));
        Task::ready(Ok("setup".into()))
    }

    fn ssh_reconnect(&self, machine_id: &str, upgrade: bool, _: &mut App) -> HostTask<String> {
        self.log(Call::SshReconnect {
            machine_id: machine_id.into(),
            upgrade,
        });
        Task::ready(Ok("setup".into()))
    }

    fn ssh_poll(&self, job_id: &str, _: &mut App) -> HostTask<SshSetup> {
        self.log(Call::SshPoll(job_id.into()));
        Task::ready(Ok(self.0.borrow().setup.clone()))
    }

    fn ssh_answer(
        &self,
        job_id: &str,
        prompt_id: &str,
        answer: String,
        _: &mut App,
    ) -> HostTask<()> {
        self.log(Call::SshAnswer {
            job_id: job_id.into(),
            prompt_id: prompt_id.into(),
            answer,
        });
        Task::ready(Ok(()))
    }

    fn ssh_cancel(&self, job_id: &str, _: &mut App) -> HostTask<()> {
        self.log(Call::SshCancel(job_id.into()));
        Task::ready(Ok(()))
    }
}
