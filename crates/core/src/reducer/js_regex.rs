//! Text helpers that keep JavaScript regex semantics for the patterns ported
//! from the TypeScript reducer.
//!
//! A JavaScript regex without the `u` flag differs from a Rust `regex` in
//! four ways that matter here. `\s` includes U+FEFF and excludes U+0085. `.`
//! stops at `\r`, U+2028, and U+2029 as well as `\n`. `\b` and `\w` use ASCII
//! word characters only. `/i` folds ASCII case only, so `k` never matches the
//! Kelvin sign. Patterns written with the `{S}`, `{NS}`, `{DOT}`, and `{B}`
//! placeholders below, and `(?i-u:...)` for case-insensitive literals, keep
//! those rules.

use regex::{Captures, Regex};

use crate::js;

/// JavaScript `\s`.
const S: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// JavaScript `\S`.
const NS: &str = r"[^\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// JavaScript `.` without the `s` flag.
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
/// JavaScript `\b`.
const B: &str = r"(?-u:\b)";

/// Compiles a pattern after expanding the placeholders.
pub(crate) fn compile(pattern: &str) -> Regex {
    let expanded = pattern
        .replace("{S}", S)
        .replace("{NS}", NS)
        .replace("{DOT}", DOT)
        .replace("{B}", B);
    Regex::new(&expanded).expect("ported pattern compiles")
}

/// A lazily compiled pattern, see [`compile`].
macro_rules! js_regex {
    ($pattern:literal) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| $crate::reducer::js_regex::compile($pattern))
    }};
}
pub(crate) use js_regex;

/// A JavaScript truthy string: `Some` and not empty.
pub(crate) fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// JavaScript `\w`.
pub(crate) fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `/^(?:a|b)\b/i.test(text)` for ASCII words that end in a word character.
pub(crate) fn starts_with_word_ci(text: &str, words: &[&str]) -> bool {
    let bytes = text.as_bytes();
    words.iter().any(|word| {
        let word = word.as_bytes();
        bytes.len() >= word.len()
            && bytes[..word.len()].eq_ignore_ascii_case(word)
            && bytes
                .get(word.len())
                .is_none_or(|next| !is_word_byte(*next))
    })
}

/// `/^(?:a|b)\s+\S/i.test(text)` (or without `/i` when `ci` is false) for
/// ASCII words.
pub(crate) fn starts_with_word_then_text(text: &str, words: &[&str], ci: bool) -> bool {
    let bytes = text.as_bytes();
    words.iter().any(|word| {
        let head = word.as_bytes();
        if bytes.len() < head.len() {
            return false;
        }
        let same = if ci {
            bytes[..head.len()].eq_ignore_ascii_case(head)
        } else {
            &bytes[..head.len()] == head
        };
        if !same {
            return false;
        }
        let rest = &text[head.len()..];
        let after = rest.trim_start_matches(js::is_space);
        after.len() < rest.len() && !after.is_empty()
    })
}

/// Runs an anchored (`^`) pattern at every JavaScript line start, the way
/// `/^.../m` does, and returns the first match.
pub(crate) fn multiline_captures<'t>(re: &Regex, text: &'t str) -> Option<Captures<'t>> {
    let mut start = 0;
    loop {
        if let Some(found) = re.captures(&text[start..]) {
            return Some(found);
        }
        let next = text[start..].find(js::is_line_terminator)?;
        let terminator = text[start + next..].chars().next()?;
        start += next + terminator.len_utf8();
    }
}

/// `decodeURIComponent`, or `None` where it throws a `URIError`.
pub(crate) fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_javascript_classes() {
        let re = compile(r"^a{S}b{DOT}$");
        assert!(re.is_match("a\u{feff}bx"));
        assert!(!re.is_match("a\u{85}bx"));
        assert!(!re.is_match("a b\r"));
        let word = compile(r"^read{B}");
        assert!(word.is_match("readé"));
        assert!(!word.is_match("reads"));
        let ci = compile(r"^(?i-u:k)$");
        assert!(ci.is_match("K"));
        assert!(!ci.is_match("\u{212a}"));
    }

    #[test]
    fn matches_word_prefixes() {
        assert!(starts_with_word_ci("Read file", &["read"]));
        assert!(starts_with_word_ci("READ", &["read"]));
        assert!(!starts_with_word_ci("Reading", &["read"]));
        assert!(starts_with_word_ci("Reading", &["read", "reading"]));
        assert!(starts_with_word_then_text("List  x", &["list"], true));
        assert!(!starts_with_word_then_text("List  ", &["list"], true));
        assert!(!starts_with_word_then_text("list x", &["List"], false));
    }

    #[test]
    fn decodes_uri_components() {
        assert_eq!(decode_uri_component("/a%20b").as_deref(), Some("/a b"));
        assert_eq!(decode_uri_component("%C3%A9").as_deref(), Some("é"));
        assert_eq!(decode_uri_component("%E0%A4%A"), None);
        assert_eq!(decode_uri_component("%FF"), None);
    }

    #[test]
    fn finds_multiline_matches_after_any_terminator() {
        let re = compile(r"^\+\+\+ (x)");
        assert!(multiline_captures(&re, "a\r+++ x").is_some());
        assert!(multiline_captures(&re, "a\u{2028}+++ x").is_some());
        assert!(multiline_captures(&re, "a +++ x").is_none());
    }
}
