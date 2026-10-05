//! Port of src/features/quick-composer/model/quickComposerShortcut.ts: the
//! stored chord format (`Command+Shift+KeyK`) shared by keybinding overrides
//! and the global Quick Composer hotkey.

use crate::platform::Platform;

/// `QUICK_COMPOSER_DEFAULT_SHORTCUT`. The same spelling works for the OS
/// global-shortcut APIs.
pub const QUICK_COMPOSER_DEFAULT_SHORTCUT: &str = "Command+Shift+Space";

/// Modifier state of a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
}

/// A key press: the physical key code (`KeyM`, `Digit1`, `ArrowUp`) and modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShortcutEvent {
    pub code: String,
    pub modifiers: Modifiers,
}

/// `SYMBOLS`: display labels for non-letter key codes.
fn symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "Space" => "Space",
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
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "Enter" | "NumpadEnter" => "Return",
        "Tab" => "Tab",
        "Backspace" => "Delete",
        "Delete" => "Delete Forward",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page Up",
        "PageDown" => "Page Down",
        _ => return None,
    })
}

/// `/^F(?:[1-9]|1[0-9]|2[0-4])$/`.
pub(crate) fn is_function_key(code: &str) -> bool {
    code.strip_prefix('F')
        .filter(|digits| {
            !digits.is_empty()
                && !digits.starts_with('0')
                && digits.bytes().all(|b| b.is_ascii_digit())
        })
        .and_then(|digits| digits.parse::<u8>().ok().filter(|_| digits.len() <= 2))
        .is_some_and(|n| (1..=24).contains(&n))
}

fn supported_code(code: &str) -> bool {
    let single_after = |prefix: &str, valid: fn(&u8) -> bool| {
        code.strip_prefix(prefix)
            .is_some_and(|rest| rest.len() == 1 && valid(&rest.as_bytes()[0]))
    };
    single_after("Key", u8::is_ascii_uppercase)
        || single_after("Digit", u8::is_ascii_digit)
        || is_function_key(code)
        || symbol(code).is_some()
}

const PRIMARY_MODIFIERS: [&str; 3] = ["Command", "Control", "Option"];
const MODIFIER_ORDER: [&str; 4] = ["Command", "Control", "Option", "Shift"];

/// `isShortcut`: a chord with at least one of Command, Control, or Option.
pub fn is_shortcut(value: &str) -> bool {
    let mut parts: Vec<&str> = value.split('+').collect();
    let code = parts.pop().unwrap_or_default();
    if code.is_empty() || !supported_code(code) {
        return false;
    }
    if !parts.iter().any(|part| PRIMARY_MODIFIERS.contains(part)) {
        return false;
    }
    let unique = parts
        .iter()
        .enumerate()
        .all(|(i, part)| !parts[..i].contains(part));
    !parts.is_empty()
        && parts.len() <= 4
        && unique
        && parts.iter().all(|part| MODIFIER_ORDER.contains(part))
}

/// `isGlobalShortcut`: an OS-wide hotkey must keep Command or Control.
pub fn is_global_shortcut(value: &str) -> bool {
    if !is_shortcut(value) {
        return false;
    }
    let parts: Vec<&str> = value.split('+').collect();
    parts[..parts.len() - 1]
        .iter()
        .any(|part| matches!(*part, "Command" | "Control"))
}

/// `canonicalShortcut`: modifiers in a fixed order, so a stored chord matches
/// what a key press produces.
pub fn canonical_shortcut(value: &str) -> Option<String> {
    if !is_shortcut(value) {
        return None;
    }
    let mut parts: Vec<&str> = value.split('+').collect();
    let code = parts.pop()?;
    let mut ordered: Vec<&str> = MODIFIER_ORDER
        .into_iter()
        .filter(|modifier| parts.contains(modifier))
        .collect();
    ordered.push(code);
    Some(ordered.join("+"))
}

/// `shortcutFromKeyEvent`: the stored form of a key press, or `None` when the
/// key is unsupported or no primary modifier is held.
pub fn shortcut_from_key_event(event: &ShortcutEvent) -> Option<String> {
    if !supported_code(&event.code) {
        return None;
    }
    let m = event.modifiers;
    if !m.meta_key && !m.ctrl_key && !m.alt_key {
        return None;
    }
    let mut parts: Vec<&str> = [
        (m.meta_key, "Command"),
        (m.ctrl_key, "Control"),
        (m.alt_key, "Option"),
        (m.shift_key, "Shift"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect();
    parts.push(&event.code);
    Some(parts.join("+"))
}

fn code_label(code: &str) -> String {
    if let Some(rest) = code.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        return rest.to_string();
    }
    symbol(code).unwrap_or(code).to_string()
}

fn modifier_labels(platform: Platform) -> [&'static str; 4] {
    if platform.is_mac() {
        ["⌘", "⌃", "⌥", "⇧"]
    } else {
        ["Win+", "Ctrl+", "Alt+", "Shift+"]
    }
}

/// `quickComposerShortcutPreview`: the label shown while recording a chord.
pub fn quick_composer_shortcut_preview(
    modifiers: Modifiers,
    code: Option<&str>,
    key: Option<&str>,
    platform: Platform,
) -> String {
    let labels = modifier_labels(platform);
    let prefix: String = [
        modifiers.meta_key,
        modifiers.ctrl_key,
        modifiers.alt_key,
        modifiers.shift_key,
    ]
    .into_iter()
    .zip(labels)
    .filter_map(|(held, label)| held.then_some(label))
    .collect();
    let Some(code) = code.filter(|code| !code.is_empty()) else {
        return prefix;
    };
    let displayed = if supported_code(code) {
        code_label(code)
    } else {
        match key.filter(|key| !key.is_empty() && *key != "Unidentified") {
            Some(key) if key.encode_utf16().count() == 1 => key.to_uppercase(),
            Some(key) => key.to_string(),
            None => String::new(),
        }
    };
    prefix + &displayed
}

/// `shortcutTokens`: `aria-keyshortcuts` form, such as `Meta+Shift+K`.
pub fn shortcut_tokens(value: &str) -> String {
    value
        .split('+')
        .map(|part| match part {
            "Command" => "Meta".to_string(),
            "Option" => "Alt".to_string(),
            _ => part
                .strip_prefix("Key")
                .or_else(|| part.strip_prefix("Digit"))
                .unwrap_or(part)
                .to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// `quickComposerShortcutLabel`: `⌘⇧Space` on macOS, `Win+Shift+Space` elsewhere.
pub fn quick_composer_shortcut_label(value: &str, platform: Platform) -> String {
    let mut parts: Vec<&str> = value.split('+').collect();
    let code = parts.pop().unwrap_or("Space");
    let labels = modifier_labels(platform);
    let prefix: String = parts
        .iter()
        .map(|part| match *part {
            "Command" => labels[0],
            "Control" => labels[1],
            "Option" => labels[2],
            "Shift" => labels[3],
            // TODO(port): JavaScript prints "undefined" for an unknown modifier.
            _ => "undefined",
        })
        .collect();
    prefix + &code_label(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_orders_chords() {
        assert!(is_shortcut("Command+Shift+KeyM"));
        assert!(is_shortcut("Option+F12"));
        assert!(!is_shortcut("Shift+KeyM"));
        assert!(!is_shortcut("KeyK"));
        assert!(!is_shortcut("Command+Command+KeyK"));
        assert!(!is_shortcut("Command+F25"));
        assert!(!is_shortcut("Command+F+5"));
        assert!(!is_shortcut("Command+F05"));
        assert!(!is_shortcut("Command+Keyk"));
        assert!(is_global_shortcut("Control+Space"));
        assert!(!is_global_shortcut("Option+Space"));
        assert_eq!(
            canonical_shortcut("Shift+Command+KeyM").as_deref(),
            Some("Command+Shift+KeyM")
        );
        assert_eq!(canonical_shortcut("Shift+KeyM"), None);
    }

    #[test]
    fn reads_key_presses() {
        let press = |code: &str, meta: bool, shift: bool| ShortcutEvent {
            code: code.into(),
            modifiers: Modifiers {
                meta_key: meta,
                shift_key: shift,
                ..Modifiers::default()
            },
        };
        assert_eq!(
            shortcut_from_key_event(&press("KeyM", true, true)).as_deref(),
            Some("Command+Shift+KeyM")
        );
        assert_eq!(shortcut_from_key_event(&press("KeyM", false, true)), None);
        assert_eq!(
            shortcut_from_key_event(&press("NumpadAdd", true, false)),
            None
        );
    }

    #[test]
    fn labels_chords_per_platform() {
        assert_eq!(
            quick_composer_shortcut_label("Command+Shift+Space", Platform::Mac),
            "⌘⇧Space"
        );
        assert_eq!(
            quick_composer_shortcut_label("Control+Option+KeyK", Platform::Windows),
            "Ctrl+Alt+K"
        );
        assert_eq!(shortcut_tokens("Command+Option+Digit1"), "Meta+Alt+1");
        assert_eq!(
            quick_composer_shortcut_preview(
                Modifiers {
                    meta_key: true,
                    ..Modifiers::default()
                },
                Some("Quote"),
                None,
                Platform::Mac
            ),
            "⌘'"
        );
        assert_eq!(
            quick_composer_shortcut_preview(
                Modifiers {
                    ctrl_key: true,
                    ..Modifiers::default()
                },
                Some("IntlBackslash"),
                Some("a"),
                Platform::Linux
            ),
            "Ctrl+A"
        );
    }
}
