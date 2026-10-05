//! Checks against the installed fx CLI, through `Children::for_host` and
//! the registry. They spawn `fx acp` and need an AI Gateway credential, so
//! they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features fx -- --ignored --nocapture fx::live
//! ```

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;

use crate::providers::fx::test_support::{LiveRig, reply_text};

use super::{FxAdapter, register};

#[test]
#[ignore = "spawns the real fx CLI"]
fn live_turn_replies_in_a_temp_directory() {
    smol::block_on(async {
        let rig = LiveRig::new("fx-turn");
        register(&rig.ctx);
        let (result, events) = rig
            .turn(
                HarnessId::Fx,
                "fx:zai/glm-5.2-fast",
                "Reply with the single word OK and nothing else.",
            )
            .await;
        eprintln!("fx events: {events:?}");
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
#[ignore = "spawns the real fx CLI"]
fn live_catalog_lists_models() {
    smol::block_on(async {
        let rig = LiveRig::new("fx-catalog");
        let adapter = FxAdapter::new(&rig.ctx);
        let models = adapter.catalog().discover(Some(&rig.cwd())).await.unwrap();
        eprintln!(
            "fx models: {:?}",
            models.iter().map(|model| &model.id).collect::<Vec<_>>()
        );
        assert!(!models.is_empty());
    });
}
