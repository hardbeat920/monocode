//! Explicit provider rollback. This never starts a prompt or edits local history.
//! The methods and message shapes mirror the checked-in Codex and Pi adapters.
use super::*;

#[derive(Debug)]
pub struct RewindError {
    pub message: String,
    /// A destructive request may have reached the provider without its reply.
    pub provider_may_have_changed: bool,
}
impl From<String> for RewindError {
    fn from(message: String) -> Self {
        Self {
            message,
            provider_may_have_changed: false,
        }
    }
}
impl From<&str> for RewindError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl std::fmt::Display for RewindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RewindError {}
fn transport_error(message: String, mutating: bool) -> RewindError {
    RewindError {
        message,
        provider_may_have_changed: mutating,
    }
}

pub fn supports_rewind(kind: Kind) -> bool {
    matches!(kind, Kind::Codex | Kind::Pi | Kind::Omp | Kind::OpenCode)
}

/// Runs on a worker, after the caller has reserved its conversation. Returns the
/// provider binding to persist with the confirmed local truncation. Pi forks to
/// a new session; Codex keeps the current thread. No prompt is sent here.
pub fn rewind_last_turn(
    request: Request,
    before_turn: Option<&str>,
) -> Result<String, RewindError> {
    if !supports_rewind(request.kind) || request.settings.text_only {
        return Err("This provider does not support editing the last turn".into());
    }
    if request.kind == Kind::OpenCode {
        return super::opencode::rewind(&request, before_turn);
    }
    let session = request
        .provider_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("The conversation has no saved provider session to rewind")?;
    // Remove the previous warm process before reverting its underlying session.
    // It must not later resume with a pre-revert in-memory context.
    parked::discard_session(&request);
    let mut command = Command::new(&request.binary);
    crate::scm::shell::apply_gui_env(&mut command);
    command.envs(&request.settings.environment);
    request.settings.account.apply(request.kind, &mut command)?;
    if request.kind == Kind::Codex {
        command.arg("app-server");
    } else {
        pi::configure(&mut command, &request, false);
    }
    command
        .current_dir(&request.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::isolate(&mut command);
    let mut wire = parked::Wire::spawn(&mut command, &request)?;
    let mut rpc = Rpc {
        wire: &mut wire,
        serial: 100,
        pi: request.kind.is_pi(),
    };
    if rpc.pi {
        let state = rpc.call("get_state", json!({}))?;
        if pi_binding(&state).as_deref() != Some(session) {
            return Err("Pi resumed a different session; history was preserved".into());
        }
        if state["isStreaming"] == true || state["isCompacting"] == true {
            return Err("Stop the current turn before editing the last message".into());
        }
        let messages = rpc.call("get_fork_messages", json!({}))?;
        let entry = last_fork_entry(&messages)?;
        let fork = rpc.call("fork", json!({"entryId":entry}))?;
        if fork["cancelled"] == true {
            return Err("Edit cancelled".into());
        }
        let state = rpc.call("get_state", json!({}))?;
        return pi_binding(&state).ok_or_else(|| "Pi did not expose the forked session".into());
    }
    rpc.call("initialize", json!({"clientInfo":{"name":"monocode_native","title":"MonoCode Native","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
    write_json(&mut rpc.wire.stdin, json!({"method":"initialized"}))?;
    let resumed = rpc.call(
        "thread/resume",
        json!({"threadId":session,"cwd":request.cwd}),
    )?;
    if resumed["thread"]["id"].as_str() != Some(session) {
        return Err("Codex resumed a different thread; history was preserved".into());
    }
    if resumed["thread"]["turns"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|turn| matches!(turn["status"].as_str(), Some("inProgress" | "running")))
    {
        return Err("Stop the current turn before editing the last message".into());
    }
    let before_turn = if let Some(id) = before_turn.filter(|id| !id.trim().is_empty()) {
        id.to_owned()
    } else {
        let page = rpc.call(
            "thread/turns/list",
            json!({"threadId":session,"limit":100,"sortDirection":"desc","itemsView":"summary"}),
        )?;
        page["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|turn| {
                turn["items"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|item| item["type"] == "userMessage"))
            })
            .and_then(|turn| turn["id"].as_str())
            .ok_or("Codex did not expose a user turn id to edit")?
            .to_owned()
    };
    rpc.call(
        "thread/revert",
        json!({"threadId":session,"beforeTurnId":before_turn}),
    )?;
    Ok(session.into())
}

fn last_fork_entry(data: &Value) -> Result<String, String> {
    data["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find_map(|item| item["entryId"].as_str().filter(|id| !id.trim().is_empty()))
        .map(str::to_owned)
        .ok_or_else(|| "No user message to edit".into())
}
fn pi_binding(state: &Value) -> Option<String> {
    let id = state["sessionId"]
        .as_str()
        .or_else(|| state["session_id"].as_str())?
        .trim();
    (!id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
    .then(|| id.to_owned())
}
struct Rpc<'a> {
    wire: &'a mut parked::Wire,
    serial: u64,
    pi: bool,
}
impl Rpc<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RewindError> {
        let mutating = method == "thread/revert";
        self.serial += 1;
        let id = if self.pi {
            json!(format!("native-edit-{}", self.serial))
        } else {
            json!(self.serial)
        };
        let message = if self.pi {
            let mut message = params;
            message["id"] = id.clone();
            message["type"] = method.into();
            message
        } else {
            json!({"id":id,"method":method,"params":params})
        };
        write_json(&mut self.wire.stdin, message)
            .map_err(|error| transport_error(error, mutating))?;
        let started = Instant::now();
        loop {
            let timeout = self
                .pi
                .then(|| Duration::from_secs(if method == "get_state" { 45 } else { 15 }));
            if timeout.is_some_and(|timeout| started.elapsed() > timeout) {
                return Err(transport_error(
                    format!("Provider did not confirm {method}; local history was preserved"),
                    mutating,
                ));
            }
            match self.wire.rx.recv_timeout(TICK) {
                Ok(Ok(line)) => {
                    let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                        continue;
                    };
                    if !self.pi
                        && value["method"] == "currentTime/read"
                        && value.get("id").is_some()
                    {
                        write_json(
                            &mut self.wire.stdin,
                            json!({"id":value["id"],"result":{"currentTimeAt":crate::time::now_ms()/1000}}),
                        )?;
                        continue;
                    }
                    if value["id"] != id {
                        continue;
                    }
                    if let Some(error) = value
                        .get("error")
                        .filter(|error| !self.pi && !error.is_null())
                    {
                        return Err(codex_error(error, "Provider rejected rewind").into());
                    }
                    if self.pi {
                        if value["type"] != "response" {
                            continue;
                        }
                        if value["success"] != true {
                            return Err(value["error"]
                                .as_str()
                                .unwrap_or("Provider rejected rewind")
                                .into());
                        }
                        return Ok(value["data"].clone());
                    }
                    if let Some(result) = value.get("result") {
                        return Ok(result.clone());
                    }
                }
                Ok(Err(error)) => return Err(transport_error(error, mutating)),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(transport_error(
                        format!(
                            "Provider exited before confirming {method}; local history was preserved"
                        ),
                        mutating,
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lost_destructive_confirmation_requires_recovery_but_explicit_rejection_does_not() {
        assert!(transport_error("Lost reply".into(), true).provider_may_have_changed);
        assert!(!transport_error("Resume failed".into(), false).provider_may_have_changed);
        assert!(!RewindError::from("Provider rejected rewind").provider_may_have_changed);
    }
    #[test]
    fn only_native_transports_with_an_explicit_rewind_protocol_advertise_support() {
        for kind in Kind::ALL {
            assert_eq!(
                supports_rewind(kind),
                matches!(kind, Kind::Codex | Kind::Pi | Kind::Omp | Kind::OpenCode)
            );
        }
    }
    #[test]
    fn pi_fork_uses_the_last_valid_user_entry_and_rejects_missing_entries() {
        assert_eq!(
            last_fork_entry(&json!({"messages":[{"entryId":"first"},{"entryId":"last"},{}]}))
                .unwrap(),
            "last"
        );
        assert!(last_fork_entry(&json!({"messages":[]})).is_err());
        assert!(last_fork_entry(&json!([])).is_err());
    }
    #[test]
    fn pi_binding_never_persists_a_session_file_as_an_id() {
        assert_eq!(
            pi_binding(&json!({"sessionId":"fork-123"})),
            Some("fork-123".into())
        );
        assert_eq!(pi_binding(&json!({"sessionId":"/tmp/session.jsonl"})), None);
        assert_eq!(pi_binding(&json!({"sessionId":" "})), None);
    }
}
