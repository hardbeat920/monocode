//! Port of host/providers.ts: the provider operations the host engine uses,
//! and the model probes `models.list` runs.
//!
//! The TypeScript imported each provider's functions. Here they are the
//! registered harness adapters, called directly rather than through the
//! registry's queues and idle parking, as the Node host called them.

use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use monocode_core::harness_event::{ApprovalDecision, CompactContextInput, SendTurnInput};
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{AgentModel, HarnessId};
use monocode_harness::core::registry::{EventSink, HarnessAdapter, HarnessRegistry, TitleInput};
use monocode_harness::core::session_title::GeneratedSessionTitle;
use monocode_harness::core::task::SharedSpawner;
use monocode_harness::core::{Children, SharedCatalog};
use monocode_remote::host::protocol::{REMOTE_PROVIDERS, RemoteProvider};

/// A provider call's result. Errors are the message the session shows.
pub type ProviderFuture<T> = BoxFuture<'static, Result<T, String>>;

/// `HostProvider`: what the engine needs from one provider.
pub trait HostProvider: Send + Sync {
    fn send(&self, input: SendTurnInput, on_event: EventSink) -> ProviderFuture<()>;
    /// Whether [`HostProvider::compact`] exists (`provider.compact != null`).
    fn can_compact(&self) -> bool {
        false
    }
    fn compact(&self, _input: CompactContextInput, _on_event: EventSink) -> ProviderFuture<()> {
        async { Err("Context compaction is unavailable for this provider".to_string()) }.boxed()
    }
    fn cancel(&self, id: &str) -> ProviderFuture<()>;
    /// Stops the child and drops its callbacks.
    fn stop(&self, id: &str) -> ProviderFuture<()>;
    /// Keeps the provider conversation for an explicit later follow-up.
    fn bind(&self, id: &str, provider_id: &str, cwd: &str);
    fn approve(&self, id: &str, request: i64, decision: ApprovalDecision) -> Result<(), String>;
    fn answer(&self, id: &str, request: i64, reply: UserQuestionReply) -> Result<(), String>;
    /// Whether [`HostProvider::generate_title`] exists.
    fn can_generate_title(&self) -> bool {
        false
    }
    fn generate_title(&self, _input: TitleInput) -> ProviderFuture<Option<GeneratedSessionTitle>> {
        async { Ok(None) }.boxed()
    }
    /// Whether [`HostProvider::generate_branch_name`] exists.
    fn can_generate_branch_name(&self) -> bool {
        false
    }
    fn generate_branch_name(&self, _cwd: &str, _message: &str) -> ProviderFuture<Option<String>> {
        async { Ok(None) }.boxed()
    }
}

/// The providers in the TypeScript table that offered each optional
/// operation.
fn compacts(id: HarnessId) -> bool {
    use HarnessId::*;
    matches!(id, Codex | Claude | Grok | Opencode | Pi | Omp)
}

fn answers_questions(id: HarnessId) -> bool {
    use HarnessId::*;
    !matches!(id, Fx | Hermes | Droid | Antigravity)
}

fn titles(id: HarnessId) -> bool {
    use HarnessId::*;
    matches!(id, Codex | Claude | Cursor | Grok | Opencode | Pi | Omp)
}

fn names_branches(id: HarnessId) -> bool {
    matches!(id, HarnessId::Codex | HarnessId::Claude)
}

fn message(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/// A registered adapter as a [`HostProvider`].
pub struct AdapterProvider {
    adapter: Arc<dyn HarnessAdapter>,
}

impl AdapterProvider {
    pub fn new(adapter: Arc<dyn HarnessAdapter>) -> Self {
        Self { adapter }
    }
}

impl HostProvider for AdapterProvider {
    fn send(&self, input: SendTurnInput, on_event: EventSink) -> ProviderFuture<()> {
        let adapter = self.adapter.clone();
        async move {
            adapter
                .send_turn(input, on_event, None)
                .await
                .map_err(message)
        }
        .boxed()
    }

    fn can_compact(&self) -> bool {
        compacts(self.adapter.id()) && self.adapter.capabilities().compact_context
    }

    fn compact(&self, input: CompactContextInput, on_event: EventSink) -> ProviderFuture<()> {
        let adapter = self.adapter.clone();
        async move {
            adapter
                .compact_context(input, on_event)
                .await
                .map_err(message)
        }
        .boxed()
    }

    fn cancel(&self, id: &str) -> ProviderFuture<()> {
        let (adapter, id) = (self.adapter.clone(), id.to_string());
        async move { adapter.cancel_turn(id).await.map_err(message) }.boxed()
    }

    fn stop(&self, id: &str) -> ProviderFuture<()> {
        let (adapter, id) = (self.adapter.clone(), id.to_string());
        async move { adapter.forget_session(id).await.map_err(message) }.boxed()
    }

    fn bind(&self, id: &str, provider_id: &str, cwd: &str) {
        self.adapter.bind_session(id, provider_id, cwd, None);
    }

    fn approve(&self, id: &str, request: i64, decision: ApprovalDecision) -> Result<(), String> {
        self.adapter.respond_approval(id, request, decision);
        Ok(())
    }

    fn answer(&self, id: &str, request: i64, reply: UserQuestionReply) -> Result<(), String> {
        if !answers_questions(self.adapter.id()) {
            return Err("This provider does not support questions".into());
        }
        self.adapter.respond_question(id, request, reply);
        Ok(())
    }

    fn can_generate_title(&self) -> bool {
        titles(self.adapter.id()) && self.adapter.capabilities().generate_title
    }

    fn generate_title(&self, input: TitleInput) -> ProviderFuture<Option<GeneratedSessionTitle>> {
        let adapter = self.adapter.clone();
        async move { adapter.generate_title(input).await.map_err(message) }.boxed()
    }

    fn can_generate_branch_name(&self) -> bool {
        names_branches(self.adapter.id()) && self.adapter.capabilities().generate_branch_name
    }

    fn generate_branch_name(&self, cwd: &str, text: &str) -> ProviderFuture<Option<String>> {
        let (adapter, cwd, text) = (self.adapter.clone(), cwd.to_string(), text.to_string());
        async move {
            adapter
                .generate_branch_name(cwd, text)
                .await
                .map_err(message)
        }
        .boxed()
    }
}

/// The engine's provider table.
pub type HostProviders = HashMap<RemoteProvider, Arc<dyn HostProvider>>;

/// `hostProviders`: every remote provider registered in `registry`.
pub fn host_providers(registry: &HarnessRegistry) -> HostProviders {
    REMOTE_PROVIDERS
        .into_iter()
        .filter_map(|id| {
            let adapter = registry.get_harness(id)?;
            Some((
                id,
                Arc::new(AdapterProvider::new(adapter)) as Arc<dyn HostProvider>,
            ))
        })
        .collect()
}

/// `discoverModels[provider](cwd)` from host/server.ts: the provider's live
/// model list, probed in `cwd`.
pub async fn discover_models(
    provider: RemoteProvider,
    cwd: &str,
    children: &Children,
    spawner: &SharedSpawner,
    catalog: &SharedCatalog,
) -> Result<Vec<AgentModel>, String> {
    use monocode_harness::providers::*;
    let cwd = Some(cwd);
    let result = match provider {
        HarnessId::Codex => {
            codex::catalog::CodexCatalog::new(children.clone(), spawner.clone(), catalog.clone())
                .discover_codex_models(cwd)
                .await
        }
        HarnessId::Claude => {
            let catalog = catalog.clone();
            claude::catalog::ClaudeCatalog::new(
                Arc::new(claude::io::ChildrenIo::new(children.clone())),
                spawner.clone(),
                Arc::new(move |models| catalog.set_harness_models(HarnessId::Claude, models)),
            )
            .discover(cwd)
            .await
        }
        HarnessId::Cursor => cursor::catalog::discover_cursor_models(children, spawner, cwd).await,
        HarnessId::Grok => Ok(grok::catalog::discover_grok_models(children, spawner, cwd).await),
        HarnessId::Opencode => opencode::catalog::discover_open_code_models(children, cwd).await,
        HarnessId::Pi => pi::catalog::discover_models(children, &pi::PI_FLAVOR, cwd).await,
        HarnessId::Omp => pi::catalog::discover_models(children, &pi::OMP_FLAVOR, cwd).await,
        HarnessId::Fx => fx::catalog::discover_fx_models(children, cwd).await,
        HarnessId::Hermes => hermes::catalog::discover_hermes_models(children, spawner, cwd).await,
        HarnessId::Droid => {
            droid::catalog::DroidCatalog::new(children.clone(), spawner.clone(), catalog.clone())
                .discover(cwd)
                .await
        }
        HarnessId::Antigravity => {
            antigravity::catalog::discover_antigravity_models(children, spawner, cwd).await
        }
    };
    result.map_err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HARNESSES;
    use monocode_harness::core::registry::RegistryOptions;
    use monocode_harness::core::{HarnessContext, register_builtin_harnesses};

    /// providers.test.ts: "exposes every local harness through the remote
    /// host contract".
    #[test]
    fn exposes_every_local_harness_through_the_remote_host_contract() {
        let runtime = crate::runtime::HostRuntime::new(1);
        let spawner = runtime.spawner();
        let registry = HarnessRegistry::new(spawner.clone(), RegistryOptions::default());
        let directory = tempfile::tempdir().unwrap();
        let (children, _backend) = crate::child_backend::host_children(
            directory.path().to_path_buf(),
            HashMap::new(),
            spawner,
        );
        register_builtin_harnesses(&HarnessContext::new(
            registry.clone(),
            children,
            SharedCatalog::new(),
        ));
        let providers = host_providers(&registry);
        let mut ids: Vec<&str> = providers.keys().map(|id| id.as_str()).collect();
        ids.sort();
        let mut expected: Vec<&str> = REMOTE_PROVIDERS.iter().map(|id| id.as_str()).collect();
        expected.sort();
        assert_eq!(ids, expected);
        let mut harnesses: Vec<&str> = HARNESSES.iter().map(|id| id.as_str()).collect();
        harnesses.sort();
        assert_eq!(expected, harnesses);
        let descriptor =
            monocode_remote::host::protocol::require_host_descriptor(&serde_json::json!({
                "protocolVersion": 1,
                "environmentId": "host",
                "name": "fixture",
                "providers": REMOTE_PROVIDERS,
                "capabilities": [],
            }))
            .unwrap();
        assert_eq!(descriptor.providers.len(), REMOTE_PROVIDERS.len());
        assert!(providers[&HarnessId::Claude].can_compact());
        assert!(!providers[&HarnessId::Cursor].can_compact());
        assert!(providers[&HarnessId::Codex].can_generate_branch_name());
        assert!(!providers[&HarnessId::Cursor].can_generate_branch_name());
        assert_eq!(
            providers[&HarnessId::Fx].answer("session", 1, UserQuestionReply::Skipped),
            Err("This provider does not support questions".into())
        );
        runtime.shutdown();
    }
}
