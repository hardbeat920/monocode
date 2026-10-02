//! Port of src/integrations/harness/providers/droid/droidCatalog.ts.
//!
//! Droid lists its models on `session/new` but reports reasoning levels only
//! for the selected one. The probe publishes the plain list first, then
//! walks the models on its throwaway session (a local switch, no inference)
//! to attach each model's own effort choices.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

use super::protocol::{
    DROID_ACP_ARGS, DroidConfigOption, droid_config_options_from, droid_effort_config,
    droid_session_id, models_from_droid_session,
};

const PROBE_ID: &str = "monocode-droid-probe";
const DISCOVERY_TIMEOUT_MS: i64 = 60_000;
const REQUEST_TIMEOUT_MS: i64 = 20_000;
const CONFIG_SETTLE_MS: u64 = 250;

struct Inner {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

/// The Droid catalog probe. Clones share one probe.
#[derive(Clone)]
pub struct DroidCatalog {
    inner: Arc<Inner>,
}

impl DroidCatalog {
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

    /// `refreshDroidCatalog`. Concurrent calls share one probe.
    pub async fn refresh(&self) {
        let run = {
            let mut inflight = self.inner.inflight.lock();
            match inflight.as_ref() {
                Some(run) => run.clone(),
                None => {
                    let inner = self.inner.clone();
                    let run = async move {
                        let catalog = inner.catalog.clone();
                        let publish = move |models: Vec<AgentModel>| {
                            if !models.is_empty() {
                                catalog.set_harness_models(HarnessId::Droid, models);
                            }
                        };
                        if let Err(error) =
                            probe_droid_models(&inner.children, &inner.spawner, publish, None).await
                        {
                            log::debug!("[monocode] droid catalog {error:#}");
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

    /// `discoverDroidModels`: the full catalog, including each model's effort
    /// choices.
    pub async fn discover(&self, working_directory: Option<&str>) -> Result<Vec<AgentModel>> {
        let latest = Arc::new(Mutex::new(Vec::new()));
        let sink = latest.clone();
        probe_droid_models(
            &self.inner.children,
            &self.inner.spawner,
            move |models| *sink.lock() = models,
            working_directory,
        )
        .await?;
        Ok(latest.lock().clone())
    }
}

/// `probeDroidModels`.
async fn probe_droid_models(
    children: &Children,
    spawner: &SharedSpawner,
    publish: impl Fn(Vec<AgentModel>),
    working_directory: Option<&str>,
) -> Result<()> {
    let path = children.resolve_droid_binary().await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let probe_id = format!("{PROBE_ID}-{}", uuid::Uuid::new_v4());
    let latest_config: Arc<Mutex<Option<Vec<DroidConfigOption>>>> = Arc::default();
    // The cell holds the client for request replies and is emptied in `stop`.
    let reply_client: Arc<Mutex<Option<AcpClient>>> = Arc::default();
    let handlers = AcpHandlers::default()
        .on_notification({
            let latest_config = latest_config.clone();
            move |method: &str, params: Value| {
                if method != "session/update" {
                    return;
                }
                if let Some(options) = droid_config_options_from(&params) {
                    *latest_config.lock() = Some(options);
                }
            }
        })
        .on_request({
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
                move |_| acp.close(Some("Droid catalog probe exited"))
            }),
            on_stderr: None,
        },
    );

    let result = async {
        children
            .spawn_child(
                &probe_id,
                &path,
                DROID_ACP_ARGS.iter().map(|arg| arg.to_string()).collect(),
                &cwd,
                None,
                Some(HarnessId::Droid),
            )
            .await?;
        let probe = async {
            acp.request_value("initialize", Some(initialize_params("monocode")), REQUEST_TIMEOUT_MS)
                .await?;
            let created = acp
                .request_value(
                    "session/new",
                    Some(json!({ "cwd": cwd, "mcpServers": [] })),
                    REQUEST_TIMEOUT_MS,
                )
                .await?;
            let models = models_from_droid_session(&created, &HashMap::new());
            publish(models.clone());
            let Some(session_id) = droid_session_id(&created) else {
                return Ok(());
            };
            if models.is_empty() {
                return Ok(());
            }

            let mut efforts: HashMap<String, DroidConfigOption> = HashMap::new();
            let initial = droid_effort_config(&droid_config_options_from(&created).unwrap_or_default()).cloned();
            for model in &models {
                let native = model.native_id.clone().unwrap_or_default();
                if native.is_empty() {
                    continue;
                }
                latest_config.lock().take();
                // Keep the model without effort choices rather than drop it.
                if let Ok(result) = acp
                    .request_value(
                        "session/set_config_option",
                        Some(json!({ "sessionId": session_id, "configId": "model", "value": native })),
                        REQUEST_TIMEOUT_MS,
                    )
                    .await
                {
                    let options = match droid_config_options_from(&result) {
                        Some(options) => Some(options),
                        None => settled_config(&latest_config).await,
                    };
                    if let Some(effort) = options.as_deref().and_then(droid_effort_config) {
                        efforts.insert(native, effort.clone());
                    }
                }
            }
            if efforts.is_empty()
                && let Some(initial) = initial
                && let Some(current) = models.first().and_then(|model| model.native_id.clone())
            {
                efforts.insert(current, initial);
            }
            publish(models_from_droid_session(&created, &efforts));
            Ok(())
        };
        match task::timeout(task::ms(DISCOVERY_TIMEOUT_MS), probe).await {
            Some(result) => result,
            None => {
                stop().await;
                Err(anyhow!("Droid model discovery timed out"))
            }
        }
    }
    .await;
    stop().await;
    result
}

/// `settledConfig`: the `config_option_update` notification can trail the
/// empty response.
async fn settled_config(
    latest: &Mutex<Option<Vec<DroidConfigOption>>>,
) -> Option<Vec<DroidConfigOption>> {
    let deadline = Instant::now() + Duration::from_millis(CONFIG_SETTLE_MS);
    loop {
        let value = latest.lock().clone();
        if value.is_some() || Instant::now() >= deadline {
            return value;
        }
        task::sleep(Duration::from_millis(10)).await;
    }
}
