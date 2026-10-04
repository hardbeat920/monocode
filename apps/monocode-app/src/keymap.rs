//! The app's actions and key bindings. Port of the keybinding table in
//! src/features/settings/model/settings.ts (`KEYBINDINGS`, now
//! `monocode_core::settings::keybindings`), the user overrides
//! (`monocode.keybindingOverrides`), src/app/model/appShortcuts.ts, and
//! `tabKeys` from monocode-layout, plus the capture-phase key handler in
//! App.tsx (lines 10066-10256).
//!
//! Every command in the table is a GPUI action here. The shell handles the
//! window-level ones (`Shell::register_actions`); quit, new window, and zoom
//! are app-level (`init`).

use gpui::{Action as _, App, Global, KeyBinding, actions};
use monocode_core::settings::KeybindingOverrides;
use std::collections::HashSet;

actions!(
    app,
    [
        // App.
        Quit,
        NewWindow,
        OpenSettings,
        OpenSearch,
        GoToFile,
        CommandPalette,
        FindInFiles,
        OpenProject,
        ToggleSidebar,
        ToggleSessionSidebar,
        SwitchModel,
        OpenInbox,
        OpenNotes,
        OpenAutomations,
        SidebarAppearance,
        CheckForUpdates,
        ToggleAutosave,
        // View.
        Reload,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        // Tabs.
        NewSession,
        CloseOtherTabs,
        CloseAllTabs,
        NextTab,
        PrevTab,
        CycleNextTab,
        CyclePrevTab,
        GoBack,
        GoForward,
        ActivateTab1,
        ActivateTab2,
        ActivateTab3,
        ActivateTab4,
        ActivateTab5,
        ActivateTab6,
        ActivateTab7,
        ActivateTab8,
        ActivateLastTab,
        // Sessions and projects.
        ArchiveSession,
        PrevSession,
        NextSession,
        PrevSessionInTab,
        NextSessionInTab,
        PrevProject,
        NextProject,
        // Panes.
        ClosePane,
        SplitRight,
        SplitDown,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        // Terminal.
        NewTerminal,
        NewTerminalTab,
        ToggleTerminal,
        // Editor.
        Find,
        Replace,
        // Help.
        OpenWebsite,
        OpenGithub,
        ReportBug,
        RequestFeature,
        // Window.
        MinimizeWindow,
        ZoomWindow,
    ]
);

/// Bind the keymap with the user's overrides and install the app-level
/// actions. Call once at startup.
pub fn init(cx: &mut App) {
    let overrides = monocode_app::boot::AppServices::try_global(cx)
        .map(|services| services.settings.settings.keybinding_overrides.clone())
        .unwrap_or_default();
    rebind(&overrides, cx);
    cx.on_action(|_: &Quit, cx| request_quit(cx));
    cx.on_action(|_: &CheckForUpdates, cx| {
        if monocode_app::boot::AppServices::try_global(cx).is_some() {
            crate::adapters::settings::check_for_updates(true, cx);
        }
    });
    cx.on_action(|_: &ZoomIn, cx| change_scale(0.1, cx));
    cx.on_action(|_: &ZoomOut, cx| change_scale(-0.1, cx));
    cx.on_action(|_: &ZoomReset, cx| set_scale(1.0, cx));
}

#[derive(Default)]
struct AppBindings(HashSet<String>);
impl Global for AppBindings {}

/// Replace app shortcuts while preserving editor and component shortcuts.
pub fn rebind(overrides: &KeybindingOverrides, cx: &mut App) {
    use monocode_core::{platform::Platform, settings::default_shortcuts_for};
    let platform = Platform::current();
    let prior = cx
        .try_global::<AppBindings>()
        .map(|bindings| bindings.0.clone())
        .unwrap_or_default();
    let editor_search = gpui_base::input::Search.name();
    let managed = [
        monocode_editor::code_editor::OpenReplace.name(),
        monocode_view_files::preview_search::OpenFind.name(),
        monocode_view_workbench::panes::workspace_picker::ToggleWorkspaceMode.name(),
    ];
    let retained = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| {
            !prior.contains(binding.action().name()) && !managed.contains(&binding.action().name())
        })
        .filter_map(|binding| {
            if binding.action().name() != editor_search {
                return Some(binding.clone());
            }
            let predicate = binding.predicate().map(|predicate| predicate.to_string());
            if predicate.as_deref() == Some("CodeEditor") {
                return None;
            }
            if predicate.as_deref() != Some("Input") {
                return Some(binding.clone());
            }
            // Input's built-in find must not restore a disabled editor key.
            let keys = binding
                .keystrokes()
                .iter()
                .map(|key| key.unparse())
                .collect::<Vec<_>>()
                .join(" ");
            let mut replacement = KeyBinding::new(
                &keys,
                gpui_base::input::Search,
                Some("Input && !CodeEditor && !FilePreviewSearch"),
            );
            if let Some(meta) = binding.meta() {
                replacement.set_meta(meta);
            }
            Some(replacement)
        })
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();
    macro_rules! bind {
        ($command:literal, $action:expr) => {
            bind!($command, $action, None);
        };
        ($command:literal, $action:expr, $context:expr) => {
            if !overrides.get($command).is_some_and(|o| o.is_disabled()) {
                let chords = overrides
                    .get($command)
                    .and_then(|o| o.shortcut.clone())
                    .map(|s| vec![s])
                    .unwrap_or_else(|| default_shortcuts_for($command, platform));
                for chord in chords {
                    let context = terminal_context($command, &chord, $context, platform);
                    bindings.push(KeyBinding::new(&native_chord(&chord), $action, context));
                }
            }
        };
    }
    bind!("App: Settings", OpenSettings);
    bind!("App: Search", OpenSearch);
    bind!("App: Go to File", GoToFile);
    bind!("App: Command Palette", CommandPalette);
    bind!("App: Find in Files", FindInFiles);
    bind!("App: Open Project", OpenProject);
    bind!("App: New Window", NewWindow);
    bind!("App: Toggle Sidebar", ToggleSidebar);
    bind!("App: Toggle Session Sidebar", ToggleSessionSidebar);
    bind!("App: Switch Model", SwitchModel);
    bind!(
        "Composer: Toggle Workspace",
        monocode_view_workbench::panes::workspace_picker::ToggleWorkspaceMode
    );
    bind!("View: Reload", Reload);
    bind!("View: Zoom In", ZoomIn);
    bind!("View: Zoom Out", ZoomOut);
    bind!("View: Reset Zoom", ZoomReset);
    bind!("Tab: New", NewSession);
    bind!("Tab: Close Others", CloseOtherTabs);
    bind!("Tab: Close All", CloseAllTabs);
    bind!("Tab: Next", NextTab);
    bind!("Tab: Previous", PrevTab);
    bind!("Tab: Cycle Next", CycleNextTab);
    bind!("Tab: Cycle Previous", CyclePrevTab);
    bind!("Tab: Back", GoBack);
    bind!("Tab: Forward", GoForward);
    bind!("Tab: Activate Last", ActivateLastTab);
    bind!("Session: Archive", ArchiveSession);
    bind!("Session: Previous", PrevSession);
    bind!("Session: Next", NextSession);
    bind!("Session: Previous in Current Tab", PrevSessionInTab);
    bind!("Session: Next in Current Tab", NextSessionInTab);
    bind!("Project: Previous", PrevProject);
    bind!("Project: Next", NextProject);
    bind!("Pane: Close", ClosePane);
    bind!("Pane: Split Right", SplitRight);
    bind!("Pane: Split Down", SplitDown);
    bind!("Pane: Focus Left", FocusLeft);
    bind!("Pane: Focus Right", FocusRight);
    bind!("Pane: Focus Up", FocusUp);
    bind!("Pane: Focus Down", FocusDown);
    bind!("Terminal: New", NewTerminal);
    bind!("Terminal: New Tab", NewTerminalTab);
    bind!("Terminal: Toggle Dock", ToggleTerminal);
    bind!("Editor: Find", gpui_base::input::Search, Some("CodeEditor"));
    bind!(
        "Editor: Find",
        monocode_view_files::preview_search::OpenFind,
        Some("FilePreviewSearch")
    );
    bind!(
        "Editor: Replace",
        monocode_editor::code_editor::OpenReplace,
        Some("CodeEditor")
    );
    let modifier = if platform.is_mac() { "cmd" } else { "ctrl" };
    bindings.push(KeyBinding::new(&format!("{modifier}-q"), Quit, None));
    let command = monocode_core::settings::ACTIVATE_RANGE_COMMAND;
    if !overrides
        .get(command)
        .is_some_and(|value| value.is_disabled())
    {
        let chords = overrides
            .get(command)
            .and_then(|value| value.shortcut.as_ref())
            .map(|shortcut| {
                (1..=8)
                    .map(|digit| format!("{}{}", &shortcut[..shortcut.len() - 1], digit))
                    .collect()
            })
            .unwrap_or_else(|| default_shortcuts_for(command, platform));
        for (index, chord) in chords.iter().enumerate() {
            let context = terminal_context(command, chord, None, platform);
            let chord = native_chord(chord);
            match index {
                0 => bindings.push(KeyBinding::new(&chord, ActivateTab1, context)),
                1 => bindings.push(KeyBinding::new(&chord, ActivateTab2, context)),
                2 => bindings.push(KeyBinding::new(&chord, ActivateTab3, context)),
                3 => bindings.push(KeyBinding::new(&chord, ActivateTab4, context)),
                4 => bindings.push(KeyBinding::new(&chord, ActivateTab5, context)),
                5 => bindings.push(KeyBinding::new(&chord, ActivateTab6, context)),
                6 => bindings.push(KeyBinding::new(&chord, ActivateTab7, context)),
                7 => bindings.push(KeyBinding::new(&chord, ActivateTab8, context)),
                _ => {}
            }
        }
    }
    cx.set_global(AppBindings(
        bindings
            .iter()
            .filter(|binding| binding.action().name() != editor_search)
            .map(|binding| binding.action().name().to_string())
            .collect(),
    ));
    cx.clear_key_bindings();
    cx.bind_keys(retained);
    cx.bind_keys(bindings);
}

pub(crate) fn native_chord(chord: &str) -> String {
    chord
        .split('+')
        .map(|part| match part {
            "Command" => "cmd".into(),
            "Control" => "ctrl".into(),
            "Option" => "alt".into(),
            "Shift" => "shift".into(),
            "Space" => "space".into(),
            "Enter" => "enter".into(),
            "Tab" => "tab".into(),
            "Backquote" => "`".into(),
            "Backslash" => "\\".into(),
            "BracketLeft" => "[".into(),
            "BracketRight" => "]".into(),
            "Comma" => ",".into(),
            "Period" => ".".into(),
            "Quote" => "'".into(),
            "Semicolon" => ";".into(),
            "Slash" => "/".into(),
            "NumpadEnter" => "enter".into(),
            "Equal" => "=".into(),
            "Minus" => "-".into(),
            "ArrowUp" => "up".into(),
            "ArrowDown" => "down".into(),
            "ArrowLeft" => "left".into(),
            "ArrowRight" => "right".into(),
            label => label
                .strip_prefix("Key")
                .or_else(|| label.strip_prefix("Digit"))
                .unwrap_or(label)
                .to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn terminal_context(
    command: &str,
    chord: &str,
    context: Option<&'static str>,
    platform: monocode_core::Platform,
) -> Option<&'static str> {
    let control_only = chord.split('+').any(|part| part == "Control")
        && !chord.split('+').any(|part| part == "Command");
    let terminal_owns = matches!(command, "App: Search" | "Tab: Back" | "Tab: Forward")
        || (platform.is_mac()
            && ["Tab:", "Session:", "Project:", "Pane:", "Terminal:"]
                .iter()
                .any(|prefix| command.starts_with(prefix)));
    if context.is_none() && control_only && terminal_owns {
        Some("!Terminal")
    } else {
        context
    }
}

fn change_scale(delta: f32, cx: &mut App) {
    set_scale(monocode_ui::Theme::of(cx).appearance.ui_scale + delta, cx);
}

fn set_scale(value: f32, cx: &mut App) {
    let mut appearance = monocode_ui::Theme::of(cx).appearance.clone();
    let value = monocode_core::appearance::normalize_ui_scale(value as f64);
    if let Some(services) = monocode_app::boot::AppServices::try_global(cx) {
        monocode_view_settings::settings::store::save_ui_scale(&services.kv, value);
    }
    appearance.ui_scale = value as f32;
    monocode_ui::set_appearance(appearance, cx);
}

pub fn request_quit(cx: &mut App) {
    if monocode_engine::runtime::Engine::try_global(cx).is_none() {
        cx.quit();
        return;
    }
    let lifecycle = monocode_engine::runtime::Engine::lifecycle(cx);
    let count = lifecycle.update(cx, |lifecycle, cx| lifecycle.in_flight_count(cx));
    let confirmation = (count > 0).then(|| {
        lifecycle.update(cx, |lifecycle, cx| {
            lifecycle.ask_quit_confirmation(count, cx)
        })
    });
    cx.spawn(async move |cx| {
        if let Some(confirmation) = confirmation
            && !confirmation.await
        {
            return;
        }
        let saved = cx
            .update(|cx| lifecycle.update(cx, |lifecycle, cx| lifecycle.commit_quit(cx)))
            .await;
        if saved {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyContext, Keystroke, TestAppContext};
    use monocode_core::settings::KeybindingOverride;

    fn matching(cx: &App, chord: &str, contexts: &[&str]) -> Vec<String> {
        let contexts = contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect::<Vec<_>>();
        cx.key_bindings()
            .borrow()
            .bindings_for_input(&[Keystroke::parse(chord).unwrap()], &contexts)
            .0
            .iter()
            .map(|binding| binding.action().name().to_string())
            .collect()
    }

    #[gpui::test]
    fn saved_editor_shortcuts_replace_and_disable_component_defaults(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_editor::init(cx);
            monocode_view_files::init(cx);
            monocode_view_workbench::panes::init(cx);
            let mut overrides = KeybindingOverrides::default();
            overrides.insert(
                "Editor: Find".into(),
                KeybindingOverride {
                    shortcut: Some("Control+Option+Slash".into()),
                    disabled: None,
                },
            );
            for command in ["Editor: Replace", "Composer: Toggle Workspace"] {
                overrides.insert(
                    command.into(),
                    KeybindingOverride {
                        shortcut: None,
                        disabled: Some(true),
                    },
                );
            }
            rebind(&overrides, cx);
            rebind(&overrides, cx);
            let find = gpui_base::input::Search.name();
            let modifier = if monocode_core::Platform::current().is_mac() {
                "cmd"
            } else {
                "ctrl"
            };
            assert!(
                !matching(
                    cx,
                    &format!("{modifier}-f"),
                    &["Shell", "CodeEditor", "Input"]
                )
                .iter()
                .any(|name| name == find)
            );
            assert!(
                matching(cx, &format!("{modifier}-f"), &["Shell", "Input"])
                    .iter()
                    .any(|name| name == find)
            );
            assert!(
                matching(cx, "ctrl-alt-/", &["Shell", "CodeEditor", "Input"])
                    .iter()
                    .any(|name| name == find)
            );
            assert!(
                matching(cx, "ctrl-alt-/", &["Shell", "FilePreviewSearch"])
                    .iter()
                    .any(|name| name == monocode_view_files::preview_search::OpenFind.name())
            );
            assert!(!cx.key_bindings().borrow().bindings().any(|binding| {
                [
                    monocode_editor::code_editor::OpenReplace.name(),
                    monocode_view_workbench::panes::workspace_picker::ToggleWorkspaceMode.name(),
                ]
                .contains(&binding.action().name())
            }));
        });
    }

    #[test]
    fn punctuation_shortcuts_use_native_key_names() {
        for (code, key) in [
            ("Backslash", "\\"),
            ("Quote", "'"),
            ("Semicolon", ";"),
            ("Slash", "/"),
        ] {
            let native = native_chord(&format!("Control+Option+{code}"));
            assert_eq!(native, format!("ctrl-alt-{key}"));
            assert_eq!(Keystroke::parse(&native).unwrap().key, key);
        }
    }

    #[gpui::test]
    fn terminal_control_chords_reach_the_terminal(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let overrides = [
                ("App: Search", "Control+KeyK"),
                ("Tab: Back", "Control+BracketLeft"),
                ("Tab: Forward", "Control+BracketRight"),
            ]
            .into_iter()
            .map(|(command, shortcut)| {
                (
                    command.into(),
                    KeybindingOverride {
                        shortcut: Some(shortcut.into()),
                        disabled: None,
                    },
                )
            })
            .collect();
            rebind(&overrides, cx);
            for (chord, action) in [
                ("ctrl-k", OpenSearch.name()),
                ("ctrl-[", GoBack.name()),
                ("ctrl-]", GoForward.name()),
            ] {
                assert!(
                    matching(cx, chord, &["Shell"])
                        .iter()
                        .any(|name| name == action)
                );
                assert!(
                    !matching(cx, chord, &["Shell", "Terminal"])
                        .iter()
                        .any(|name| name == action)
                );
            }
        });
    }

    #[gpui::test]
    fn rebind_removes_disabled_app_shortcuts_and_preserves_editor_bindings(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new(
                "secondary-shift-i",
                monocode_editor::code_editor::FormatDocument,
                Some("CodeEditor"),
            )]);
            rebind(&KeybindingOverrides::default(), cx);
            let mut overrides = KeybindingOverrides::default();
            overrides.insert(
                "App: Switch Model".into(),
                KeybindingOverride {
                    disabled: Some(true),
                    shortcut: None,
                },
            );
            rebind(&overrides, cx);
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            assert!(
                !keymap
                    .bindings()
                    .any(|binding| binding.action().name() == SwitchModel.name())
            );
            assert_eq!(
                keymap
                    .bindings()
                    .filter(|binding| binding.action().name()
                        == monocode_editor::code_editor::FormatDocument.name())
                    .count(),
                1
            );
        });
    }
}
