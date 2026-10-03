//! Small JavaScript string helpers the inbox ports share: `localeCompare`,
//! UTF-16 clipping with an ellipsis, and the `\s+` collapse used by activity
//! summaries.

use std::cmp::Ordering;

use monocode_core::js;

/// Approximation of `String.prototype.localeCompare` with the default ICU
/// collation: punctuation and symbols sort before digits, digits before
/// letters, letters compare without case first, and lowercase wins a tie.
// TODO(port): ICU collation has more rules (accents, ignorable characters).
// This matches it for the paths, names, and kinds the inbox sorts.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    fn class(c: char) -> u8 {
        if c.is_alphabetic() {
            2
        } else if c.is_numeric() {
            1
        } else {
            0
        }
    }
    let primary = |s: &str| -> Vec<(u8, String)> {
        s.chars()
            .map(|c| (class(c), c.to_lowercase().collect::<String>()))
            .collect()
    };
    primary(a).cmp(&primary(b)).then_with(|| {
        // Tertiary level: lowercase before uppercase.
        let flip =
            |s: &str| -> Vec<(bool, char)> { s.chars().map(|c| (c.is_uppercase(), c)).collect() };
        flip(a).cmp(&flip(b))
    })
}

/// `value.length <= max ? value : `${value.slice(0, max - 1)}…``, in UTF-16
/// code units.
pub fn clip(value: &str, max_chars: usize) -> String {
    if js::len(value) <= max_chars {
        return value.to_string();
    }
    format!("{}…", js::slice_prefix(value, max_chars.saturating_sub(1)))
}

/// `value.replace(/\s+/g, " ").trim()`.
pub fn one_line(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;
    for c in value.chars() {
        if js::is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    js::trim(&out).to_string()
}

/// `JSON.stringify(value)` for a string.
pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// `value?.trim().toLowerCase() ?? ""`.
pub fn normalized(value: Option<&str>) -> String {
    value
        .map(|value| js::trim(value).to_lowercase())
        .unwrap_or_default()
}

/// `value?.trim() || fallback` for an optional string: `None` when the
/// trimmed value is empty.
pub fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    value.map(js::trim).filter(|value| !value.is_empty())
}

/// `value || fallback` for an optional string: `None` when it is empty.
pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_like_locale_compare_for_paths() {
        assert_eq!(locale_compare("", "/tmp/first"), Ordering::Less);
        assert_eq!(locale_compare("/tmp/a", "/tmp/B"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("issue", "pr"), Ordering::Less);
        assert_eq!(locale_compare("Billing", "Onboarding"), Ordering::Less);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
    }

    #[test]
    fn clips_in_utf16_units() {
        assert_eq!(clip("abcdef", 6), "abcdef");
        assert_eq!(clip("abcdefg", 6), "abcde…");
        assert_eq!(one_line("  a \n\t b  "), "a b");
    }
}
