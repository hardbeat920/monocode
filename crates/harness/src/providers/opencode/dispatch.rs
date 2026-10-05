use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, RewindLastTurnInput, RewindLastTurnResult,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;
use parking_lot::Mutex;

use super::adapter::OpenCodeAdapter as V1;
use super::git::SharedGitSource;
use super::v2::adapter::Adapter as V2;
use super::v2::protocol::{MajorVersion, version};
use super::v2::server::Options;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, Children};
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture, SharedSpawner};

/// Select the CLI's protocol once per resolved binary path.
#[derive(Clone)]
pub struct OpenCodeAdapter {
    inner: Arc<Inner>,
}

struct Inner {
    children: Children,
    one: V1,
    two: V2,
    version: smol::lock::Mutex<Option<(String, MajorVersion)>>,
    active: Mutex<HashMap<String, MajorVersion>>,
}

impl OpenCodeAdapter {
    pub fn new(
        children: Children,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        git: Option<SharedGitSource>,
    ) -> Self {
        Self::with_options(children, catalog, spawner, git, Options::default())
    }

    pub fn with_options(
        children: Children,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        git: Option<SharedGitSource>,
        options: Options,
    ) -> Self {
        let one = V1::new(
            children.clone(),
            catalog.clone(),
            spawner.clone(),
            git.clone(),
        );
        let two = V2::with_options(children.clone(), catalog, spawner, git, options);
        Self {
            inner: Arc::new(Inner {
                children,
                one,
                two,
                version: smol::lock::Mutex::new(None),
                active: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn adapter(&self, major: MajorVersion) -> &dyn HarnessAdapter {
        match major {
            MajorVersion::One => &self.inner.one,
            MajorVersion::Two => &self.inner.two,
        }
    }

    async fn select(&self) -> Result<MajorVersion> {
        let binary = self.inner.children.resolve_open_code_binary().await?;
        let mut cache = self.inner.version.lock().await;
        if let Some((path, version)) = &*cache
            && *path == binary.path
        {
            return Ok(*version);
        }
        let output = self
            .inner
            .children
            .exec_child(
                &binary.path,
                vec!["--version".into()],
                None,
                Some(HarnessId::Opencode),
                BinaryPathChoice::Runtime,
            )
            .await?;
        let major = version(&output)?;
        *cache = Some((binary.path, major));
        Ok(major)
    }

    async fn for_thread(&self, thread: &str) -> Result<&dyn HarnessAdapter> {
        let major = self.select().await?;
        let previous = self.inner.active.lock().insert(thread.into(), major);
        if let Some(previous) = previous.filter(|previous| *previous != major) {
            self.adapter(previous).stop_session(thread.into()).await?;
        }
        Ok(self.adapter(major))
    }

    fn active(&self, thread: &str) -> Option<&dyn HarnessAdapter> {
        self.inner
            .active
            .lock()
            .get(thread)
            .copied()
            .map(|major| self.adapter(major))
    }
}

impl HarnessAdapter for OpenCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Opencode
    }
    fn capabilities(&self) -> AdapterCapabilities {
        self.inner.one.capabilities()
    }
    fn send_turn(
        &self,
        input: SendTurnInput,
        sink: EventSink,
        accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        async move {
            self.for_thread(&input.session.session_id)
                .await?
                .send_turn(input, sink, accepted)
                .await
        }
        .boxed()
    }
    fn compact_context(
        &self,
        input: CompactContextInput,
        sink: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        async move {
            self.for_thread(&input.session_id)
                .await?
                .compact_context(input, sink)
                .await
        }
        .boxed()
    }
    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        sink: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        async move {
            self.for_thread(&input.session.session_id)
                .await?
                .rewind_last_turn(input, sink)
                .await
        }
        .boxed()
    }
    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        async move {
            self.for_thread(&input.session_id)
                .await?
                .steer_turn(input)
                .await
        }
        .boxed()
    }
    fn cancel_turn(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            if let Some(adapter) = self.active(&thread) {
                adapter.cancel_turn(thread).await
            } else {
                self.for_thread(&thread).await?.cancel_turn(thread).await
            }
        }
        .boxed()
    }
    fn respond_approval(&self, thread: &str, request: i64, decision: ApprovalDecision) {
        if let Some(adapter) = self.active(thread) {
            adapter.respond_approval(thread, request, decision);
        }
    }
    fn respond_question(&self, thread: &str, request: i64, reply: UserQuestionReply) {
        if let Some(adapter) = self.active(thread) {
            adapter.respond_question(thread, request, reply);
        }
    }
    fn stop_session(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            if let Some(adapter) = self.active(&thread) {
                adapter.stop_session(thread).await?;
            }
            Ok(())
        }
        .boxed()
    }
    fn forget_session(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            self.inner.two.forget_session(thread.clone()).await?;
            self.inner.one.forget_session(thread.clone()).await?;
            self.inner.active.lock().remove(&thread);
            Ok(())
        }
        .boxed()
    }
    fn bind_session(&self, thread: &str, provider: &str, cwd: &str, account: Option<&str>) {
        self.inner.one.bind_session(thread, provider, cwd, account);
        self.inner.two.bind_session(thread, provider, cwd, account);
    }
    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        async move { self.adapter(self.select().await?).refresh_catalog().await }.boxed()
    }
    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        async move {
            self.adapter(self.select().await?)
                .generate_title(input)
                .await
        }
        .boxed()
    }
    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        async move {
            self.adapter(self.select().await?)
                .generate_commit_message(cwd, signal)
                .await
        }
        .boxed()
    }
    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        async move {
            self.adapter(self.select().await?)
                .generate_pr_content(cwd)
                .await
        }
        .boxed()
    }
    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        async move {
            self.adapter(self.select().await?)
                .generate_branch_name(cwd, message)
                .await
        }
        .boxed()
    }
    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        async move { self.adapter(self.select().await?).warmup_text(cwd).await }.boxed()
    }
    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        async move {
            self.adapter(self.select().await?)
                .run_text_prompt(input)
                .await
        }
        .boxed()
    }
    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        async move {
            self.inner.two.stop_text_prompt().await?;
            self.inner.one.stop_text_prompt().await
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::ignore_events;
    use crate::providers::opencode::test_support::{FakeHost, path_of, wait_for};
    use monocode_core::harness::RuntimeMode;
    use monocode_core::harness_event::HarnessSessionInput;
    use serde_json::json;

    fn adapter(host: &FakeHost) -> OpenCodeAdapter {
        OpenCodeAdapter::new(host.children(), SharedCatalog::new(), host.spawner(), None)
    }

    #[test]
    fn caches_the_version_for_the_resolved_binary_and_rechecks_changed_sources() {
        smol::block_on(async {
            let host = FakeHost::v2();
            let adapter = adapter(&host);
            assert_eq!(adapter.select().await.unwrap(), MajorVersion::Two);
            host.set_exec_output("opencode 1.14.19");
            assert_eq!(adapter.select().await.unwrap(), MajorVersion::Two);
            assert_eq!(host.exec_calls().len(), 1);
            host.set_runtime_path("/owned/other/opencode");
            assert_eq!(adapter.select().await.unwrap(), MajorVersion::One);
            assert_eq!(host.exec_calls().len(), 2);
            host.set_runtime_path("/owned/unsupported/opencode");
            host.set_exec_output("opencode 3.0.0");
            assert!(adapter.select().await.is_err());
            assert!(host.spawns().is_empty());
        });
    }

    #[test]
    fn forgetting_a_bound_unsent_session_clears_both_resume_maps() {
        smol::block_on(async {
            let host = FakeHost::v2();
            host.respond_with(|request| {
                match (request.method.as_str(), path_of(&request.url).as_str()) {
                    ("POST", "/api/session") => (200, json!({"data":{"id":"ses_new"}}).to_string()),
                    _ => (204, String::new()),
                }
            });
            let adapter = adapter(&host);
            adapter.bind_session("native_owned", "ses_previous", "/owned/work", None);
            adapter.forget_session("native_owned".into()).await.unwrap();
            let turn = {
                let adapter = adapter.clone();
                smol::spawn(async move {
                    adapter
                        .send_turn(
                            SendTurnInput {
                                session: HarnessSessionInput {
                                    session_id: "native_owned".into(),
                                    cwd: "/owned/work".into(),
                                    model: "opencode:fixture/free".into(),
                                    model_settings: None,
                                    provider_account_id: None,
                                    runtime_mode: RuntimeMode::Supervised,
                                    intent: None,
                                    controls_agents: None,
                                    app_access: None,
                                },
                                text: "one fixture".into(),
                                attachments: None,
                            },
                            ignore_events(),
                            None,
                        )
                        .await
                })
            };
            wait_for("new provider prompt", || {
                !host.calls_to("/prompt").is_empty()
            })
            .await;
            assert_eq!(
                host.calls_to("/api/session")
                    .iter()
                    .filter(|request| request.method == "POST"
                        && path_of(&request.url) == "/api/session")
                    .count(),
                1
            );
            assert!(
                host.http_calls()
                    .iter()
                    .all(|request| !request.url.contains("ses_previous"))
            );
            let stream = host
                .sse_opens()
                .into_iter()
                .find(|(_, url)| path_of(url) == "/api/event")
                .unwrap()
                .0;
            host.sse(
                &stream,
                json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_new"}}),
            );
            turn.await.unwrap();
            adapter.stop_session("native_owned".into()).await.unwrap();
            assert_eq!(host.watched_children(), 0);
            assert_eq!(host.watched_streams(), 0);
        });
    }
}
