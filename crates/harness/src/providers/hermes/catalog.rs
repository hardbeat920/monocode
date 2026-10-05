//! Port of src/integrations/harness/providers/hermes/hermesCatalog.ts: read
//! Hermes' models from a throwaway `hermes acp` session.

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
use crate::core::child::{ChildHandlers, Children};
use crate::core::task::{self, BoxFuture, SharedSpawner};
use crate::providers::grok::shared::{initialize_params, spawn_method_not_found};

use super::protocol::models_from_hermes_session;

const PROBE_ID: &str = "monocode-hermes-probe";
const DISCOVERY_TIMEOUT_MS: i64 = 30_000;
const REQUEST_TIMEOUT_MS: i64 = 20_000;

struct Inner {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

/// The Hermes catalog probe. Clones share one probe.
#[derive(Clone)]
pub struct HermesCatalog {
    inner: Arc<Inner>,
}

impl HermesCatalog {
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

    /// `refreshHermesCatalog`. Concurrent calls share one probe.
    pub async fn refresh(&self) {
        let run = {
            let mut inflight = self.inner.inflight.lock();
            match inflight.as_ref() {
                Some(run) => run.clone(),
                None => {
                    let inner = self.inner.clone();
                    let run = async move {
                        match discover_hermes_models(&inner.children, &inner.spawner, None).await {
                            Ok(models) if !models.is_empty() => {
                                inner.catalog.set_harness_models(HarnessId::Hermes, models)
                            }
                            Ok(_) => {}
                            Err(error) => log::debug!("[monocode] hermes catalog {error:#}"),
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

    /// `discoverHermesModels`.
    pub async fn discover(&self, working_directory: Option<&str>) -> Result<Vec<AgentModel>> {
        discover_hermes_models(&self.inner.children, &self.inner.spawner, working_directory).await
    }
}

/// `discoverHermesModels`.
pub async fn discover_hermes_models(
    children: &Children,
    spawner: &SharedSpawner,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_hermes_binary().await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
    // The cell holds the client for request replies and is emptied in `stop`.
    let reply_client: Arc<Mutex<Option<AcpClient>>> = Arc::default();
    let handlers = AcpHandlers::default().on_request({
        let reply_client = reply_client.clone();
        let spawner = spawner.clone();
        move |id: i64, method: &str, _params: Value| {
            if let Some(acp) = reply_client.lock().clone() {
                spawn_method_not_found(&spawner, acp, id, method);
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
                move |_| acp.close(Some("Hermes catalog probe exited"))
            }),
            on_stderr: None,
        },
    );

    let result = async {
        children
            .spawn_child(
                &probe_id,
                &path,
                vec!["acp".into()],
                &cwd,
                None,
                Some(HarnessId::Hermes),
            )
            .await?;
        let probe = async {
            acp.request_value(
                "initialize",
                Some(initialize_params("monocode")),
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
            Ok(models_from_hermes_session(&created))
        };
        match task::timeout(task::ms(DISCOVERY_TIMEOUT_MS), probe).await {
            Some(result) => result,
            None => {
                stop().await;
                Err(anyhow!("Hermes model discovery timed out"))
            }
        }
    }
    .await;
    stop().await;
    result
}
