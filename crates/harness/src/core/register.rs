//! Port of src/integrations/harness/core/register.ts: register every built-in
//! provider adapter.
//!
//! How a provider registers: each module under `providers/<name>/` exposes
//!
//! ```ignore
//! pub fn register(ctx: &HarnessContext)
//! ```
//!
//! behind its cargo feature. It builds its adapter from the context and calls
//! `ctx.registry.register_harness(Arc::new(adapter))`. It must be idempotent,
//! like the TypeScript `ensure<Name>Registered`: return early when
//! `ctx.registry.is_registered(id)` is already true, so a second call keeps
//! the live adapter and its session state. `pi` registers both `pi` and `omp`.

use super::catalog::SharedCatalog;
use super::child::Children;
use super::registry::HarnessRegistry;
use super::task::SharedSpawner;

/// What a provider adapter gets from the app when it registers. The
/// TypeScript adapters reached these through module imports.
#[derive(Clone)]
pub struct HarnessContext {
    /// Where the adapter registers, and the registry it can dispatch through.
    pub registry: HarnessRegistry,
    /// Process I/O: spawn, write, kill, watch, resolve, HTTP, SSE.
    pub children: Children,
    /// Runs detached tasks such as stdout pumps and timers.
    pub spawner: SharedSpawner,
    /// The live model catalog that `refresh_catalog` writes.
    pub catalog: SharedCatalog,
}

impl HarnessContext {
    pub fn new(registry: HarnessRegistry, children: Children, catalog: SharedCatalog) -> Self {
        let spawner = registry.spawner().clone();
        Self {
            registry,
            children,
            spawner,
            catalog,
        }
    }
}

/// `registerBuiltinHarnesses`: register every provider compiled into this
/// build. Idempotent.
///
/// A provider joins this list once its module exposes `register`. The order
/// is the TypeScript order: claude, cursor, codex, grok, opencode, pi (which
/// also registers omp), fx, hermes, droid, antigravity.
pub fn register_builtin_harnesses(ctx: &HarnessContext) {
    #[cfg(feature = "claude")]
    crate::providers::claude::register(ctx);
    #[cfg(feature = "codex")]
    crate::providers::codex::register(ctx);
    #[cfg(feature = "pi")]
    crate::providers::pi::register(ctx);
    // Providers without a `register` function yet: cursor, grok, opencode,
    // fx, hermes, droid, antigravity.
    let _ = ctx;
}

/// The support matrices from registry.test.ts. They need every provider, so
/// they only build with all provider features on (the default features).
#[cfg(all(
    test,
    feature = "claude",
    feature = "codex",
    feature = "cursor",
    feature = "grok",
    feature = "opencode",
    feature = "pi",
    feature = "fx",
    feature = "hermes",
    feature = "droid",
    feature = "antigravity"
))]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use monocode_core::harness::HarnessId;

    use super::*;
    use crate::core::registry::RegistryOptions;
    use crate::core::task::SmolSpawner;
    use crate::core::testing::{Fake, children};

    fn builtin() -> HarnessRegistry {
        let registry = HarnessRegistry::new(Arc::new(SmolSpawner), RegistryOptions::default());
        let (children, _fake) = children(Fake::default());
        let ctx = HarnessContext::new(registry.clone(), children, SharedCatalog::new());
        register_builtin_harnesses(&ctx);
        // A second call keeps the live adapters.
        register_builtin_harnesses(&ctx);
        registry
    }

    fn matrix(ids: &[HarnessId], check: impl Fn(HarnessId) -> bool) -> BTreeMap<HarnessId, bool> {
        ids.iter().map(|id| (*id, check(*id))).collect()
    }

    use HarnessId::*;

    #[test]
    fn advertises_isolated_text_prompt_support_by_harness() {
        let registry = builtin();
        let ids = [
            Claude,
            Codex,
            Cursor,
            Grok,
            Opencode,
            Pi,
            Omp,
            Fx,
            Hermes,
            Antigravity,
        ];
        assert_eq!(
            matrix(&ids, |id| registry.can_run_harness_text_prompt(id)),
            BTreeMap::from([
                (Claude, true),
                (Codex, true),
                (Cursor, true),
                (Grok, true),
                (Opencode, true),
                (Pi, true),
                (Omp, true),
                (Fx, false),
                (Hermes, false),
                (Antigravity, false),
            ])
        );
    }

    #[test]
    fn exposes_the_native_compaction_support_matrix() {
        let registry = builtin();
        let ids = [
            Claude,
            Codex,
            Cursor,
            Grok,
            Opencode,
            Pi,
            Omp,
            Fx,
            Antigravity,
        ];
        assert_eq!(
            matrix(&ids, |id| registry.can_compact_harness_context(id)),
            BTreeMap::from([
                (Claude, true),
                (Codex, true),
                (Cursor, false),
                (Grok, true),
                (Opencode, true),
                (Pi, true),
                (Omp, true),
                (Fx, false),
                (Antigravity, false),
            ])
        );
    }

    #[test]
    fn exposes_the_edit_last_turn_support_matrix() {
        let registry = builtin();
        let ids = [Claude, Codex, Cursor, Grok, Opencode, Pi, Omp, Fx];
        assert_eq!(
            matrix(&ids, |id| registry.can_rewind_harness_last_turn(id)),
            BTreeMap::from([
                (Claude, false),
                (Codex, true),
                (Cursor, false),
                (Grok, false),
                (Opencode, true),
                (Pi, true),
                (Omp, true),
                (Fx, false),
            ])
        );
    }

    #[test]
    fn registers_antigravity_as_a_live_fx_tier_harness() {
        let registry = builtin();
        assert!(registry.is_live_harness(Antigravity));
        let adapter = registry.get_harness(Antigravity).unwrap();
        assert!(!adapter.can_steer());
        let capabilities = adapter.capabilities();
        assert!(capabilities.refresh_catalog);
        assert!(!capabilities.generate_title);
        assert!(!capabilities.generate_commit_message);
    }
}
