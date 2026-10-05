//! Checks against the installed Hermes Agent CLI, through
//! `Children::for_host` and the registry. They spawn `hermes acp` and use its
//! configured provider, so they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features hermes -- --ignored --nocapture hermes::live
//! ```

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;

use crate::providers::grok::test_support::{LiveRig, reply_text};

use super::{HermesAdapter, register};

#[test]
#[ignore = "spawns the real Hermes Agent CLI"]
fn live_turn_replies_in_a_temp_directory() {
    smol::block_on(async {
        let rig = LiveRig::new("hermes-turn");
        register(&rig.ctx);
        let (result, events) = rig
            .turn(
                HarnessId::Hermes,
                "hermes:default",
                "Reply with the single word OK and nothing else.",
            )
            .await;
        eprintln!("hermes events: {events:?}");
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
#[ignore = "spawns the real Hermes Agent CLI"]
fn live_catalog_lists_models() {
    smol::block_on(async {
        let rig = LiveRig::new("hermes-catalog");
        let adapter = HermesAdapter::new(&rig.ctx);
        let models = adapter.catalog().discover(Some(&rig.cwd())).await.unwrap();
        eprintln!(
            "hermes models: {:?}",
            models.iter().map(|model| &model.id).collect::<Vec<_>>()
        );
        assert!(!models.is_empty());
    });
}
