//! Port of src/integrations/harness/core/availability.ts: probe which CLIs
//! are installed. The probe only checks that the binary exists, never that
//! it is signed in, so the hint must not blame a login.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::Shared;
use parking_lot::Mutex;

use monocode_core::harness::{HARNESSES, HarnessId};

use super::availability_state::{HarnessAvailabilityStore, now_ms};
use super::child::Children;
use super::registry::HarnessRegistry;
use super::task::BoxFuture;

/// `CLI`: the product name and install command for each harness.
fn cli(id: HarnessId) -> (&'static str, Option<&'static str>) {
    match id {
        HarnessId::Claude => ("Claude Code CLI", None),
        HarnessId::Codex => ("Codex CLI", None),
        HarnessId::Cursor => ("Cursor CLI", None),
        HarnessId::Grok => (
            "Grok Build CLI",
            Some("curl -fsSL https://x.ai/cli/install.sh | bash"),
        ),
        HarnessId::Opencode => ("OpenCode CLI", None),
        HarnessId::Pi => ("Pi CLI", Some("npm i -g @earendil-works/pi-coding-agent")),
        HarnessId::Omp => ("omp CLI", Some("curl -fsSL https://omp.sh/install | sh")),
        HarnessId::Fx => ("fx CLI", Some("curl -fsSL https://fx.sh/setup.sh | bash")),
        HarnessId::Hermes => (
            "Hermes Agent CLI",
            Some("Install from hermes-agent.nousresearch.com, then run hermes model"),
        ),
        HarnessId::Droid => (
            "Factory Droid CLI",
            Some("curl -fsSL https://app.factory.ai/cli | sh"),
        ),
        HarnessId::Antigravity => ("Antigravity ACP server (agy_acp_server.par)", None),
    }
}

/// `PROBE_TTL_MS`. A probe stats about 100 paths across the resolvers. The
/// model picker and the providers pane both probe on open, so without a TTL
/// every open pays again to learn what it already knows. Installing a CLI
/// mid-session is rare, and `force` covers it.
pub const PROBE_TTL_MS: i64 = 30_000;

/// `harnessUnavailableHint`.
pub fn harness_unavailable_hint(id: HarnessId) -> String {
    let (name, install) = cli(id);
    let how = install
        .map(|install| format!(" (`{install}`)"))
        .unwrap_or_default();
    format!("{name} not found{how}. Install it, or restart MonoCode if it is already installed.")
}

type Probe = Shared<BoxFuture<'static, ()>>;

/// Runs the installer probe into a [`HarnessAvailabilityStore`].
#[derive(Clone)]
pub struct HarnessAvailabilityProbe {
    registry: HarnessRegistry,
    children: Children,
    store: HarnessAvailabilityStore,
    inflight: Arc<Mutex<Option<Probe>>>,
}

impl HarnessAvailabilityProbe {
    pub fn new(
        registry: HarnessRegistry,
        children: Children,
        store: HarnessAvailabilityStore,
    ) -> Self {
        Self {
            registry,
            children,
            store,
            inflight: Arc::default(),
        }
    }

    pub fn store(&self) -> &HarnessAvailabilityStore {
        &self.store
    }

    /// `probeHarnessAvailability`. Joins a probe that is already running.
    /// The probe starts on the spawner at once, as the promise did.
    pub fn probe_harness_availability(&self, force: bool) -> BoxFuture<'static, ()> {
        let mut inflight = self.inflight.lock();
        if let Some(probe) = inflight.as_ref() {
            return probe.clone().boxed();
        }
        let last_probe = self.store.harness_availability_probed_at();
        if !force && last_probe > 0 && now_ms() - last_probe < PROBE_TTL_MS {
            return async {}.boxed();
        }

        let (done_tx, done_rx) = futures::channel::oneshot::channel::<()>();
        let probe: Probe = async move {
            let _ = done_rx.await;
        }
        .boxed()
        .shared();
        *inflight = Some(probe.clone());
        drop(inflight);

        let this = self.clone();
        self.registry.spawner().spawn(
            async move {
                let checks = HARNESSES.into_iter().map(|id| {
                    let this = this.clone();
                    async move {
                        if !this.registry.is_live_harness(id) {
                            return (id, false);
                        }
                        (id, this.children.resolve_binary(id).await.is_ok())
                    }
                });
                let entries = futures::future::join_all(checks).await;
                this.store.set_harness_availability(
                    entries.into_iter().filter(|(_, ok)| *ok).map(|(id, _)| id),
                );
                this.store.emit_harness_availability();
                // TODO(port): listeners run before `probedAt` is set, so a
                // listener that reads `has_probed` during the first emit sees
                // false. Kept from the TypeScript.
                this.store.mark_harness_availability_probed();
                *this.inflight.lock() = None;
                let _ = done_tx.send(());
            }
            .boxed(),
        );
        probe.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::RegistryOptions;
    use crate::core::task::SmolSpawner;
    use crate::core::testing::{Call, Fake, StubAdapter, children};

    #[test]
    fn hints_name_the_cli_and_install_command() {
        assert_eq!(
            harness_unavailable_hint(HarnessId::Grok),
            "Grok Build CLI not found (`curl -fsSL https://x.ai/cli/install.sh | bash`). Install it, or restart MonoCode if it is already installed."
        );
        assert_eq!(
            harness_unavailable_hint(HarnessId::Claude),
            "Claude Code CLI not found. Install it, or restart MonoCode if it is already installed."
        );
    }

    #[test]
    fn probes_live_harnesses_once_within_the_ttl() {
        let (children, fake) = children(Fake {
            missing: vec![HarnessId::Codex],
            ..Default::default()
        });
        let registry = HarnessRegistry::new(Arc::new(SmolSpawner), RegistryOptions::default());
        registry.register_harness(Arc::new(StubAdapter(HarnessId::Claude)));
        registry.register_harness(Arc::new(StubAdapter(HarnessId::Codex)));
        let probe =
            HarnessAvailabilityProbe::new(registry, children, HarnessAvailabilityStore::new());
        smol::block_on(probe.probe_harness_availability(false));
        assert!(probe.store().has_probed_harness_availability());
        assert!(probe.store().is_harness_available(HarnessId::Claude));
        assert!(!probe.store().is_harness_available(HarnessId::Codex));
        // Unregistered harnesses are not resolved at all.
        assert!(!probe.store().is_harness_available(HarnessId::Pi));
        assert!(!fake.calls().contains(&Call::ResolveDefault(HarnessId::Pi)));

        let version = probe.store().get_harness_availability_snapshot();
        smol::block_on(probe.probe_harness_availability(false));
        assert_eq!(probe.store().get_harness_availability_snapshot(), version);
        smol::block_on(probe.probe_harness_availability(true));
        assert_eq!(
            probe.store().get_harness_availability_snapshot(),
            version + 1
        );
    }

    #[test]
    fn concurrent_probes_share_one_run() {
        let (children, fake) = children(Fake::default());
        let registry = HarnessRegistry::new(Arc::new(SmolSpawner), RegistryOptions::default());
        registry.register_harness(Arc::new(StubAdapter(HarnessId::Claude)));
        let probe =
            HarnessAvailabilityProbe::new(registry, children, HarnessAvailabilityStore::new());
        let first = probe.probe_harness_availability(true);
        let second = probe.probe_harness_availability(true);
        smol::block_on(futures::future::join(first, second));
        let resolves = fake
            .calls()
            .into_iter()
            .filter(|call| *call == Call::ResolveDefault(HarnessId::Claude))
            .count();
        assert_eq!(resolves, 1);
    }
}
