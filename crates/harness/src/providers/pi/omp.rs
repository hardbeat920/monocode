//! Port of src/integrations/harness/providers/omp/omp.ts and ompAdapter.ts.
//!
//! omp (oh-my-pi) is a fork of Pi with the same RPC protocol, so it runs on
//! the Pi family core. Live omp sessions load the user's config and
//! extensions (no `--no-extensions`), so plugins in `~/.omp/agent` keep
//! working. omp adds raw slash commands, workflow dialogs answered as
//! questions, advisor interjections, and fast mode.

use std::sync::Arc;

use monocode_core::HarnessId;

use crate::core::register::HarnessContext;

use super::adapter::PiFamilyAdapter;
use super::flavor::OMP_FLAVOR;

/// `ensureOmpRegistered`. Idempotent: a second call keeps the live adapter.
pub fn register_omp(ctx: &HarnessContext) {
    if ctx.registry.is_registered(HarnessId::Omp) {
        return;
    }
    ctx.registry
        .register_harness(Arc::new(PiFamilyAdapter::new(OMP_FLAVOR, ctx)));
}
