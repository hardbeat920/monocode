//! Port of src/integrations/harness/providers/claude/claudeAdapter.ts: the
//! Claude Code `HarnessAdapter` and its registration.

use std::sync::Arc;

use anyhow::Result;
use futures::future::BoxFuture;
use monocode_core::block::TaskListMeta;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, SendTurnInput, SteerTurnInput,
};
use monocode_core::models::AgentModel;
use monocode_core::user_question::UserQuestionReply;

use crate::core::register::HarnessContext;
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, SharedSpawner};

use super::catalog::ClaudeCatalog;
use super::git::{
    SharedGitSource, generate_claude_branch_name, generate_claude_commit_message,
    generate_claude_pr_content,
};
use super::io::{ChildrenIo, SharedChildIo};
use super::session::{ClaudeSessionOptions, ClaudeSessions};
use super::text::ClaudeText;
use super::title::generate_claude_session_title;

/// What the app supplies that [`HarnessContext`] does not carry: the
/// `monocode.claudeHooks` setting and the git reads behind commit and pull
/// request text.
#[derive(Clone, Default)]
pub struct ClaudeAppHooks {
    /// `loadClaudeHooks()`. `None` reads as the setting's default (on).
    pub claude_hooks: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// `gitStagedContext` and `gitRangeContext`. Without it, commit message
    /// and pull request generation fail with "Git context is not available".
    pub git: Option<SharedGitSource>,
}

/// The Claude Code adapter (`claudeAdapter`).
pub struct ClaudeAdapter {
    sessions: ClaudeSessions,
    text: ClaudeText,
    catalog: ClaudeCatalog,
    git: Option<SharedGitSource>,
}

/// The pieces a [`ClaudeAdapter`] is built from. `register` fills them from a
/// [`HarnessContext`]; tests pass a scripted child IO.
pub struct ClaudeAdapterParts {
    pub io: SharedChildIo,
    pub spawner: SharedSpawner,
    pub session_options: ClaudeSessionOptions,
    /// `modelsFor("claude")`, for the text runner's model choice.
    pub models: Arc<dyn Fn() -> Vec<AgentModel> + Send + Sync>,
    /// `setHarnessModels("claude", models)`.
    pub set_models: Arc<dyn Fn(Vec<AgentModel>) + Send + Sync>,
    pub git: Option<SharedGitSource>,
}

impl ClaudeAdapter {
    pub fn new(parts: ClaudeAdapterParts) -> Self {
        Self {
            sessions: ClaudeSessions::new(
                parts.io.clone(),
                parts.spawner.clone(),
                parts.session_options,
            ),
            text: ClaudeText::new(parts.io.clone(), parts.models),
            catalog: ClaudeCatalog::new(parts.io, parts.spawner, parts.set_models),
            git: parts.git,
        }
    }

    /// The live sessions, for callers that need more than the trait.
    pub fn sessions(&self) -> &ClaudeSessions {
        &self.sessions
    }

    pub fn text(&self) -> &ClaudeText {
        &self.text
    }

    pub fn catalog(&self) -> &ClaudeCatalog {
        &self.catalog
    }
}

/// `ensureClaudeRegistered` with the app's settings defaults.
pub fn register(ctx: &HarnessContext) {
    register_with(ctx, ClaudeAppHooks::default());
}

/// `ensureClaudeRegistered`. Idempotent: a second call keeps the live adapter.
pub fn register_with(ctx: &HarnessContext, hooks: ClaudeAppHooks) {
    if ctx.registry.is_registered(HarnessId::Claude) {
        return;
    }
    let mut session_options = ClaudeSessionOptions::default();
    if let Some(claude_hooks) = hooks.claude_hooks {
        session_options.claude_hooks = claude_hooks;
    }
    let catalog = ctx.catalog.clone();
    session_options.native_model_id =
        Arc::new(move |model| catalog.read().native_model_id_for(model));
    let models = {
        let catalog = ctx.catalog.clone();
        Arc::new(move || catalog.read().models_for(HarnessId::Claude).to_vec())
    };
    let set_models = {
        let catalog = ctx.catalog.clone();
        Arc::new(move |models| catalog.set_harness_models(HarnessId::Claude, models))
    };
    let adapter = ClaudeAdapter::new(ClaudeAdapterParts {
        io: Arc::new(ChildrenIo::new(ctx.children.clone())),
        spawner: ctx.spawner.clone(),
        session_options,
        models,
        set_models,
        git: hooks.git,
    });
    ctx.registry.register_harness(Arc::new(adapter));
}

impl HarnessAdapter for ClaudeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Claude
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            rewind_last_turn: false,
            respond_question: true,
            keep_question_open: false,
            restore_task_lists: true,
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
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.sessions.send_turn(input, on_event))
    }

    fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.sessions.compact_context(input, on_event))
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

    fn restore_task_lists(&self, thread_id: &str, lists: Vec<TaskListMeta>) {
        self.sessions.restore_task_lists(thread_id, &lists);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.catalog.refresh().await;
            Ok(())
        })
    }

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        Box::pin(async move { Ok(generate_claude_session_title(&self.text, input).await) })
    }

    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            generate_claude_commit_message(&self.text, self.git.as_ref(), &cwd, signal).await
        })
    }

    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        Box::pin(
            async move { generate_claude_pr_content(&self.text, self.git.as_ref(), &cwd).await },
        )
    }

    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        Box::pin(async move { Ok(generate_claude_branch_name(&self.text, &cwd, &message).await) })
    }

    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.text.warmup(&cwd).await })
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        Box::pin(self.text.run(input))
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.text.stop())
    }
}
