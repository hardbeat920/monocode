//! Port of src/integrations/harness/providers/cursor/cursorAdapter.ts: the
//! Cursor `HarnessAdapter` and its registration.

use std::sync::Arc;

use anyhow::Result;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{ApprovalDecision, SendTurnInput, SteerTurnInput};
use monocode_core::user_question::UserQuestionReply;

use crate::core::catalog::SharedCatalog;
use crate::core::child::Children;
use crate::core::register::HarnessContext;
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture, SharedSpawner};

use super::catalog::CatalogRefresh;
use super::git::{
    SharedGitSource, generate_cursor_branch_name, generate_cursor_commit_message,
    generate_cursor_pr_content,
};
use super::session::{CursorSessionOptions, CursorSessions};
use super::store::{CursorStore, NoCursorStore, StoreReader};
use super::text::TextRunner;
use super::title::generate_cursor_session_title;

/// What the app supplies that [`HarnessContext`] does not carry.
#[derive(Clone, Default)]
pub struct CursorAppHooks {
    /// Cursor's own SQLite stores (`cursor_tool_calls` and
    /// `cursor_subagent_runs`). Without it, tool label enrichment and stored
    /// subagent steps find nothing.
    pub store: Option<Arc<dyn CursorStore>>,
    /// `gitStagedContext` and `gitRangeContext`. Without it, commit message
    /// and pull request generation fail with "Git context is not available".
    pub git: Option<SharedGitSource>,
    /// Test seam for the session timeouts.
    pub session_options: CursorSessionOptions,
}

/// The Cursor adapter (`cursorAdapter`).
pub struct CursorAdapter {
    sessions: Arc<CursorSessions>,
    text: Arc<TextRunner>,
    catalog_refresh: Arc<CatalogRefresh>,
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    git: Option<SharedGitSource>,
}

impl CursorAdapter {
    pub fn new(
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
        hooks: CursorAppHooks,
    ) -> Self {
        let store = hooks.store.unwrap_or_else(|| Arc::new(NoCursorStore));
        let reader = StoreReader::new(store, children.has_headless_child_backend());
        Self {
            sessions: CursorSessions::new(
                children.clone(),
                spawner.clone(),
                catalog.clone(),
                reader,
                hooks.session_options,
            ),
            text: TextRunner::new(children.clone(), spawner.clone()),
            catalog_refresh: Arc::new(CatalogRefresh::default()),
            children,
            spawner,
            catalog,
            git: hooks.git,
        }
    }

    /// The live session state, for code that drives it directly.
    pub fn sessions(&self) -> &Arc<CursorSessions> {
        &self.sessions
    }

    /// The isolated text runner.
    pub fn text(&self) -> &Arc<TextRunner> {
        &self.text
    }
}

impl HarnessAdapter for CursorAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Cursor
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            respond_question: true,
            refresh_catalog: true,
            generate_title: true,
            generate_commit_message: true,
            generate_pr_content: true,
            generate_branch_name: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
            ..AdapterCapabilities::default()
        }
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.send_cursor_turn(input, on_event).await })
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.steer_cursor_turn(input).await })
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.cancel_cursor_turn(&session_id).await })
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.sessions
            .respond_cursor_approval(session_id, request_id, decision);
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        self.sessions
            .respond_cursor_question(session_id, request_id, reply);
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.stop_cursor_session(&session_id).await })
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.forget_cursor_session(&session_id).await })
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        _provider_account_id: Option<&str>,
    ) {
        self.sessions
            .bind_cursor_session(thread_id, provider_session_id, cwd);
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

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        Box::pin(async move { Ok(generate_cursor_session_title(&self.text, &input).await) })
    }

    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            generate_cursor_commit_message(&self.text, self.git.as_ref(), &cwd, signal).await
        })
    }

    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        Box::pin(
            async move { generate_cursor_pr_content(&self.text, self.git.as_ref(), &cwd).await },
        )
    }

    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        Box::pin(async move { Ok(generate_cursor_branch_name(&self.text, &cwd, &message).await) })
    }

    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.text.warmup_cursor_text(&cwd).await;
            Ok(())
        })
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.text.run_cursor_text_prompt(input).await })
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.text.stop_cursor_text_prompt(None).await;
            Ok(())
        })
    }
}

/// `ensureCursorRegistered`, with no Cursor store or git source.
pub fn register(ctx: &HarnessContext) {
    register_with(ctx, CursorAppHooks::default());
}

/// `ensureCursorRegistered` with the app's Cursor store and git source.
/// Idempotent: a second call keeps the live adapter and its sessions.
pub fn register_with(ctx: &HarnessContext, hooks: CursorAppHooks) {
    if ctx.registry.is_registered(HarnessId::Cursor) {
        return;
    }
    let adapter = CursorAdapter::new(
        ctx.children.clone(),
        ctx.spawner.clone(),
        ctx.catalog.clone(),
        hooks,
    );
    ctx.registry.register_harness(Arc::new(adapter));
}
