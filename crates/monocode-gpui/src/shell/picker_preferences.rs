//! Keep remembered picker/file choices in the native workspace sidecar.
use super::*;

impl Shell {
    pub(super) fn observe_picker_preferences(&mut self, cx: &mut Context<Self>) {
        let observer = cx.observe_global::<crate::settings::Preferences>(|this, cx| {
            let prefs = cx.global::<crate::settings::Preferences>();
            let saved = &mut this.workspace.preferences;
            if saved.model_picker_tab == prefs.model_picker_tab
                && saved.favorite_models == prefs.favorite_models
                && saved.recent_models == prefs.recent_models
                && saved.file_view_mode == prefs.file_view_mode
                && saved.default_access == prefs.default_access
                && saved.provider_efforts == prefs.provider_efforts
            {
                return;
            }
            saved.model_picker_tab = prefs.model_picker_tab.clone();
            saved.favorite_models = prefs.favorite_models.clone();
            saved.recent_models = prefs.recent_models.clone();
            saved.file_view_mode = prefs.file_view_mode.clone();
            saved.default_access = prefs.default_access.clone();
            saved.provider_efforts = prefs.provider_efforts.clone();
            this.sync_settings(cx);
            this.persist(cx);
        });
        self.observations.push(observer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::store::Store;

    #[gpui::test]
    fn favorite_models_reach_the_workspace_and_the_saved_state(cx: &mut TestAppContext) {
        let store = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/work/monocode").unwrap();
        let summaries = store.sessions().unwrap();
        let mut state = Workspace::default();
        state.open(summary.id);
        let saved = serde_json::to_string(&state).unwrap();
        let database = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    Shell::new(
                        database.clone(),
                        summaries,
                        "/work/monocode".into(),
                        Some(saved),
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        let favorite = crate::settings::ModelChoice {
            provider: "hermes".into(),
            model: Some("hermes:example-vendor:example-model".into()),
        };
        let stored = |favorites: &[crate::settings::ModelChoice]| {
            let state = locked(&database).unwrap().ui_state().unwrap().unwrap();
            let state: Workspace = serde_json::from_str(&state).unwrap();
            assert_eq!(state.preferences.favorite_models, favorites);
        };
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.favorite_models.push(favorite.clone())
            })
        });
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert_eq!(
                    shell.workspace.preferences.favorite_models,
                    std::slice::from_ref(&favorite)
                )
            })
            .unwrap();
        stored(std::slice::from_ref(&favorite));
        // Removing the last favorite is saved too.
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.favorite_models.clear()
            })
        });
        cx.run_until_parked();
        window
            .update(cx, |shell, _, _| {
                assert!(shell.workspace.preferences.favorite_models.is_empty())
            })
            .unwrap();
        stored(&[]);
    }
}
