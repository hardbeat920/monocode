//! Checks against the installed Grok Build CLI, through `Children::for_host`
//! and the registry. They spawn `grok` and use the signed-in account, so
//! they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features grok -- --ignored --nocapture grok::live
//! ```

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;

use crate::core::registry::TextPromptInput;
use crate::providers::grok::test_support::{LiveRig, reply_text};

use super::register;

#[test]
#[ignore = "spawns the real Grok Build CLI"]
fn live_turn_replies_in_a_temp_directory() {
    smol::block_on(async {
        let rig = LiveRig::new("grok-turn");
        register(&rig.ctx);
        let (result, events) = rig
            .turn(
                HarnessId::Grok,
                "grok:grok-4.6",
                "Reply with the single word OK and nothing else.",
            )
            .await;
        eprintln!("grok events: {events:?}");
        result.unwrap();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, HarnessEvent::SessionProviderBound { .. }))
        );
        assert!(
            reply_text(&events).to_uppercase().contains("OK"),
            "{}",
            reply_text(&events)
        );
    });
}

#[test]
#[ignore = "spawns the real Grok Build CLI"]
fn live_text_prompt_and_catalog() {
    smol::block_on(async {
        let rig = LiveRig::new("grok-text");
        register(&rig.ctx);
        let output = rig
            .ctx
            .registry
            .run_harness_text_prompt(
                HarnessId::Grok,
                TextPromptInput {
                    cwd: rig.cwd(),
                    prompt: "Reply with the single word READY.".into(),
                    timeout_ms: Some(120_000),
                    ..TextPromptInput::default()
                },
            )
            .await
            .unwrap();
        eprintln!("grok text: {output:?}");
        assert!(output.to_uppercase().contains("READY"));

        let adapter = super::GrokAdapter::new(&rig.ctx, super::GrokHost::default());
        let models = adapter.catalog().discover(Some(&rig.cwd())).await;
        eprintln!(
            "grok models: {:?}",
            models.iter().map(|model| &model.id).collect::<Vec<_>>()
        );
        assert!(!models.is_empty());
    });
}
