//! Live checks against the installed `pi` and `omp` CLIs, through the real
//! framework: `Children::for_host`, the registry, and `register`. They spawn
//! the CLIs, so they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features pi -- --ignored --nocapture live_
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{BridgeLease, Children, HostChildOptions};
use crate::core::register::HarnessContext;
use crate::core::registry::{HarnessRegistry, RegistryOptions, event_sink};
use crate::core::task::{SmolSpawner, timeout};

use super::register;

struct LiveRig {
    dir: PathBuf,
    registry: HarnessRegistry,
    catalog: SharedCatalog,
    _lease: BridgeLease,
}

impl LiveRig {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("monocode-pi-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let spawner = Arc::new(SmolSpawner);
        let (children, _host) = Children::for_host(
            HostChildOptions {
                data_dir: dir.join("data"),
                control: None,
                updater: None,
            },
            spawner.clone(),
        );
        let lease = children.start_harness_bridge();
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let catalog = SharedCatalog::new();
        register(&HarnessContext::new(
            registry.clone(),
            children,
            catalog.clone(),
        ));
        Self {
            dir,
            registry,
            catalog,
            _lease: lease,
        }
    }

    fn cwd(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
}

impl Drop for LiveRig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn live_turn(harness: HarnessId) {
    let rig = LiveRig::new(harness.as_str());
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    let adapter = rig.registry.get_harness(harness).unwrap();
    let session_id = format!("live-{harness}-{}", uuid::Uuid::new_v4());
    let input = SendTurnInput {
        session: HarnessSessionInput {
            session_id: session_id.clone(),
            cwd: rig.cwd(),
            model: format!("{harness}:default"),
            model_settings: None,
            provider_account_id: None,
            runtime_mode: RuntimeMode::Supervised,
            intent: None,
            controls_agents: None,
            app_access: None,
        },
        text: "Reply with exactly the word ok and nothing else.".into(),
        attachments: None,
    };
    smol::block_on(async {
        let sink = event_sink(move |event| seen.lock().push(event));
        let turn = adapter.send_turn(input, sink, None);
        let result = timeout(Duration::from_secs(180), turn).await;
        let _ = adapter.forget_session(session_id.clone()).await;
        let result = result.expect("the turn finished within three minutes");
        let events = events.lock().clone();
        for event in &events {
            eprintln!("{}", serde_json::to_string(event).unwrap());
        }
        result.unwrap();
        assert!(events.contains(&HarnessEvent::SessionStarted));
        let reply: String = events
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::MessageDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(reply.to_lowercase().contains("ok"), "reply: {reply:?}");
        assert!(events.contains(&HarnessEvent::MessageCompleted));
    });
}

fn live_catalog(harness: HarnessId) {
    let rig = LiveRig::new(&format!("{harness}-catalog"));
    let adapter = rig.registry.get_harness(harness).unwrap();
    smol::block_on(async {
        adapter.refresh_catalog().await.unwrap();
    });
    assert!(rig.catalog.has_live_catalog(harness));
    let catalog = rig.catalog.read();
    let models = catalog.models_for(harness);
    eprintln!(
        "{} models, first {:?}",
        models.len(),
        models.first().map(|model| &model.id)
    );
    assert!(
        models
            .iter()
            .all(|model| model.id.starts_with(&format!("{harness}:")))
    );
}

#[test]
#[ignore = "spawns the real Pi CLI"]
fn live_pi_turn_replies_ok_in_a_temp_directory() {
    live_turn(HarnessId::Pi);
}

#[test]
#[ignore = "spawns the real omp CLI"]
fn live_omp_turn_replies_ok_in_a_temp_directory() {
    live_turn(HarnessId::Omp);
}

#[test]
#[ignore = "spawns the real Pi CLI"]
fn live_pi_catalog_lists_models() {
    live_catalog(HarnessId::Pi);
}

#[test]
#[ignore = "spawns the real omp CLI"]
fn live_omp_catalog_lists_models() {
    live_catalog(HarnessId::Omp);
}
