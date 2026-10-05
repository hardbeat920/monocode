//! Port of src/integrations/harness/providers/antigravity/antigravityAdapter.ts:
//! the Antigravity `HarnessAdapter` and its registration.

use std::sync::Arc;

use anyhow::Result;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{ApprovalDecision, SendTurnInput, SteerTurnInput};

use crate::core::catalog::SharedCatalog;
use crate::core::child::Children;
use crate::core::register::HarnessContext;
use crate::core::registry::{AcceptedHook, AdapterCapabilities, EventSink, HarnessAdapter};
use crate::core::task::{BoxFuture, SharedSpawner};

use super::catalog::CatalogRefresh;
use super::session::{AntigravityOptions, AntigravitySessions};

/// What the app may supply beyond [`HarnessContext`]. Only tests change it.
#[derive(Clone, Default)]
pub struct AntigravityAppHooks {
    pub options: AntigravityOptions,
}

/// The Antigravity adapter (`antigravityAdapter`). It cannot steer a running
/// turn.
pub struct AntigravityAdapter {
    sessions: Arc<AntigravitySessions>,
    catalog_refresh: Arc<CatalogRefresh>,
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
}

impl AntigravityAdapter {
    pub fn new(
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
        hooks: AntigravityAppHooks,
    ) -> Self {
        Self {
            sessions: AntigravitySessions::new(
                children.clone(),
                spawner.clone(),
                catalog.clone(),
                hooks.options,
            ),
            catalog_refresh: Arc::new(CatalogRefresh::default()),
            children,
            spawner,
            catalog,
        }
    }

    /// The live session state, for code that drives it directly.
    pub fn sessions(&self) -> &Arc<AntigravitySessions> {
        &self.sessions
    }
}

impl HarnessAdapter for AntigravityAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Antigravity
    }

    fn can_steer(&self) -> bool {
        false
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            refresh_catalog: true,
            ..AdapterCapabilities::default()
        }
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        self.sessions.send_antigravity_turn(input, on_event)
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.steer_antigravity_turn(input).await })
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        self.sessions.cancel_antigravity_turn_now(&session_id);
        Box::pin(async { Ok(()) })
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.sessions
            .respond_antigravity_approval(session_id, request_id, decision);
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.stop_antigravity_session(&session_id).await })
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.forget_antigravity_session(&session_id).await })
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        _provider_account_id: Option<&str>,
    ) {
        self.sessions
            .bind_antigravity_session(thread_id, provider_session_id, cwd);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        let job = self.catalog_refresh.refresh(
            self.children.clone(),
            self.spawner.clone(),
            self.catalog.clone(),
        );
        Box::pin(async move {
            job.await;
            Ok(())
        })
    }
}

/// `ensureAntigravityRegistered`.
pub fn register(ctx: &HarnessContext) {
    register_with(ctx, AntigravityAppHooks::default());
}

/// `ensureAntigravityRegistered` with explicit hooks. Idempotent.
pub fn register_with(ctx: &HarnessContext, hooks: AntigravityAppHooks) {
    if ctx.registry.is_registered(HarnessId::Antigravity) {
        return;
    }
    let adapter = AntigravityAdapter::new(
        ctx.children.clone(),
        ctx.spawner.clone(),
        ctx.catalog.clone(),
        hooks,
    );
    ctx.registry.register_harness(Arc::new(adapter));
}
