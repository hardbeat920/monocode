//! The localStorage writes the submit pipeline makes, over
//! `monocode_settings::Kv` with the same keys and JSON values:
//! `saveRecentModelChoice` and `saveLastModelSettings` from
//! src/features/sessions/model/models.ts, and a `LocalStore` view of the
//! store for the harness crate's provider account helpers.

use monocode_core::HarnessId;
use monocode_core::ModelSettings;
use monocode_core::models::{
    LAST_MODEL_SETTINGS_KEY, ModelPrefs, RECENT_MODELS_KEY, SaveSettingsMode,
};
use monocode_harness::core::local_store::LocalStore;
use monocode_settings::Kv;

/// `Kv` as the harness crate's `LocalStore`. `Kv` writes never fail.
#[derive(Clone)]
pub struct KvStore(pub Kv);

impl LocalStore for KvStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.get_item(key)
    }

    fn set_item(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.set_item(key, value);
        Ok(())
    }
}

/// Every model preference, read from the store.
pub fn load_model_prefs(kv: &Kv) -> ModelPrefs {
    ModelPrefs::from_local_storage(|key| kv.get_item(key))
}

/// `saveRecentModelChoice`: move a choice to the front of the recent list.
pub fn save_recent_model_choice(kv: &Kv, harness: HarnessId, model: &str) {
    let mut prefs = load_model_prefs(kv);
    let recent = prefs.save_recent_model_choice(harness, model);
    if let Ok(raw) = serde_json::to_string(recent) {
        kv.set_item(RECENT_MODELS_KEY, &raw);
    }
}

/// `saveLastModelSettings`.
pub fn save_last_model_settings(kv: &Kv, settings: &ModelSettings, mode: SaveSettingsMode) {
    let mut prefs = load_model_prefs(kv);
    prefs.save_last_model_settings(settings, mode);
    if let Ok(raw) = serde_json::to_string(&prefs.last_model_settings) {
        kv.set_item(LAST_MODEL_SETTINGS_KEY, &raw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_recent_choices_and_last_settings_in_the_stored_shape() {
        let kv = Kv::in_memory();
        save_recent_model_choice(&kv, HarnessId::Claude, "claude:opus");
        save_recent_model_choice(&kv, HarnessId::Codex, "gpt-5");
        save_recent_model_choice(&kv, HarnessId::Claude, "claude:opus");
        assert_eq!(
            kv.get_item(RECENT_MODELS_KEY).as_deref(),
            Some(
                r#"[{"harness":"claude","model":"claude:opus"},{"harness":"codex","model":"gpt-5"}]"#
            )
        );
        let mut settings = ModelSettings::new();
        settings.insert("effort".into(), "high".into());
        save_last_model_settings(&kv, &settings, SaveSettingsMode::Overwrite);
        settings.insert("effort".into(), "low".into());
        settings.insert("fast".into(), "true".into());
        save_last_model_settings(&kv, &settings, SaveSettingsMode::Fill);
        assert_eq!(
            kv.get_item(LAST_MODEL_SETTINGS_KEY).as_deref(),
            Some(r#"{"effort":"high","fast":"true"}"#)
        );
    }
}
