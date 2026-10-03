//! Live checks against the installed Cursor CLI (`cursor-agent`), through
//! the real framework: `Children::for_host`, the registry, and `register`.
//! They spawn the CLI and spend a few tokens, so they are ignored by default:
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features cursor -- --ignored --nocapture live_
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{HarnessEvent, SendTurnInput};
use parking_lot::Mutex;
use serde_json::json;

use crate::core::catalog::SharedCatalog;
use crate::core::child::{BridgeLease, Children, HostChildOptions};
use crate::core::register::HarnessContext;
use crate::core::registry::{HarnessRegistry, RegistryOptions, TitleInput};
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
        let dir =
            std::env::temp_dir().join(format!("monocode-cursor-{name}-{}", uuid::Uuid::new_v4()));
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

#[test]
#[ignore = "spawns the real Cursor CLI and spends a few tokens"]
fn live_turn_replies_ok_in_a_temp_directory() {
    let rig = LiveRig::new("turn");
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        Arc::new(move |event: HarnessEvent| events.lock().push(event))
    };
    let input: SendTurnInput = serde_json::from_value(json!({
        "sessionId": "live-turn",
        "cwd": rig.cwd(),
        "model": "cursor:composer-2.5",
        "runtimeMode": "supervised",
        "text": "Reply with the word ok and nothing else.",
    }))
    .unwrap();
    let result = smol::block_on(async {
        let turn = rig
            .registry
            .send_harness_turn(HarnessId::Cursor, input, sink, None);
        let result = timeout(Duration::from_secs(180), turn).await;
        let _ = rig
            .registry
            .forget_harness_session(HarnessId::Cursor, "live-turn")
            .await;
        result
    });

    let events = events.lock().clone();
    for event in &events {
        println!("{}", serde_json::to_string(event).unwrap());
    }
    result.expect("turn timed out").expect("turn failed");
    let reply: String = events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(reply.to_lowercase().contains("ok"), "reply: {reply:?}");
    assert!(events.contains(&HarnessEvent::SessionStarted));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, HarnessEvent::SessionProviderBound { .. }))
    );
    assert!(events.contains(&HarnessEvent::MessageCompleted));
}

#[test]
#[ignore = "spawns the real Cursor CLI"]
fn live_catalog_lists_models() {
    let rig = LiveRig::new("catalog");
    smol::block_on(async {
        rig.registry
            .refresh_harness_catalogs([HarnessId::Cursor], true, |_| false)
            .await;
    });
    assert!(rig.catalog.has_live_catalog(HarnessId::Cursor));
    let models = rig.catalog.read().models_for(HarnessId::Cursor).to_vec();
    println!(
        "{} models, first {:?}",
        models.len(),
        models.first().map(|model| &model.id)
    );
    assert!(!models.is_empty());
}

#[test]
#[ignore = "spawns the real Cursor CLI and spends a few tokens"]
fn live_title_comes_from_the_text_runner() {
    let rig = LiveRig::new("title");
    let title = smol::block_on(async {
        timeout(
            Duration::from_secs(120),
            rig.registry.generate_harness_title(
                HarnessId::Cursor,
                TitleInput {
                    session_id: "live-title".into(),
                    cwd: rig.cwd(),
                    message: "Add a dark mode toggle to the settings page".into(),
                    provider_account_id: None,
                },
            ),
        )
        .await
    });
    let title = title.expect("title timed out").expect("title failed");
    println!("{title:?}");
    assert!(title.is_some_and(|title| !title.title.is_empty()));
}
