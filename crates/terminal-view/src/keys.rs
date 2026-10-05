//! Keyboard and mouse input encoded as bytes for the PTY.
//!
//! The key table follows xterm.js `evaluateKeyboardEvent`, which
//! src/features/terminal/ui/TerminalView.tsx relied on, with
//! `macOptionIsMeta` on (Option sends an ESC prefix). The macOS editing
//! shortcuts port src/features/terminal/model/terminalKeys.ts.

use gpui::{Keystroke, Modifiers};

use crate::emulator::MouseMode;

/// Terminal modes that change what a key sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyMode {
    /// DECCKM. Arrows, Home, and End send SS3 (`ESC O`) instead of CSI.
    pub app_cursor: bool,
}

/// Port of `macTerminalShortcutData` in terminalKeys.ts: macOS editing
/// shortcuts translated into sequences that common shells understand.
pub fn mac_shortcut_bytes(keystroke: &Keystroke) -> Option<&'static [u8]> {
    let m = &keystroke.modifiers;
    if m.control || m.shift {
        return None;
    }
    if m.alt && !m.platform {
        return match keystroke.key.as_str() {
            "left" => Some(b"\x1bb"),
            "right" => Some(b"\x1bf"),
            _ => None,
        };
    }
    if m.platform && !m.alt {
        return match keystroke.key.as_str() {
            "left" => Some(b"\x01"),
            "right" => Some(b"\x05"),
            "backspace" => Some(b"\x15"),
            _ => None,
        };
    }
    None
}

/// Port of `isMacTerminalClearShortcut` in terminalKeys.ts: Cmd+K with no
/// other modifier. It only reaches the terminal when no app binding claims it
/// first, because GPUI runs key bindings before key listeners.
pub fn is_mac_clear_shortcut(keystroke: &Keystroke) -> bool {
    let m = &keystroke.modifiers;
    m.platform && !m.control && !m.alt && !m.shift && keystroke.key.eq_ignore_ascii_case("k")
}

/// True for a keystroke that only types text: a printable character with no
/// Control, Alt, or Cmd. The view leaves these to the platform input handler
/// so dead keys and input methods can compose them.
pub fn is_text_input(keystroke: &Keystroke) -> bool {
    let m = &keystroke.modifiers;
    if m.control || m.alt || m.platform || m.function {
        return false;
    }
    if matches!(
        keystroke.key.as_str(),
        "enter" | "tab" | "escape" | "backspace"
    ) {
        return false;
    }
    keystroke
        .key_char
        .as_deref()
        .is_some_and(|text| !text.is_empty() && text.chars().all(|c| !c.is_control()))
}

/// xterm's modifier parameter: 1 plus shift 1, alt 2, ctrl 4.
fn modifier_param(m: &Modifiers) -> u8 {
    1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.control as u8)
}

/// The bytes a keystroke sends, or `None` when the terminal does not handle
/// it (Cmd shortcuts, unknown keys, Ctrl with a key that has no control code).
pub fn keystroke_bytes(keystroke: &Keystroke, mode: KeyMode) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    if m.platform {
        return None;
    }
    let key = keystroke.key.as_str();
    let param = modifier_param(m);
    let modified = param > 1;

    // CSI 1;m X for arrows and Home/End with modifiers, otherwise CSI or SS3.
    let cursor_key = |letter: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{}", letter as char).into_bytes()
        } else if mode.app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    // CSI n ~ keys.
    let tilde = |n: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{n};{param}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    // F1 to F4 are SS3 P to S, CSI 1;m P with modifiers.
    let ss3_function = |letter: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{param}{}", letter as char).into_bytes()
        } else {
            vec![0x1b, b'O', letter]
        }
    };
    let alt_prefix = |bytes: Vec<u8>| -> Vec<u8> {
        if m.alt {
            let mut out = vec![0x1b];
            out.extend(bytes);
            out
        } else {
            bytes
        }
    };

    let bytes = match key {
        "up" => cursor_key(b'A'),
        "down" => cursor_key(b'B'),
        "right" => cursor_key(b'C'),
        "left" => cursor_key(b'D'),
        "home" => cursor_key(b'H'),
        "end" => cursor_key(b'F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3_function(b'P'),
        "f2" => ss3_function(b'Q'),
        "f3" => ss3_function(b'R'),
        "f4" => ss3_function(b'S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "f13" => tilde(25),
        "f14" => tilde(26),
        "f15" => tilde(28),
        "f16" => tilde(29),
        "f17" => tilde(31),
        "f18" => tilde(32),
        "f19" => tilde(33),
        "f20" => tilde(34),
        "enter" => alt_prefix(b"\r".to_vec()),
        "escape" => alt_prefix(vec![0x1b]),
        "tab" => {
            if m.shift {
                b"\x1b[Z".to_vec()
            } else {
                alt_prefix(b"\t".to_vec())
            }
        }
        "backspace" => alt_prefix(vec![if m.control { 0x08 } else { 0x7f }]),
        "space" => {
            if m.control {
                alt_prefix(vec![0x00])
            } else {
                alt_prefix(b" ".to_vec())
            }
        }
        _ => {
            if m.control {
                let mut chars = key.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return None;
                };
                alt_prefix(vec![control_byte(c)?])
            } else if m.alt {
                // Option as Meta: ESC plus the key without Option. Letters
                // keep their case from Shift. Shifted symbols already arrive
                // as the symbol.
                let mut chars = key.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return None;
                };
                let c = if m.shift { c.to_ascii_uppercase() } else { c };
                let mut out = vec![0x1b];
                let mut buf = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                out
            } else {
                let text = keystroke
                    .key_char
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .or_else(|| (key.chars().count() == 1).then_some(key))?;
                text.as_bytes().to_vec()
            }
        }
    };
    Some(bytes)
}

/// Control code for Ctrl plus a character, following xterm.js: letters map
/// to 1 to 26, and the digits 2 to 8 stand in for the punctuation keys of a
/// US layout.
pub fn control_byte(c: char) -> Option<u8> {
    match c.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '/' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// Port of xterm.js `Terminal.paste`: line breaks become `\r`, and the text is
/// wrapped in `ESC [200~` and `ESC [201~` when the program enabled bracketed
/// paste. Unlike xterm.js, an `ESC [201~` inside the text is removed so a
/// paste cannot end bracketed mode early and run the rest as typed input.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let text = text.replace("\x1b[201~", "");
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.into_bytes()
    }
}

/// Focus change report for DECSET 1004.
pub fn focus_report(focused: bool) -> &'static [u8] {
    if focused { b"\x1b[I" } else { b"\x1b[O" }
}

/// A mouse button as the terminal protocol numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportButton {
    Left,
    Middle,
    Right,
    /// Motion with no button held.
    None,
    WheelUp,
    WheelDown,
}

impl ReportButton {
    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
            Self::None => 3,
            Self::WheelUp => 64,
            Self::WheelDown => 65,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Encode a mouse event for a program that enabled mouse reporting. `col`
/// and `row` are 0-based viewport cells. Returns `None` when the mode does
/// not want this event, or when the cell cannot be encoded (X10 encoding
/// stops at column 223).
pub fn mouse_report(
    button: ReportButton,
    action: MouseAction,
    modifiers: &Modifiers,
    col: usize,
    row: usize,
    mode: MouseMode,
) -> Option<Vec<u8>> {
    if !mode.reporting() {
        return None;
    }
    match action {
        MouseAction::Motion => {
            let held = !matches!(button, ReportButton::None);
            if !(mode.motion || (mode.drag && held)) {
                return None;
            }
        }
        MouseAction::Release => {
            if matches!(button, ReportButton::WheelUp | ReportButton::WheelDown) {
                return None;
            }
        }
        MouseAction::Press => {}
    }

    let mut code = button.code();
    if action == MouseAction::Motion {
        code += 32;
    }
    code +=
        4 * (modifiers.shift as u8) + 8 * (modifiers.alt as u8) + 16 * (modifiers.control as u8);

    if mode.sgr {
        let suffix = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{};{}{suffix}", col + 1, row + 1).into_bytes());
    }

    // Normal and UTF-8 encodings report every release as button 3.
    if action == MouseAction::Release {
        code = 3 + (code & !3);
    }
    let mut out = b"\x1b[M".to_vec();
    out.push(32 + code);
    for value in [col + 1, row + 1] {
        let value = 32 + value;
        if mode.utf8 {
            let c = char::from_u32(value as u32).filter(|_| value <= 2047)?;
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(u8::try_from(value).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    fn bytes(source: &str) -> Option<Vec<u8>> {
        keystroke_bytes(&ks(source), KeyMode::default())
    }

    fn app(source: &str) -> Option<Vec<u8>> {
        keystroke_bytes(&ks(source), KeyMode { app_cursor: true })
    }

    #[test]
    fn printable_keys_send_their_text() {
        assert_eq!(bytes("a"), Some(b"a".to_vec()));
        let mut shifted = ks("shift-a");
        shifted.key_char = Some("A".into());
        assert_eq!(
            keystroke_bytes(&shifted, KeyMode::default()),
            Some(b"A".to_vec())
        );
        let mut accented = ks("e");
        accented.key_char = Some("é".into());
        assert_eq!(
            keystroke_bytes(&accented, KeyMode::default()),
            Some("é".as_bytes().to_vec())
        );
        assert_eq!(bytes("/"), Some(b"/".to_vec()));
        assert_eq!(bytes("capslock"), None);
    }

    #[test]
    fn editing_keys() {
        assert_eq!(bytes("enter"), Some(b"\r".to_vec()));
        assert_eq!(bytes("shift-enter"), Some(b"\r".to_vec()));
        assert_eq!(bytes("backspace"), Some(vec![0x7f]));
        assert_eq!(bytes("ctrl-backspace"), Some(vec![0x08]));
        assert_eq!(bytes("tab"), Some(b"\t".to_vec()));
        assert_eq!(bytes("shift-tab"), Some(b"\x1b[Z".to_vec()));
        assert_eq!(bytes("escape"), Some(vec![0x1b]));
        assert_eq!(bytes("space"), Some(b" ".to_vec()));
        assert_eq!(bytes("insert"), Some(b"\x1b[2~".to_vec()));
        assert_eq!(bytes("delete"), Some(b"\x1b[3~".to_vec()));
        assert_eq!(bytes("pageup"), Some(b"\x1b[5~".to_vec()));
        assert_eq!(bytes("pagedown"), Some(b"\x1b[6~".to_vec()));
        assert_eq!(bytes("ctrl-delete"), Some(b"\x1b[3;5~".to_vec()));
    }

    #[test]
    fn arrows_follow_application_cursor_mode() {
        assert_eq!(bytes("up"), Some(b"\x1b[A".to_vec()));
        assert_eq!(bytes("down"), Some(b"\x1b[B".to_vec()));
        assert_eq!(bytes("right"), Some(b"\x1b[C".to_vec()));
        assert_eq!(bytes("left"), Some(b"\x1b[D".to_vec()));
        assert_eq!(bytes("home"), Some(b"\x1b[H".to_vec()));
        assert_eq!(bytes("end"), Some(b"\x1b[F".to_vec()));
        assert_eq!(app("up"), Some(b"\x1bOA".to_vec()));
        assert_eq!(app("left"), Some(b"\x1bOD".to_vec()));
        assert_eq!(app("home"), Some(b"\x1bOH".to_vec()));
        assert_eq!(app("end"), Some(b"\x1bOF".to_vec()));
    }

    #[test]
    fn modified_arrows_use_the_xterm_parameter() {
        assert_eq!(bytes("shift-up"), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(bytes("alt-left"), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(bytes("ctrl-right"), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(bytes("ctrl-shift-home"), Some(b"\x1b[1;6H".to_vec()));
        // Modifiers win over application cursor mode.
        assert_eq!(app("ctrl-up"), Some(b"\x1b[1;5A".to_vec()));
    }

    #[test]
    fn function_keys() {
        assert_eq!(bytes("f1"), Some(b"\x1bOP".to_vec()));
        assert_eq!(bytes("f4"), Some(b"\x1bOS".to_vec()));
        assert_eq!(bytes("f5"), Some(b"\x1b[15~".to_vec()));
        assert_eq!(bytes("f10"), Some(b"\x1b[21~".to_vec()));
        assert_eq!(bytes("f12"), Some(b"\x1b[24~".to_vec()));
        assert_eq!(bytes("shift-f1"), Some(b"\x1b[1;2P".to_vec()));
        assert_eq!(bytes("ctrl-f5"), Some(b"\x1b[15;5~".to_vec()));
    }

    #[test]
    fn ctrl_combos_send_control_codes() {
        assert_eq!(bytes("ctrl-c"), Some(vec![0x03]));
        assert_eq!(bytes("ctrl-a"), Some(vec![0x01]));
        assert_eq!(bytes("ctrl-z"), Some(vec![0x1a]));
        assert_eq!(bytes("ctrl-shift-c"), Some(vec![0x03]));
        assert_eq!(bytes("ctrl-space"), Some(vec![0x00]));
        assert_eq!(bytes("ctrl-["), Some(vec![0x1b]));
        assert_eq!(bytes("ctrl-\\"), Some(vec![0x1c]));
        assert_eq!(bytes("ctrl-]"), Some(vec![0x1d]));
        assert_eq!(bytes("ctrl-/"), Some(vec![0x1f]));
        assert_eq!(bytes("ctrl-2"), Some(vec![0x00]));
        assert_eq!(bytes("ctrl-8"), Some(vec![0x7f]));
        assert_eq!(bytes("ctrl-1"), None);
    }

    #[test]
    fn alt_sends_an_escape_prefix() {
        assert_eq!(bytes("alt-b"), Some(b"\x1bb".to_vec()));
        assert_eq!(bytes("alt-shift-b"), Some(b"\x1bB".to_vec()));
        assert_eq!(bytes("alt-."), Some(b"\x1b.".to_vec()));
        assert_eq!(bytes("alt-ctrl-c"), Some(vec![0x1b, 0x03]));
        assert_eq!(bytes("alt-enter"), Some(b"\x1b\r".to_vec()));
        assert_eq!(bytes("alt-backspace"), Some(b"\x1b\x7f".to_vec()));
    }

    #[test]
    fn cmd_combos_are_not_terminal_input() {
        assert_eq!(bytes("cmd-c"), None);
        assert_eq!(bytes("cmd-enter"), None);
    }

    #[test]
    fn mac_shortcuts_port_terminal_keys_ts() {
        assert_eq!(mac_shortcut_bytes(&ks("alt-left")), Some(&b"\x1bb"[..]));
        assert_eq!(mac_shortcut_bytes(&ks("alt-right")), Some(&b"\x1bf"[..]));
        assert_eq!(mac_shortcut_bytes(&ks("cmd-left")), Some(&b"\x01"[..]));
        assert_eq!(mac_shortcut_bytes(&ks("cmd-right")), Some(&b"\x05"[..]));
        assert_eq!(mac_shortcut_bytes(&ks("cmd-backspace")), Some(&b"\x15"[..]));
        assert_eq!(mac_shortcut_bytes(&ks("alt-up")), None);
        assert_eq!(mac_shortcut_bytes(&ks("shift-alt-left")), None);
        assert_eq!(mac_shortcut_bytes(&ks("ctrl-cmd-left")), None);
        assert_eq!(mac_shortcut_bytes(&ks("cmd-alt-left")), None);
        assert!(is_mac_clear_shortcut(&ks("cmd-k")));
        assert!(!is_mac_clear_shortcut(&ks("cmd-shift-k")));
        assert!(!is_mac_clear_shortcut(&ks("ctrl-k")));
    }

    #[test]
    fn text_input_detection() {
        let mut a = ks("a");
        a.key_char = Some("a".into());
        assert!(is_text_input(&a));
        let mut space = ks("space");
        space.key_char = Some(" ".into());
        assert!(is_text_input(&space));
        let mut enter = ks("enter");
        enter.key_char = Some("\n".into());
        assert!(!is_text_input(&enter));
        assert!(!is_text_input(&ks("ctrl-a")));
        assert!(!is_text_input(&ks("alt-a")));
        assert!(!is_text_input(&ks("up")));
    }

    #[test]
    fn paste_normalizes_newlines_and_brackets() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(paste_bytes("hi", true), b"\x1b[200~hi\x1b[201~".to_vec());
        assert_eq!(
            paste_bytes("a\x1b[201~rm -rf", true),
            b"\x1b[200~arm -rf\x1b[201~".to_vec()
        );
    }

    #[test]
    fn focus_reports() {
        assert_eq!(focus_report(true), b"\x1b[I");
        assert_eq!(focus_report(false), b"\x1b[O");
    }

    fn mode(sgr: bool) -> MouseMode {
        MouseMode {
            click: true,
            sgr,
            ..MouseMode::default()
        }
    }

    #[test]
    fn sgr_mouse_reports() {
        let none = Modifiers::default();
        let report = |b, a, m: MouseMode| mouse_report(b, a, &none, 4, 2, m);
        assert_eq!(
            report(ReportButton::Left, MouseAction::Press, mode(true)),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            report(ReportButton::Left, MouseAction::Release, mode(true)),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
        assert_eq!(
            report(ReportButton::WheelUp, MouseAction::Press, mode(true)),
            Some(b"\x1b[<64;5;3M".to_vec())
        );
        assert_eq!(
            report(ReportButton::WheelDown, MouseAction::Press, mode(true)),
            Some(b"\x1b[<65;5;3M".to_vec())
        );
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(
            mouse_report(
                ReportButton::Right,
                MouseAction::Press,
                &ctrl,
                0,
                0,
                mode(true)
            ),
            Some(b"\x1b[<18;1;1M".to_vec())
        );
    }

    #[test]
    fn motion_needs_drag_or_any_motion_mode() {
        let none = Modifiers::default();
        let click_only = mode(true);
        assert_eq!(
            mouse_report(
                ReportButton::Left,
                MouseAction::Motion,
                &none,
                1,
                1,
                click_only
            ),
            None
        );
        let drag = MouseMode {
            drag: true,
            ..click_only
        };
        assert_eq!(
            mouse_report(ReportButton::Left, MouseAction::Motion, &none, 1, 1, drag),
            Some(b"\x1b[<32;2;2M".to_vec())
        );
        assert_eq!(
            mouse_report(ReportButton::None, MouseAction::Motion, &none, 1, 1, drag),
            None
        );
        let any = MouseMode {
            motion: true,
            ..click_only
        };
        assert_eq!(
            mouse_report(ReportButton::None, MouseAction::Motion, &none, 1, 1, any),
            Some(b"\x1b[<35;2;2M".to_vec())
        );
    }

    #[test]
    fn x10_and_utf8_mouse_encoding() {
        let none = Modifiers::default();
        assert_eq!(
            mouse_report(
                ReportButton::Left,
                MouseAction::Press,
                &none,
                0,
                0,
                mode(false)
            ),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            mouse_report(
                ReportButton::Middle,
                MouseAction::Release,
                &none,
                2,
                3,
                mode(false)
            ),
            Some(vec![0x1b, b'[', b'M', 35, 35, 36])
        );
        assert_eq!(
            mouse_report(
                ReportButton::Left,
                MouseAction::Press,
                &none,
                300,
                0,
                mode(false)
            ),
            None,
            "X10 cannot encode column 301"
        );
        let utf8 = MouseMode {
            utf8: true,
            ..mode(false)
        };
        let mut expected = vec![0x1b, b'[', b'M', 32];
        expected.extend_from_slice("\u{14d}".as_bytes());
        expected.push(33);
        assert_eq!(
            mouse_report(ReportButton::Left, MouseAction::Press, &none, 300, 0, utf8),
            Some(expected)
        );
        assert_eq!(
            mouse_report(
                ReportButton::Left,
                MouseAction::Press,
                &none,
                0,
                0,
                MouseMode::default()
            ),
            None,
            "no report without a mouse mode"
        );
    }
}
