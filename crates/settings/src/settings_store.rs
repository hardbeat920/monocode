//! Port of the storage half of src/features/settings/model/settings.ts.
//!
//! `monocode_core::settings` has the shapes, defaults, and parsing. This
//! module reads and writes them through `Kv` under the same keys and with the
//! same stored strings. Each `subscribe_*` function replaces a
//! `monocode:*-change` window event; it fires when the stored value changes.

use monocode_core::Platform;
use monocode_core::settings::*;
use monocode_core::shortcut::{ShortcutEvent, is_global_shortcut};
use monocode_core::{AppSettings, appearance, models, project_providers};
use std::sync::{Mutex, PoisonError};

use crate::kv::{Kv, Subscription};
use crate::storage_flags::{read_flag, write_flag};

/// Every localStorage key `AppSettings::from_local_storage` reads: each
/// `*_KEY` constant in `monocode_core`.
pub const APP_SETTINGS_KEYS: [&str; 53] = [
    // settings.rs
    SECTION_KEY,
    FOLLOW_UP_BEHAVIOR_KEY,
    FILE_TAB_MODE_KEY,
    TAB_ANIMATIONS_ENABLED_KEY,
    COLLAPSED_PROJECT_RAIL_MODE_KEY,
    MODEL_CONTROLS_KEY,
    COMPOSER_EFFORT_VISIBLE_KEY,
    COMPOSER_RUNNER_KEY,
    NOTES_ENABLED_KEY,
    QUICK_COMPOSER_ENABLED_KEY,
    QUICK_COMPOSER_SHORTCUT_KEY,
    LIVE_AGENTS_ENABLED_KEY,
    CLOSE_TO_TRAY_KEY,
    GRID_ARCADE_ENABLED_KEY,
    DIFF_VIEWER_KEY,
    FORMAT_ON_SAVE_KEY,
    AUTOSAVE_KEY,
    CLAUDE_HOOKS_KEY,
    KEYBINDING_OVERRIDES_KEY,
    // appearance.rs
    appearance::ACCENT_COLOR_KEY,
    appearance::THEME_HUE_KEY,
    appearance::THEME_SATURATION_KEY,
    appearance::THEME_DARK_LIGHTNESS_KEY,
    appearance::OPACITY_KEY,
    appearance::MAIN_OPACITY_KEY,
    appearance::BLUR_KEY,
    appearance::PROJECT_RAIL_OPEN_KEY,
    appearance::SESSION_SIDEBAR_OPEN_KEY,
    appearance::BODY_KEY,
    appearance::SCHEME_KEY,
    appearance::SIDEBAR_TAB_ORDER_KEY,
    appearance::PROJECT_RAIL_WIDTH_KEY,
    appearance::TRANSCRIPT_LAYOUT_KEY,
    appearance::TRANSCRIPT_ANCHOR_KEY,
    appearance::CHAT_BACKGROUND_PATH_KEY,
    appearance::CHAT_BACKGROUND_OPACITY_KEY,
    appearance::CHAT_BACKGROUND_EMPTY_OPACITY_KEY,
    appearance::CHAT_BACKGROUND_SESSION_OPACITY_KEY,
    appearance::CHAT_BACKGROUND_SCOPE_KEY,
    appearance::CHAT_BACKGROUND_BLUR_KEY,
    appearance::NEW_THREAD_BACKGROUND_EFFECT_KEY,
    appearance::CHANGES_VIEW_KEY,
    appearance::DIFF_PALETTE_KEY,
    appearance::SHOW_EXCLUDED_FILES_KEY,
    appearance::UI_SCALE_KEY,
    // models.rs
    models::FAVORITES_KEY,
    models::MODEL_PICKER_TAB_KEY,
    models::HIDDEN_PICKER_PROVIDERS_KEY,
    models::LAST_MODEL_KEY,
    models::LAST_MODEL_SETTINGS_KEY,
    models::DEFAULT_MODELS_KEY,
    models::RECENT_MODELS_KEY,
    // project_providers.rs
    project_providers::PROJECT_PROVIDER_SETTINGS_KEY,
];

/// Every stored preference, read the way each `load*` function did.
pub fn load_app_settings(kv: &Kv, platform: Platform) -> AppSettings {
    AppSettings::from_local_storage(|key| kv.get_item(key), platform)
}

/// The app behavior settings from settings.ts.
pub fn load_settings(kv: &Kv, platform: Platform) -> Settings {
    Settings::from_local_storage(|key| kv.get_item(key), platform)
}

fn on_change(
    kv: &Kv,
    key: &str,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    kv.subscribe_key(key, move |_| on_store_change())
}

/// `loadSettingsSection`.
pub fn load_settings_section(kv: &Kv) -> SettingsSectionId {
    SettingsSectionId::parse(kv.get_item(SECTION_KEY).as_deref())
}

/// `saveSettingsSection`.
pub fn save_settings_section(kv: &Kv, id: SettingsSectionId) {
    kv.set_item(SECTION_KEY, id.as_str());
}

/// `loadFollowUpBehavior`.
pub fn load_follow_up_behavior(kv: &Kv) -> FollowUpBehavior {
    FollowUpBehavior::parse(kv.get_item(FOLLOW_UP_BEHAVIOR_KEY).as_deref())
}

/// `saveFollowUpBehavior`.
pub fn save_follow_up_behavior(kv: &Kv, value: FollowUpBehavior) {
    kv.set_item(FOLLOW_UP_BEHAVIOR_KEY, value.as_str());
}

/// `loadFileTabMode`.
pub fn load_file_tab_mode(kv: &Kv) -> FileTabMode {
    FileTabMode::parse(kv.get_item(FILE_TAB_MODE_KEY).as_deref())
}

/// `saveFileTabMode`.
pub fn save_file_tab_mode(kv: &Kv, value: FileTabMode) {
    kv.set_item(FILE_TAB_MODE_KEY, value.as_str());
}

/// `loadTabAnimationsEnabled`.
pub fn load_tab_animations_enabled(kv: &Kv) -> bool {
    read_flag(kv, TAB_ANIMATIONS_ENABLED_KEY).unwrap_or(TAB_ANIMATIONS_ENABLED_DEFAULT)
}

/// `saveTabAnimationsEnabled`.
pub fn save_tab_animations_enabled(kv: &Kv, value: bool) {
    write_flag(kv, TAB_ANIMATIONS_ENABLED_KEY, value);
}

/// `loadCollapsedProjectRailMode`.
pub fn load_collapsed_project_rail_mode(kv: &Kv) -> CollapsedProjectRailMode {
    CollapsedProjectRailMode::parse(kv.get_item(COLLAPSED_PROJECT_RAIL_MODE_KEY).as_deref())
}

/// `saveCollapsedProjectRailMode`.
pub fn save_collapsed_project_rail_mode(kv: &Kv, value: CollapsedProjectRailMode) {
    kv.set_item(COLLAPSED_PROJECT_RAIL_MODE_KEY, value.as_str());
}

/// `subscribeCollapsedProjectRailMode`.
pub fn subscribe_collapsed_project_rail_mode(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, COLLAPSED_PROJECT_RAIL_MODE_KEY, on_store_change)
}

/// `loadModelControls`, migrating the legacy effort-control toggle.
pub fn load_model_controls(kv: &Kv) -> ModelControls {
    parse_model_controls(
        kv.get_item(MODEL_CONTROLS_KEY).as_deref(),
        kv.get_item(COMPOSER_EFFORT_VISIBLE_KEY).as_deref(),
    )
}

/// `saveModelControls`.
pub fn save_model_controls(kv: &Kv, value: ModelControls) {
    kv.set_item(MODEL_CONTROLS_KEY, value.as_str());
}

/// `subscribeModelControls`.
pub fn subscribe_model_controls(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, MODEL_CONTROLS_KEY, on_store_change)
}

/// `loadComposerRunner`.
pub fn load_composer_runner(kv: &Kv) -> bool {
    read_flag(kv, COMPOSER_RUNNER_KEY).unwrap_or(COMPOSER_RUNNER_DEFAULT)
}

/// `saveComposerRunner`.
pub fn save_composer_runner(kv: &Kv, value: bool) {
    write_flag(kv, COMPOSER_RUNNER_KEY, value);
}

/// Listens for `COMPOSER_RUNNER_CHANGE_EVENT`, which settings.ts dispatched
/// without a subscribe helper.
pub fn subscribe_composer_runner(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, COMPOSER_RUNNER_KEY, on_store_change)
}

/// `loadNotesEnabled`.
pub fn load_notes_enabled(kv: &Kv) -> bool {
    read_flag(kv, NOTES_ENABLED_KEY).unwrap_or(NOTES_ENABLED_DEFAULT)
}

/// `saveNotesEnabled`.
pub fn save_notes_enabled(kv: &Kv, value: bool) {
    write_flag(kv, NOTES_ENABLED_KEY, value);
}

/// `subscribeNotesEnabled`.
pub fn subscribe_notes_enabled(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, NOTES_ENABLED_KEY, on_store_change)
}

/// `loadQuickComposerEnabled`.
pub fn load_quick_composer_enabled(kv: &Kv) -> bool {
    read_flag(kv, QUICK_COMPOSER_ENABLED_KEY).unwrap_or(QUICK_COMPOSER_ENABLED_DEFAULT)
}

/// `saveQuickComposerEnabled`.
pub fn save_quick_composer_enabled(kv: &Kv, value: bool) {
    write_flag(kv, QUICK_COMPOSER_ENABLED_KEY, value);
}

/// `loadQuickComposerShortcut`.
pub fn load_quick_composer_shortcut(kv: &Kv) -> String {
    parse_quick_composer_shortcut(kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY).as_deref())
}

/// `saveQuickComposerShortcut`: ignores chords that cannot be global, and
/// returns the conflict message the TypeScript threw.
pub fn save_quick_composer_shortcut(
    kv: &Kv,
    value: &str,
    platform: Platform,
) -> Result<(), String> {
    if !is_global_shortcut(value) {
        return Ok(());
    }
    let mut settings = load_settings(kv, platform);
    settings.save_quick_composer_shortcut(value, platform)?;
    kv.set_item(
        QUICK_COMPOSER_SHORTCUT_KEY,
        &settings.quick_composer_shortcut,
    );
    Ok(())
}

/// `loadLiveAgentsEnabled`.
pub fn load_live_agents_enabled(kv: &Kv) -> bool {
    read_flag(kv, LIVE_AGENTS_ENABLED_KEY).unwrap_or(LIVE_AGENTS_ENABLED_DEFAULT)
}

/// `saveLiveAgentsEnabled`.
pub fn save_live_agents_enabled(kv: &Kv, value: bool) {
    write_flag(kv, LIVE_AGENTS_ENABLED_KEY, value);
}

/// `subscribeLiveAgentsEnabled`.
pub fn subscribe_live_agents_enabled(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, LIVE_AGENTS_ENABLED_KEY, on_store_change)
}

/// `loadCloseToTray`: Close to tray is Windows-only, so other platforms read
/// `false` without consulting the store.
pub fn load_close_to_tray(kv: &Kv, platform: Platform) -> bool {
    if !platform.is_windows() {
        return false;
    }
    read_flag(kv, CLOSE_TO_TRAY_KEY).unwrap_or(CLOSE_TO_TRAY_DEFAULT)
}

/// `saveCloseToTray`.
pub fn save_close_to_tray(kv: &Kv, value: bool) {
    write_flag(kv, CLOSE_TO_TRAY_KEY, value);
}

/// `loadGridArcadeEnabled`.
pub fn load_grid_arcade_enabled(kv: &Kv) -> bool {
    read_flag(kv, GRID_ARCADE_ENABLED_KEY).unwrap_or(GRID_ARCADE_ENABLED_DEFAULT)
}

/// `saveGridArcadeEnabled`.
pub fn save_grid_arcade_enabled(kv: &Kv, value: bool) {
    write_flag(kv, GRID_ARCADE_ENABLED_KEY, value);
}

/// `subscribeGridArcadeEnabled`.
pub fn subscribe_grid_arcade_enabled(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, GRID_ARCADE_ENABLED_KEY, on_store_change)
}

/// `loadDiffViewer`.
pub fn load_diff_viewer(kv: &Kv) -> DiffViewer {
    DiffViewer::parse(kv.get_item(DIFF_VIEWER_KEY).as_deref())
}

/// `saveDiffViewer`. The TypeScript mapped unknown strings to the default;
/// the enum has no unknown values.
pub fn save_diff_viewer(kv: &Kv, value: DiffViewer) {
    kv.set_item(DIFF_VIEWER_KEY, value.as_str());
}

/// `subscribeDiffViewer`.
pub fn subscribe_diff_viewer(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, DIFF_VIEWER_KEY, on_store_change)
}

/// `loadFormatOnSave`.
pub fn load_format_on_save(kv: &Kv) -> bool {
    read_flag(kv, FORMAT_ON_SAVE_KEY).unwrap_or(FORMAT_ON_SAVE_DEFAULT)
}

/// `saveFormatOnSave`.
pub fn save_format_on_save(kv: &Kv, value: bool) {
    write_flag(kv, FORMAT_ON_SAVE_KEY, value);
}

/// `loadAutosave`.
pub fn load_autosave(kv: &Kv) -> bool {
    read_flag(kv, AUTOSAVE_KEY).unwrap_or(AUTOSAVE_DEFAULT)
}

/// `saveAutosave`: returns the value as stored.
pub fn save_autosave(kv: &Kv, value: bool) -> bool {
    write_flag(kv, AUTOSAVE_KEY, value);
    load_autosave(kv)
}

/// `subscribeAutosave`: the change event and the cross-window `storage`
/// event are both changes to the key.
pub fn subscribe_autosave(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, AUTOSAVE_KEY, on_store_change)
}

/// `loadClaudeHooks`.
pub fn load_claude_hooks(kv: &Kv) -> bool {
    read_flag(kv, CLAUDE_HOOKS_KEY).unwrap_or(CLAUDE_HOOKS_DEFAULT)
}

/// `saveClaudeHooks`.
pub fn save_claude_hooks(kv: &Kv, value: bool) {
    write_flag(kv, CLAUDE_HOOKS_KEY, value);
}

/// The last parsed overrides, keyed by store, platform, and stored string.
struct OverridesCache {
    kv: u64,
    platform: Platform,
    raw: Option<String>,
    value: KeybindingOverrides,
}

static OVERRIDES_CACHE: Mutex<Option<OverridesCache>> = Mutex::new(None);

/// `loadKeybindingOverrides`: cached per stored string, because key handlers
/// call it several times on every key press. Returns a fresh copy.
pub fn load_keybinding_overrides(kv: &Kv, platform: Platform) -> KeybindingOverrides {
    let raw = kv.get_item(KEYBINDING_OVERRIDES_KEY);
    let mut cache = OVERRIDES_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(cached) = cache.as_ref()
        && cached.kv == kv.id()
        && cached.platform == platform
        && cached.raw == raw
    {
        return cached.value.clone();
    }
    let value = parse_keybinding_overrides(raw.as_deref(), platform);
    *cache = Some(OverridesCache {
        kv: kv.id(),
        platform,
        raw,
        value: value.clone(),
    });
    value
}

/// Settings holding only the stored overrides, for the read helpers below.
fn overrides_only(kv: &Kv, platform: Platform) -> Settings {
    Settings {
        keybinding_overrides: load_keybinding_overrides(kv, platform),
        ..Settings::default()
    }
}

/// `validateKeybindingShortcut`: the canonical chord, or the message the
/// settings row shows.
pub fn validate_keybinding_shortcut(
    kv: &Kv,
    command: &str,
    shortcut: &str,
    platform: Platform,
) -> Result<String, String> {
    load_settings(kv, platform).validate_keybinding_shortcut(command, shortcut, platform)
}

/// `saveKeybindingOverride`: stores the overrides as JSON, removes the key
/// when none are left, and returns them. Errors carry the message the
/// TypeScript threw.
pub fn save_keybinding_override(
    kv: &Kv,
    command: &str,
    override_: &KeybindingOverride,
    platform: Platform,
) -> Result<KeybindingOverrides, String> {
    let mut settings = load_settings(kv, platform);
    settings.save_keybinding_override(command, override_, platform)?;
    let next = settings.keybinding_overrides;
    if next.is_empty() {
        kv.remove_item(KEYBINDING_OVERRIDES_KEY);
    } else {
        let json = serde_json::to_string(&next).map_err(|_| "Could not save shortcuts")?;
        kv.set_item(KEYBINDING_OVERRIDES_KEY, &json);
    }
    Ok(next)
}

/// `matchCustomKeybinding`.
pub fn match_custom_keybinding(
    kv: &Kv,
    event: &ShortcutEvent,
    platform: Platform,
) -> Option<String> {
    overrides_only(kv, platform)
        .match_custom_keybinding(event)
        .map(str::to_string)
}

/// `keybindingPressed`.
pub fn keybinding_pressed(
    kv: &Kv,
    command: &str,
    event: &ShortcutEvent,
    default_match: bool,
    platform: Platform,
) -> bool {
    overrides_only(kv, platform).keybinding_pressed(command, event, default_match)
}

/// `keybindingShortcutLabel`: `None` when the user disabled the command.
pub fn keybinding_shortcut_label(
    kv: &Kv,
    command: &str,
    fallback: &str,
    platform: Platform,
) -> Option<String> {
    overrides_only(kv, platform).keybinding_shortcut_label(command, fallback, platform)
}

/// `keybindingShortcutTokens`.
pub fn keybinding_shortcut_tokens(
    kv: &Kv,
    command: &str,
    fallback: &str,
    platform: Platform,
) -> Option<String> {
    overrides_only(kv, platform).keybinding_shortcut_tokens(command, fallback)
}

/// `subscribeKeybindings`.
pub fn subscribe_keybindings(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    on_change(kv, KEYBINDING_OVERRIDES_KEY, on_store_change)
}

/// `currentKeybindings`.
pub fn current_keybindings(kv: &Kv, platform: Platform) -> Vec<KeybindingRow> {
    load_settings(kv, platform).current_keybindings(platform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::shortcut::Modifiers;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const MAC: Platform = Platform::Mac;

    fn key(code: &str, modifiers: Modifiers) -> ShortcutEvent {
        ShortcutEvent {
            code: code.into(),
            modifiers,
        }
    }

    fn meta() -> Modifiers {
        Modifiers {
            meta_key: true,
            ..Modifiers::default()
        }
    }

    fn meta_shift() -> Modifiers {
        Modifiers {
            meta_key: true,
            shift_key: true,
            ..Modifiers::default()
        }
    }

    fn shortcut(value: &str) -> KeybindingOverride {
        KeybindingOverride {
            disabled: None,
            shortcut: Some(value.into()),
        }
    }

    fn disabled() -> KeybindingOverride {
        KeybindingOverride {
            disabled: Some(true),
            shortcut: None,
        }
    }

    fn counter() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync + 'static) {
        let count = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&count);
        (count, move || {
            sink.fetch_add(1, Ordering::SeqCst);
        })
    }

    type Load = fn(&Kv) -> bool;
    type Save = fn(&Kv, bool);

    fn load_close_to_tray_on_windows(kv: &Kv) -> bool {
        load_close_to_tray(kv, Platform::Windows)
    }

    /// The settings.ts rows of settings.flags.test.ts.
    const FLAGS: [(&str, Load, Save, bool); 7] = [
        (
            TAB_ANIMATIONS_ENABLED_KEY,
            load_tab_animations_enabled,
            save_tab_animations_enabled,
            false,
        ),
        (
            COMPOSER_RUNNER_KEY,
            load_composer_runner,
            save_composer_runner,
            true,
        ),
        (
            NOTES_ENABLED_KEY,
            load_notes_enabled,
            save_notes_enabled,
            true,
        ),
        (
            LIVE_AGENTS_ENABLED_KEY,
            load_live_agents_enabled,
            save_live_agents_enabled,
            true,
        ),
        (
            CLOSE_TO_TRAY_KEY,
            load_close_to_tray_on_windows,
            save_close_to_tray,
            true,
        ),
        (
            GRID_ARCADE_ENABLED_KEY,
            load_grid_arcade_enabled,
            save_grid_arcade_enabled,
            true,
        ),
        (CLAUDE_HOOKS_KEY, load_claude_hooks, save_claude_hooks, true),
    ];

    #[test]
    fn flags_use_their_default_when_unset() {
        for (key, load, _, fallback) in FLAGS {
            assert_eq!(load(&Kv::in_memory()), fallback, "{key}");
        }
    }

    #[test]
    fn flags_read_stored_strings_like_read_flag() {
        for (key, load, _, _) in FLAGS {
            for (stored, expected) in [
                ("1", true),
                ("true", true),
                ("0", false),
                ("false", false),
                ("", false),
                ("TRUE", false),
                (" true ", false),
                ("invalid", false),
            ] {
                let kv = Kv::in_memory();
                kv.set_item(key, stored);
                assert_eq!(load(&kv), expected, "{key} {stored:?}");
            }
        }
    }

    #[test]
    fn flags_persist_before_notifying_listeners() {
        for (key, load, save, _) in FLAGS {
            for value in [false, true] {
                let kv = Kv::in_memory();
                let reader = kv.clone();
                let heard = Arc::new(AtomicUsize::new(0));
                let sink = Arc::clone(&heard);
                let _subscription = kv.subscribe_key(key, move |_| {
                    assert_eq!(load(&reader), value);
                    sink.fetch_add(1, Ordering::SeqCst);
                });
                save(&kv, value);
                assert_eq!(
                    kv.get_item(key).as_deref(),
                    Some(if value { "1" } else { "0" })
                );
                assert_eq!(kv.len(), 1);
                assert_eq!(load(&kv), value);
                assert_eq!(heard.load(Ordering::SeqCst), 1, "{key}");
            }
        }
    }

    #[test]
    fn disables_close_to_tray_outside_windows_without_consulting_storage() {
        let kv = Kv::in_memory();
        save_close_to_tray(&kv, true);
        assert!(!load_close_to_tray(&kv, Platform::Mac));
        assert!(!load_close_to_tray(&kv, Platform::Linux));
        assert!(load_close_to_tray(&kv, Platform::Windows));
    }

    // follow-up behavior setting
    #[test]
    fn follow_up_behavior_defaults_persists_and_ignores_unknown_values() {
        let kv = Kv::in_memory();
        assert_eq!(load_follow_up_behavior(&kv), FollowUpBehavior::Steer);
        save_follow_up_behavior(&kv, FollowUpBehavior::Queue);
        assert_eq!(
            kv.get_item(FOLLOW_UP_BEHAVIOR_KEY).as_deref(),
            Some("queue")
        );
        assert_eq!(load_follow_up_behavior(&kv), FollowUpBehavior::Queue);
        kv.set_item(FOLLOW_UP_BEHAVIOR_KEY, "interrupt");
        assert_eq!(load_follow_up_behavior(&kv), FollowUpBehavior::Steer);
    }

    // composer runner setting
    #[test]
    fn composer_runner_persists_an_off_switch() {
        let kv = Kv::in_memory();
        assert!(load_composer_runner(&kv));
        let (count, on_change) = counter();
        let _subscription = subscribe_composer_runner(&kv, on_change);
        save_composer_runner(&kv, false);
        assert_eq!(kv.get_item(COMPOSER_RUNNER_KEY).as_deref(), Some("0"));
        assert!(!load_composer_runner(&kv));
        save_composer_runner(&kv, true);
        assert!(load_composer_runner(&kv));
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    // model controls setting
    #[test]
    fn model_controls_persist_and_migrate_the_legacy_toggle() {
        let kv = Kv::in_memory();
        assert_eq!(load_model_controls(&kv), ModelControls::Menu);
        let (count, on_change) = counter();
        let _subscription = subscribe_model_controls(&kv, on_change);
        save_model_controls(&kv, ModelControls::Beside);
        assert_eq!(kv.get_item(MODEL_CONTROLS_KEY).as_deref(), Some("beside"));
        assert_eq!(load_model_controls(&kv), ModelControls::Beside);
        save_model_controls(&kv, ModelControls::Menu);
        assert_eq!(load_model_controls(&kv), ModelControls::Menu);
        assert_eq!(count.load(Ordering::SeqCst), 2);

        kv.set_item(MODEL_CONTROLS_KEY, "everywhere");
        assert_eq!(load_model_controls(&kv), ModelControls::Menu);

        kv.remove_item(MODEL_CONTROLS_KEY);
        kv.set_item(COMPOSER_EFFORT_VISIBLE_KEY, "1");
        assert_eq!(load_model_controls(&kv), ModelControls::Beside);
        kv.set_item(COMPOSER_EFFORT_VISIBLE_KEY, "0");
        assert_eq!(load_model_controls(&kv), ModelControls::Menu);
    }

    // notes, live agents, grid arcade, format on save, tab animations
    #[test]
    fn switches_with_change_events_notify_their_subscribers() {
        let kv = Kv::in_memory();
        let (notes, on_notes) = counter();
        let (agents, on_agents) = counter();
        let (arcade, on_arcade) = counter();
        let _a = subscribe_notes_enabled(&kv, on_notes);
        let _b = subscribe_live_agents_enabled(&kv, on_agents);
        let _c = subscribe_grid_arcade_enabled(&kv, on_arcade);
        save_notes_enabled(&kv, false);
        save_live_agents_enabled(&kv, false);
        save_grid_arcade_enabled(&kv, false);
        save_format_on_save(&kv, false);
        save_tab_animations_enabled(&kv, true);
        assert!(!load_notes_enabled(&kv));
        assert!(!load_live_agents_enabled(&kv));
        assert!(!load_grid_arcade_enabled(&kv));
        assert!(!load_format_on_save(&kv));
        assert!(load_tab_animations_enabled(&kv));
        assert_eq!(kv.get_item(FORMAT_ON_SAVE_KEY).as_deref(), Some("0"));
        assert_eq!(
            [&notes, &agents, &arcade].map(|count| count.load(Ordering::SeqCst)),
            [1, 1, 1]
        );
    }

    // quick composer settings
    #[test]
    fn quick_composer_defaults_and_persists_a_custom_binding() {
        let kv = Kv::in_memory();
        assert!(load_quick_composer_enabled(&kv));
        save_quick_composer_enabled(&kv, false);
        assert!(!load_quick_composer_enabled(&kv));
        assert_eq!(load_quick_composer_shortcut(&kv), "Command+Shift+Space");
        save_quick_composer_shortcut(&kv, "Command+Option+KeyK", MAC).unwrap();
        assert_eq!(
            kv.get_item(QUICK_COMPOSER_SHORTCUT_KEY).as_deref(),
            Some("Command+Option+KeyK")
        );
        assert_eq!(load_quick_composer_shortcut(&kv), "Command+Option+KeyK");
        assert_eq!(
            save_quick_composer_shortcut(&kv, "Command+KeyK", MAC),
            Err("Already used by App: Search".into())
        );
        save_quick_composer_shortcut(&kv, "Option+KeyK", MAC).unwrap();
        assert_eq!(load_quick_composer_shortcut(&kv), "Command+Option+KeyK");
    }

    #[test]
    fn quick_composer_ignores_malformed_stored_bindings() {
        let kv = Kv::in_memory();
        kv.set_item(QUICK_COMPOSER_SHORTCUT_KEY, "Shift+Space");
        assert_eq!(load_quick_composer_shortcut(&kv), "Command+Shift+Space");
    }

    #[test]
    fn quick_composer_cannot_take_a_rebound_chord() {
        let kv = Kv::in_memory();
        save_keybinding_override(&kv, "App: Search", &shortcut("Command+Shift+KeyM"), MAC).unwrap();
        assert_eq!(
            save_quick_composer_shortcut(&kv, "Command+Shift+KeyM", MAC),
            Err("Already used by App: Search".into())
        );
    }

    // autosave, diff viewer, file tab mode, collapsed rail, section
    #[test]
    fn autosave_defaults_to_off_and_returns_the_stored_value() {
        let kv = Kv::in_memory();
        let (count, on_change) = counter();
        let _subscription = subscribe_autosave(&kv, on_change);
        assert!(!load_autosave(&kv));
        assert!(save_autosave(&kv, true));
        assert_eq!(kv.get_item(AUTOSAVE_KEY).as_deref(), Some("1"));
        assert!(load_autosave(&kv));
        // Another window writing the key is the same change.
        kv.set_item(AUTOSAVE_KEY, "0");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn diff_viewer_persists_the_unified_layout() {
        let kv = Kv::in_memory();
        let (count, on_change) = counter();
        let _subscription = subscribe_diff_viewer(&kv, on_change);
        assert_eq!(load_diff_viewer(&kv), DiffViewer::Editor);
        save_diff_viewer(&kv, DiffViewer::Unified);
        assert_eq!(kv.get_item(DIFF_VIEWER_KEY).as_deref(), Some("unified"));
        assert_eq!(load_diff_viewer(&kv), DiffViewer::Unified);
        save_diff_viewer(&kv, DiffViewer::Editor);
        assert_eq!(load_diff_viewer(&kv), DiffViewer::Editor);
        kv.set_item(DIFF_VIEWER_KEY, "split");
        assert_eq!(load_diff_viewer(&kv), DiffViewer::Editor);
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn file_tab_mode_persists_top_level_file_tabs() {
        let kv = Kv::in_memory();
        assert_eq!(load_file_tab_mode(&kv), FileTabMode::Pane);
        save_file_tab_mode(&kv, FileTabMode::Workspace);
        assert_eq!(kv.get_item(FILE_TAB_MODE_KEY).as_deref(), Some("workspace"));
        assert_eq!(load_file_tab_mode(&kv), FileTabMode::Workspace);
        kv.set_item(FILE_TAB_MODE_KEY, "window");
        assert_eq!(load_file_tab_mode(&kv), FileTabMode::Pane);
    }

    #[test]
    fn collapsed_project_rail_persists_the_hidden_mode() {
        let kv = Kv::in_memory();
        let (count, on_change) = counter();
        let _subscription = subscribe_collapsed_project_rail_mode(&kv, on_change);
        assert_eq!(
            load_collapsed_project_rail_mode(&kv),
            CollapsedProjectRailMode::Compact
        );
        save_collapsed_project_rail_mode(&kv, CollapsedProjectRailMode::Hidden);
        assert_eq!(
            kv.get_item(COLLAPSED_PROJECT_RAIL_MODE_KEY).as_deref(),
            Some("hidden")
        );
        assert_eq!(
            load_collapsed_project_rail_mode(&kv),
            CollapsedProjectRailMode::Hidden
        );
        kv.set_item(COLLAPSED_PROJECT_RAIL_MODE_KEY, "floating");
        assert_eq!(
            load_collapsed_project_rail_mode(&kv),
            CollapsedProjectRailMode::Compact
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn settings_section_persists() {
        let kv = Kv::in_memory();
        assert_eq!(load_settings_section(&kv), SettingsSectionId::General);
        save_settings_section(&kv, SettingsSectionId::Mcp);
        assert_eq!(kv.get_item(SECTION_KEY).as_deref(), Some("mcp"));
        assert_eq!(load_settings_section(&kv), SettingsSectionId::Mcp);
    }

    // keybinding overrides
    #[test]
    fn persists_a_custom_shortcut_and_matches_only_that_combination() {
        let kv = Kv::in_memory();
        let (count, on_change) = counter();
        let _subscription = subscribe_keybindings(&kv, on_change);
        save_keybinding_override(&kv, "App: Search", &shortcut("Command+Shift+KeyM"), MAC).unwrap();
        assert_eq!(
            kv.get_item(KEYBINDING_OVERRIDES_KEY).as_deref(),
            Some(r#"{"App: Search":{"shortcut":"Command+Shift+KeyM"}}"#)
        );
        assert!(keybinding_pressed(
            &kv,
            "App: Search",
            &key("KeyM", meta_shift()),
            true,
            MAC
        ));
        assert!(!keybinding_pressed(
            &kv,
            "App: Search",
            &key("KeyK", meta()),
            true,
            MAC
        ));
        assert_eq!(
            match_custom_keybinding(&kv, &key("KeyM", meta_shift()), MAC).as_deref(),
            Some("App: Search")
        );
        save_keybinding_override(&kv, "App: Search", &KeybindingOverride::default(), MAC).unwrap();
        assert_eq!(kv.get_item(KEYBINDING_OVERRIDES_KEY), None);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn serves_a_cached_value_without_letting_callers_mutate_it() {
        let kv = Kv::in_memory();
        save_keybinding_override(&kv, "App: Search", &shortcut("Command+KeyY"), MAC).unwrap();
        let mut first = load_keybinding_overrides(&kv, MAC);
        first.insert("App: Go to File".into(), disabled());
        assert!(!load_keybinding_overrides(&kv, MAC).contains_key("App: Go to File"));
        // The cache follows the stored string.
        kv.set_item(
            KEYBINDING_OVERRIDES_KEY,
            r#"{"Tab: New":{"disabled":true}}"#,
        );
        assert_eq!(
            load_keybinding_overrides(&kv, MAC),
            KeybindingOverrides::from([("Tab: New".to_string(), disabled())])
        );
    }

    #[test]
    fn disables_a_shortcut_and_restores_the_default() {
        let kv = Kv::in_memory();
        save_keybinding_override(&kv, "App: Search", &disabled(), MAC).unwrap();
        assert_eq!(
            load_keybinding_overrides(&kv, MAC),
            KeybindingOverrides::from([("App: Search".to_string(), disabled())])
        );
        assert!(!keybinding_pressed(
            &kv,
            "App: Search",
            &key("KeyK", meta()),
            true,
            MAC
        ));
        assert_eq!(
            keybinding_shortcut_label(&kv, "App: Search", "⌘K", MAC),
            None
        );
        assert_eq!(
            keybinding_shortcut_tokens(&kv, "App: Search", "Meta+K", MAC),
            None
        );
        assert_eq!(
            keybinding_shortcut_label(&kv, "App: Go to File", "⌘P", MAC).as_deref(),
            Some("⌘P")
        );
    }

    #[test]
    fn rejects_a_shortcut_already_used_by_another_command() {
        let kv = Kv::in_memory();
        save_keybinding_override(&kv, "App: Search", &shortcut("Command+KeyY"), MAC).unwrap();
        assert_eq!(
            save_keybinding_override(&kv, "App: Go to File", &shortcut("Command+KeyY"), MAC),
            Err("Already used by App: Search".into())
        );
        assert_eq!(
            validate_keybinding_shortcut(&kv, "App: Go to File", "Command+KeyY", MAC),
            Err("Already used by App: Search".into())
        );
        assert_eq!(
            load_keybinding_overrides(&kv, MAC).len(),
            1,
            "a rejected save stores nothing"
        );
    }

    #[test]
    fn rejects_a_shortcut_that_shadows_another_commands_default() {
        for (platform, primary) in [(Platform::Mac, "Command"), (Platform::Windows, "Control")] {
            let kv = Kv::in_memory();
            assert_eq!(
                save_keybinding_override(
                    &kv,
                    "App: Search",
                    &shortcut(&format!("{primary}+KeyP")),
                    platform
                ),
                Err("Already used by App: Go to File".into())
            );
            assert_eq!(
                save_keybinding_override(&kv, "Tab: New", &shortcut("Control+Tab"), platform),
                Err("Already used by Tab: Cycle Next".into())
            );
        }
    }

    #[test]
    fn rejects_a_chord_the_command_cannot_use_and_invalid_chords() {
        let kv = Kv::in_memory();
        assert!(
            save_keybinding_override(&kv, "Tab: Activate 1–8", &shortcut("Control+KeyM"), MAC)
                .unwrap_err()
                .contains("needs a number key")
        );
        assert!(
            save_keybinding_override(&kv, "App: Search", &shortcut("KeyK"), MAC)
                .unwrap_err()
                .contains("not a valid shortcut")
        );
        assert!(kv.is_empty());
    }

    #[test]
    fn normalises_modifier_order_when_reading_stored_shortcuts() {
        let kv = Kv::in_memory();
        kv.set_item(
            KEYBINDING_OVERRIDES_KEY,
            r#"{"App: Search":{"shortcut":"Shift+Command+KeyM"}}"#,
        );
        assert_eq!(
            load_keybinding_overrides(&kv, MAC),
            KeybindingOverrides::from([(
                "App: Search".to_string(),
                shortcut("Command+Shift+KeyM")
            )])
        );
    }

    #[test]
    fn ignores_malformed_unknown_and_invalid_stored_overrides() {
        let kv = Kv::in_memory();
        kv.set_item(
            KEYBINDING_OVERRIDES_KEY,
            r#"{"Unknown: Command":{"disabled":true},"App: Search":{"shortcut":"KeyK"},"Tab: New":{"shortcut":"Command+KeyT"}}"#,
        );
        assert_eq!(
            load_keybinding_overrides(&kv, MAC),
            KeybindingOverrides::from([("Tab: New".to_string(), shortcut("Command+KeyT"))])
        );
        kv.set_item(KEYBINDING_OVERRIDES_KEY, "not-json");
        assert!(load_keybinding_overrides(&kv, MAC).is_empty());
    }

    #[test]
    fn shows_overrides_and_quick_composer_state_in_the_table() {
        let kv = Kv::in_memory();
        save_keybinding_override(&kv, "App: Search", &shortcut("Command+Shift+KeyM"), MAC).unwrap();
        save_quick_composer_enabled(&kv, false);
        let rows = current_keybindings(&kv, MAC);
        let find = |command: &str| {
            rows.iter()
                .find(|row| row.command == command)
                .unwrap()
                .keys
                .clone()
        };
        assert_eq!(find("App: Search"), "⌘⇧M");
        assert_eq!(find(QUICK_COMPOSER_COMMAND), "Disabled");
        assert_eq!(find("App: Go to File"), "⌘P");
        assert_eq!(
            keybinding_shortcut_tokens(&kv, "App: Search", "Meta+K", MAC).as_deref(),
            Some("Meta+Shift+M")
        );
    }

    #[test]
    fn loads_every_group_through_the_store() {
        let kv = Kv::in_memory();
        save_follow_up_behavior(&kv, FollowUpBehavior::Queue);
        kv.set_item(appearance::THEME_HUE_KEY, "120");
        let all = load_app_settings(&kv, MAC);
        assert_eq!(all.settings.follow_up_behavior, FollowUpBehavior::Queue);
        assert_eq!(all.appearance.theme_hue, 120);
        assert_eq!(load_settings(&kv, MAC), all.settings);
    }
}
