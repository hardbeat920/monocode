//! Checks against the installed Factory Droid CLI, through
//! `Children::for_host` and the registry. They spawn `droid` and use the
//! signed-in account, so they are ignored by default. Run them with
//!
//! ```text
//! cargo test -p monocode-harness --no-default-features --features droid -- --ignored --nocapture droid::live
//! ```

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;

use crate::providers::grok::test_support::{LiveRig, reply_text};

use super::{DroidAdapter, register};

#[test]
#[ignore = "spawns the real Factory Droid CLI"]
fn live_turn_replies_in_a_temp_directory() {
    smol::block_on(async {
        let rig = LiveRig::new("droid-turn");
        register(&rig.ctx);
        let (result, events) = rig
            .turn(
                HarnessId::Droid,
                "droid:default",
                "Reply with the single word OK and nothing else.",
            )
            .await;
        eprintln!("droid events: {events:?}");
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
#[ignore = "spawns the real Factory Droid CLI"]
fn live_catalog_lists_models_with_effort_levels() {
    smol::block_on(async {
        let rig = LiveRig::new("droid-catalog");
        let adapter = DroidAdapter::new(&rig.ctx);
        let models = adapter.catalog().discover(Some(&rig.cwd())).await.unwrap();
        for model in &models {
            let efforts: Vec<_> = model
                .settings
                .iter()
                .flatten()
                .flat_map(|setting| setting.options.iter().map(|option| option.value.clone()))
                .collect();
            eprintln!("droid model {} efforts {efforts:?}", model.id);
        }
        assert!(!models.is_empty());
    });
}
