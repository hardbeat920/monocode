//! Port of src/integrations/harness/core/availabilityState.ts: the probed
//! installer state for every harness, kept apart from the binary resolvers
//! so the model layer can read it alone.
//!
//! The TypeScript kept this in module globals with a `useSyncExternalStore`
//! subscription. Here it is a [`HarnessAvailabilityStore`] value; clones
//! share one state.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

use monocode_core::harness::HarnessId;
use monocode_core::models::HarnessAvailability;

type Listener = Arc<dyn Fn() + Send + Sync>;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Default)]
struct State {
    installed: BTreeSet<HarnessId>,
    version: u64,
    probed_at: i64,
    listeners: Vec<(u64, Listener)>,
    next_listener: u64,
}

/// The availability store.
#[derive(Clone, Default)]
pub struct HarnessAvailabilityStore {
    state: Arc<Mutex<State>>,
}

impl HarnessAvailabilityStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `emitHarnessAvailability`: bump the version and call every listener.
    pub fn emit_harness_availability(&self) {
        let listeners: Vec<Listener> = {
            let mut state = self.state.lock();
            state.version += 1;
            state
                .listeners
                .iter()
                .map(|(_, listener)| listener.clone())
                .collect()
        };
        for listener in listeners {
            listener();
        }
    }

    /// `subscribeHarnessAvailability`. Returns an id for
    /// [`HarnessAvailabilityStore::unsubscribe_harness_availability`].
    pub fn subscribe_harness_availability(
        &self,
        listener: impl Fn() + Send + Sync + 'static,
    ) -> u64 {
        let mut state = self.state.lock();
        state.next_listener += 1;
        let id = state.next_listener;
        state.listeners.push((id, Arc::new(listener)));
        id
    }

    pub fn unsubscribe_harness_availability(&self, id: u64) {
        self.state
            .lock()
            .listeners
            .retain(|(entry, _)| *entry != id);
    }

    /// `getHarnessAvailabilitySnapshot`: a version that changes on each emit.
    pub fn get_harness_availability_snapshot(&self) -> u64 {
        self.state.lock().version
    }

    /// `hasProbedHarnessAvailability`.
    pub fn has_probed_harness_availability(&self) -> bool {
        self.state.lock().probed_at > 0
    }

    /// `isHarnessAvailable`.
    pub fn is_harness_available(&self, id: HarnessId) -> bool {
        self.state.lock().installed.contains(&id)
    }

    /// `setHarnessAvailability`: replace the whole map. Ids missing from
    /// `installed` read as not installed.
    pub fn set_harness_availability(&self, installed: impl IntoIterator<Item = HarnessId>) {
        self.state.lock().installed = installed.into_iter().collect();
    }

    /// `harnessAvailabilityProbedAt`: when the probe last finished, or 0.
    pub fn harness_availability_probed_at(&self) -> i64 {
        self.state.lock().probed_at
    }

    /// `markHarnessAvailabilityProbed`.
    pub fn mark_harness_availability_probed(&self) {
        self.mark_harness_availability_probed_at(now_ms());
    }

    /// `markHarnessAvailabilityProbed` at a given time. Test seam.
    pub fn mark_harness_availability_probed_at(&self, at_ms: i64) {
        self.state.lock().probed_at = at_ms;
    }

    /// The state as `monocode_core::models::HarnessAvailability`, for `ModelEnv`.
    pub fn snapshot(&self) -> HarnessAvailability {
        let state = self.state.lock();
        HarnessAvailability {
            installed: state.installed.clone(),
            probed: state.probed_at > 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn tracks_installs_versions_and_listeners() {
        let store = HarnessAvailabilityStore::new();
        assert!(!store.is_harness_available(HarnessId::Claude));
        assert!(!store.has_probed_harness_availability());
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let id = store.subscribe_harness_availability(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        store.set_harness_availability([HarnessId::Claude]);
        store.emit_harness_availability();
        store.mark_harness_availability_probed();
        assert!(store.is_harness_available(HarnessId::Claude));
        assert!(store.has_probed_harness_availability());
        assert_eq!(store.get_harness_availability_snapshot(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        store.unsubscribe_harness_availability(id);
        store.emit_harness_availability();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let snapshot = store.snapshot();
        assert!(snapshot.probed && snapshot.is_available(HarnessId::Claude));
    }
}
