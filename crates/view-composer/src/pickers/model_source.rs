//! Port of src/features/sessions/ui/modelSource.ts: where the model picker
//! gets its models and provider availability. The default is this
//! computer's catalog; a remote session supplies its host's.

use std::rc::Rc;

use monocode_core::HarnessId;
use monocode_core::models::{AgentModel, HarnessAvailability, ModelCatalog};

/// `ModelSource`.
pub trait ModelSource {
    /// Present only for non-local sources.
    fn id(&self) -> Option<&str> {
        None
    }
    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel>;
    fn resolve(&self, harness: HarnessId, id: Option<&str>) -> AgentModel;
    fn find(&self, id: &str) -> Option<AgentModel>;
    fn available(&self, harness: HarnessId) -> bool;
    fn probed(&self) -> bool;
    /// Refresh availability and catalogs, when the source supports it.
    fn refresh(&self, harnesses: &[HarnessId]);
    /// `harnessUnavailableHint`: why a provider's models cannot be picked.
    fn unavailable_hint(&self, harness: HarnessId) -> String {
        format!(
            "{} not found. Install it, or restart MonoCode if it is already installed.",
            harness.title()
        )
    }
}

type RefreshFn = Rc<dyn Fn(&[HarnessId])>;
type HintFn = Rc<dyn Fn(HarnessId) -> String>;

/// `LOCAL_MODEL_SOURCE`: the catalog and installer probe as plain values.
/// The owner replaces the source when either changes.
#[derive(Clone)]
pub struct LocalModelSource {
    pub catalog: ModelCatalog,
    pub availability: HarnessAvailability,
    refresh: Option<RefreshFn>,
    hint: Option<HintFn>,
}

impl LocalModelSource {
    pub fn new(catalog: ModelCatalog, availability: HarnessAvailability) -> Self {
        Self {
            catalog,
            availability,
            refresh: None,
            hint: None,
        }
    }

    /// Runs when the picker opens or switches tabs: probe availability and
    /// refresh the CLI catalogs (`probeHarnessAvailability`,
    /// `refreshHarnessCatalogs`).
    pub fn on_refresh(mut self, refresh: impl Fn(&[HarnessId]) + 'static) -> Self {
        self.refresh = Some(Rc::new(refresh));
        self
    }

    /// Pass `monocode_harness::harness_unavailable_hint`.
    pub fn with_unavailable_hint(mut self, hint: impl Fn(HarnessId) -> String + 'static) -> Self {
        self.hint = Some(Rc::new(hint));
        self
    }
}

impl ModelSource for LocalModelSource {
    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        self.catalog.models_for(harness).to_vec()
    }

    fn resolve(&self, harness: HarnessId, id: Option<&str>) -> AgentModel {
        self.catalog.resolve_model(harness, id)
    }

    fn find(&self, id: &str) -> Option<AgentModel> {
        self.catalog.find_model(id).cloned()
    }

    fn available(&self, harness: HarnessId) -> bool {
        self.availability.is_available(harness)
    }

    fn probed(&self) -> bool {
        self.availability.probed
    }

    fn refresh(&self, harnesses: &[HarnessId]) {
        if let Some(refresh) = &self.refresh {
            refresh(harnesses);
        }
    }

    fn unavailable_hint(&self, harness: HarnessId) -> String {
        match &self.hint {
            Some(hint) => hint(harness),
            None => format!(
                "{} not found. Install it, or restart MonoCode if it is already installed.",
                harness.title()
            ),
        }
    }
}

/// Every provider installed and probed, for galleries and tests.
pub fn all_available() -> HarnessAvailability {
    HarnessAvailability {
        installed: monocode_core::HARNESSES.into_iter().collect(),
        probed: true,
    }
}
