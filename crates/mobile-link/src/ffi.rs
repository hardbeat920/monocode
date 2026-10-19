use crate::{Address, Identity, LinkResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    os::raw::c_char,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
enum Call {
    Identity,
    Fingerprint {
        identity: Identity,
    },
    Proof {
        identity: Identity,
        invitation: String,
    },
    Exchange {
        identity: Identity,
        pin: String,
        addresses: Vec<Address>,
        request: Value,
    },
    /// Opens a watch and returns its ID; `next` then pulls its updates.
    Watch {
        identity: Identity,
        pin: String,
        addresses: Vec<Address>,
        request: Value,
    },
    #[serde(rename_all = "camelCase")]
    Next {
        watch_id: u64,
        timeout_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    Unwatch {
        watch_id: u64,
    },
    #[serde(rename_all = "camelCase")]
    TranscriptCreate {
        #[serde(default)]
        turns: Vec<Value>,
        #[serde(default)]
        collapsed: bool,
    },
    #[serde(rename_all = "camelCase")]
    TranscriptUpdate {
        transcript_id: u64,
        command: Value,
    },
    #[serde(rename_all = "camelCase")]
    TranscriptNext {
        transcript_id: u64,
        watch_id: u64,
        timeout_ms: u64,
    },
    #[serde(rename_all = "camelCase")]
    TranscriptDrop {
        transcript_id: u64,
    },
}
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Native QUIC runtime"))
}
fn watches() -> &'static Mutex<HashMap<u64, Arc<crate::Watch>>> {
    static WATCHES: OnceLock<Mutex<HashMap<u64, Arc<crate::Watch>>>> = OnceLock::new();
    WATCHES.get_or_init(Default::default)
}
fn watching(id: u64) -> LinkResult<Arc<crate::Watch>> {
    watches()
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "Desktop stopped watching".into())
}
type TranscriptHandle = Arc<Mutex<crate::transcript::Transcript>>;
fn transcripts() -> &'static Mutex<HashMap<u64, TranscriptHandle>> {
    static TRANSCRIPTS: OnceLock<Mutex<HashMap<u64, TranscriptHandle>>> = OnceLock::new();
    TRANSCRIPTS.get_or_init(Default::default)
}
fn update_transcript(id: u64, command: Value) -> LinkResult<Value> {
    let handle = transcripts()
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or("Transcript closed")?;
    let result = handle.lock().unwrap().update(command);
    result
}
fn execute(input: &str) -> LinkResult<Value> {
    if input.len() > crate::REQUEST_LIMIT {
        return Err("Request is too large".into());
    }
    let call: Call =
        serde_json::from_str(input).map_err(|_| "Invalid native connection request")?;
    match call {
        Call::Identity => serde_json::to_value(Identity::generate()?).map_err(|e| e.to_string()),
        Call::Fingerprint { identity } => Ok(json!(identity.fingerprint()?)),
        Call::Proof {
            identity,
            invitation,
        } => Ok(json!(identity.proof(&invitation)?)),
        Call::Exchange {
            identity,
            pin,
            addresses,
            request,
        } => runtime().block_on(crate::exchange(&identity, &pin, &addresses, request)),
        Call::Watch {
            identity,
            pin,
            addresses,
            request,
        } => {
            static NEXT_ID: AtomicU64 = AtomicU64::new(1);
            let watch = runtime().block_on(crate::watch(&identity, &pin, &addresses, request))?;
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            watches().lock().unwrap().insert(id, Arc::new(watch));
            Ok(json!({ "watchId": id }))
        }
        Call::Next {
            watch_id,
            timeout_ms,
        } => {
            let watch = watching(watch_id)?;
            let wait = Duration::from_millis(timeout_ms.min(30_000));
            match runtime().block_on(watch.next(wait)) {
                Ok(updates) => Ok(json!({ "updates": updates })),
                Err(error) => {
                    // An ended watch is gone; the phone opens a new one.
                    if let Some(watch) = watches().lock().unwrap().remove(&watch_id) {
                        runtime().spawn(async move { watch.close().await });
                    }
                    Err(error)
                }
            }
        }
        Call::Unwatch { watch_id } => {
            if let Some(watch) = watches().lock().unwrap().remove(&watch_id) {
                runtime().block_on(watch.close());
            }
            Ok(Value::Null)
        }
        Call::TranscriptCreate { turns, collapsed } => {
            static NEXT_TRANSCRIPT: AtomicU64 = AtomicU64::new(1);
            let id = NEXT_TRANSCRIPT.fetch_add(1, Ordering::Relaxed);
            transcripts().lock().unwrap().insert(
                id,
                Arc::new(Mutex::new(crate::transcript::Transcript::new(
                    turns, collapsed,
                ))),
            );
            Ok(json!({"transcriptId":id}))
        }
        Call::TranscriptUpdate {
            transcript_id,
            command,
        } => update_transcript(transcript_id, command),
        Call::TranscriptNext {
            transcript_id,
            watch_id,
            timeout_ms,
        } => {
            let watch = watching(watch_id)?;
            // Project on this native worker, before serializing the FFI reply.
            // A burst produces one delta; JS never receives the text.
            let updates =
                runtime().block_on(watch.next(Duration::from_millis(timeout_ms.min(30_000))))?;
            if updates.is_empty() {
                return Ok(json!({"idle":true}));
            }
            update_transcript(transcript_id, json!({"type":"frames","frames":updates}))
        }
        Call::TranscriptDrop { transcript_id } => {
            transcripts().lock().unwrap().remove(&transcript_id);
            Ok(Value::Null)
        }
    }
}
fn call(input: &str) -> String {
    match std::panic::catch_unwind(|| execute(input)) {
        Ok(Ok(result)) => json!({"ok":true,"result":result}).to_string(),
        Ok(Err(error)) => json!({"ok":false,"error":error}).to_string(),
        Err(_) => json!({"ok":false,"error":"Native connection failed"}).to_string(),
    }
}

/// # Safety
/// `input` must point to a live, NUL-terminated UTF-8 string for the call's duration.
#[no_mangle]
pub unsafe extern "C" fn monocode_link_call(input: *const c_char) -> *mut c_char {
    let output = if input.is_null() {
        call("")
    } else {
        call(CStr::from_ptr(input).to_str().unwrap_or(""))
    };
    CString::new(output).unwrap().into_raw()
}
/// # Safety
/// `output` must be a pointer returned by `monocode_link_call`, freed exactly once.
#[no_mangle]
pub unsafe extern "C" fn monocode_link_free(output: *mut c_char) {
    if !output.is_null() {
        drop(CString::from_raw(output));
    }
}
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_expo_modules_monocodelink_MonocodeLinkModule_nativeCall(
    mut env: jni::JNIEnv,
    _: jni::objects::JObject,
    input: jni::objects::JString,
) -> jni::sys::jstring {
    let output = env
        .get_string(&input)
        .map(|v| call(&String::from(v)))
        .unwrap_or_else(|_| call(""));
    env.new_string(output)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_transcript_wire_contract_and_cleanup() {
        let invoke =
            |input: Value| -> Value { serde_json::from_str(&call(&input.to_string())).unwrap() };
        let opened = invoke(
            json!({"operation":"transcriptCreate","turns":[{"id":"1","items":[{"type":"text","id":"r","text":"Hello **native** 👋"}]}],"collapsed":true}),
        );
        assert_eq!(opened["ok"], true);
        let id = &opened["result"]["transcriptId"];
        let frame = invoke(
            json!({"operation":"transcriptUpdate","transcriptId":id,"command":{"type":"render"}}),
        );
        assert_eq!(frame["ok"], true);
        assert_eq!(frame["result"]["order"], json!(["1/r:0"]));
        assert!(frame["result"]["rows"][0]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|run| run["bold"] == true && run["text"] == "native"));
        assert_eq!(
            invoke(json!({"operation":"transcriptDrop","transcriptId":id}))["ok"],
            true
        );
        assert_eq!(
            invoke(
                json!({"operation":"transcriptUpdate","transcriptId":id,"command":{"type":"render"}})
            )["ok"],
            false
        );
    }
}
