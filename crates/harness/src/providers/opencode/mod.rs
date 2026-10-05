//! Port of src/integrations/harness/providers/opencode.
//!
//! OpenCode runs as `opencode serve` on a free loopback port. The adapter
//! talks to it over HTTP and reads its Server-Sent Events stream.
//!
//! - `protocol`: pure helpers over server output, SSE payloads, and parts.
//! - `client`: the HTTP and SSE client for one server.
//! - `adapter`: live sessions, the [`HarnessAdapter`] implementation.
//! - `text`, `title`, `git`: one-shot prompts on a separate server.
//! - `catalog`: models from `opencode models --verbose` and `agent list`.
//!
//! Version dispatch preserves the OpenCode 1 API and selects the separate
//! OpenCode 2 HTTP adapter for CLI major version 2.

pub mod adapter;
pub mod catalog;
pub mod client;
mod deps;
pub mod dispatch;
pub mod git;
pub mod protocol;
pub mod text;
pub mod title;
pub mod v2;

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod test_support;
#[cfg(all(test, unix))]
mod transport_tests;

use std::sync::Arc;

use monocode_core::harness::HarnessId;

pub use dispatch::OpenCodeAdapter;
pub use git::{GitContextSource, GitRangeContext, GitStagedContext, SharedGitSource};

use crate::core::register::HarnessContext;
use crate::core::registry::HarnessAdapter;

/// Register the OpenCode adapter without git context, so commit message and
/// pull request generation fail with "Git context is not available".
/// Idempotent, like `ensureOpenCodeRegistered`.
pub fn register(ctx: &HarnessContext) {
    register_with_git(ctx, None);
}

/// Register the OpenCode adapter with the repository reads that commit
/// message and pull request generation need. Idempotent.
pub fn register_with_git(ctx: &HarnessContext, git: Option<SharedGitSource>) {
    if ctx.registry.is_registered(HarnessId::Opencode) {
        return;
    }
    let adapter: Arc<dyn HarnessAdapter> = Arc::new(OpenCodeAdapter::new(
        ctx.children.clone(),
        ctx.catalog.clone(),
        ctx.spawner.clone(),
        git,
    ));
    ctx.registry.register_harness(adapter);
}
