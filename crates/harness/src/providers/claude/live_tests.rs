//! Live checks against the installed Claude Code CLI, through the real
//! framework: `Children::for_host`, the registry, and `register`. They spawn
//! `claude`, so they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features claude -- --ignored --nocapture live_
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};
use parking_lot::Mutex;

use crate::core::catalog::SharedCatalog;
use crate::core::child::{Children, HostChildOptions};
use crate::core::register::HarnessContext;
use crate::core::registry::{HarnessRegistry, RegistryOptions, TitleInput};
use crate::core::task::{SmolSpawner, timeout};

use super::register;

struct LiveRig {
    dir: PathBuf,
    registry: HarnessRegistry,
    catalog: SharedCatalog,
    _lease: crate::core::child::BridgeLease,
}

impl LiveRig {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("monocode-claude-{name}-{}", uuid::Uuid::new_v4()));
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
#[ignore = "spawns the real Claude Code CLI"]
fn live_turn_replies_ok_in_a_temp_directory() {
    let rig = LiveRig::new("turn");
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
    let sink = {
        let events = events.clone();
        Arc::new(move |event: HarnessEvent| events.lock().push(event))
    };
    let input = SendTurnInput {
        session: HarnessSessionInput {
            session_id: "live-turn".into(),
            cwd: rig.cwd(),
            model: "claude:haiku".into(),
            model_settings: None,
            provider_account_id: None,
            runtime_mode: RuntimeMode::Supervised,
            intent: None,
            controls_agents: None,
            app_access: None,
        },
        text: "Reply with the word ok and nothing else.".into(),
        attachments: None,
    };
    let result = smol::block_on(async {
        let turn = rig
            .registry
            .send_harness_turn(HarnessId::Claude, input, sink, None);
        let result = timeout(Duration::from_secs(120), turn).await;
        let _ = rig
            .registry
            .forget_harness_session(HarnessId::Claude, "live-turn")
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
#[ignore = "spawns the real Claude Code CLI"]
fn live_catalog_lists_models_without_a_model_call() {
    let rig = LiveRig::new("catalog");
    smol::block_on(async {
        rig.registry
            .refresh_harness_catalogs([HarnessId::Claude], true, |_| false)
            .await;
    });
    assert!(rig.catalog.has_live_catalog(HarnessId::Claude));
    let catalog = rig.catalog.read();
    let models = catalog.models_for(HarnessId::Claude);
    for model in models {
        println!("{} {} {:?}", model.id, model.name, model.native_id);
    }
    assert!(!models.is_empty());
}

#[test]
#[ignore = "spawns the real Claude Code CLI"]
fn live_title_comes_back_from_the_text_runner() {
    let rig = LiveRig::new("title");
    let title = smol::block_on(async {
        timeout(
            Duration::from_secs(90),
            rig.registry.generate_harness_title(
                HarnessId::Claude,
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
