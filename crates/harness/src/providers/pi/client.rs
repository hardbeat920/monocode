//! Port of src/integrations/harness/providers/pi/piClient.ts: the JSONL
//! request and response multiplexer for `pi --mode rpc` and the identical
//! `omp --mode rpc`.
//!
//! The TypeScript took an `onFrame` callback. Here [`PiRpc::push_line`]
//! returns the frame that is not a response, so the caller handles it and the
//! client holds no reference back to its owner.
//!
//! Writes go through one queue per client, in the order they were made, so a
//! multi-threaded executor cannot reorder two commands.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::channel::oneshot;
use parking_lot::Mutex;
use serde_json::Value;

use crate::core::child::Children;
use crate::core::task::{BoxFuture, ms, timeout};

use super::deps::Rec;
use super::protocol::{parse_json_line, parse_rpc_response, string_field};

/// The default request timeout, `15_000` in TypeScript.
pub const DEFAULT_REQUEST_TIMEOUT_MS: i64 = 15_000;

type Reply = Result<Rec, String>;

struct Pending {
    token: u64,
    reply: oneshot::Sender<Reply>,
}

#[derive(Default)]
struct RpcState {
    next_id: u64,
    next_token: u64,
    pending: HashMap<String, Pending>,
    closed: bool,
}

struct Write {
    line: String,
    /// The request this line carries, rejected if the write fails.
    request: Option<(String, u64)>,
    done: Option<oneshot::Sender<Result<(), String>>>,
}

struct RpcInner {
    label: String,
    state: Mutex<RpcState>,
    writes: async_channel::Sender<Write>,
}

impl RpcInner {
    fn reject(&self, id: &str, token: u64, error: String) {
        let pending = {
            let mut state = self.state.lock();
            match state.pending.get(id) {
                Some(pending) if pending.token == token => state.pending.remove(id),
                _ => None,
            }
        };
        if let Some(pending) = pending {
            let _ = pending.reply.send(Err(error));
        }
    }
}

/// `PiRpc`. Clones share one client.
#[derive(Clone)]
pub struct PiRpc {
    inner: Arc<RpcInner>,
}

impl PiRpc {
    /// A client for the child `session_id`. `label` names the CLI in errors.
    pub fn new(children: &Children, session_id: &str, label: &str) -> Self {
        let (writes, queue) = async_channel::unbounded::<Write>();
        let inner = Arc::new(RpcInner {
            label: label.to_string(),
            state: Mutex::new(RpcState {
                next_id: 1,
                ..RpcState::default()
            }),
            writes,
        });
        let weak: Weak<RpcInner> = Arc::downgrade(&inner);
        let writer = children.clone();
        let session_id = session_id.to_string();
        children.spawner().spawn(
            async move {
                while let Ok(write) = queue.recv().await {
                    let result = writer
                        .write_child(&session_id, &write.line)
                        .await
                        .map_err(|error| error.to_string());
                    if let (Err(error), Some((id, token))) = (&result, &write.request)
                        && let Some(inner) = weak.upgrade()
                    {
                        inner.reject(id, *token, error.clone());
                    }
                    if let Some(done) = write.done {
                        let _ = done.send(result);
                    }
                }
            }
            .boxed(),
        );
        Self { inner }
    }

    /// `pushLine`. Resolves the request a response answers and returns
    /// `None`. Returns any other JSON object frame for the caller to handle.
    pub fn push_line(&self, line: &str) -> Option<Rec> {
        let rec = parse_json_line(line)?;
        if let Some(response) = parse_rpc_response(&rec)
            && let Some(id) = response.id.as_deref()
        {
            let pending = self.inner.state.lock().pending.remove(id);
            if let Some(pending) = pending {
                let reply = if response.success {
                    Ok(rec)
                } else {
                    Err(response
                        .error
                        .filter(|error| !error.is_empty())
                        .unwrap_or_else(|| {
                            format!("{} {} failed", self.inner.label, response.command)
                        }))
                };
                let _ = pending.reply.send(reply);
                return None;
            }
        }
        Some(rec)
    }

    /// `request`. Registers the request and queues its line at once, as the
    /// TypeScript did before its first await, and returns the reply.
    pub fn request(&self, command: Rec, timeout_ms: i64) -> BoxFuture<'static, Result<Rec>> {
        let label = self.inner.label.clone();
        let registered = {
            let mut state = self.inner.state.lock();
            if state.closed {
                Err(anyhow!("{label} process is not running"))
            } else {
                let id = match string_field(Some(&command), "id") {
                    Some(id) => id.to_string(),
                    None => {
                        let id = format!("mc_{}", state.next_id);
                        state.next_id += 1;
                        id
                    }
                };
                if state.pending.contains_key(&id) {
                    Err(anyhow!("Duplicate RPC request id: {id}"))
                } else {
                    state.next_token += 1;
                    let token = state.next_token;
                    let (reply, receiver) = oneshot::channel();
                    state.pending.insert(id.clone(), Pending { token, reply });
                    Ok((id, token, receiver))
                }
            }
        };
        let (id, token, receiver) = match registered {
            Ok(registered) => registered,
            Err(error) => return async move { Err(error) }.boxed(),
        };
        let kind = string_field(Some(&command), "type")
            .unwrap_or("command")
            .to_string();
        let mut payload = command;
        payload.insert("id".into(), Value::String(id.clone()));
        let line = Value::Object(payload).to_string();
        let _ = self.inner.writes.try_send(Write {
            line,
            request: Some((id.clone(), token)),
            done: None,
        });
        let inner = self.inner.clone();
        async move {
            match timeout(ms(timeout_ms), receiver).await {
                Some(Ok(reply)) => reply.map_err(|error| anyhow!(error)),
                Some(Err(_)) => Err(anyhow!("{} process exited", inner.label)),
                None => {
                    inner.reject(&id, token, String::new());
                    Err(anyhow!("{} {kind} timed out", inner.label))
                }
            }
        }
        .boxed()
    }

    /// Write one raw line (such as an `extension_ui_response`) in queue
    /// order. The TypeScript called `writeChild` for these.
    pub fn write_line(&self, line: String) -> BoxFuture<'static, Result<()>> {
        let (done, receiver) = oneshot::channel();
        let queued = self.inner.writes.try_send(Write {
            line,
            request: None,
            done: Some(done),
        });
        async move {
            if queued.is_err() {
                return Err(anyhow!("Harness process is not running"));
            }
            match receiver.await {
                Ok(result) => result.map_err(|error| anyhow!(error)),
                Err(_) => Err(anyhow!("Harness process is not running")),
            }
        }
        .boxed()
    }

    /// `close`: reject every pending request. Idempotent.
    pub fn close(&self, error: Option<String>) {
        let pending = {
            let mut state = self.inner.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            std::mem::take(&mut state.pending)
        };
        let error = error.unwrap_or_else(|| format!("{} process exited", self.inner.label));
        for (_, pending) in pending {
            let _ = pending.reply.send(Err(error.clone()));
        }
    }

    /// `cancelRequest`.
    pub fn cancel_request(&self, id: &str) {
        let pending = self.inner.state.lock().pending.remove(id);
        if let Some(pending) = pending {
            let _ = pending
                .reply
                .send(Err(format!("{} request cancelled", self.inner.label)));
        }
    }

    /// True once `close` ran.
    pub fn is_closed(&self) -> bool {
        self.inner.state.lock().closed
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, WriteReply};
    use super::*;
    use serde_json::json;

    fn command(value: Value) -> Rec {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn rejects_when_the_transport_write_fails() {
        let fake = Fake::new();
        fake.on_write(|_, _, _| WriteReply::Fail("write failed".into()));
        smol::block_on(async {
            let rpc = PiRpc::new(&fake.children, "probe", "Pi");
            let error = rpc
                .request(
                    command(json!({ "type": "get_commands" })),
                    DEFAULT_REQUEST_TIMEOUT_MS,
                )
                .await
                .unwrap_err();
            assert!(error.to_string().contains("write failed"), "{error}");
            rpc.close(None);
        });
    }

    #[test]
    fn times_out_even_when_the_transport_write_stalls() {
        let fake = Fake::new();
        fake.on_write(|_, _, _| WriteReply::Stall);
        smol::block_on(async {
            let rpc = PiRpc::new(&fake.children, "probe", "Pi");
            let error = rpc
                .request(command(json!({ "type": "get_commands" })), 100)
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "Pi get_commands timed out");
            rpc.close(None);
        });
    }

    #[test]
    fn resolves_responses_and_passes_other_frames_through() {
        let fake = Fake::new();
        smol::block_on(async {
            let rpc = PiRpc::new(&fake.children, "probe", "Pi");
            let reply = rpc.request(command(json!({ "type": "get_state" })), 1_000);
            fake.wait_for(|| !fake.requests().is_empty()).await;
            let (_, sent) = fake.requests().remove(0);
            assert_eq!(sent.get("id"), Some(&json!("mc_1")));
            let frame = json!({ "type": "agent_end" }).to_string();
            assert!(rpc.push_line(&frame).is_some());
            let response = json!({
                "type": "response", "id": "mc_1", "command": "get_state", "success": true, "data": { "x": 1 }
            });
            assert!(rpc.push_line(&response.to_string()).is_none());
            let rec = reply.await.unwrap();
            assert_eq!(rec.get("data"), Some(&json!({ "x": 1 })));

            let failing = rpc.request(command(json!({ "type": "set_model" })), 1_000);
            let response = json!({ "type": "response", "id": "mc_2", "command": "set_model", "success": false });
            assert!(rpc.push_line(&response.to_string()).is_none());
            assert_eq!(
                failing.await.unwrap_err().to_string(),
                "Pi set_model failed"
            );

            let duplicate = rpc.request(command(json!({ "type": "x", "id": "same" })), 1_000);
            let second = rpc.request(command(json!({ "type": "x", "id": "same" })), 1_000);
            assert_eq!(
                second.await.unwrap_err().to_string(),
                "Duplicate RPC request id: same"
            );
            rpc.cancel_request("same");
            assert_eq!(
                duplicate.await.unwrap_err().to_string(),
                "Pi request cancelled"
            );

            let open = rpc.request(command(json!({ "type": "y" })), 1_000);
            rpc.close(Some("boom".into()));
            assert_eq!(open.await.unwrap_err().to_string(), "boom");
            assert_eq!(
                rpc.request(command(json!({ "type": "y" })), 1_000)
                    .await
                    .unwrap_err()
                    .to_string(),
                "Pi process is not running"
            );
        });
    }
}
