//! Desktop localStorage settings expressed in native workspace field names.
use super::{reconcile_preference, DesktopPreferences};
use serde_json::{json, Map, Value};

impl DesktopPreferences {
    pub(super) fn apply_preferences(
        &self,
        target: &mut Map<String, Value>,
        baseline: &Value,
        next: &mut Map<String, Value>,
    ) {
        for (field, key, default) in [
            ("sounds", "sounds", true),
            ("notifications", "notifications", false),
            ("tab_animations", "tabAnimationsEnabled", false),
            ("quick_composer", "quickComposerEnabled", true),
            ("working_agents", "liveAgentsEnabled", true),
            ("close_to_tray", "closeToTray", true),
            ("inset_glass", "insetGlass", true),
            ("body_glass", "bodyGlass", true),
            ("show_excluded_files", "showExcludedFiles", false),
            ("anchor_prompts", "transcriptAnchor", true),
            ("paced_reveal", "pacedReveal", true),
            ("autocorrect", "composerAutocorrect", false),
            ("format_on_save", "formatOnSave", true),
            ("autosave", "autosave", false),
            ("composer_mascot", "composerRunner", true),
            ("show_remaining_usage", "showRemainingUsage", false),
            ("mask_emails", "maskEmails", true),
            ("claude_hooks", "claudeHooks", true),
            ("claude_entrypoint", "claudeEntrypoint", false),
            ("monos", "monosEnabled", true),
            ("rail_monos_pinned", "railMonosPinned", false),
            ("mono_menu_bar_icon", "monoMenuBarIcon", true),
        ] {
            reconcile_preference(
                target,
                baseline,
                next,
                field,
                json!(self.flag(key).unwrap_or(default)),
            );
        }
        // Store reconciliation runs before Workspace::restore. Migrate the
        // native choice here too, before a new desktop field can replace it.
        let games = target
            .remove("empty_session_games")
            .and_then(|value| value.as_bool());
        if let Some(games) = games {
            target
                .entry("empty_session")
                .or_insert_with(|| json!(if games { "games" } else { "none" }));
        }
        let source = json!(if self.flag("gridArcadeEnabled") == Some(false) {
            "none"
        } else {
            "ink"
        });
        let previous = baseline.get("empty_session").cloned().or_else(|| {
            baseline["empty_session_games"]
                .as_bool()
                .map(|games| json!(if games { "ink" } else { "none" }))
        });
        if previous.is_none() && target.contains_key("empty_session") {
            // First import of this field: retain the saved native choice and
            // record the desktop baseline for subsequent deliberate changes.
            next.insert("empty_session".into(), source);
        } else {
            // Translate the old desktop baseline too, so a desktop toggle
            // changed since the last native launch still takes effect.
            let baseline = json!({"empty_session":previous});
            reconcile_preference(target, &baseline, next, "empty_session", source);
        }
        for (field, key, allowed, default) in [
            (
                "theme",
                "colorScheme",
                &["dark", "light", "system"][..],
                "dark",
            ),
            (
                "file_tabs",
                "fileTabMode",
                &["pane", "workspace"][..],
                "pane",
            ),
            (
                "file_view_mode",
                "fileViewMode",
                &["preview", "source"][..],
                "preview",
            ),
            (
                "follow_up",
                "followUpBehavior",
                &["steer", "queue"][..],
                "steer",
            ),
            (
                "diff_view",
                "diffViewer",
                &["editor", "unified"][..],
                "editor",
            ),
            (
                "transcript_layout",
                "transcriptLayout",
                &["chat", "full"][..],
                "chat",
            ),
            (
                "diff_palette",
                "diffPalette",
                &["default", "colorblind", "high-contrast"][..],
                "default",
            ),
            (
                "collapsed_project_rail",
                "collapsedProjectRailMode",
                &["compact"][..],
                "compact",
            ),
            (
                "chat_background_scope",
                "chatBackgroundScope",
                &["empty", "all"][..],
                "all",
            ),
            (
                "chat_background_effect",
                "newThreadBackgroundEffect",
                &[
                    "none",
                    "dither",
                    "ascii",
                    "halftone",
                    "scanlines",
                    "gradient-blur",
                ][..],
                "none",
            ),
            (
                "workspace_theme",
                "workspaceTheme",
                &[
                    "default", "harbor", "moss", "ember", "orchid", "tidepool", "dune", "rosewood",
                    "glacier", "arcade", "ink",
                ][..],
                "default",
            ),
        ] {
            let value = self
                .get(&format!("monocode.{key}"))
                .filter(|value| allowed.contains(value))
                .unwrap_or(default);
            reconcile_preference(target, baseline, next, field, json!(value));
        }
        for (field, key, default, min, max, scale) in [
            ("theme_hue", "themeHue", 240., 0., 360., 1.),
            ("theme_saturation", "themeSaturation", 0., 0., 100., 1.),
            (
                "theme_dark_lightness",
                "themeDarkLightness",
                9.,
                0.,
                30.,
                1.,
            ),
            ("sidebar_opacity", "sidebarOpacity", 0.85, 0.15, 1., 100.),
            ("sidebar_blur", "sidebarBlur", 24., 1., 64., 1.),
            ("ui_scale", "uiScale", 1., 0.5, 2., 100.),
        ] {
            let mut number = self.number(key).unwrap_or(default).clamp(min, max);
            if field == "ui_scale" {
                number = (number * 10.).round() / 10.;
            }
            reconcile_preference(
                target,
                baseline,
                next,
                field,
                json!((number * scale).round() as u32),
            );
        }
        for (field, key) in [
            ("chat_background_empty", "chatBackgroundEmptyOpacity"),
            ("chat_background_session", "chatBackgroundSessionOpacity"),
        ] {
            let opacity = self
                .number(key)
                .or_else(|| self.number("chatBackgroundOpacity"))
                .unwrap_or(0.24)
                .clamp(0.05, 0.65);
            reconcile_preference(
                target,
                baseline,
                next,
                field,
                json!((opacity * 100.).round() as u32),
            );
        }
        reconcile_preference(
            target,
            baseline,
            next,
            "accent_color",
            json!(self.get("monocode.accentColor").filter(|v| valid_hex(v))),
        );
        reconcile_preference(
            target,
            baseline,
            next,
            "chat_background",
            json!(self
                .get("monocode.chatBackgroundPath")
                .filter(|v| !v.trim().is_empty())),
        );
        let idle = self
            .number("idleAgentLimit")
            .filter(|v| [0., 1., 2., 3., 5.].contains(v))
            .unwrap_or(3.);
        reconcile_preference(target, baseline, next, "idle_agents", json!(idle as u32));
        for (field, key, default) in [
            (
                "project_notifications",
                "monocode.projectNotifications.v1",
                json!({}),
            ),
            (
                "notification_projects",
                "monocode.notificationProjects.v2",
                json!([]),
            ),
        ] {
            let value = self
                .json(key)
                .filter(|value| {
                    if field == "project_notifications" {
                        value.is_object()
                    } else {
                        value.is_array()
                    }
                })
                .unwrap_or(default);
            reconcile_preference(target, baseline, next, field, value);
        }
        let paths: Map<String, Value> = self
            .json("monocode.providerBinaryPaths.v1")
            .as_ref()
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(provider, _)| crate::provider::Kind::parse(provider).is_some())
            .filter_map(|(provider, value)| {
                value
                    .as_str()
                    .filter(|path| !path.trim().is_empty())
                    .map(|path| (provider.clone(), json!(path.trim())))
            })
            .collect();
        reconcile_preference(target, baseline, next, "binary_paths", json!(paths));
        let disabled = self
            .json("monocode.disabledSkillPaths")
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        reconcile_preference(
            target,
            baseline,
            next,
            "disabled_skill_paths",
            json!(disabled),
        );
        reconcile_preference(
            target,
            baseline,
            next,
            "default_models",
            json!(native_models(
                &self.json("monocode.defaultModels").unwrap_or(Value::Null)
            )),
        );
        reconcile_preference(
            target,
            baseline,
            next,
            "hidden_providers",
            json!(providers(
                &self
                    .json("monocode.hiddenPickerProviders")
                    .unwrap_or(Value::Null)
            )),
        );
        let filters = self.json("monocode.sessionSidebarFilters");
        let archived = filters
            .as_ref()
            .map(|v| v["showArchived"] == true)
            .unwrap_or_else(|| self.flag("sessionsShowArchived").unwrap_or(false));
        reconcile_preference(target, baseline, next, "show_archived", json!(archived));
        let keys: Map<String, Value> = self
            .json("monocode.keybindingOverrides")
            .as_ref()
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(command, value)| {
                if let Some(shortcut) = value["shortcut"].as_str().and_then(shortcut) {
                    Some((command.clone(), json!(shortcut)))
                } else if value["disabled"] == true {
                    Some((command.clone(), Value::Null))
                } else {
                    None
                }
            })
            .collect();
        reconcile_preference(target, baseline, next, "keybindings", json!(keys));
        let favorites = self.json("monocode.favoriteModels");
        let mut choices = Vec::new();
        for id in favorites
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let Some((provider, model)) = id.split_once(':') else {
                continue;
            };
            if let Some(choice) =
                model_choice(provider, model).filter(|choice| !choices.contains(choice))
            {
                choices.push(choice);
            }
        }
        reconcile_preference(target, baseline, next, "favorite_models", json!(choices));
        let recent = self.json("monocode.recentModels");
        let mut choices = Vec::new();
        for choice in recent
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(provider), Some(model)) =
                (choice["harness"].as_str(), choice["model"].as_str())
            else {
                continue;
            };
            if let Some(choice) =
                model_choice(provider, model).filter(|choice| !choices.contains(choice))
            {
                choices.push(choice);
            }
            if choices.len() == 6 {
                break;
            }
        }
        reconcile_preference(target, baseline, next, "recent_models", json!(choices));
        let tab = self
            .get("monocode.modelPickerTab")
            .filter(|tab| *tab == "favorites" || crate::provider::Kind::parse(tab).is_some())
            .unwrap_or("favorites");
        reconcile_preference(target, baseline, next, "model_picker_tab", json!(tab));
    }

    pub(super) fn apply_view_preferences(
        &self,
        target: &mut Map<String, Value>,
        baseline: &Value,
        next: &mut Map<String, Value>,
    ) {
        let old = baseline.get("workspace").unwrap_or(&Value::Null);
        // A fresh desktop import names the actual navigation layout. Old
        // native sidecars used classic_layout solely for the boxy frame and
        // migrate in Workspace::restore instead.
        if !target.contains_key("classic_layout") {
            target.insert("new_layout_boxy".into(), json!(false));
        }
        let mut source = Map::new();
        reconcile_preference(
            target,
            old,
            &mut source,
            "new_layout_prompt_dismissed",
            json!(self.flag("newLayoutPromptDismissed").unwrap_or(false)),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "classic_layout",
            json!(!self.flag("insetLayout").unwrap_or(false)),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "sidebar_visible",
            json!(self.flag("sessionSidebarOpen").unwrap_or(true)),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "project_rail_open",
            json!(self.flag("projectRailOpen").unwrap_or(true)),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "project_rail_width",
            json!(self
                .number("projectRailWidth")
                .unwrap_or(220.)
                .clamp(176., 360.)
                .round()),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "right_panel_open",
            json!(self.flag("rightPanelOpen").unwrap_or(false)),
        );
        reconcile_preference(
            target,
            old,
            &mut source,
            "right_panel_width",
            json!(self
                .number("rightPanelWidth")
                .unwrap_or(288.)
                .clamp(220., 640.)
                .round()),
        );
        let tab = self
            .get("monocode.rightPanelTab")
            .filter(|s| ["files", "changes", "graph"].contains(s))
            .unwrap_or("files");
        reconcile_preference(target, old, &mut source, "right_panel_tab", json!(tab));
        let order = self
            .json("monocode.rightPanelTabOrder")
            .or_else(|| self.json("monocode.sidebarTabOrder"));
        let mut tabs = Vec::new();
        for tab in order
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .chain(["files", "changes", "graph"])
        {
            if ["files", "changes", "graph"].contains(&tab) && !tabs.contains(&tab) {
                tabs.push(tab);
            }
        }
        reconcile_preference(target, old, &mut source, "right_panel_tabs", json!(tabs));
        reconcile_preference(
            target,
            old,
            &mut source,
            "changes_view",
            json!(if self.get("monocode.changesView") == Some("tree") {
                "tree"
            } else {
                "list"
            }),
        );
        let collapsed = self
            .json("monocode:tab-groups:collapsed")
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        reconcile_preference(target, old, &mut source, "collapsed", json!(collapsed));
        let filters = self
            .json("monocode.sessionSidebarFilters")
            .unwrap_or(Value::Null);
        let time = filters["time"]
            .as_str()
            .filter(|s| ["all", "today", "7d", "30d"].contains(s))
            .unwrap_or("all");
        reconcile_preference(
            target,
            old,
            &mut source,
            "session_filters",
            json!({"hidden_providers":providers(&filters["hiddenHarnesses"]), "time":time, "status":{"working":filters["status"]["working"] == true, "needs_approval":filters["status"]["needsApproval"] == true, "done":filters["status"]["done"] == true}}),
        );
        next.insert("workspace".into(), json!(source));
        // Preserve exact strings for source concepts with no native equivalent.
        let metadata: Map<String, Value> = [
            "monocode.sessionFolders",
            "monocode.pinnedSessionsCollapsed",
            "monocode.reminderSessionsCollapsed",
            "monocode.projectProviderSettings.v1",
            "monocode.archivedProjects",
            "monocode.settingsSection",
            "monocode.quickComposerShortcut",
            "monocode.sidebarOpen",
            "monocode.sidebarLayout",
            "monocode.favoriteModels",
            "monocode.recentModels",
            "monocode.projectGroups",
            "monocode.projectGroupAssignments",
            "monocode.projectLocations",
            "monocode.projectSidebarTabs.v1",
            "monocode.sidebarTabOrder",
            "monocode.projectRailOpen",
            "monocode.projectRailWidth",
            "monocode.projectRailPinned",
            "monocode.projectRailOrder",
            "monocode.railMonosPinned",
            "monocode.transcriptZen",
            "monocode.projectNotifications.v1",
            "monocode.notificationProjects.v2",
            "monocode.newLayoutPromptDismissed",
            "monocode.linkedSessionSeen",
        ]
        .into_iter()
        .filter_map(|key| self.get(key).map(|value| (key.to_owned(), json!(value))))
        .collect();
        target.insert("desktop_preferences_metadata".into(), json!(metadata));
    }

    fn flag(&self, key: &str) -> Option<bool> {
        self.get(&format!("monocode.{key}"))
            .map(|value| matches!(value, "1" | "true"))
    }
    pub(super) fn number(&self, key: &str) -> Option<f64> {
        self.get(&format!("monocode.{key}"))?
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
}

fn model_choice(provider: &str, model: &str) -> Option<Value> {
    crate::provider::Kind::parse(provider)?;
    if model.split_once(':').is_some_and(|(prefix, _)| {
        crate::provider::Kind::parse(prefix).is_some() && prefix != provider
    }) {
        return None;
    }
    Some(json!({"provider":provider, "model":crate::store::native_model(model, provider)}))
}

pub(super) fn valid_hex(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
pub(super) fn providers(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for provider in value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| crate::provider::Kind::parse(id).is_some())
    {
        if !out.iter().any(|id| id == provider) {
            out.push(provider.to_owned());
        }
    }
    out
}
pub(super) fn native_models(value: &Value) -> Map<String, Value> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(provider, model)| {
            crate::provider::Kind::parse(provider)?;
            Some((
                provider.clone(),
                json!(crate::store::native_model(model.as_str()?, provider)?),
            ))
        })
        .collect()
}

// Desktop chords use DOM physical codes. Native keymaps use GPUI key names.
fn shortcut(value: &str) -> Option<String> {
    let mut parts = value.split('+').collect::<Vec<_>>();
    let code = parts.pop()?;
    if parts.is_empty() {
        return None;
    }
    let mut modifiers = Vec::new();
    for part in parts {
        let native = match part {
            "Command" => "cmd",
            "Control" => "ctrl",
            "Option" => "alt",
            "Shift" => "shift",
            _ => return None,
        };
        if modifiers.contains(&native) {
            return None;
        }
        modifiers.push(native);
    }
    let key = match code {
        "Space" => "space",
        "Enter" | "NumpadEnter" => "enter",
        "Tab" => "tab",
        "Backspace" => "backspace",
        "Delete" => "delete",
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "Backquote" => "`",
        "Backslash" => "\\",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Comma" => ",",
        "Equal" => "=",
        "Minus" => "-",
        "Period" => ".",
        "Quote" => "'",
        "Semicolon" => ";",
        "Slash" => "/",
        _ if code
            .strip_prefix("Key")
            .is_some_and(|v| v.len() == 1 && v.bytes().all(|b| b.is_ascii_uppercase())) =>
        {
            &code[3..]
        }
        _ if code
            .strip_prefix("Digit")
            .is_some_and(|v| v.len() == 1 && v.bytes().all(|b| b.is_ascii_digit())) =>
        {
            &code[5..]
        }
        _ if code
            .strip_prefix('F')
            .and_then(|v| v.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n)) =>
        {
            code
        }
        _ => return None,
    };
    Some(format!(
        "{}-{}",
        modifiers.join("-"),
        key.to_ascii_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(entries: &[(&str, &str)]) -> DesktopPreferences {
        DesktopPreferences {
            available: true,
            values: entries
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
        }
    }

    #[test]
    fn empty_session_migration_precedes_desktop_reconciliation() {
        let enabled = source(&[("monocode.gridArcadeEnabled", "1")]);
        let disabled = source(&[("monocode.gridArcadeEnabled", "0")]);
        for (preferences, expected) in [
            (json!({}), "ink"),
            (json!({"empty_session_games":false}), "none"),
            (json!({"empty_session_games":true}), "games"),
            (
                json!({"empty_session_games":false,"empty_session":"games"}),
                "games",
            ),
            (
                json!({"empty_session_games":true,"empty_session":"none"}),
                "none",
            ),
        ] {
            let mut workspace = json!({
                "preferences":preferences,
                "desktop_project_preferences":{"empty_session_games":true}
            });
            for _ in 0..2 {
                enabled.apply_workspace(&mut workspace);
                assert_eq!(workspace["preferences"]["empty_session"], expected);
                assert!(workspace["preferences"]
                    .get("empty_session_games")
                    .is_none());
            }
            disabled.apply_workspace(&mut workspace);
            assert_eq!(workspace["preferences"]["empty_session"], "none");
            enabled.apply_workspace(&mut workspace);
            assert_eq!(workspace["preferences"]["empty_session"], "ink");
        }
    }

    #[test]
    fn desktop_empty_session_changes_during_upgrade_still_apply() {
        for (before, value, expected) in [(true, "0", "none"), (false, "1", "ink")] {
            let mut workspace = json!({
                "preferences":{"empty_session_games":before},
                "desktop_project_preferences":{"empty_session_games":before}
            });
            source(&[("monocode.gridArcadeEnabled", value)]).apply_workspace(&mut workspace);
            assert_eq!(workspace["preferences"]["empty_session"], expected);
        }
    }

    #[test]
    fn desktop_settings_and_sidebar_filters_use_native_names_units_and_enums() {
        let preferences = source(&[
            ("monocode.sounds", "0"),
            ("monocode.colorScheme", "light"),
            ("monocode.workspaceTheme", "tidepool"),
            ("monocode.insetLayout", "1"),
            ("monocode.sessionSidebarOpen", "0"),
            ("monocode.projectRailOpen", "1"),
            ("monocode.projectRailWidth", "248"),
            ("monocode.collapsedProjectRailMode", "hidden"),
            ("monocode.sidebarOpacity", "0.42"),
            ("monocode.uiScale", "1.4"),
            ("monocode.chatBackgroundSessionOpacity", "0.31"),
            (
                "monocode.rightPanelTabOrder",
                "[\"changes\",\"files\",\"changes\"]",
            ),
            ("monocode.changesView", "tree"),
            (
                "monocode.sessionSidebarFilters",
                r#"{"showArchived":true,"hiddenHarnesses":["codex","unknown","codex"],"time":"7d","status":{"needsApproval":true}}"#,
            ),
            (
                "monocode.defaultModels",
                r#"{"codex":"codex:gpt-6.1-sol","claude":"claude:opus-4.8"}"#,
            ),
            (
                "monocode.keybindingOverrides",
                r#"{"App: Search":{"shortcut":"Command+KeyJ"},"App: Settings":{"disabled":true}}"#,
            ),
        ]);
        let mut workspace = json!({});
        preferences.apply_workspace(&mut workspace);
        let settings = &workspace["preferences"];
        assert_eq!(settings["sounds"], false);
        assert_eq!(settings["theme"], "light");
        assert_eq!(settings["workspace_theme"], "tidepool");
        assert_eq!(settings["sidebar_opacity"], 42);
        assert_eq!(settings["ui_scale"], 140);
        assert_eq!(settings["chat_background_session"], 31);
        assert_eq!(settings["show_archived"], true);
        assert_eq!(settings["default_models"]["codex"], "gpt-6.1-sol");
        assert_eq!(settings["default_models"]["claude"], "claude-opus-4-8");
        assert_eq!(settings["keybindings"]["App: Search"], "cmd-j");
        assert!(settings["keybindings"]["App: Settings"].is_null());
        assert_eq!(workspace["classic_layout"], false);
        assert_eq!(workspace["sidebar_visible"], false);
        assert_eq!(workspace["new_layout_boxy"], false);
        assert_eq!(workspace["project_rail_open"], true);
        assert_eq!(workspace["project_rail_width"], 248.);
        assert_eq!(settings["collapsed_project_rail"], "compact");
        assert_eq!(
            workspace["right_panel_tabs"],
            json!(["changes", "files", "graph"])
        );
        assert_eq!(workspace["changes_view"], "tree");
        let filters: crate::session_filters::Filters =
            serde_json::from_value(workspace["session_filters"].clone()).unwrap();
        assert_eq!(filters.hidden_providers, ["codex"]);
        assert_eq!(filters.time, crate::session_filters::TimeFilter::Week);
        assert!(filters.status.needs_approval);
    }

    #[test]
    fn unrelated_desktop_map_edits_preserve_native_model_choices_and_added_keys() {
        let initial = source(&[(
            "monocode.defaultModels",
            r#"{"codex":"codex:gpt-old","claude":"claude:opus-4.8"}"#,
        )]);
        let mut workspace = json!({});
        initial.apply_workspace(&mut workspace);
        workspace["preferences"]["default_models"]["codex"] = json!("gpt-native");
        workspace["preferences"]["default_models"]["cursor"] = json!("native-added");
        workspace["preferences"]["sounds"] = json!(false);
        let changed = source(&[(
            "monocode.defaultModels",
            r#"{"codex":"codex:gpt-old","claude":"claude:sonnet-4.8"}"#,
        )]);
        changed.apply_workspace(&mut workspace);
        assert_eq!(
            workspace["preferences"]["default_models"]["codex"],
            "gpt-native"
        );
        assert_eq!(
            workspace["preferences"]["default_models"]["cursor"],
            "native-added"
        );
        assert_eq!(
            workspace["preferences"]["default_models"]["claude"],
            "claude-sonnet-4-8"
        );
        assert_eq!(workspace["preferences"]["sounds"], false);
    }

    #[test]
    fn project_backgrounds_keep_independent_opacities_and_native_overrides() {
        let source = source(&[(
            "monocode:project-chat-backgrounds",
            r#"{"/work":{"path":"/images/background.png","emptyOpacity":0.2,"sessionOpacity":0.4,"scope":"empty","effect":"ascii"}}"#,
        )]);
        let mut workspace = json!({});
        source.apply_workspace(&mut workspace);
        let appearance: crate::project_appearance::ProjectAppearance =
            serde_json::from_value(workspace["project_appearance"]["/work"].clone()).unwrap();
        let background = appearance.background.unwrap();
        assert!((background.empty_opacity - 0.2).abs() < 1e-6);
        assert!((background.session_opacity - 0.4).abs() < 1e-6);
        assert_eq!(background.scope, crate::project_appearance::Scope::Empty);
        assert_eq!(background.effect, crate::project_appearance::Effect::Ascii);
        workspace["project_appearance"]["/work"]["background"]["empty_opacity"] = json!(0.3);
        source.apply_workspace(&mut workspace);
        assert_eq!(
            workspace["project_appearance"]["/work"]["background"]["empty_opacity"],
            0.3
        );
    }

    #[test]
    fn desktop_chords_translate_without_interpreting_unknown_codes() {
        assert_eq!(
            shortcut("Command+Shift+KeyP").as_deref(),
            Some("cmd-shift-p")
        );
        assert_eq!(
            shortcut("Control+Option+ArrowLeft").as_deref(),
            Some("ctrl-alt-left")
        );
        assert!(shortcut("Command+Invalid").is_none());
    }

    #[test]
    fn source_picker_choices_and_file_mode_reconcile_and_unported_prefs_stay_exact() {
        let source = source(&[
            (
                "monocode.favoriteModels",
                r#"["claude:opus-4.8","codex:gpt-6.1-sol","claude:opus-4.8","unknown:model"]"#,
            ),
            (
                "monocode.recentModels",
                r#"[{"harness":"codex","model":"codex:gpt-6.1-sol"},{"harness":"claude","model":"claude:opus-4.8"}]"#,
            ),
            ("monocode.modelPickerTab", "codex"),
            ("monocode.fileViewMode", "source"),
            ("monocode.projectSidebarTabs.v1", r#"{"/work":"sessions"}"#),
        ]);
        let mut workspace = json!({});
        source.apply_workspace(&mut workspace);
        assert_eq!(
            workspace["preferences"]["favorite_models"],
            json!([
                {"provider":"claude", "model":"claude-opus-4-8"},
                {"provider":"codex", "model":"gpt-6.1-sol"}
            ])
        );
        assert_eq!(
            workspace["preferences"]["recent_models"][0]["provider"],
            "codex"
        );
        assert_eq!(workspace["preferences"]["model_picker_tab"], "codex");
        assert_eq!(workspace["preferences"]["file_view_mode"], "source");
        assert_eq!(
            workspace["desktop_preferences_metadata"]["monocode.projectSidebarTabs.v1"],
            r#"{"/work":"sessions"}"#
        );
        workspace["preferences"]["model_picker_tab"] = json!("favorites");
        workspace["preferences"]["file_view_mode"] = json!("preview");
        source.apply_workspace(&mut workspace);
        assert_eq!(workspace["preferences"]["model_picker_tab"], "favorites");
        assert_eq!(workspace["preferences"]["file_view_mode"], "preview");
    }

    #[test]
    fn absent_desktop_inset_setting_uses_the_desktop_classic_default() {
        let mut workspace = json!({});
        source(&[]).apply_workspace(&mut workspace);
        assert_eq!(workspace["classic_layout"], true);
        let mut isolated = json!({"classic_layout":false});
        DesktopPreferences::default().apply_workspace(&mut isolated);
        assert_eq!(isolated["classic_layout"], false);
    }
}
