//! Port of src/features/terminal/model/terminalChrome.ts.

/// `isOscColorQuery`: an OSC 10, 11, or 12 payload asking for a color.
pub fn is_osc_color_query(data: &str) -> bool {
    data == "?" || data.starts_with('?')
}

/// `oscColorReply`: reply to a terminal color query with a `#rrggbb` value.
/// `code` is 10 (foreground), 11 (background), or 12 (cursor).
pub fn osc_color_reply(code: u8, hex: &str) -> String {
    let value: Vec<u16> = hex
        .strip_prefix('#')
        .unwrap_or(hex)
        .encode_utf16()
        .collect();
    if value.len() != 6 {
        return String::new();
    }
    let part = |range: std::ops::Range<usize>| String::from_utf16_lossy(&value[range]);
    let (r, g, b) = (part(0..2), part(2..4), part(4..6));
    format!("\x1b]{code};rgb:{r}{r}/{g}{g}/{b}{b}\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_color_query_detects_a_palette_request() {
        assert!(is_osc_color_query("?"));
        assert!(!is_osc_color_query("rgb:0000/0000/0000"));
    }

    #[test]
    fn osc_color_query_reports_16_bit_rgb_for_a_hex_color() {
        assert_eq!(
            osc_color_reply(11, "#141b1f"),
            "\x1b]11;rgb:1414/1b1b/1f1f\x1b\\"
        );
        assert_eq!(osc_color_reply(10, "#fff"), "");
    }
}
