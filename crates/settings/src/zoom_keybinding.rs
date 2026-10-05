//! Port of src/features/settings/model/zoomKeybinding.ts: which zoom command
//! a key press runs.

use monocode_core::Platform;
use monocode_core::appearance::{UiScaleCommand, ui_scale_command};

use crate::app_shortcuts::AppShortcutEvent;
use crate::kv::Kv;
use crate::settings_store::{keybinding_pressed, match_custom_keybinding};

/// `ZoomAction`: `zoom-in`, `zoom-out`, or `zoom-reset`.
pub type ZoomAction = UiScaleCommand;

/// `ZoomKeyEvent` has the same fields as `AppShortcutEvent`.
pub type ZoomKeyEvent = AppShortcutEvent;

/// `ZOOM_BY_COMMAND`.
fn zoom_by_command(command: &str) -> Option<ZoomAction> {
    match command {
        "View: Zoom In" => Some(UiScaleCommand::ZoomIn),
        "View: Zoom Out" => Some(UiScaleCommand::ZoomOut),
        "View: Reset Zoom" => Some(UiScaleCommand::ZoomReset),
        _ => None,
    }
}

/// `ZOOM_BY_ACTION`.
fn command_by_zoom(zoom: ZoomAction) -> &'static str {
    match zoom {
        UiScaleCommand::ZoomIn => "View: Zoom In",
        UiScaleCommand::ZoomOut => "View: Zoom Out",
        UiScaleCommand::ZoomReset => "View: Reset Zoom",
    }
}

/// `defaultZoomAction`: the browser-standard chords, which the webview
/// handled instead of the native menu.
fn default_zoom_action(event: &ZoomKeyEvent) -> Option<ZoomAction> {
    let modifiers = event.event.modifiers;
    if !modifiers.meta_key && !modifiers.ctrl_key {
        return None;
    }
    if modifiers.alt_key || event.is_composing {
        return None;
    }
    ui_scale_command(&event.key, &event.event.code)
}

/// `resolveZoomKeybinding`: the zoom command a key press runs, or `None`. A
/// rebound chord is resolved first and may be Option-only, so it does not
/// sit behind the Cmd or Ctrl check that only the defaults need.
pub fn resolve_zoom_keybinding(
    kv: &Kv,
    event: &ZoomKeyEvent,
    platform: Platform,
) -> Option<ZoomAction> {
    let custom_command = if event.is_composing {
        None
    } else {
        match_custom_keybinding(kv, &event.event, platform)
    };
    let custom_zoom = custom_command.as_deref().and_then(zoom_by_command);
    let default_zoom = if custom_zoom.is_some() {
        None
    } else {
        default_zoom_action(event)
    };
    let zoom = custom_zoom.or(default_zoom)?;
    let binding = command_by_zoom(zoom);
    keybinding_pressed(
        kv,
        binding,
        &event.event,
        Some(zoom) == default_zoom,
        platform,
    )
    .then_some(zoom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_store::save_keybinding_override;
    use monocode_core::settings::KeybindingOverride;
    use monocode_core::shortcut::{Modifiers, ShortcutEvent};

    const MAC: Platform = Platform::Mac;

    fn press(key: &str, code: &str, modifiers: Modifiers, is_composing: bool) -> ZoomKeyEvent {
        ZoomKeyEvent {
            event: ShortcutEvent {
                code: code.into(),
                modifiers,
            },
            key: key.into(),
            is_composing,
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl_key: true,
            ..Modifiers::default()
        }
    }

    fn meta() -> Modifiers {
        Modifiers {
            meta_key: true,
            ..Modifiers::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt_key: true,
            ..Modifiers::default()
        }
    }

    fn save(kv: &Kv, command: &str, override_: KeybindingOverride) {
        save_keybinding_override(kv, command, &override_, MAC).unwrap();
    }

    fn rebind(shortcut: &str) -> KeybindingOverride {
        KeybindingOverride {
            disabled: None,
            shortcut: Some(shortcut.into()),
        }
    }

    #[test]
    fn keeps_the_browser_standard_chords_working() {
        let kv = Kv::in_memory();
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("+", "", ctrl(), false), MAC),
            Some(UiScaleCommand::ZoomIn)
        );
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("-", "", meta(), false), MAC),
            Some(UiScaleCommand::ZoomOut)
        );
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("0", "", ctrl(), false), MAC),
            Some(UiScaleCommand::ZoomReset)
        );
    }

    #[test]
    fn fires_an_option_only_rebound_that_the_default_guard_would_swallow() {
        let kv = Kv::in_memory();
        save(&kv, "View: Zoom In", rebind("Option+Equal"));
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("=", "Equal", alt(), false), MAC),
            Some(UiScaleCommand::ZoomIn)
        );
    }

    #[test]
    fn fires_an_option_only_reset_rebound() {
        let kv = Kv::in_memory();
        save(&kv, "View: Reset Zoom", rebind("Option+Digit0"));
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("0", "Digit0", alt(), false), MAC),
            Some(UiScaleCommand::ZoomReset)
        );
    }

    #[test]
    fn stops_the_replaced_default_from_firing_once_rebound() {
        let kv = Kv::in_memory();
        save(&kv, "View: Zoom In", rebind("Option+Equal"));
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("+", "", ctrl(), false), MAC),
            None
        );
    }

    #[test]
    fn honours_a_disabled_zoom_binding_and_ignores_composition() {
        let kv = Kv::in_memory();
        save(
            &kv,
            "View: Zoom In",
            KeybindingOverride {
                disabled: Some(true),
                shortcut: None,
            },
        );
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("+", "", ctrl(), false), MAC),
            None
        );
        assert_eq!(
            resolve_zoom_keybinding(&kv, &press("+", "", ctrl(), true), MAC),
            None
        );
    }
}
