//! New sessions from the live model catalog and the stored model settings.
//! `ModelEnvSessions` in the workspace package holds owned copies; this
//! factory reads the current catalog, preferences, availability, and
//! project defaults each time it builds a session, so "+" picks the model
//! the user chose last.

use monocode_core::block::ModelSettings;
use monocode_core::models::{ModelEnv, ModelPrefs};
use monocode_core::project_providers::{PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviders};
use monocode_core::{HarnessId, RuntimeMode, Session};
use monocode_engine::workspace::{ModelEnvSessions, SessionFactory};
use monocode_harness::HarnessAvailabilityStore;
use monocode_harness::core::catalog::SharedCatalog;
use monocode_settings::Kv;

/// A [`SessionFactory`] over the app's settings and catalog.
pub struct AppSessionFactory {
    kv: Kv,
    catalog: SharedCatalog,
    availability: HarnessAvailabilityStore,
    /// The settings at boot. `env` must return borrowed values, and only
    /// the workspace snapshot hydration at boot reads it.
    boot: ModelEnvSessions,
}

impl AppSessionFactory {
    pub fn new(kv: Kv, catalog: SharedCatalog, availability: HarnessAvailabilityStore) -> Self {
        let mut factory = Self {
            kv,
            catalog,
            availability,
            boot: ModelEnvSessions::default(),
        };
        factory.boot = factory.snapshot();
        factory
    }

    /// The model environment as it is now.
    pub fn snapshot(&self) -> ModelEnvSessions {
        let get = |key: &str| self.kv.get_item(key);
        ModelEnvSessions {
            catalog: self.catalog.snapshot(),
            prefs: ModelPrefs::from_local_storage(get),
            availability: self.availability.snapshot(),
            projects: ProjectProviders::parse(get(PROJECT_PROVIDER_SETTINGS_KEY).as_deref()),
        }
    }
}

impl SessionFactory for AppSessionFactory {
    fn env(&self) -> ModelEnv<'_> {
        self.boot.env()
    }

    fn new_session(
        &self,
        harness: HarnessId,
        cwd: &str,
        model: Option<&str>,
        runtime_mode: Option<RuntimeMode>,
        model_settings: Option<&ModelSettings>,
    ) -> Session {
        self.snapshot()
            .new_session(harness, cwd, model, runtime_mode, model_settings)
    }

    fn new_default_session(&self, cwd: &str, runtime_mode: Option<RuntimeMode>) -> Session {
        self.snapshot().new_default_session(cwd, runtime_mode)
    }

    fn new_session_like(&self, seed: Option<&Session>, cwd: &str) -> Session {
        self.snapshot().new_session_like(seed, cwd)
    }
}
