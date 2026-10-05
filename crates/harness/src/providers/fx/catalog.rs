//! Port of src/integrations/harness/providers/fx/fxCatalog.ts: read fx's
//! models from `fx models --json`, plus the active model from
//! `fx status --json`.

use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt;
use futures::future::Shared;
use parking_lot::Mutex;

use monocode_core::harness::HarnessId;
use monocode_core::models::AgentModel;

use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, Children};
use crate::core::task::BoxFuture;

use super::protocol::{
    merge_fx_catalog_models, model_from_fx_status_output, models_from_fx_output,
};

struct Inner {
    children: Children,
    catalog: SharedCatalog,
    inflight: Mutex<Option<Shared<BoxFuture<'static, ()>>>>,
}

/// The fx catalog probe. Clones share one probe.
#[derive(Clone)]
pub struct FxCatalog {
    inner: Arc<Inner>,
}

impl FxCatalog {
    pub fn new(children: Children, catalog: SharedCatalog) -> Self {
        Self {
            inner: Arc::new(Inner {
                children,
                catalog,
                inflight: Mutex::new(None),
            }),
        }
    }

    /// `refreshFxCatalog`. Concurrent calls share one probe.
    pub async fn refresh(&self) {
        let run = {
            let mut inflight = self.inner.inflight.lock();
            match inflight.as_ref() {
                Some(run) => run.clone(),
                None => {
                    let inner = self.inner.clone();
                    let run = async move {
                        match discover_fx_models(&inner.children, None).await {
                            Ok(models) if !models.is_empty() => {
                                inner.catalog.set_harness_models(HarnessId::Fx, models)
                            }
                            Ok(_) => {}
                            Err(error) => log::debug!("[monocode] fx catalog {error:#}"),
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

    /// `discoverFxModels`.
    pub async fn discover(&self, working_directory: Option<&str>) -> Result<Vec<AgentModel>> {
        discover_fx_models(&self.inner.children, working_directory).await
    }
}

/// `discoverFxModels`.
pub async fn discover_fx_models(
    children: &Children,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_fx_binary().await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let exec = |args: [&str; 2]| {
        children.exec_child(
            &path,
            args.iter().map(|arg| arg.to_string()).collect(),
            Some(&cwd),
            Some(HarnessId::Fx),
            BinaryPathChoice::Runtime,
        )
    };
    let (models, status) = futures::join!(exec(["models", "--json"]), exec(["status", "--json"]));
    Ok(merge_fx_catalog_models(
        models_from_fx_output(&models?),
        model_from_fx_status_output(&status.unwrap_or_default()),
    ))
}
