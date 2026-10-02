//! Plumbing the Grok, Droid, and Hermes adapters repeat in TypeScript: the
//! `liveRef` and `muteGate` cells their ACP handlers close over, the
//! `initialize` parameters, and the small ACP replies.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Weak};

use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};

use crate::core::acp::AcpClient;
use crate::core::json_rpc::RpcErrorBody;
use crate::core::task::SharedSpawner;

/// `CLIENT_CAPABILITIES`.
pub fn client_capabilities() -> Value {
    json!({ "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false })
}

/// The `initialize` parameters every ACP adapter here sends.
pub fn initialize_params(client_name: &str) -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": client_capabilities(),
        "clientInfo": { "name": client_name, "version": "0.1.0" },
    })
}

/// `{ outcome: { outcome: "selected", optionId } }`.
pub fn selected_outcome(option_id: &str) -> Value {
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

/// `respondError(id, { code: -32601, message: "Method not found: ..." })`,
/// with the error ignored.
pub async fn respond_method_not_found(acp: &AcpClient, id: i64, method: &str) {
    let _ = acp
        .respond_error(
            id,
            RpcErrorBody {
                code: -32601,
                message: format!("Method not found: {method}"),
                data: None,
            },
        )
        .await;
}

/// `void acp.respondError(...)` from a synchronous handler.
pub fn spawn_method_not_found(spawner: &SharedSpawner, acp: AcpClient, id: i64, method: &str) {
    let method = method.to_string();
    spawner.spawn(Box::pin(async move {
        respond_method_not_found(&acp, id, &method).await;
    }));
}

static TRANSPORT_FAILURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)timed out|not running|exited|closed|pipe").unwrap());

/// `ignoreUnsupportedControl`: a control request the harness does not
/// support is fine, but a transport failure is fatal for the turn.
pub fn ignore_unsupported_control(
    provider: &str,
    method: &str,
    error: anyhow::Error,
) -> anyhow::Result<()> {
    log::debug!("[monocode] {provider} {method} failed: {error:#}");
    if TRANSPORT_FAILURE.is_match(&error.to_string()) {
        return Err(error);
    }
    Ok(())
}

/// The `liveRef`, `muteGate`, and client cells an adapter's ACP handlers
/// close over. The handlers are built before the client and the live record
/// exist, so they read these cells instead.
///
/// The live record is held weakly and the client only until the live record
/// is set, so the client's own handlers never keep it alive.
pub struct Wiring<L> {
    live: Mutex<Weak<L>>,
    acp: Mutex<Option<AcpClient>>,
    mute_gate: AtomicBool,
}

impl<L> Wiring<L> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            live: Mutex::new(Weak::new()),
            acp: Mutex::new(None),
            mute_gate: AtomicBool::new(false),
        })
    }

    /// `liveRef.current`.
    pub fn live(&self) -> Option<Arc<L>> {
        self.live.lock().upgrade()
    }

    /// `liveRef.current = live`.
    pub fn set_live(&self, live: &Arc<L>) {
        *self.live.lock() = Arc::downgrade(live);
        self.acp.lock().take();
    }

    /// The client, for replies sent before the live record exists.
    pub fn acp(&self) -> Option<AcpClient> {
        self.acp.lock().clone()
    }

    pub fn set_acp(&self, acp: &AcpClient) {
        *self.acp.lock() = Some(acp.clone());
    }

    /// Drop the client cell after a failed start.
    pub fn clear(&self) {
        self.acp.lock().take();
    }

    /// `muteGate.current`.
    pub fn muted(&self) -> bool {
        self.mute_gate.load(Ordering::SeqCst)
    }

    pub fn set_muted(&self, muted: bool) {
        self.mute_gate.store(muted, Ordering::SeqCst);
    }
}
