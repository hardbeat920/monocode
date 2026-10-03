//! Port of src/integrations/harness/providers/antigravity/antigravityCatalog.ts:
//! the live model list from a probe `session/new`.

use std::sync::{Arc, Mutex as StdMutex};

use anyhow::Result;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use monocode_core::harness::HarnessId;
use monocode_core::models::AgentModel;
use parking_lot::Mutex;
use serde_json::json;

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildHandlers, Children};
use crate::core::json_rpc::RpcErrorBody;
use crate::core::task::SharedSpawner;

use super::protocol::{antigravity_spawn_cwd, models_from_session_new};

const PROBE_ID: &str = "monocode-antigravity-probe";
const REQUEST_TIMEOUT_MS: i64 = 12_000;

/// `refreshAntigravityCatalog`'s in-flight promise: concurrent refreshes
/// share one probe.
#[derive(Default)]
pub struct CatalogRefresh {
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

impl CatalogRefresh {
    /// `refreshAntigravityCatalog`. Offline or signed out, the last live
    /// catalog (or the startup seeds) stays.
    pub fn refresh(
        self: &Arc<Self>,
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
    ) -> Shared<BoxFuture<'static, ()>> {
        let mut inflight = self.inflight.lock();
        if let Some(existing) = inflight.as_ref() {
            return existing.clone();
        }
        let this = Arc::downgrade(self);
        let job_spawner = spawner.clone();
        let job: BoxFuture<'static, ()> = async move {
            match discover_antigravity_models(&children, &job_spawner, None).await {
                Ok(models) if !models.is_empty() => {
                    catalog.set_harness_models(HarnessId::Antigravity, models)
                }
                Ok(_) => {}
                Err(error) => log::debug!("[monocode] antigravity catalog {error:#}"),
            }
            if let Some(this) = this.upgrade() {
                *this.inflight.lock() = None;
            }
        }
        .boxed();
        let shared = job.shared();
        *inflight = Some(shared.clone());
        drop(inflight);
        spawner.spawn(Box::pin(shared.clone()));
        shared
    }
}

/// `discoverAntigravityModels`. Authentication stays in the provider's own
/// Terminal UI, so the probe never starts a sign-in.
pub async fn discover_antigravity_models(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let resolved = children.resolve_antigravity_binary().await?;
    let path = resolved.path;
    let args = resolved.args.unwrap_or_default();
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
    // The probe's client, so its request handler can answer. Cleared at the
    // end so the handler and the client do not keep each other alive.
    let cell: Arc<StdMutex<Option<AcpClient>>> = Arc::new(StdMutex::new(None));
    let handlers = AcpHandlers::default().on_request({
        let cell = cell.clone();
        let spawner = spawner.clone();
        move |id, method, _params| {
            let Some(acp) = cell.lock().ok().and_then(|cell| cell.clone()) else {
                return;
            };
            let permission = method == "session/request_permission";
            let method = method.to_string();
            spawner.spawn(Box::pin(async move {
                let _ = if permission {
                    acp.respond(id, json!({ "outcome": { "outcome": "cancelled" } }))
                        .await
                } else {
                    acp.respond_error(
                        id,
                        RpcErrorBody {
                            code: -32601,
                            message: format!("Method not found: {method}"),
                            data: None,
                        },
                    )
                    .await
                };
            }));
        }
    });
    let acp = AcpClient::new(&probe_id, Arc::new(children.clone()), handlers);
    if let Ok(mut slot) = cell.lock() {
        *slot = Some(acp.clone());
    }
    children.watch_child_with(
        &probe_id,
        ChildHandlers {
            on_line: Box::new({
                let acp = acp.clone();
                move |line| acp.push_line(&line)
            }),
            on_exit: Box::new({
                let acp = acp.clone();
                move |_| acp.close(Some("Antigravity probe exited"))
            }),
            on_stderr: None,
        },
    );
    let result = async {
        children
            .spawn_child(
                &probe_id,
                &path,
                args,
                &antigravity_spawn_cwd(&path, &cwd),
                None,
                Some(HarnessId::Antigravity),
            )
            .await?;
        acp.request_value(
            "initialize",
            Some(json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false,
                    // Without this claim a compliant agent may leave
                    // configOptions out of session/new.
                    "session": { "configOptions": { "boolean": {} } },
                },
                "clientInfo": { "name": "monocode", "version": "0.1.0" },
            })),
            REQUEST_TIMEOUT_MS,
        )
        .await?;
        let created = acp
            .request_value(
                "session/new",
                Some(json!({ "cwd": cwd, "mcpServers": [] })),
                REQUEST_TIMEOUT_MS,
            )
            .await?;
        Ok(models_from_session_new(&created))
    }
    .await;
    acp.close(None);
    if let Ok(mut slot) = cell.lock() {
        slot.take();
    }
    children.unwatch_child(&probe_id);
    let _ = children.kill_child(&probe_id).await;
    result
}
