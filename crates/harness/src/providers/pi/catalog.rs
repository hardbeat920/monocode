//! Port of src/integrations/harness/providers/pi/piCatalog.ts: read the live
//! model list from a throwaway `--mode rpc` probe.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use parking_lot::Mutex;
use serde_json::json;

use monocode_core::HarnessId;
use monocode_core::models::AgentModel;

use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildEvent, Children};
use crate::core::task::{ms, timeout};

use super::client::PiRpc;
use super::deps::Rec;
use super::flavor::PiFlavor;
use super::protocol::{PiSpawnOptions, build_pi_spawn_args, models_from_rpc_data};

const DISCOVERY_TIMEOUT_MS: i64 = 45_000;

type Refresh = Shared<BoxFuture<'static, ()>>;

/// The catalog refresher for one flavor. `inflight` deduplicates refreshes,
/// as the TypeScript module map did.
#[derive(Clone)]
pub struct PiCatalog {
    flavor: PiFlavor,
    children: Children,
    catalog: SharedCatalog,
    inflight: Arc<Mutex<Option<Refresh>>>,
}

impl PiCatalog {
    pub fn new(flavor: PiFlavor, children: Children, catalog: SharedCatalog) -> Self {
        Self {
            flavor,
            children,
            catalog,
            inflight: Arc::default(),
        }
    }

    /// `refreshCatalog`: probe the CLI and publish its models. Failures are
    /// logged, never returned.
    pub fn refresh_catalog(&self) -> Refresh {
        let mut inflight = self.inflight.lock();
        if let Some(running) = inflight.as_ref() {
            return running.clone();
        }
        let this = self.clone();
        let run = async move {
            match discover_models(&this.children, &this.flavor, None).await {
                Ok(models) if !models.is_empty() => {
                    this.catalog.set_harness_models(this.flavor.id, models)
                }
                Ok(_) => {}
                Err(error) => log::debug!("[monocode] {} catalog {error}", this.flavor.id),
            }
            this.inflight.lock().take();
        }
        .boxed()
        .shared();
        *inflight = Some(run.clone());
        run
    }
}

/// `discoverModels`. Without `working_directory` the probe runs in the home
/// directory of the machine that runs the child.
pub async fn discover_models(
    children: &Children,
    flavor: &PiFlavor,
    working_directory: Option<&str>,
) -> Result<Vec<AgentModel>> {
    let path = children.resolve_binary(flavor.id).await?.path;
    let cwd = match working_directory {
        Some(cwd) => cwd.to_string(),
        None => children.home_dir().await?,
    };
    let probe_id = format!("{}-{}", flavor.probe_child_id, uuid::Uuid::new_v4());
    let rpc = PiRpc::new(children, &probe_id, flavor.label);
    let label = flavor.label;

    let events = children.watch_child(&probe_id);
    let pump = rpc.clone();
    children.spawner().spawn(
        async move {
            while let Ok(event) = events.recv().await {
                match event {
                    ChildEvent::Stdout(line) => {
                        let _ = pump.push_line(&line);
                    }
                    ChildEvent::Exit(_) => {
                        pump.close(Some(format!("{label} catalog probe exited")))
                    }
                    ChildEvent::Stderr(_) => {}
                }
            }
        }
        .boxed(),
    );

    let result: Result<Vec<AgentModel>> = async {
        let args = build_pi_spawn_args(
            flavor,
            &PiSpawnOptions {
                no_session: true,
                // Pi packages can register models, so the Pi probe loads
                // extensions. omp keeps its probe isolated.
                no_extensions: flavor.id != HarnessId::Pi,
                ..PiSpawnOptions::default()
            },
        );
        children
            .spawn_child(&probe_id, &path, args, &cwd, None, Some(flavor.id))
            .await?;
        let request: Rec = json!({ "type": "get_available_models" })
            .as_object()
            .cloned()
            .unwrap_or_default();
        let response = timeout(
            ms(DISCOVERY_TIMEOUT_MS),
            rpc.request(request, DISCOVERY_TIMEOUT_MS),
        )
        .await
        .ok_or_else(|| anyhow!("{label} model discovery timed out"))??;
        Ok(models_from_rpc_data(flavor, response.get("data")))
    }
    .await;
    rpc.close(None);
    children.unwatch_child(&probe_id);
    let _ = children.kill_child(&probe_id).await;
    result
}

#[cfg(test)]
mod tests {
    use super::super::flavor::{OMP_FLAVOR, PI_FLAVOR};
    use super::super::testing::{Fake, WriteReply};
    use super::*;

    #[test]
    fn stops_the_probe_after_a_successful_discovery() {
        let fake = Fake::new();
        fake.on_write(|responder, session_id, line| {
            let rec: Rec = serde_json::from_str(line).unwrap();
            responder.respond(
                session_id,
                &rec,
                Some(json!({ "models": [{ "id": "m", "name": "M", "provider": "p", "reasoning": true }] })),
            );
            WriteReply::Ok
        });
        smol::block_on(async {
            let models = discover_models(&fake.children, &PI_FLAVOR, Some("/workspace"))
                .await
                .unwrap();
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].id, "pi:p/m");
        });
        let spawn = fake.spawns().remove(0);
        assert!(spawn.session_id.starts_with("monocode-pi-probe-"));
        // piCatalog.test.ts: "loads Pi extensions when discovering
        // package-provided models".
        assert_eq!(spawn.args, ["--mode", "rpc", "--no-session"]);
        assert_eq!(spawn.cwd, "/workspace");
        assert_eq!(fake.kills(), [spawn.session_id]);
    }

    /// piCatalog.test.ts: "preserves extension isolation for omp catalog
    /// probes".
    #[test]
    fn preserves_extension_isolation_for_omp_catalog_probes() {
        let fake = Fake::new();
        fake.on_write(|responder, session_id, line| {
            let rec: Rec = serde_json::from_str(line).unwrap();
            responder.respond(session_id, &rec, Some(json!({ "models": [] })));
            WriteReply::Ok
        });
        smol::block_on(async {
            discover_models(&fake.children, &OMP_FLAVOR, Some("/workspace"))
                .await
                .unwrap();
        });
        let spawn = fake.spawns().remove(0);
        assert!(spawn.args.iter().any(|arg| arg == "--no-extensions"));
        assert_eq!(spawn.cwd, "/workspace");
    }

    #[test]
    fn publishes_a_refreshed_catalog_from_the_home_directory() {
        let fake = Fake::new();
        fake.on_write(|responder, session_id, line| {
            let rec: Rec = serde_json::from_str(line).unwrap();
            responder.respond(
                session_id,
                &rec,
                Some(json!({ "models": [{ "id": "opus", "name": "Opus", "provider": "anthropic" }] })),
            );
            WriteReply::Ok
        });
        let shared = SharedCatalog::new();
        let catalog = PiCatalog::new(OMP_FLAVOR, fake.children.clone(), shared.clone());
        smol::block_on(async {
            let first = catalog.refresh_catalog();
            let second = catalog.refresh_catalog();
            first.await;
            second.await;
        });
        assert!(shared.has_live_catalog(HarnessId::Omp));
        assert_eq!(fake.spawns().len(), 1);
        assert_eq!(fake.spawns()[0].cwd, "/home/test");
        assert_eq!(
            shared.read().models_for(HarnessId::Omp)[0].id,
            "omp:anthropic/opus"
        );
    }
}
