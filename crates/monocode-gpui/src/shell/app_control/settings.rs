//! The settings agents may read and change. Listed by hand, so anything that
//! runs code, grants access or exposes data stays with the person: binary
//! paths, hooks, skills, keybindings, device agents, default access, accounts.
use super::*;
use crate::settings::Preferences;

enum Kind {
    Flag,
    Choice(&'static [&'static str]),
    Number(&'static [u32]),
    Range(u32, u32),
}
use Kind::*;

const SETTINGS: [(&str, Kind); 32] = [
    ("sounds", Flag),
    ("notifications", Flag),
    ("file_word_wrap", Flag),
    ("tab_animations", Flag),
    ("quick_composer", Flag),
    ("search_panel", Flag),
    ("working_agents", Flag),
    ("action_opens_terminal", Flag),
    ("show_excluded_files", Flag),
    ("anchor_prompts", Flag),
    ("paced_reveal", Flag),
    ("collapse_tool_groups", Flag),
    ("fold_finished_work", Flag),
    ("autocorrect", Flag),
    ("format_on_save", Flag),
    ("autosave", Flag),
    ("composer_mascot", Flag),
    ("show_remaining_usage", Flag),
    ("body_glass", Flag),
    ("theme", Choice(&["system", "dark", "light"])),
    ("terminal_placement", Choice(&["bottom", "panel", "title"])),
    ("file_tabs", Choice(&["pane", "workspace"])),
    ("transcript_layout", Choice(&["full", "chat"])),
    ("follow_up", Choice(&["queue", "steer"])),
    ("model_controls", Choice(&["menu", "beside"])),
    ("diff_view", Choice(&["editor", "unified"])),
    (
        "diff_palette",
        Choice(&["default", "colorblind", "high-contrast"]),
    ),
    ("empty_session", Choice(&["ink", "games", "none"])),
    ("ui_scale", Number(&crate::settings::UI_SCALE_PERCENTS)),
    ("sidebar_opacity", Range(15, 100)),
    ("theme_hue", Range(0, 360)),
    ("theme_saturation", Range(0, 100)),
];

fn check(key: &str, value: &Value) -> Result<(), String> {
    let (_, kind) = SETTINGS
        .iter()
        .find(|(name, _)| *name == key)
        .ok_or_else(|| format!("{key} isn't a setting agents can change"))?;
    let valid = match kind {
        Flag => value.is_boolean(),
        Choice(options) => value.as_str().is_some_and(|v| options.contains(&v)),
        Number(options) => value
            .as_u64()
            .is_some_and(|v| options.iter().any(|o| u64::from(*o) == v)),
        Range(low, high) => value
            .as_u64()
            .is_some_and(|v| (u64::from(*low)..=u64::from(*high)).contains(&v)),
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{key} can't be {value}"))
    }
}

fn values(prefs: &Preferences) -> Result<Value, String> {
    let mut saved = serde_json::to_value(prefs).map_err(|e| e.to_string())?;
    saved["terminal_placement"] = prefs.terminal_placement().into();
    Ok(saved)
}

/// `prefs` with `changes` applied, or why none of them can be.
fn changed(prefs: &Preferences, changes: &Value) -> Result<Preferences, String> {
    let changes = changes.as_object().ok_or("set must be an object")?;
    let mut saved = serde_json::to_value(prefs).map_err(|e| e.to_string())?;
    for (key, value) in changes {
        check(key, value)?;
        saved[key] = value.clone();
    }
    let mut next: Preferences = serde_json::from_value(saved).map_err(|e| e.to_string())?;
    if let Some(placement) = changes.get("terminal_placement").and_then(Value::as_str) {
        next.set_terminal_placement(placement);
    }
    Ok(next)
}

impl Shell {
    pub(super) fn control_settings(&self) -> Value {
        let saved = values(&self.workspace.preferences).unwrap_or_default();
        let mut settings = serde_json::Map::new();
        let mut options = serde_json::Map::new();
        for (key, kind) in &SETTINGS {
            settings.insert((*key).into(), saved[*key].clone());
            let allowed = match kind {
                Flag => continue,
                Choice(choices) => json!(choices),
                Number(choices) => json!(choices),
                Range(low, high) => json!({"min":low,"max":high}),
            };
            options.insert((*key).into(), allowed);
        }
        json!({"settings":settings,"options":options})
    }

    /// As if the person changed them in Settings: all of them or none.
    pub(super) fn control_configure(
        &mut self,
        changes: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let prefs = changed(&self.workspace.preferences, changes)?;
        self.workspace.preferences = prefs;
        self.apply_preferences(cx);
        self.persist(cx);
        self.invalidate_chrome(cx);
        self.sync_settings(cx);
        cx.notify();
        Ok(self.control_settings())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_setting_is_a_real_preference() {
        let saved = values(&Preferences::default()).unwrap();
        for (key, _) in &SETTINGS {
            assert!(saved.get(*key).is_some(), "{key}");
        }
    }

    #[test]
    fn a_bad_value_changes_nothing() {
        let prefs = Preferences::default();
        assert!(changed(&prefs, &json!({"theme":"light","sounds":"loud"})).is_err());
        assert!(changed(&prefs, &json!({"ui_scale":7})).is_err());
        assert!(changed(&prefs, &json!({"sidebar_opacity":101})).is_err());
        let next = changed(&prefs, &json!({"theme":"light"})).unwrap();
        assert_eq!(next.theme, "light");
    }

    #[test]
    fn settings_that_grant_power_are_refused() {
        let prefs = Preferences::default();
        for change in [
            json!({"default_access":"full-access"}),
            json!({"binary_paths":{"claude":"/tmp/x"}}),
            json!({"claude_hooks":true}),
            json!({"device_agents":true}),
            json!({"keybindings":{}}),
            json!({"mask_emails":false}),
        ] {
            let error = changed(&prefs, &change).unwrap_err();
            assert!(error.contains("isn't a setting"), "{change}");
        }
    }

    #[test]
    fn terminal_placement_keeps_its_legacy_flag_in_step() {
        let prefs = Preferences::default();
        let next = changed(&prefs, &json!({"terminal_placement":"panel"})).unwrap();
        assert!(next.terminal_in_panel);
        assert_eq!(next.terminal_placement(), "panel");
    }
}
