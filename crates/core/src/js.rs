//! Small helpers that reproduce JavaScript string and number semantics, so the
//! ported functions keep the TypeScript edge cases (UTF-16 lengths, `\s`,
//! `String.prototype.trim`, `Math.round`, `toFixed`).

/// JavaScript `\s` and the characters `String.prototype.trim` removes.
///
/// This differs from `char::is_whitespace`: JavaScript counts U+FEFF and does
/// not count U+0085.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// JavaScript line terminators, which `.` and `$` (with the `m` flag) stop at.
pub fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `String.prototype.trimEnd`.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `String.prototype.length`, counted in UTF-16 code units.
pub fn len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(0, units)` measured in UTF-16 code units. A surrogate pair that
/// would be cut in half is dropped instead, since Rust strings cannot hold one
/// half of a pair.
pub fn slice_prefix(s: &str, units: usize) -> &str {
    let mut used = 0;
    for (index, c) in s.char_indices() {
        let width = c.len_utf16();
        if used + width > units {
            return &s[..index];
        }
        used += width;
    }
    s
}

/// `Math.round`: halves round toward positive infinity.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Number.prototype.toFixed(1)`. Exact ties round up, as JavaScript does,
/// where Rust's formatter would round them to even.
pub fn to_fixed_1(x: f64) -> String {
    if x < 0.0 {
        return format!("-{}", to_fixed_1(-x));
    }
    let quarters = x * 4.0;
    if quarters.fract() == 0.0 && (quarters as i64) % 2 != 0 {
        // x is k + 0.25 or k + 0.75, an exact tie at one decimal place.
        return format!("{:.1}", x + 0.05);
    }
    format!("{x:.1}")
}

/// `String(n)` for the values the ported code formats: integers print
/// without a fraction, other values use the shortest round-trip form.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        if n == 0.0 {
            return "0".into();
        }
        return format!("{n:.0}");
    }
    format!("{n}")
}

/// `Number(raw)` for a stored string: surrounding whitespace is ignored, an
/// empty string is 0, and `0x`, `0o`, `0b` prefixes are integers. Returns
/// `None` where JavaScript yields `NaN`.
pub fn parse_number(raw: &str) -> Option<f64> {
    let text = trim(raw);
    if text.is_empty() {
        return Some(0.0);
    }
    let radix = |prefix: &str, base: u32| -> Option<Option<f64>> {
        let rest = text
            .strip_prefix(prefix)
            .or_else(|| text.strip_prefix(&prefix.to_ascii_uppercase()))?;
        Some(u64::from_str_radix(rest, base).ok().map(|v| v as f64))
    };
    for (prefix, base) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(value) = radix(prefix, base) {
            return value;
        }
    }
    match text {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    // Rust accepts spellings JavaScript rejects ("inf", "nan", "infinity").
    let lower = text.to_ascii_lowercase();
    if lower.contains("inf") || lower.contains("nan") {
        return None;
    }
    text.parse::<f64>().ok()
}

/// `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `Math.min(max, Math.max(min, value))`.
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    max.min(min.max(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_like_math_round() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(68.75), 69.0);
    }

    #[test]
    fn to_fixed_rounds_ties_up() {
        assert_eq!(to_fixed_1(1.25), "1.3");
        assert_eq!(to_fixed_1(1.5), "1.5");
        assert_eq!(to_fixed_1(1.0), "1.0");
        assert_eq!(to_fixed_1(1.2), "1.2");
    }

    #[test]
    fn parses_numbers_like_number() {
        assert_eq!(parse_number(" 12 "), Some(12.0));
        assert_eq!(parse_number(""), Some(0.0));
        assert_eq!(parse_number("0x10"), Some(16.0));
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("Infinity"), Some(f64::INFINITY));
    }

    #[test]
    fn utf16_prefix_keeps_pairs_whole() {
        assert_eq!(slice_prefix("abc", 2), "ab");
        assert_eq!(slice_prefix("a😀b", 2), "a");
        assert_eq!(slice_prefix("a😀b", 3), "a😀");
        assert_eq!(len("a😀"), 3);
    }

    #[test]
    fn encodes_uri_components() {
        assert_eq!(encode_uri_component("a b/ü"), "a%20b%2F%C3%BC");
        assert_eq!(encode_uri_component("x-y_z.(1)!"), "x-y_z.(1)!");
    }
}
