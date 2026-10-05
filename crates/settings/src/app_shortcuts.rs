//! Port of src/features/settings/model/appShortcuts.ts: which app command a
//! key press runs.

use monocode_core::Platform;
use monocode_core::shortcut::ShortcutEvent;

use crate::kv::Kv;
use crate::settings_store::{keybinding_pressed, match_custom_keybinding};

/// `AppShortcutEvent`: a key press with the typed key and the IME state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppShortcutEvent {
    pub event: ShortcutEvent,
    /// `KeyboardEvent.key`, such as `k` or `P`.
    pub key: String,
    pub is_composing: bool,
}

/// `APP_SHORTCUTS`: command, key, and whether Shift must be held.
const APP_SHORTCUTS: [(&str, &str, bool); 10] = [
    ("App: New Window", "n", true),
    ("App: Open Project", "o", false),
    ("App: Toggle Sidebar", "b", false),
    ("App: Toggle Session Sidebar", "b", true),
    ("App: Go to File", "p", false),
    ("App: Command Palette", "p", true),
    ("View: Reload", "r", true),
    ("App: Search", "k", false),
    ("App: Settings", ",", false),
    ("App: Find in Files", "f", true),
];

/// `resolveAppShortcut`: the app command a key press runs, or `None`.
/// Ignores the press during IME composition, so a chord cannot interrupt a
/// composition, and honours a rebound or disabled shortcut.
pub fn resolve_app_shortcut(
    kv: &Kv,
    event: &AppShortcutEvent,
    platform: Platform,
) -> Option<&'static str> {
    if event.is_composing {
        return None;
    }
    // A rebound chord is resolved before the default table, otherwise a new
    // combination could never fire, and it may be Alt-only.
    if let Some(custom) = match_custom_keybinding(kv, &event.event, platform)
        && let Some((command, _, _)) = APP_SHORTCUTS
            .iter()
            .find(|(command, _, _)| *command == custom)
    {
        return keybinding_pressed(kv, command, &event.event, false, platform).then_some(*command);
    }
    let modifiers = event.event.modifiers;
    if !modifiers.meta_key && !modifiers.ctrl_key {
        return None;
    }
    if modifiers.alt_key {
        return None;
    }
    let key = event.key.to_lowercase();
    for (command, expected, shift) in APP_SHORTCUTS {
        if key != expected || modifiers.shift_key != shift {
            continue;
        }
        return keybinding_pressed(kv, command, &event.event, true, platform).then_some(command);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_store::save_keybinding_override;
    use monocode_core::settings::KeybindingOverride;
    use monocode_core::shortcut::Modifiers;

    const MAC: Platform = Platform::Mac;

    fn press(key: &str, code: &str, modifiers: Modifiers, is_composing: bool) -> AppShortcutEvent {
        AppShortcutEvent {
            event: ShortcutEvent {
                code: code.into(),
                modifiers,
            },
            key: key.into(),
            is_composing,
        }
    }

    fn mods(meta: bool, ctrl: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            meta_key: meta,
            ctrl_key: ctrl,
            alt_key: alt,
            shift_key: shift,
        }
    }

    fn rebind(kv: &Kv, command: &str, shortcut: &str) {
        save_keybinding_override(
            kv,
            command,
            &KeybindingOverride {
                disabled: None,
                shortcut: Some(shortcut.into()),
            },
            MAC,
        )
        .unwrap();
    }

    #[test]
    fn resolves_the_default_app_chords() {
        let kv = Kv::in_memory();
        let resolve = |event| resolve_app_shortcut(&kv, &event, MAC);
        assert_eq!(
            resolve(press("k", "", mods(true, false, false, false), false)),
            Some("App: Search")
        );
        assert_eq!(
            resolve(press("P", "", mods(true, false, false, true), false)),
            Some("App: Command Palette")
        );
        assert_eq!(
            resolve(press(",", "", mods(false, true, false, false), false)),
            Some("App: Settings")
        );
        assert_eq!(
            resolve(press("b", "", mods(true, false, false, true), false)),
            Some("App: Toggle Session Sidebar")
        );
    }

    #[test]
    fn ignores_a_chord_while_an_ime_is_composing() {
        let kv = Kv::in_memory();
        for (key, modifiers) in [
            ("k", mods(true, false, false, false)),
            ("P", mods(true, false, false, true)),
            (",", mods(false, true, false, false)),
            ("b", mods(true, false, false, true)),
            ("f", mods(true, false, false, true)),
        ] {
            assert_eq!(
                resolve_app_shortcut(&kv, &press(key, "", modifiers, true), MAC),
                None
            );
        }
    }

    #[test]
    fn ignores_a_rebound_chord_while_composing() {
        let kv = Kv::in_memory();
        rebind(&kv, "App: Search", "Command+Shift+KeyM");
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("M", "KeyM", mods(true, false, false, true), true),
                MAC
            ),
            None
        );
    }

    #[test]
    fn fires_a_rebound_chord_and_drops_the_default_it_replaced() {
        let kv = Kv::in_memory();
        rebind(&kv, "App: Search", "Command+Shift+KeyM");
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("M", "KeyM", mods(true, false, false, true), false),
                MAC
            ),
            Some("App: Search")
        );
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("k", "", mods(true, false, false, false), false),
                MAC
            ),
            None
        );
    }

    #[test]
    fn requires_a_primary_modifier_and_refuses_alt() {
        let kv = Kv::in_memory();
        assert_eq!(
            resolve_app_shortcut(&kv, &press("k", "", Modifiers::default(), false), MAC),
            None
        );
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("k", "", mods(true, false, true, false), false),
                MAC
            ),
            None
        );
    }

    #[test]
    fn honours_a_disabled_command() {
        let kv = Kv::in_memory();
        save_keybinding_override(
            &kv,
            "App: Search",
            &KeybindingOverride {
                disabled: Some(true),
                shortcut: None,
            },
            MAC,
        )
        .unwrap();
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("k", "", mods(true, false, false, false), false),
                MAC
            ),
            None
        );
    }

    #[test]
    fn fires_an_alt_only_rebound_for_an_app_command() {
        let kv = Kv::in_memory();
        rebind(&kv, "App: Go to File", "Option+KeyG");
        assert_eq!(
            resolve_app_shortcut(
                &kv,
                &press("g", "KeyG", mods(false, false, true, false), false),
                MAC
            ),
            Some("App: Go to File")
        );
    }
}
