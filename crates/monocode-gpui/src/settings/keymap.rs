//! Rebuild overrides from the original native keymap, preserving each action's
//! focus predicate. Resetting restores defaults without accumulating bindings.
use super::Preferences;
use gpui::{App, Global, KeyBinding};
use std::collections::{HashMap, HashSet};
struct Defaults {
    bindings: Vec<KeyBinding>,
    applied: HashMap<String, Option<String>>,
}
impl Global for Defaults {}
pub(crate) fn apply(prefs: &Preferences, cx: &mut App) {
    if !cx.has_global::<Defaults>() {
        let bindings = cx.key_bindings().borrow().bindings().cloned().collect();
        cx.set_global(Defaults {
            bindings,
            applied: HashMap::new(),
        });
    }
    if cx.global::<Defaults>().applied == prefs.keybindings {
        return;
    }
    let baseline = cx.global::<Defaults>().bindings.clone();
    let mut bindings = Vec::new();
    let mut rebound = HashSet::new();
    for binding in baseline {
        let Some(command) = command(&binding) else {
            bindings.push(binding);
            continue;
        };
        let Some(override_) = prefs.keybindings.get(command) else {
            bindings.push(binding);
            continue;
        };
        if let Some(shortcut) = override_
            && rebound.insert((command, binding.action().name()))
            && let Ok(custom) = KeyBinding::load(
                shortcut,
                binding.action().boxed_clone(),
                binding.predicate(),
                false,
                binding.action_input(),
                &gpui::DummyKeyboardMapper,
            )
        {
            bindings.push(custom);
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    cx.global_mut::<Defaults>().applied = prefs.keybindings.clone();
}
fn command(binding: &KeyBinding) -> Option<&'static str> {
    let keys = binding.keystrokes().first()?.unparse();
    Some(match binding.action().name() {
        "workspace::GoToFile" => "App: Go to File",
        "workspace::OpenCommandPalette" => "App: Command Palette",
        "workspace::OpenSettings" => "App: Settings",
        "workspace::FocusSearch" => "App: Search",
        "workspace::OpenProject" => "App: Open Project",
        "workspace::ToggleSidebar" => "App: Toggle Sidebar",
        "workspace::ToggleRightPanel" => "App: Toggle Session Sidebar",
        "workspace::SwitchModel" => "App: Switch Model",
        "workspace::NewCodex" if keys.ends_with("-t") => "Tab: New",
        "workspace::CloseOtherTabs" => "Tab: Close Others",
        "workspace::CloseAllTabs" => "Tab: Close All",
        "workspace::NextTab" if keys == "ctrl-tab" => "Tab: Cycle Next",
        "workspace::PreviousTab" if keys == "ctrl-shift-tab" => "Tab: Cycle Previous",
        "workspace::NextTab" => "Tab: Next",
        "workspace::PreviousTab" => "Tab: Previous",
        "workspace::VisitBack" => "Tab: Back",
        "workspace::VisitForward" => "Tab: Forward",
        "workspace::CloseTab" => "Pane: Close",
        "workspace::ArchiveSession" => "Session: Archive",
        "workspace::PreviousSession" => "Session: Previous",
        "workspace::NextSession" => "Session: Next",
        "workspace::PreviousSessionInTab" => "Session: Previous in Current Tab",
        "workspace::NextSessionInTab" => "Session: Next in Current Tab",
        "workspace::PreviousProject" => "Project: Previous",
        "workspace::NextProject" => "Project: Next",
        "workspace::MoveProjectBack" => "Project: Move Back",
        "workspace::MoveProjectForward" => "Project: Move Forward",
        "workspace::SplitRight" => "Pane: Split Right",
        "workspace::SplitDown" => "Pane: Split Down",
        "workspace::FocusPaneLeft" => "Pane: Focus Left",
        "workspace::FocusPaneRight" => "Pane: Focus Right",
        "workspace::FocusPaneUp" => "Pane: Focus Up",
        "workspace::FocusPaneDown" => "Pane: Focus Down",
        "workspace::NewTerminal" => "Terminal: New",
        "workspace::ToggleTerminal" => "Terminal: Toggle Dock",
        "file_editor::Find" => "Editor: Find",
        "file_editor::Replace" => "Editor: Replace",
        "workspace::ZoomIn" | "image_lightbox::ZoomIn" => "View: Zoom In",
        "workspace::ZoomOut" | "image_lightbox::ZoomOut" => "View: Zoom Out",
        "workspace::ResetZoom" | "image_lightbox::ResetZoom" => "View: Reset Zoom",
        "workspace::Reload" => "View: Reload",
        _ => return None,
    })
}
