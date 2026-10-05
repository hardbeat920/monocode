//! Port of src/features/orchestration/model/orchestrationCatalog.ts: worker
//! choices are discovered only when the user sends an orchestration request.

use std::future::Future;

use monocode_core::orchestration::OrchestrationSettings;
use monocode_core::{HARNESSES, HarnessId, ModelCatalog};
use serde_json::json;

use super::plan::validate_orchestration_settings;

/// `discoverOrchestrationSettings`. `probe` is `probeHarnessAvailability`,
/// `refresh` is `refreshHarnessCatalogs`, and `catalog` reads the model
/// catalog after the refresh.
pub async fn discover_orchestration_settings<P, R>(
    probe: impl FnOnce() -> P,
    is_available: impl Fn(HarnessId) -> bool,
    refresh: impl FnOnce(Vec<HarnessId>) -> R,
    catalog: impl FnOnce() -> ModelCatalog,
) -> Result<OrchestrationSettings, String>
where
    P: Future<Output = ()>,
    R: Future<Output = ()>,
{
    probe().await;
    let installed: Vec<HarnessId> = HARNESSES
        .into_iter()
        .filter(|id| is_available(*id))
        .collect();
    refresh(installed.clone()).await;
    let catalog = catalog();
    let choices: Vec<_> = installed
        .into_iter()
        .filter(|id| is_available(*id))
        .flat_map(|harness| {
            catalog
                .models_for(harness)
                .iter()
                .map(move |model| json!({ "harness": harness, "model": model.id, "name": model.name }))
                .collect::<Vec<_>>()
        })
        .collect();
    validate_orchestration_settings(&json!({ "maxWorkers": 2, "choices": choices }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use futures::executor::block_on;
    use monocode_core::AgentModel;

    fn available(id: HarnessId) -> bool {
        id == HarnessId::Codex || id == HarnessId::Claude
    }

    #[test]
    fn discovers_every_installed_harness_and_reads_its_refreshed_models_without_a_user_selected_pool()
     {
        let probes = RefCell::new(0);
        let refreshed = RefCell::new(Vec::new());
        let catalog = RefCell::new(ModelCatalog::new());
        let settings = block_on(discover_orchestration_settings(
            || {
                *probes.borrow_mut() += 1;
                async {}
            },
            available,
            |ids| {
                *refreshed.borrow_mut() = ids;
                catalog.borrow_mut().set_harness_models(
                    HarnessId::Codex,
                    vec![AgentModel::new(
                        "codex:live",
                        HarnessId::Codex,
                        "Live Codex",
                    )],
                );
                async {}
            },
            || catalog.borrow().clone(),
        ))
        .unwrap();
        assert_eq!(*probes.borrow(), 1);
        assert_eq!(
            *refreshed.borrow(),
            vec![HarnessId::Claude, HarnessId::Codex]
        );
        assert!(
            settings
                .choices
                .iter()
                .any(|choice| choice.harness == HarnessId::Codex
                    && choice.model == "codex:live"
                    && choice.name == "Live Codex")
        );
        assert!(
            settings
                .choices
                .iter()
                .any(|choice| choice.harness == HarnessId::Claude)
        );
        assert!(
            !settings
                .choices
                .iter()
                .any(|choice| choice.harness == HarnessId::Cursor)
        );
        assert_eq!(settings.max_workers, 2);
    }

    #[test]
    fn does_not_truncate_catalogs_at_the_former_manual_selection_limit() {
        let mut catalog = ModelCatalog::new();
        catalog.set_harness_models(
            HarnessId::Codex,
            (0..80)
                .map(|i| {
                    AgentModel::new(
                        &format!("codex:{i}"),
                        HarnessId::Codex,
                        &format!("Model {i}"),
                    )
                })
                .collect(),
        );
        let settings = block_on(discover_orchestration_settings(
            || async {},
            available,
            |_| async {},
            || catalog,
        ))
        .unwrap();
        assert_eq!(
            settings
                .choices
                .iter()
                .filter(|choice| choice.harness == HarnessId::Codex)
                .count(),
            80
        );
    }

    #[test]
    fn fails_planning_clearly_when_no_harness_is_available() {
        let error = block_on(discover_orchestration_settings(
            || async {},
            |_| false,
            |_| async {},
            ModelCatalog::new,
        ))
        .unwrap_err();
        assert!(error.contains("No worker models are available"));
    }
}
