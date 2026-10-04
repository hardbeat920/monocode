//! Small JavaScript string helpers the inbox ports share: `localeCompare`,
//! UTF-16 clipping with an ellipsis, and the `\s+` collapse used by activity
//! summaries.

use std::cmp::Ordering;

use monocode_core::js;

/// `String.prototype.localeCompare` with the OS default locale.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    monocode_locale::compare(a, b)
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
    fn matches_intl_text_punctuation_and_accents() {
        for locale in ["en", "fr", "ja", "ar"] {
            monocode_locale::with_locale(locale, || {
                assert_eq!(locale_compare("file_a.rs", "file-a.rs"), Ordering::Less);
                assert_eq!(locale_compare("file-a.rs", "file.a.rs"), Ordering::Less);
                assert_eq!(locale_compare("filee.rs", "fileé.rs"), Ordering::Less);
                assert_eq!(locale_compare("fileé.rs", "filez.rs"), Ordering::Less);
            })
            .unwrap();
        }
    }

    #[test]
    fn matches_intl_text_canonical_equivalence() {
        for locale in ["en", "fr", "ja", "ar"] {
            monocode_locale::with_locale(locale, || {
                assert_eq!(
                    locale_compare("fileé.rs", "filee\u{301}.rs"),
                    Ordering::Equal
                );
                assert_eq!(locale_compare("fileÅ.rs", "fileÅ.rs"), Ordering::Equal);
            })
            .unwrap();
        }
    }

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
