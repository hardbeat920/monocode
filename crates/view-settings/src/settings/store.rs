//! The localStorage reads and writes the settings page makes, through `Kv`.
//!
//! `monocode_settings::settings_store` covers settings.ts. This module adds
//! the save half of appearance.ts and uiScale.ts, plus the few keys other
//! features own that the page edits directly: sounds, notifications, the
//! sidebar's archive filter, hidden Linear teams and Jira projects, CLI
//! paths, and the model choices. Each function writes the string its
//! TypeScript counterpart wrote, under the same key.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use monocode_core::Platform;
use monocode_core::appearance::*;
use monocode_core::harness::HarnessId;
use monocode_core::js;
use monocode_core::models::{
    DEFAULT_MODELS_KEY, HIDDEN_PICKER_PROVIDERS_KEY, LAST_MODEL_KEY, LastModelChoice, ModelPrefs,
};
use monocode_core::project_providers::{PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviders};
use monocode_settings::Kv;
use monocode_settings::storage_flags::{read_flag, write_flag};
use serde_json::{Map, Value, json};

/// `writeNumber`: `String(value)`.
fn write_number(kv: &Kv, key: &str, value: f64) {
    kv.set_item(key, &js::number_to_string(value));
}

/// Every appearance value, read the way each `load*` function did.
pub fn load_appearance(kv: &Kv, platform: Platform) -> AppearanceSettings {
    AppearanceSettings::from_local_storage(|key| kv.get_item(key), platform)
}

/// `saveAccentColor`: a lowercase `#rrggbb`, or no key for the default.
pub fn save_accent_color(kv: &Kv, value: Option<&str>) {
    match normalize_accent_color(value) {
        Some(next) => kv.set_item(ACCENT_COLOR_KEY, &next),
        None => kv.remove_item(ACCENT_COLOR_KEY),
    }
}

/// `saveThemeHue`.
pub fn save_theme_hue(kv: &Kv, value: f64) {
    write_number(kv, THEME_HUE_KEY, clamp_theme_hue(value) as f64);
}

/// `saveThemeSaturation`.
pub fn save_theme_saturation(kv: &Kv, value: f64) {
    write_number(
        kv,
        THEME_SATURATION_KEY,
        clamp_theme_saturation(value) as f64,
    );
}

/// `saveThemeDarkLightness`.
pub fn save_theme_dark_lightness(kv: &Kv, value: f64) {
    write_number(
        kv,
        THEME_DARK_LIGHTNESS_KEY,
        clamp_theme_dark_lightness(value) as f64,
    );
}

/// `saveThemePreference`.
pub fn save_theme_preference(kv: &Kv, value: ThemePreference) {
    kv.set_item(SCHEME_KEY, value.as_str());
}

/// `saveSidebarOpacity`.
pub fn save_sidebar_opacity(kv: &Kv, value: f64) {
    write_number(kv, OPACITY_KEY, clamp_sidebar_opacity(value));
}

/// `saveMainOpacity`.
pub fn save_main_opacity(kv: &Kv, value: f64) {
    write_number(kv, MAIN_OPACITY_KEY, clamp_main_opacity(value));
}

/// `saveSidebarBlur`.
pub fn save_sidebar_blur(kv: &Kv, value: f64) {
    write_number(kv, BLUR_KEY, clamp_sidebar_blur(value) as f64);
}

/// `saveBodyGlass`.
pub fn save_body_glass(kv: &Kv, value: bool) {
    write_flag(kv, BODY_KEY, value);
}

/// `saveShowExcludedFiles`.
pub fn save_show_excluded_files(kv: &Kv, value: bool) {
    write_flag(kv, SHOW_EXCLUDED_FILES_KEY, value);
}

/// `saveChatBackgroundPath`.
pub fn save_chat_background_path(kv: &Kv, value: Option<&str>) {
    match value.filter(|value| !value.is_empty()) {
        Some(path) => kv.set_item(CHAT_BACKGROUND_PATH_KEY, path),
        None => kv.remove_item(CHAT_BACKGROUND_PATH_KEY),
    }
}

/// `saveNewThreadBackgroundEffect`.
pub fn save_new_thread_background_effect(kv: &Kv, effect: NewThreadBackgroundEffect) {
    kv.set_item(NEW_THREAD_BACKGROUND_EFFECT_KEY, effect.as_str());
}

/// `saveChatBackgroundEmptyOpacity`.
pub fn save_chat_background_empty_opacity(kv: &Kv, value: f64) {
    write_number(
        kv,
        CHAT_BACKGROUND_EMPTY_OPACITY_KEY,
        clamp_chat_background_opacity(value),
    );
}

/// `saveChatBackgroundSessionOpacity`.
pub fn save_chat_background_session_opacity(kv: &Kv, value: f64) {
    write_number(
        kv,
        CHAT_BACKGROUND_SESSION_OPACITY_KEY,
        clamp_chat_background_opacity(value),
    );
}

/// `saveChatBackgroundBlur`.
pub fn save_chat_background_blur(kv: &Kv, value: f64) {
    write_number(
        kv,
        CHAT_BACKGROUND_BLUR_KEY,
        clamp_chat_background_blur(value) as f64,
    );
}

/// `saveChatBackgroundScope`.
pub fn save_chat_background_scope(kv: &Kv, value: ChatBackgroundScope) {
    kv.set_item(CHAT_BACKGROUND_SCOPE_KEY, value.as_str());
}

/// `saveTranscriptLayout`.
pub fn save_transcript_layout(kv: &Kv, value: TranscriptLayout) {
    kv.set_item(TRANSCRIPT_LAYOUT_KEY, value.as_str());
}

/// `saveTranscriptAnchor`.
pub fn save_transcript_anchor(kv: &Kv, value: bool) {
    write_flag(kv, TRANSCRIPT_ANCHOR_KEY, value);
}

/// `saveUiScale`: the normalized scale, which it returns.
pub fn save_ui_scale(kv: &Kv, value: f64) -> f64 {
    let next = normalize_ui_scale(value);
    write_number(kv, UI_SCALE_KEY, next);
    next
}

/// `loadUiScale`.
pub fn load_ui_scale(kv: &Kv) -> f64 {
    parse_ui_scale(kv.get_item(UI_SCALE_KEY).as_deref())
}

/// Converts the stored appearance into what `monocode_ui`'s theme reads.
pub fn ui_appearance(settings: &AppearanceSettings) -> monocode_ui::AppearanceSettings {
    monocode_ui::AppearanceSettings {
        theme_preference: match settings.theme_preference {
            ThemePreference::Dark => monocode_ui::ThemePreference::Dark,
            ThemePreference::Light => monocode_ui::ThemePreference::Light,
            ThemePreference::System => monocode_ui::ThemePreference::System,
        },
        theme_hue: settings.theme_hue as f64,
        theme_saturation: settings.theme_saturation as f64,
        theme_dark_lightness: settings.theme_dark_lightness as f64,
        accent_color: settings.accent_color.clone(),
        sidebar_opacity: settings.sidebar_opacity as f32,
        main_opacity: settings.main_opacity as f32,
        sidebar_blur: settings.sidebar_blur as f32,
        body_glass: settings.body_glass,
        chat_background_empty_opacity: settings.chat_background_empty_opacity as f32,
        chat_background_session_opacity: settings.chat_background_session_opacity as f32,
        chat_background_blur: settings.chat_background_blur as f32,
        ui_scale: settings.ui_scale as f32,
    }
    .normalized()
}

// sounds.ts

pub const SOUNDS_KEY: &str = "monocode.sounds";
pub const SOUNDS_ENABLED_AT_KEY: &str = "monocode.soundsEnabledAt";
pub const SOUNDS_DEFAULT: bool = true;

/// `loadSoundsEnabled`.
pub fn load_sounds_enabled(kv: &Kv) -> bool {
    read_flag(kv, SOUNDS_KEY).unwrap_or(SOUNDS_DEFAULT)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `saveSoundsEnabled`: turning sounds back on records when, so cues for
/// older activity stay quiet.
pub fn save_sounds_enabled(kv: &Kv, value: bool) {
    let resuming = value && !load_sounds_enabled(kv);
    write_flag(kv, SOUNDS_KEY, value);
    if resuming {
        kv.set_item(SOUNDS_ENABLED_AT_KEY, &now_ms().to_string());
    }
}

// notifications.ts

pub const NOTIFICATIONS_KEY: &str = "monocode.notifications";
/// Off until the user opts in; enabling asks the OS for permission.
pub const NOTIFICATIONS_DEFAULT: bool = false;

/// `loadNotificationsEnabled`.
pub fn load_notifications_enabled(kv: &Kv) -> bool {
    read_flag(kv, NOTIFICATIONS_KEY).unwrap_or(NOTIFICATIONS_DEFAULT)
}

/// `saveNotificationsEnabled`.
pub fn save_notifications_enabled(kv: &Kv, value: bool) {
    write_flag(kv, NOTIFICATIONS_KEY, value);
}

// sessionFilters.ts

pub const SESSION_FILTERS_KEY: &str = "monocode.sessionSidebarFilters";
const LEGACY_SHOW_ARCHIVED_KEY: &str = "monocode.sessionsShowArchived";

/// `SessionSidebarFilters`, with only the fields the archive page edits
/// modeled. The rest pass through `loadSessionSidebarFilters`' cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSidebarFilters {
    pub show_archived: bool,
    pub hidden_harnesses: Vec<HarnessId>,
    pub time: String,
    pub working: bool,
    pub needs_approval: bool,
    pub done: bool,
}

impl Default for SessionSidebarFilters {
    fn default() -> Self {
        Self {
            show_archived: false,
            hidden_harnesses: Vec::new(),
            time: "all".into(),
            working: false,
            needs_approval: false,
            done: false,
        }
    }
}

/// `loadSessionSidebarFilters`.
pub fn load_session_sidebar_filters(kv: &Kv) -> SessionSidebarFilters {
    let Some(raw) = kv
        .get_item(SESSION_FILTERS_KEY)
        .filter(|raw| !raw.is_empty())
    else {
        let legacy = kv.get_item(LEGACY_SHOW_ARCHIVED_KEY).as_deref() == Some("1");
        return SessionSidebarFilters {
            show_archived: legacy,
            ..Default::default()
        };
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return SessionSidebarFilters::default();
    };
    let flag = |value: Option<&Value>| value == Some(&Value::Bool(true));
    let status = parsed.get("status");
    SessionSidebarFilters {
        show_archived: flag(parsed.get("showArchived")),
        hidden_harnesses: parsed
            .get("hiddenHarnesses")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|id| id.as_str().and_then(HarnessId::parse))
                    .collect()
            })
            .unwrap_or_default(),
        time: parsed
            .get("time")
            .and_then(Value::as_str)
            .filter(|time| matches!(*time, "all" | "today" | "7d" | "30d"))
            .unwrap_or("all")
            .into(),
        working: flag(status.and_then(|status| status.get("working"))),
        needs_approval: flag(status.and_then(|status| status.get("needsApproval"))),
        done: flag(status.and_then(|status| status.get("done"))),
    }
}

/// `saveSessionSidebarFilters`.
pub fn save_session_sidebar_filters(kv: &Kv, filters: &SessionSidebarFilters) {
    let value = json!({
        "showArchived": filters.show_archived,
        "hiddenHarnesses": filters.hidden_harnesses,
        "time": filters.time,
        "status": {
            "working": filters.working,
            "needsApproval": filters.needs_approval,
            "done": filters.done,
        },
    });
    kv.set_item(SESSION_FILTERS_KEY, &value.to_string());
}

// linear.ts and jira.ts

pub const LINEAR_HIDDEN_TEAMS_KEY: &str = "monocode.linearHiddenTeams";
pub const JIRA_HIDDEN_PROJECTS_KEY: &str = "monocode.jiraHiddenProjects";

/// `loadHiddenLinearTeamIds` and `loadHiddenJiraProjectIds`.
pub fn load_hidden_ids(kv: &Kv, key: &str) -> Vec<String> {
    kv.get_item(key)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value.as_array().map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// `saveHiddenLinearTeamIds` and `saveHiddenJiraProjectIds`.
pub fn save_hidden_ids(kv: &Kv, key: &str, ids: &[String]) {
    kv.set_item(
        key,
        &serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()),
    );
}

// providerBinaryPaths.ts

pub const PROVIDER_BINARY_PATHS_KEY: &str = "monocode.providerBinaryPaths.v1";

/// `readProviderBinaryPaths`: string entries only, in stored order.
fn read_provider_binary_paths(kv: &Kv) -> Map<String, Value> {
    let raw = kv.get_item(PROVIDER_BINARY_PATHS_KEY);
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw.as_deref().unwrap_or("{}"))
    else {
        return Map::new();
    };
    map.into_iter()
        .filter(|(_, path)| path.is_string())
        .collect()
}

/// `loadProviderBinaryPath`: the trimmed path, or `None` for auto-detect.
pub fn load_provider_binary_path(kv: &Kv, provider: HarnessId) -> Option<String> {
    read_provider_binary_paths(kv)
        .get(provider.as_str())
        .and_then(Value::as_str)
        .map(js::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

/// `saveProviderBinaryPath`. Returns whether the path was stored.
pub fn save_provider_binary_path(kv: &Kv, provider: HarnessId, path: Option<&str>) -> bool {
    let mut stored = read_provider_binary_paths(kv);
    match path.map(js::trim).filter(|path| !path.is_empty()) {
        Some(value) => {
            stored.insert(provider.as_str().into(), Value::String(value.into()));
        }
        None => {
            stored.remove(provider.as_str());
        }
    }
    kv.set_item(
        PROVIDER_BINARY_PATHS_KEY,
        &Value::Object(stored).to_string(),
    );
    true
}

// models.ts

/// The model choices, read the way each `load*` function did.
pub fn load_model_prefs(kv: &Kv) -> ModelPrefs {
    ModelPrefs::from_local_storage(|key| kv.get_item(key))
}

// TODO(port): JSON.stringify kept the record's insertion order; this map
// sorts by provider. Only the stored key order differs.
fn write_default_models(kv: &Kv, models: &BTreeMap<HarnessId, String>) {
    let record: Map<String, Value> = models
        .iter()
        .map(|(harness, model)| (harness.as_str().to_string(), Value::String(model.clone())))
        .collect();
    kv.set_item(DEFAULT_MODELS_KEY, &Value::Object(record).to_string());
}

/// `saveDefaultModel`.
pub fn save_default_model(kv: &Kv, harness: HarnessId, model: &str) {
    let mut models = ModelPrefs::parse_default_models(kv.get_item(DEFAULT_MODELS_KEY).as_deref());
    models.insert(harness, model.to_string());
    write_default_models(kv, &models);
}

/// `saveLastModelChoice`: also remembers the model for its provider.
pub fn save_last_model_choice(kv: &Kv, harness: HarnessId, model: &str) {
    save_default_model(kv, harness, model);
    let choice = LastModelChoice {
        harness,
        model: model.to_string(),
    };
    kv.set_item(
        LAST_MODEL_KEY,
        &serde_json::to_string(&choice).unwrap_or_default(),
    );
}

/// `savePickerProviderVisible`.
pub fn save_picker_provider_visible(kv: &Kv, harness: HarnessId, visible: bool) {
    let mut prefs = ModelPrefs {
        hidden_picker_providers: ModelPrefs::parse_hidden_picker_providers(
            kv.get_item(HIDDEN_PICKER_PROVIDERS_KEY).as_deref(),
        ),
        ..Default::default()
    };
    prefs.set_picker_provider_visible(harness, visible);
    kv.set_item(
        HIDDEN_PICKER_PROVIDERS_KEY,
        &serde_json::to_string(&prefs.hidden_picker_providers).unwrap_or_else(|_| "[]".into()),
    );
}

// projectProviders.ts

/// Every project's provider overrides.
pub fn load_project_providers(kv: &Kv) -> ProjectProviders {
    ProjectProviders::parse(kv.get_item(PROJECT_PROVIDER_SETTINGS_KEY).as_deref())
}

/// Applies `edit` to the stored project overrides and writes them back.
pub fn update_project_providers(kv: &Kv, edit: impl FnOnce(&mut ProjectProviders)) {
    let mut providers = load_project_providers(kv);
    edit(&mut providers);
    kv.set_item(PROJECT_PROVIDER_SETTINGS_KEY, &providers.to_json());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_numbers_the_way_string_did() {
        let kv = Kv::in_memory();
        save_sidebar_opacity(&kv, 0.85);
        assert_eq!(kv.get_item(OPACITY_KEY).as_deref(), Some("0.85"));
        save_sidebar_opacity(&kv, 2.0);
        assert_eq!(kv.get_item(OPACITY_KEY).as_deref(), Some("1"));
        save_theme_hue(&kv, 400.4);
        assert_eq!(kv.get_item(THEME_HUE_KEY).as_deref(), Some("360"));
        save_sidebar_blur(&kv, 23.6);
        assert_eq!(kv.get_item(BLUR_KEY).as_deref(), Some("24"));
        assert_eq!(save_ui_scale(&kv, 1.5), 1.5);
        assert_eq!(kv.get_item(UI_SCALE_KEY).as_deref(), Some("1.5"));
        save_chat_background_empty_opacity(&kv, 0.4);
        assert_eq!(
            kv.get_item(CHAT_BACKGROUND_EMPTY_OPACITY_KEY).as_deref(),
            Some("0.4")
        );
    }

    #[test]
    fn accent_color_is_lowercased_or_removed() {
        let kv = Kv::in_memory();
        save_accent_color(&kv, Some("#AABBCC"));
        assert_eq!(kv.get_item(ACCENT_COLOR_KEY).as_deref(), Some("#aabbcc"));
        save_accent_color(&kv, None);
        assert_eq!(kv.get_item(ACCENT_COLOR_KEY), None);
    }

    #[test]
    fn records_when_sounds_resume() {
        let kv = Kv::in_memory();
        assert!(load_sounds_enabled(&kv));
        save_sounds_enabled(&kv, false);
        assert_eq!(kv.get_item(SOUNDS_KEY).as_deref(), Some("0"));
        assert_eq!(kv.get_item(SOUNDS_ENABLED_AT_KEY), None);
        save_sounds_enabled(&kv, true);
        assert!(kv.get_item(SOUNDS_ENABLED_AT_KEY).is_some());
    }

    #[test]
    fn session_filters_keep_the_other_fields() {
        let kv = Kv::in_memory();
        kv.set_item(
            SESSION_FILTERS_KEY,
            r#"{"showArchived":false,"hiddenHarnesses":["pi","nope"],"time":"7d","status":{"done":true}}"#,
        );
        let mut filters = load_session_sidebar_filters(&kv);
        assert_eq!(filters.hidden_harnesses, vec![HarnessId::Pi]);
        filters.show_archived = true;
        save_session_sidebar_filters(&kv, &filters);
        assert_eq!(
            kv.get_item(SESSION_FILTERS_KEY).as_deref(),
            Some(
                r#"{"showArchived":true,"hiddenHarnesses":["pi"],"time":"7d","status":{"working":false,"needsApproval":false,"done":true}}"#
            )
        );
        let legacy = Kv::in_memory();
        legacy.set_item(LEGACY_SHOW_ARCHIVED_KEY, "1");
        assert!(load_session_sidebar_filters(&legacy).show_archived);
    }

    #[test]
    fn stores_and_clears_cli_paths() {
        let kv = Kv::in_memory();
        assert!(save_provider_binary_path(
            &kv,
            HarnessId::Codex,
            Some(" /opt/codex ")
        ));
        assert_eq!(
            load_provider_binary_path(&kv, HarnessId::Codex).as_deref(),
            Some("/opt/codex")
        );
        save_provider_binary_path(&kv, HarnessId::Opencode, Some("/opt/opencode"));
        save_provider_binary_path(&kv, HarnessId::Codex, None);
        assert_eq!(
            kv.get_item(PROVIDER_BINARY_PATHS_KEY).as_deref(),
            Some(r#"{"opencode":"/opt/opencode"}"#)
        );
    }

    #[test]
    fn model_choices_round_trip() {
        let kv = Kv::in_memory();
        save_last_model_choice(&kv, HarnessId::Claude, "claude:opus-5");
        assert_eq!(
            kv.get_item(LAST_MODEL_KEY).as_deref(),
            Some(r#"{"harness":"claude","model":"claude:opus-5"}"#)
        );
        assert_eq!(
            kv.get_item(DEFAULT_MODELS_KEY).as_deref(),
            Some(r#"{"claude":"claude:opus-5"}"#)
        );
        save_picker_provider_visible(&kv, HarnessId::Cursor, false);
        assert_eq!(
            kv.get_item(HIDDEN_PICKER_PROVIDERS_KEY).as_deref(),
            Some(r#"["cursor"]"#)
        );
        save_picker_provider_visible(&kv, HarnessId::Cursor, true);
        assert_eq!(
            kv.get_item(HIDDEN_PICKER_PROVIDERS_KEY).as_deref(),
            Some("[]")
        );
    }

    #[test]
    fn hidden_ids_skip_junk() {
        let kv = Kv::in_memory();
        kv.set_item(JIRA_HIDDEN_PROJECTS_KEY, r#"["10000","",4]"#);
        assert_eq!(
            load_hidden_ids(&kv, JIRA_HIDDEN_PROJECTS_KEY),
            vec!["10000"]
        );
        save_hidden_ids(&kv, LINEAR_HIDDEN_TEAMS_KEY, &["a".into()]);
        assert_eq!(
            kv.get_item(LINEAR_HIDDEN_TEAMS_KEY).as_deref(),
            Some(r#"["a"]"#)
        );
    }
}
