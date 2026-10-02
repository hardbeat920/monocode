//! Port of src/integrations/harness/providers/pi/pi.ts and piAdapter.ts: the
//! `HarnessAdapter` that drives `pi --mode rpc`, and its registration.
//!
//! Pi and omp share this adapter type; a [`PiFlavor`] picks the CLI. Live Pi
//! sessions load the user's config and extensions (no `--no-extensions`), so
//! todos and subagent packages in `~/.pi/agent` keep working. TUI-only
//! widgets do not appear in MonoCode.

use std::sync::Arc;

use anyhow::Result;

use monocode_core::HarnessId;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, RewindLastTurnInput, RewindLastTurnResult,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;

use crate::core::child::Children;
use crate::core::native_commands::{CommandContext, NativeCommand, NativeCommandProvider};
use crate::core::register::HarnessContext;
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, HarnessAdapter, TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::BoxFuture;

use super::catalog::PiCatalog;
use super::family::PiFamily;
use super::flavor::{PI_FLAVOR, PiFlavor};
use super::skills::discover_pi_skills;
use super::text::PiText;
use super::title::generate_session_title;

/// `piAdapter` and `ompAdapter`.
pub struct PiFamilyAdapter {
    flavor: PiFlavor,
    family: PiFamily,
    text: PiText,
    catalog: PiCatalog,
    commands: Arc<dyn NativeCommandProvider>,
}

impl PiFamilyAdapter {
    pub fn new(flavor: PiFlavor, ctx: &HarnessContext) -> Self {
        let family = PiFamily::new(flavor, ctx.children.clone(), ctx.catalog.clone());
        let commands: Arc<dyn NativeCommandProvider> = if flavor.is_omp() {
            family.command_provider()
        } else {
            Arc::new(PiSkillsProvider {
                children: ctx.children.clone(),
            })
        };
        Self {
            flavor,
            text: PiText::new(flavor, ctx.children.clone(), ctx.catalog.clone()),
            catalog: PiCatalog::new(flavor, ctx.children.clone(), ctx.catalog.clone()),
            family,
            commands,
        }
    }

    /// The session core, for tests and diagnostics.
    pub fn family(&self) -> &PiFamily {
        &self.family
    }
}

impl HarnessAdapter for PiFamilyAdapter {
    fn id(&self) -> HarnessId {
        self.flavor.id
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            rewind_last_turn: true,
            respond_question: self.flavor.is_omp(),
            refresh_catalog: true,
            generate_title: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
            ..AdapterCapabilities::default()
        }
    }

    fn commands(&self) -> Option<Arc<dyn NativeCommandProvider>> {
        Some(self.commands.clone())
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.family.send_turn(input, on_event, on_accepted))
    }

    fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.family.compact_context(input, on_event))
    }

    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        Box::pin(self.family.rewind_last_turn(input, on_event))
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(self.family.steer_turn(input))
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.family.cancel_turn(&session_id).await })
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.family
            .respond_approval(session_id, request_id, decision);
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        if self.flavor.is_omp() {
            self.family.respond_question(session_id, request_id, reply);
        }
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.family.stop_session(&session_id).await })
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.family.forget_session(&session_id).await })
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        _provider_account_id: Option<&str>,
    ) {
        self.family
            .bind_session(thread_id, provider_session_id, cwd);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        let refresh = self.catalog.refresh_catalog();
        Box::pin(async move {
            refresh.await;
            Ok(())
        })
    }

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        Box::pin(async move { Ok(generate_session_title(&self.text, input).await) })
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

/// Pi's `commands`: skills from a sessionless probe in the project.
struct PiSkillsProvider {
    children: Children,
}

impl NativeCommandProvider for PiSkillsProvider {
    fn discover(&self, context: CommandContext) -> BoxFuture<'_, Result<Vec<NativeCommand>>> {
        Box::pin(async move { discover_pi_skills(&self.children, &context.cwd).await })
    }
}

/// `ensurePiRegistered`. Idempotent: a second call keeps the live adapter.
pub fn register_pi(ctx: &HarnessContext) {
    if ctx.registry.is_registered(HarnessId::Pi) {
        return;
    }
    ctx.registry
        .register_harness(Arc::new(PiFamilyAdapter::new(PI_FLAVOR, ctx)));
}

#[cfg(test)]
mod tests {
    use super::super::register;
    use super::super::testing::Fake;
    use super::*;

    #[test]
    fn registers_pi_and_omp_once() {
        let fake = Fake::new();
        let ctx = fake.context();
        register(&ctx);
        let pi = ctx.registry.get_harness(HarnessId::Pi).unwrap();
        let omp = ctx.registry.get_harness(HarnessId::Omp).unwrap();
        register(&ctx);
        assert!(Arc::ptr_eq(
            &pi,
            &ctx.registry.get_harness(HarnessId::Pi).unwrap()
        ));
        assert!(Arc::ptr_eq(
            &omp,
            &ctx.registry.get_harness(HarnessId::Omp).unwrap()
        ));
        assert!(pi.capabilities().compact_context && pi.capabilities().run_text_prompt);
        assert!(!pi.capabilities().respond_question);
        assert!(omp.capabilities().respond_question);
        assert!(omp.commands().unwrap().raw_slash_commands());
        assert!(!pi.commands().unwrap().raw_slash_commands());
    }
}
