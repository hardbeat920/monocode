//! Port of src/integrations/harness/providers/grok/grokCatalog.ts: discover
//! Grok Build's models over ACP, then `grok models`, then the bundled
//! fallback.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::Shared;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::harness::HarnessId;
use monocode_core::models::AgentModel;

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, ChildHandlers, Children};
use crate::core::task::{self, BoxFuture, SharedSpawner};

use super::protocol::{
    GrokSpawnInput, fallback_grok_models, grok_auth_method_id, grok_spawn_args,
    models_from_grok_models_output, models_from_initialize, models_from_session_new,
};
use super::shared::initialize_params;

const PROBE_ID: &str = "monocode-grok-probe";
const DISCOVERY_TIMEOUT_MS: i64 = 15_000;
const REQUEST_TIMEOUT_MS: i64 = 12_000;

struct Inner {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

/// The Grok catalog probe. Clones share one probe.
#[derive(Clone)]
pub struct GrokCatalog {
    inner: Arc<Inner>,
}

impl GrokCatalog {
    pub fn new(children: Children, spawner: SharedSpawner, catalog: SharedCatalog) -> Self {
        Self {
            inner: Arc::new(Inner {
                children,
                spawner,
                catalog,
                inflight: Mutex::new(None),
            }),
        }
    }

    /// `refreshGrokCatalog`. Concurrent calls share one probe.
    pub async fn refresh(&self) {
        let run = {
            let mut inflight = self.inner.inflight.lock();
            match inflight.as_ref() {
                Some(run) => run.clone(),
                None => {
                    let inner = self.inner.clone();
                    let run = async move {
                        let models =
                            discover_grok_models(&inner.children, &inner.spawner, None).await;
                        if !models.is_empty() {
                            inner.catalog.set_harness_models(HarnessId::Grok, models);
                        }
                        inner.inflight.lock().take();
                    }
                    .boxed()
                    .shared();
                    *inflight = Some(run.clone());
                    run
                }
            }
        };
        run.await;
    }

    /// `discoverGrokModels`.
    pub async fn discover(&self, working_directory: Option<&str>) -> Vec<AgentModel> {
        discover_grok_models(&self.inner.children, &self.inner.spawner, working_directory).await
    }
}

/// `discoverGrokModels`: ACP first, then the CLI list, then the fallback.
pub async fn discover_grok_models(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Vec<AgentModel> {
    let from_acp = discover_via_acp(children, spawner, working_directory)
        .await
        .unwrap_or_else(|error| {
            log::debug!("[monocode] grok ACP catalog failed {error:#}");
            Vec::new()
        });
    if !from_acp.is_empty() {
        return from_acp;
    }
    let from_cli = discover_via_cli(children, working_directory)
        .await
        .unwrap_or_else(|error| {
            log::debug!("[monocode] grok CLI catalog failed {error:#}");
            Vec::new()
        });
    if !from_cli.is_empty() {
        return from_cli;
    }
    fallback_grok_models()
}

async fn working_dir(children: &Children, working_directory: Option<&str>) -> Result<String> {
    match working_directory {
        Some(cwd) => Ok(cwd.to_string()),
        None => children.home_dir().await,
    }
}

async fn discover_via_acp(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_grok_binary().await?.path;
    let cwd = working_dir(children, working_directory).await?;
    let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
    // The probe answers every agent request with `{}`. The cell holds the
    // client for those replies and is emptied in `stop`.
    let reply_client: Arc<Mutex<Option<AcpClient>>> = Arc::default();
    let handlers = AcpHandlers::default().on_request({
        let reply_client = reply_client.clone();
        let spawner = spawner.clone();
        move |id: i64, _method: &str, _params: Value| {
            if let Some(acp) = reply_client.lock().clone() {
                spawner.spawn(Box::pin(async move {
                    let _ = acp.respond(id, json!({})).await;
                }));
            }
        }
    });
    let acp = AcpClient::new(&probe_id, Arc::new(children.clone()), handlers);
    *reply_client.lock() = Some(acp.clone());

    let stop = || {
        let acp = acp.clone();
        let children = children.clone();
        let probe_id = probe_id.clone();
        let reply_client = reply_client.clone();
        async move {
            acp.close(None);
            reply_client.lock().take();
            children.unwatch_child(&probe_id);
            let _ = children.kill_child(&probe_id).await;
        }
    };

    children.watch_child_with(
        &probe_id,
        ChildHandlers {
            on_line: Box::new({
                let acp = acp.clone();
                move |line| acp.push_line(&line)
            }),
            on_exit: Box::new({
                let acp = acp.clone();
                move |_| acp.close(Some("Grok Build probe exited"))
            }),
            on_stderr: None,
        },
    );

    let result = async {
        children
            .spawn_child(
                &probe_id,
                &path,
                grok_spawn_args(GrokSpawnInput::default()),
                &cwd,
                None,
                Some(HarnessId::Grok),
            )
            .await?;
        let probe = async {
            let init = acp
                .request_value(
                    "initialize",
                    Some(initialize_params("monocode")),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            let from_init = models_from_initialize(&init);
            if !from_init.is_empty() {
                return Ok(from_init);
            }
            if let Some(method_id) = grok_auth_method_id(&init) {
                let _ = acp
                    .request_value(
                        "authenticate",
                        Some(json!({ "methodId": method_id, "_meta": { "headless": true } })),
                        REQUEST_TIMEOUT_MS,
                    )
                    .await;
            }
            let created = acp
                .request_value(
                    "session/new",
                    Some(json!({ "cwd": cwd, "mcpServers": [] })),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            Ok(models_from_session_new(&created))
        };
        match task::timeout(task::ms(DISCOVERY_TIMEOUT_MS), probe).await {
            Some(models) => models,
            None => {
                stop().await;
                Err(anyhow!("Grok Build catalog probe timed out"))
            }
        }
    }
    .await;
    stop().await;
    result
}

async fn discover_via_cli(
    children: &Children,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_grok_binary().await?.path;
    let cwd = working_dir(children, working_directory).await?;
    let stdout = children
        .exec_child(
            &path,
            vec!["models".into()],
            Some(&cwd),
            Some(HarnessId::Grok),
            BinaryPathChoice::Runtime,
        )
        .await?;
    Ok(models_from_grok_models_output(&stdout))
}
