//! Port of src/integrations/harness/providers/codex/codexAdapter.ts: the
//! Codex `HarnessAdapter` and its registration.

use std::sync::Arc;

use anyhow::Result;

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, RewindLastTurnInput, RewindLastTurnResult,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;

use crate::core::register::HarnessContext;
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture};

use super::catalog::CodexCatalog;
use super::git::{
    GitContexts, generate_codex_branch_name, generate_codex_commit_message,
    generate_codex_pr_content,
};
use super::session::{CodexSessions, GeneratedImages, SessionOptions};
use super::text::CodexText;
use super::title::generate_codex_session_title;

/// Host services the TypeScript reached through Tauri commands in
/// src/platform/tauri/fs.ts. Without them, generated images become session
/// errors and git text generation fails with "Git context is not available".
#[derive(Clone, Default)]
pub struct CodexHost {
    pub images: Option<Arc<dyn GeneratedImages>>,
    pub git: Option<Arc<dyn GitContexts>>,
}

/// `codexAdapter`.
pub struct CodexAdapter {
    sessions: CodexSessions,
    text: CodexText,
    catalog: CodexCatalog,
    git: Option<Arc<dyn GitContexts>>,
}

impl CodexAdapter {
    pub fn new(ctx: &HarnessContext, host: CodexHost) -> Self {
        Self::with_options(
            ctx,
            SessionOptions {
                images: host.images,
                ..Default::default()
            },
            host.git,
        )
    }

    /// An adapter with explicit session options (clock, question deadline).
    pub fn with_options(
        ctx: &HarnessContext,
        options: SessionOptions,
        git: Option<Arc<dyn GitContexts>>,
    ) -> Self {
        Self {
            sessions: CodexSessions::new(
                ctx.children.clone(),
                ctx.spawner.clone(),
                ctx.catalog.clone(),
                options,
            ),
            text: CodexText::new(
                ctx.children.clone(),
                ctx.spawner.clone(),
                ctx.catalog.clone(),
            ),
            catalog: CodexCatalog::new(
                ctx.children.clone(),
                ctx.spawner.clone(),
                ctx.catalog.clone(),
            ),
            git,
        }
    }

    /// The live sessions behind this adapter.
    pub fn sessions(&self) -> &CodexSessions {
        &self.sessions
    }

    /// The shared text runner behind titles and git text.
    pub fn text(&self) -> &CodexText {
        &self.text
    }

    /// Model discovery.
    pub fn catalog(&self) -> &CodexCatalog {
        &self.catalog
    }
}

impl HarnessAdapter for CodexAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            rewind_last_turn: true,
            respond_question: true,
            keep_question_open: true,
            restore_task_lists: false,
            refresh_catalog: true,
            generate_title: true,
            generate_commit_message: true,
            generate_pr_content: true,
            generate_branch_name: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
        }
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.sessions.send_turn(input, on_event, on_accepted))
    }

    fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.sessions.compact_context(input, on_event))
    }

    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        Box::pin(self.sessions.rewind_last_turn(input, on_event))
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.sessions.steer_turn(input))
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.cancel_turn(&session_id).await })
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.sessions
            .respond_approval(session_id, request_id, decision);
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        self.sessions
            .respond_question(session_id, request_id, reply);
    }

    fn keep_question_open(&self, session_id: &str, request_id: i64) {
        self.sessions.keep_question_open(session_id, request_id);
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.stop_session(&session_id).await })
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.sessions.forget_session(&session_id).await })
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
    ) {
        self.sessions
            .bind_session(thread_id, provider_session_id, cwd, provider_account_id);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.catalog.refresh())
    }

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        Box::pin(async move { Ok(generate_codex_session_title(&self.text, &input).await) })
    }

    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            generate_codex_commit_message(&self.text, self.git.as_ref(), &cwd, signal).await
        })
    }

    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        Box::pin(
            async move { generate_codex_pr_content(&self.text, self.git.as_ref(), &cwd).await },
        )
    }

    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        Box::pin(async move { generate_codex_branch_name(&self.text, &cwd, &message).await })
    }

    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.text.warmup_text(&cwd).await })
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        Box::pin(self.text.run_text_prompt(input))
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.text.stop_text_prompt())
    }
}

/// `ensureCodexRegistered`, with no host services. See [`register_with`].
pub fn register(ctx: &HarnessContext) {
    register_with(ctx, CodexHost::default());
}

/// Register the Codex adapter with the app's image store and git context.
/// A second call keeps the live adapter and its sessions.
pub fn register_with(ctx: &HarnessContext, host: CodexHost) {
    if ctx.registry.is_registered(HarnessId::Codex) {
        return;
    }
    ctx.registry
        .register_harness(Arc::new(CodexAdapter::new(ctx, host)));
}
