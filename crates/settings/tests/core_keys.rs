//! Every `*_KEY` constant in `monocode-core` round-trips through `Kv`: the
//! stored string survives a write, a reopen, and the parse into
//! `AppSettings`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use monocode_core::settings::*;
use monocode_core::{AppSettings, Platform, appearance, models, project_providers};
use monocode_settings::{APP_SETTINGS_KEYS, KV_FILE_NAME, Kv, load_app_settings};

/// A valid stored string for every key, each different from the default.
const SAMPLES: [(&str, &str); 53] = [
    (SECTION_KEY, "mcp"),
    (FOLLOW_UP_BEHAVIOR_KEY, "queue"),
    (FILE_TAB_MODE_KEY, "workspace"),
    (TAB_ANIMATIONS_ENABLED_KEY, "1"),
    (COLLAPSED_PROJECT_RAIL_MODE_KEY, "hidden"),
    (MODEL_CONTROLS_KEY, "beside"),
    (COMPOSER_EFFORT_VISIBLE_KEY, "1"),
    (COMPOSER_RUNNER_KEY, "0"),
    (NOTES_ENABLED_KEY, "0"),
    (QUICK_COMPOSER_ENABLED_KEY, "0"),
    (QUICK_COMPOSER_SHORTCUT_KEY, "Command+Option+KeyK"),
    (LIVE_AGENTS_ENABLED_KEY, "0"),
    (CLOSE_TO_TRAY_KEY, "0"),
    (GRID_ARCADE_ENABLED_KEY, "0"),
    (DIFF_VIEWER_KEY, "unified"),
    (FORMAT_ON_SAVE_KEY, "0"),
    (AUTOSAVE_KEY, "1"),
    (CLAUDE_HOOKS_KEY, "0"),
    (
        KEYBINDING_OVERRIDES_KEY,
        r#"{"App: Search":{"shortcut":"Command+Shift+KeyM"},"Tab: New":{"disabled":true}}"#,
    ),
    (appearance::ACCENT_COLOR_KEY, "#FF8800"),
    (appearance::THEME_HUE_KEY, "120"),
    (appearance::THEME_SATURATION_KEY, "40"),
    (appearance::THEME_DARK_LIGHTNESS_KEY, "12"),
    (appearance::OPACITY_KEY, "0.5"),
    (appearance::MAIN_OPACITY_KEY, "0.7"),
    (appearance::BLUR_KEY, "32"),
    (appearance::PROJECT_RAIL_OPEN_KEY, "0"),
    (appearance::SESSION_SIDEBAR_OPEN_KEY, "0"),
    (appearance::BODY_KEY, "0"),
    (appearance::SCHEME_KEY, "light"),
    (
        appearance::SIDEBAR_TAB_ORDER_KEY,
        r#"["changes","files","sessions","inbox"]"#,
    ),
    (appearance::PROJECT_RAIL_WIDTH_KEY, "240"),
    (appearance::TRANSCRIPT_LAYOUT_KEY, "full"),
    (appearance::TRANSCRIPT_ANCHOR_KEY, "0"),
    (
        appearance::CHAT_BACKGROUND_PATH_KEY,
        "/Users/me/Pictures/caf\u{e9} \u{1f600} \"wall\"\\paper.png",
    ),
    (appearance::CHAT_BACKGROUND_OPACITY_KEY, "0.3"),
    (appearance::CHAT_BACKGROUND_EMPTY_OPACITY_KEY, "0.4"),
    (appearance::CHAT_BACKGROUND_SESSION_OPACITY_KEY, "0.2"),
    (appearance::CHAT_BACKGROUND_SCOPE_KEY, "empty"),
    (appearance::CHAT_BACKGROUND_BLUR_KEY, "12"),
    (appearance::NEW_THREAD_BACKGROUND_EFFECT_KEY, "halftone"),
    (appearance::CHANGES_VIEW_KEY, "tree"),
    (appearance::DIFF_PALETTE_KEY, "colorblind"),
    (appearance::SHOW_EXCLUDED_FILES_KEY, "1"),
    (appearance::UI_SCALE_KEY, "1.2"),
    (models::FAVORITES_KEY, r#"["claude:opus","codex:gpt-5"]"#),
    (models::MODEL_PICKER_TAB_KEY, "codex"),
    (models::HIDDEN_PICKER_PROVIDERS_KEY, r#"["pi"]"#),
    (
        models::LAST_MODEL_KEY,
        r#"{"harness":"grok","model":"grok:grok-4.6"}"#,
    ),
    (models::LAST_MODEL_SETTINGS_KEY, r#"{"effort":"high"}"#),
    (
        models::DEFAULT_MODELS_KEY,
        r#"{"claude":"opus","codex":"gpt-5"}"#,
    ),
    (
        models::RECENT_MODELS_KEY,
        r#"[{"harness":"claude","model":"opus"},{"harness":"codex","model":"gpt-5"}]"#,
    ),
    (
        project_providers::PROJECT_PROVIDER_SETTINGS_KEY,
        r#"{"/r":{"hidden":["pi"]}}"#,
    ),
];

/// Legacy keys that only count when their newer key is missing.
const FALLBACKS: [(&str, &str); 2] = [
    (COMPOSER_EFFORT_VISIBLE_KEY, MODEL_CONTROLS_KEY),
    (
        appearance::CHAT_BACKGROUND_OPACITY_KEY,
        appearance::CHAT_BACKGROUND_EMPTY_OPACITY_KEY,
    ),
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "monocode-settings-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every `const <NAME>_KEY: &str = "<value>";` in core's sources.
fn core_key_constants(dir: &Path, out: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            core_key_constants(&path, out);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        for line in fs::read_to_string(&path).unwrap().lines() {
            let line = line.trim();
            let line = line.strip_prefix("pub ").unwrap_or(line);
            let Some(rest) = line.strip_prefix("const ") else {
                continue;
            };
            let Some((name, value)) = rest.split_once(": &str = \"") else {
                continue;
            };
            if !name.ends_with("_KEY") {
                continue;
            }
            let value = value.strip_suffix("\";").expect("one-line key constant");
            out.insert(name.to_string(), value.to_string());
        }
    }
}

fn parse(get: impl Fn(&str) -> Option<String>) -> AppSettings {
    AppSettings::from_local_storage(get, Platform::Mac)
}

#[test]
fn the_key_list_covers_every_core_key_constant() {
    let core_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/src");
    let mut constants = BTreeMap::new();
    core_key_constants(&core_src, &mut constants);
    let in_core: BTreeSet<&str> = constants.values().map(String::as_str).collect();
    let listed: BTreeSet<&str> = APP_SETTINGS_KEYS.iter().copied().collect();
    let sampled: BTreeSet<&str> = SAMPLES.iter().map(|(key, _)| *key).collect();
    assert_eq!(in_core.len(), 53, "{constants:?}");
    assert_eq!(listed, in_core, "APP_SETTINGS_KEYS is out of date");
    assert_eq!(sampled, in_core, "SAMPLES is out of date");
    assert!(in_core.iter().all(|key| key.starts_with("monocode.")));
}

#[test]
fn every_core_key_round_trips_through_kv() {
    let dir = TempDir::new("core-keys");
    let kv = Kv::open(&dir.0).unwrap();
    for (key, value) in SAMPLES {
        kv.set_item(key, value);
    }
    kv.flush().unwrap();
    let before = load_app_settings(&kv, Platform::Mac);
    drop(kv);

    // The file holds exactly the stored strings.
    let file: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.0.join(KV_FILE_NAME)).unwrap()).unwrap();
    let items = file["items"].as_object().unwrap();
    assert_eq!(items.len(), SAMPLES.len());
    for (key, value) in SAMPLES {
        assert_eq!(items[key], value, "{key}");
    }

    let kv = Kv::open(&dir.0).unwrap();
    for (key, value) in SAMPLES {
        assert_eq!(kv.get_item(key).as_deref(), Some(value), "{key}");
    }
    let stored: HashMap<&str, &str> = SAMPLES.into_iter().collect();
    let direct = parse(|key| stored.get(key).map(|value| value.to_string()));
    let through_kv = load_app_settings(&kv, Platform::Mac);
    assert_eq!(through_kv, direct);
    assert_eq!(through_kv, before);
    assert_ne!(through_kv, AppSettings::default());
    let json = serde_json::to_string(&through_kv).unwrap();
    assert_eq!(
        serde_json::from_str::<AppSettings>(&json).unwrap(),
        through_kv
    );
}

#[test]
fn every_core_key_changes_the_parsed_settings() {
    let kv = Kv::in_memory();
    for (key, value) in SAMPLES {
        kv.set_item(key, value);
    }
    let without = |hidden: &[&str]| {
        parse(|name| {
            if hidden.contains(&name) {
                None
            } else {
                kv.get_item(name)
            }
        })
    };
    for (key, _) in SAMPLES {
        // A legacy key counts only once its newer key is gone, and a newer
        // key is tested with its legacy key gone, so neither hides the other.
        let partner: Vec<&str> = FALLBACKS
            .iter()
            .filter_map(|(legacy, newer)| match key {
                k if k == *legacy => Some(*newer),
                k if k == *newer => Some(*legacy),
                _ => None,
            })
            .collect();
        let mut hidden = partner.clone();
        hidden.push(key);
        assert_ne!(
            without(&hidden),
            without(&partner),
            "{key} does not reach AppSettings"
        );
    }
}
