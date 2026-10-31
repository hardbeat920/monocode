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
                && saved.last_models == prefs.last_models
                && saved.file_view_mode == prefs.file_view_mode
                && saved.default_access == prefs.default_access
                && saved.provider_efforts == prefs.provider_efforts
                && saved.hidden_models == prefs.hidden_models
                && saved.project_providers == prefs.project_providers
            {
                return;
            }
            saved.model_picker_tab = prefs.model_picker_tab.clone();
            saved.favorite_models = prefs.favorite_models.clone();
            saved.recent_models = prefs.recent_models.clone();
            saved.last_models = prefs.last_models.clone();
            saved.file_view_mode = prefs.file_view_mode.clone();
            saved.default_access = prefs.default_access.clone();
            saved.provider_efforts = prefs.provider_efforts.clone();
            saved.hidden_models = prefs.hidden_models.clone();
            saved.project_providers = prefs.project_providers.clone();
            this.sync_settings(cx);
            this.persist(cx);
        });
        self.observations.push(observer);
    }
}
