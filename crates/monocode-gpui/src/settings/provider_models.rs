//! Settings owns account-scoped catalogs, even when no chat is open.
use super::SettingsView;
use gpui::{Context, Task};
use monocode_core::{accounts::Profile, catalog, provider::Kind};
use std::path::PathBuf;

pub(super) struct Catalog {
    pub profile: Profile,
    pub binary: PathBuf,
    pub generation: u64,
    pub models: Vec<(String, String)>,
    pub loading: bool,
    pub error: Option<String>,
    pub task: Option<Task<()>>,
}

impl SettingsView {
    pub(super) fn load_provider_models(&mut self, kind: Kind, retry: bool, cx: &mut Context<Self>) {
        let Some(binary) = self
            .discovery
            .binaries
            .iter()
            .find_map(|(k, binary)| (*k == kind).then(|| binary.clone()).flatten())
        else {
            self.provider_models.remove(&kind);
            return;
        };
        let project = if self.provider_scope == "global" {
            ""
        } else {
            &self.provider_scope
        };
        let profile = crate::accounts::get(cx)
            .map(|center| center.read(cx).selected(kind, project))
            .unwrap_or_default();
        let previous = self.provider_models.get(&kind);
        if !retry && previous.is_some_and(|p| p.profile == profile && p.binary == binary) {
            return;
        }
        if retry || previous.is_some_and(|p| p.binary != binary) {
            catalog::invalidate(kind, &profile);
        }
        let models = catalog::cached_models_for(kind, &profile)
            .unwrap_or_default()
            .into_iter()
            .map(|model| (model.id, model.name))
            .collect();
        self.provider_models_generation += 1;
        let generation = self.provider_models_generation;
        self.provider_models.insert(
            kind,
            Catalog {
                profile: profile.clone(),
                binary,
                generation,
                models,
                loading: true,
                error: None,
                task: None,
            },
        );
        if self.select_open == Some(kind.id()) {
            self.select_open = None;
        }
        // UI tests complete requests explicitly instead of launching installed CLIs.
        if cfg!(test) {
            return;
        }
        let work = cx.background_executor().spawn(async move {
            catalog::models_for(kind, &profile).map(|models| {
                models
                    .into_iter()
                    .map(|model| (model.id, model.name))
                    .collect()
            })
        });
        let task = cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_models_finished(kind, generation, result, cx);
            });
        });
        self.provider_models.get_mut(&kind).unwrap().task = Some(task);
    }

    pub(super) fn provider_models_finished(
        &mut self,
        kind: Kind,
        generation: u64,
        result: Result<Vec<(String, String)>, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self
            .provider_models
            .get_mut(&kind)
            .filter(|state| state.generation == generation)
        else {
            return;
        };
        state.loading = false;
        match result {
            Ok(models) => {
                state.models = models;
                state.error = None;
            }
            Err(error) => state.error = Some(error),
        }
        cx.notify();
    }
}
