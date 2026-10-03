//! New sessions for the workspace: `newSession`, `newDefaultSession`, and
//! `newSessionLike` from src/features/sessions/model/session.ts, with a
//! fresh id each time (`crypto.randomUUID()`).
//!
//! The constructors in `monocode_core::session` need a `ModelEnv`, which
//! comes from settings the workspace does not own. `SessionFactory` lets
//! the app pass its live catalog and preferences; `ModelEnvSessions` is the
//! default over owned copies of them.

use monocode_core::block::ModelSettings;
use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs};
use monocode_core::project_providers::ProjectProviders;
use monocode_core::session::{new_default_session, new_session, new_session_like};
use monocode_core::{HarnessId, RuntimeMode, Session};

/// Builds new conversations for tabs and panes from the app's model
/// catalog and preferences.
pub trait SessionFactory {
    /// The catalog, preferences, availability, and project defaults.
    fn env(&self) -> ModelEnv<'_>;

    /// `newSession`.
    fn new_session(
        &self,
        harness: HarnessId,
        cwd: &str,
        model: Option<&str>,
        runtime_mode: Option<RuntimeMode>,
        model_settings: Option<&ModelSettings>,
    ) -> Session {
        new_session(
            &self.env(),
            new_id(),
            harness,
            cwd,
            model,
            runtime_mode,
            model_settings,
        )
    }

    /// `newDefaultSession`.
    fn new_default_session(&self, cwd: &str, runtime_mode: Option<RuntimeMode>) -> Session {
        new_default_session(&self.env(), new_id(), cwd, runtime_mode)
    }

    /// `newSessionLike`.
    fn new_session_like(&self, seed: Option<&Session>, cwd: &str) -> Session {
        new_session_like(&self.env(), new_id(), seed, cwd)
    }
}

/// A `SessionFactory` over owned settings.
#[derive(Debug, Clone, Default)]
pub struct ModelEnvSessions {
    pub catalog: ModelCatalog,
    pub prefs: ModelPrefs,
    pub availability: HarnessAvailability,
    pub projects: ProjectProviders,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl SessionFactory for ModelEnvSessions {
    fn env(&self) -> ModelEnv<'_> {
        ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        }
    }
}

/// `newSession(seed?.harness ?? "claude", cwd, seed?.model,
/// seed?.runtimeMode, seed?.modelSettings)`, the replacement App.tsx made
/// when a tab must stay open.
pub fn session_seeded_from(
    factory: &dyn SessionFactory,
    seed: Option<&Session>,
    cwd: &str,
) -> Session {
    factory.new_session(
        seed.map_or(HarnessId::Claude, |seed| seed.harness),
        cwd,
        seed.map(|seed| seed.model.as_str()),
        seed.map(|seed| seed.runtime_mode),
        seed.map(|seed| &seed.model_settings),
    )
}
