//! The live model catalog that `models.ts` kept in module globals
//! (`setHarnessModels`, `hasLiveCatalog`, `resetHarnessModelOverlays`),
//! shared between the engine and the provider adapters that refresh it.

use std::sync::Arc;

use parking_lot::{Mutex, RwLock, RwLockReadGuard};

use monocode_core::harness::HarnessId;
use monocode_core::models::{AgentModel, ModelCatalog};

type Listener = Arc<dyn Fn(HarnessId) + Send + Sync>;

/// A [`ModelCatalog`] behind a lock, with change listeners. Clones share one
/// catalog.
#[derive(Clone, Default)]
pub struct SharedCatalog {
    catalog: Arc<RwLock<ModelCatalog>>,
    listeners: Arc<Mutex<Vec<(u64, Listener)>>>,
    next_listener: Arc<Mutex<u64>>,
}

impl SharedCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the catalog. Do not hold the guard across an await.
    pub fn read(&self) -> RwLockReadGuard<'_, ModelCatalog> {
        self.catalog.read()
    }

    /// A copy of the current catalog.
    pub fn snapshot(&self) -> ModelCatalog {
        self.catalog.read().clone()
    }

    /// `setHarnessModels`. Listeners run after the write lock is released.
    pub fn set_harness_models(&self, harness: HarnessId, models: Vec<AgentModel>) {
        self.catalog.write().set_harness_models(harness, models);
        let listeners: Vec<Listener> = self
            .listeners
            .lock()
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect();
        for listener in listeners {
            listener(harness);
        }
    }

    /// `hasLiveCatalog`.
    pub fn has_live_catalog(&self, harness: HarnessId) -> bool {
        self.catalog.read().has_live_catalog(harness)
    }

    /// `resetHarnessModelOverlays`. Test seam.
    pub fn reset_overlays(&self) {
        self.catalog.write().reset_overlays();
    }

    /// Call `listener` after each `set_harness_models`. Returns an id for
    /// [`SharedCatalog::unsubscribe`].
    pub fn subscribe(&self, listener: impl Fn(HarnessId) + Send + Sync + 'static) -> u64 {
        let id = {
            let mut next = self.next_listener.lock();
            *next += 1;
            *next
        };
        self.listeners.lock().push((id, Arc::new(listener)));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        self.listeners.lock().retain(|(entry, _)| *entry != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn notifies_after_a_live_list_lands() {
        let catalog = SharedCatalog::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let id = catalog.subscribe(move |harness| {
            assert_eq!(harness, HarnessId::Pi);
            seen.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!catalog.has_live_catalog(HarnessId::Pi));
        catalog.set_harness_models(
            HarnessId::Pi,
            vec![
                AgentModel::new("pi:opus", HarnessId::Pi, "Opus").with_native_id("anthropic/opus"),
            ],
        );
        assert!(catalog.has_live_catalog(HarnessId::Pi));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        catalog.unsubscribe(id);
        catalog.reset_overlays();
        assert!(!catalog.clone().has_live_catalog(HarnessId::Pi));
    }
}
