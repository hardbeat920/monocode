//! Port of the intent of codexLive.test.ts against the real `codex` CLI. The
//! TypeScript file mocks the child, so its cases run in `session` and
//! `subagents`. This test spawns `codex app-server` for real, so it is
//! `#[ignore]`: run it with
//! `cargo test -p monocode-harness --no-default-features --features codex -- --ignored live`.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{Children, HostChildOptions};
use crate::core::register::HarnessContext;
use crate::core::registry::{HarnessAdapter, HarnessRegistry, RegistryOptions, TextPromptInput};
use crate::core::task::{SharedSpawner, SmolSpawner, timeout};

use super::super::adapter::{CodexAdapter, CodexHost};

#[test]
#[ignore = "spawns the real codex CLI and spends a few tokens"]
fn runs_one_short_turn_against_the_real_cli() {
    smol::block_on(async {
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let (children, _host) = Children::for_host(HostChildOptions::default(), spawner.clone());
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let catalog = SharedCatalog::new();
        let ctx = HarnessContext::new(registry, children, catalog.clone());
        let adapter = CodexAdapter::new(&ctx, CodexHost::default());

        let refreshed = timeout(Duration::from_secs(60), adapter.refresh_catalog()).await;
        let models: Vec<String> = catalog
            .read()
            .models_for(HarnessId::Codex)
            .iter()
            .map(|model| model.id.clone())
            .collect();
        eprintln!(
            "catalog refresh: {refreshed:?}, live: {}, models: {models:?}",
            catalog.has_live_catalog(HarnessId::Codex)
        );
        let model = catalog.read().default_model_id(HarnessId::Codex);

        let dir =
            std::env::temp_dir().join(format!("monocode-codex-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_string_lossy().into_owned();

        let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::default();
        let sink = {
            let events = events.clone();
            Arc::new(move |event: HarnessEvent| {
                eprintln!("event {}", serde_json::to_string(&event).unwrap());
                events.lock().push(event);
            })
        };
        let session_id = "codex-live-real";
        let input = SendTurnInput {
            session: HarnessSessionInput {
                session_id: session_id.into(),
                cwd: cwd.clone(),
                model: model.clone(),
                model_settings: None,
                provider_account_id: None,
                runtime_mode: RuntimeMode::Supervised,
                intent: None,
                controls_agents: None,
                app_access: None,
            },
            text: "Reply with the single word pong and nothing else. Do not run any commands."
                .into(),
            attachments: None,
        };
        eprintln!("model {model}, cwd {cwd}");
        let turn = timeout(
            Duration::from_secs(180),
            adapter.send_turn(input, sink, None),
        )
        .await;
        eprintln!("turn result: {turn:?}");

        let text = timeout(
            Duration::from_secs(120),
            adapter.run_text_prompt(TextPromptInput {
                cwd: cwd.clone(),
                prompt: "Reply with the single word ping and nothing else.".into(),
                timeout_ms: Some(120_000),
                ..Default::default()
            }),
        )
        .await;
        eprintln!("text prompt result: {text:?}");

        adapter.stop_session(session_id.into()).await.unwrap();
        adapter.stop_text_prompt().await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        turn.expect("turn timed out").expect("turn failed");
        let events = events.lock().clone();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, HarnessEvent::SessionProviderBound { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, HarnessEvent::TurnStarted { .. }))
        );
        let reply: String = events
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::MessageDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(reply.to_lowercase().contains("pong"), "reply was {reply:?}");
        let text = text
            .expect("text prompt timed out")
            .expect("text prompt failed");
        assert!(text.to_lowercase().contains("ping"), "text was {text:?}");
    });
}
