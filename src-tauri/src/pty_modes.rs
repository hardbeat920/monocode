//! Terminal modes a program running in a PTY has switched on.
//!
//! A reloaded page (or one macOS rebuilt after its web process crashed) gets
//! a fresh xterm at default modes, but the shell or TUI on the other side of
//! the PTY still thinks bracketed paste, the alternate screen or application
//! cursor keys are on, and most programs never resend them on SIGWINCH. The
//! reader thread feeds every output chunk through here so a reattach can hand
//! the new view the modes to restore before it accepts input. The page being
//! gone is fine: this state lives in the backend, not in the view.

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    #[default]
    Ground,
    Esc,
    /// `ESC` plus intermediates, e.g. the charset designation `ESC ( B`.
    EscIntermediate,
    Csi,
    /// OSC, DCS, SOS, PM and APC bodies, skipped up to BEL or ST.
    String,
    /// `ESC` inside a string: `\` ends it, anything else starts a new escape.
    StringEsc,
}

/// Long enough for any mode sequence; longer CSIs are not ours to track.
const MAX_CSI: usize = 64;

#[derive(Default, Clone, Debug)]
pub(crate) struct TermModes {
    state: State,
    csi: Vec<u8>,
    csi_overflow: bool,
    alt_screen: bool,
    app_cursor: bool,
    app_keypad: bool,
    bracketed_paste: bool,
    focus_events: bool,
    /// 9, 1000, 1002 or 1003. They are exclusive, and resetting any clears it.
    mouse_tracking: Option<u16>,
    /// 1005, 1006, 1015 or 1016.
    mouse_encoding: Option<u16>,
    cursor_hidden: bool,
    no_autowrap: bool,
    reverse_video: bool,
    /// DECSCUSR shape. `None` is the view's own default.
    cursor_style: Option<u16>,
}

impl TermModes {
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.step(byte);
        }
    }

    /// Escape sequences that bring a terminal at default modes to these.
    /// The alternate screen goes first: entering it is what a TUI's redraw
    /// paints into, and the other modes are per-terminal, not per-screen.
    pub(crate) fn restore_sequence(&self) -> String {
        let mut out = String::new();
        if self.alt_screen {
            out.push_str("\x1b[?1049h");
        }
        if self.reverse_video {
            out.push_str("\x1b[?5h");
        }
        if self.no_autowrap {
            out.push_str("\x1b[?7l");
        }
        if self.app_cursor {
            out.push_str("\x1b[?1h");
        }
        if self.app_keypad {
            out.push_str("\x1b=");
        }
        if self.bracketed_paste {
            out.push_str("\x1b[?2004h");
        }
        if self.focus_events {
            out.push_str("\x1b[?1004h");
        }
        if let Some(mode) = self.mouse_tracking {
            out.push_str(&format!("\x1b[?{mode}h"));
        }
        if let Some(mode) = self.mouse_encoding {
            out.push_str(&format!("\x1b[?{mode}h"));
        }
        if let Some(style) = self.cursor_style {
            out.push_str(&format!("\x1b[{style} q"));
        }
        if self.cursor_hidden {
            out.push_str("\x1b[?25l");
        }
        out
    }

    fn step(&mut self, byte: u8) {
        // CAN and SUB abort any sequence; ESC always starts a new one.
        match (self.state, byte) {
            (State::String, 0x1b) => {
                self.state = State::StringEsc;
                return;
            }
            (State::StringEsc, b'\\') => {
                self.state = State::Ground;
                return;
            }
            (State::StringEsc, _) => {
                self.state = State::Esc;
                self.step(byte);
                return;
            }
            (State::String, 0x07) => {
                self.state = State::Ground;
                return;
            }
            (State::String, _) => return,
            (_, 0x18 | 0x1a) => {
                self.state = State::Ground;
                return;
            }
            (_, 0x1b) => {
                self.state = State::Esc;
                return;
            }
            _ => {}
        }
        match self.state {
            State::Ground => {}
            State::Esc => self.esc(byte),
            State::EscIntermediate => {
                if (0x30..=0x7e).contains(&byte) {
                    self.state = State::Ground;
                }
            }
            State::Csi => self.csi(byte),
            State::String | State::StringEsc => unreachable!(),
        }
    }

    fn esc(&mut self, byte: u8) {
        self.state = State::Ground;
        match byte {
            b'[' => {
                self.csi.clear();
                self.csi_overflow = false;
                self.state = State::Csi;
            }
            b']' | b'P' | b'X' | b'^' | b'_' => self.state = State::String,
            b'=' => self.app_keypad = true,
            b'>' => self.app_keypad = false,
            b'c' => self.reset(),
            0x20..=0x2f => self.state = State::EscIntermediate,
            _ => {}
        }
    }

    fn csi(&mut self, byte: u8) {
        match byte {
            0x20..=0x3f => {
                if self.csi.len() < MAX_CSI {
                    self.csi.push(byte);
                } else {
                    self.csi_overflow = true;
                }
            }
            0x40..=0x7e => {
                self.state = State::Ground;
                if !self.csi_overflow {
                    let body = std::mem::take(&mut self.csi);
                    self.dispatch_csi(&body, byte);
                    self.csi = body;
                }
            }
            // C0 controls run inside a CSI without ending it.
            0x00..=0x1f => {}
            _ => self.state = State::Ground,
        }
    }

    fn dispatch_csi(&mut self, body: &[u8], fin: u8) {
        let split = body
            .iter()
            .position(|b| (0x20..=0x2f).contains(b))
            .unwrap_or(body.len());
        let (params, intermediates) = body.split_at(split);
        match (params.first(), intermediates, fin) {
            (Some(b'?'), [], b'h' | b'l') => {
                for mode in parse_params(&params[1..]) {
                    self.dec_mode(mode, fin == b'h');
                }
            }
            (Some(b'0'..=b'9') | None, [b' '], b'q') => {
                let style = parse_params(params).next().unwrap_or(0);
                self.cursor_style = (style != 0).then_some(style);
            }
            (None, [b'!'], b'p') => self.soft_reset(),
            _ => {}
        }
    }

    fn dec_mode(&mut self, mode: u16, on: bool) {
        match mode {
            1 => self.app_cursor = on,
            5 => self.reverse_video = on,
            7 => self.no_autowrap = !on,
            25 => self.cursor_hidden = !on,
            47 | 1047 | 1049 => self.alt_screen = on,
            66 => self.app_keypad = on,
            9 | 1000 | 1002 | 1003 => self.mouse_tracking = on.then_some(mode),
            1004 => self.focus_events = on,
            1005 | 1006 | 1015 | 1016 => self.mouse_encoding = on.then_some(mode),
            2004 => self.bracketed_paste = on,
            _ => {}
        }
    }

    /// DECSTR, as xterm.js applies it: input modes go back to defaults, the
    /// screen and mouse reporting stay.
    fn soft_reset(&mut self) {
        self.app_cursor = false;
        self.app_keypad = false;
        self.bracketed_paste = false;
        self.focus_events = false;
        self.cursor_hidden = false;
        self.no_autowrap = false;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn parse_params(params: &[u8]) -> impl Iterator<Item = u16> + '_ {
    params
        .split(|b| *b == b';')
        .filter(|param| !param.is_empty() && param.iter().all(u8::is_ascii_digit))
        .map(|param| {
            param.iter().fold(0_u16, |acc, digit| {
                acc.saturating_mul(10)
                    .saturating_add(u16::from(digit - b'0'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(stream: &[u8]) -> String {
        let mut modes = TermModes::default();
        modes.feed(stream);
        modes.restore_sequence()
    }

    #[test]
    fn a_fresh_terminal_needs_nothing_restored() {
        assert_eq!(modes(b""), "");
        assert_eq!(modes(b"plain \x1b[1;31mred\x1b[0m text\r\n"), "");
    }

    #[test]
    fn a_shell_prompt_keeps_bracketed_paste() {
        // bash/readline and zsh's prompt.
        assert_eq!(modes(b"\x1b[?2004huser$ "), "\x1b[?2004h");
        // Running a command turns it off until the next prompt.
        assert_eq!(modes(b"\x1b[?2004huser$ ls\r\n\x1b[?2004l\rfile\r\n"), "");
    }

    #[test]
    fn vim_keeps_the_alternate_screen_and_application_keys() {
        // Vim's t_ti/t_ks/t_BE on xterm-256color.
        let stream = b"\x1b[?1049h\x1b[22;0;0t\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h\x1b[H\x1b[2J~";
        assert_eq!(
            modes(stream),
            "\x1b[?1049h\x1b[?1h\x1b=\x1b[?2004h\x1b[?1004h"
        );
        // And quitting puts everything back.
        let quit = b"\x1b[?2004l\x1b[?1004l\x1b[?1l\x1b>\x1b[?1049l\x1b[23;0;0t";
        assert_eq!(modes(&[&stream[..], &quit[..]].concat()), "");
    }

    #[test]
    fn sequences_split_across_reads_still_count() {
        let stream = b"\x1b[?1049h\x1b[?1000;1006h\x1b[2 q\x1b[?25l";
        let mut whole = TermModes::default();
        whole.feed(stream);
        for cut in 0..stream.len() {
            let mut split = TermModes::default();
            split.feed(&stream[..cut]);
            split.feed(&stream[cut..]);
            assert_eq!(
                split.restore_sequence(),
                whole.restore_sequence(),
                "cut at {cut}"
            );
        }
        assert_eq!(
            whole.restore_sequence(),
            "\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[2 q\x1b[?25l"
        );
    }

    #[test]
    fn mouse_tracking_is_one_mode_at_a_time() {
        assert_eq!(modes(b"\x1b[?1000h\x1b[?1003h"), "\x1b[?1003h");
        assert_eq!(modes(b"\x1b[?1003h\x1b[?1000l"), "");
    }

    #[test]
    fn strings_do_not_leak_mode_lookalikes() {
        // A title or a DCS that happens to contain a mode sequence.
        assert_eq!(modes(b"\x1b]0;[?2004h-ish\x07"), "");
        assert_eq!(modes(b"\x1b]0;title\x1b\\\x1b[?2004h"), "\x1b[?2004h");
        assert_eq!(modes(b"\x1bPq#0;2;0;0;0[?1h\x1b\\"), "");
        // ESC inside a string aborts it and starts the next sequence.
        assert_eq!(modes(b"\x1b]0;cut\x1b[?1h"), "\x1b[?1h");
    }

    #[test]
    fn resets_return_to_defaults() {
        assert_eq!(modes(b"\x1b[?1049h\x1b[?2004h\x1bc"), "");
        // DECSTR keeps the screen and the mouse, drops the input modes.
        assert_eq!(
            modes(b"\x1b[?1049h\x1b[?1000h\x1b[?2004h\x1b[?1h\x1b=\x1b[!p"),
            "\x1b[?1049h\x1b[?1000h"
        );
    }

    #[test]
    fn ordinary_sequences_are_not_modes() {
        // Non-private h/l (insert mode), SGR, cursor moves, charsets.
        assert_eq!(modes(b"\x1b[4h\x1b[1;2H\x1b(B\x1b)0\x1b[38;5;1m"), "");
        // A cursor-style reset is the view's default.
        assert_eq!(modes(b"\x1b[5 q\x1b[0 q"), "");
        // CAN aborts mid-sequence.
        assert_eq!(modes(b"\x1b[?2004\x18h"), "");
    }

    #[test]
    fn oversized_sequences_are_ignored() {
        let mut stream = b"\x1b[?".to_vec();
        stream.extend(std::iter::repeat_n(b'1', 200));
        stream.extend(b"h\x1b[?1h");
        assert_eq!(modes(&stream), "\x1b[?1h");
    }
}
