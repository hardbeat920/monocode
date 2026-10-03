//! JavaScript string helpers the ported submit modules share: UTF-16 offsets
//! and the `^\s*\/name(?=\s|$)\s*` command prefix that Rust's `regex` cannot
//! express because it has no lookahead.
//!
//! Copied from monocode-engine's `submit` package, which this crate may not
//! depend on yet. Delete this copy once the view crates depend on the engine.

use monocode_core::js;

/// Byte offset of the UTF-16 offset `units` in `s`, clamped to its length.
/// An offset inside a surrogate pair rounds down to the pair's start.
pub fn byte_at_utf16(s: &str, units: usize) -> usize {
    let mut used = 0;
    for (index, c) in s.char_indices() {
        if used >= units {
            return index;
        }
        let width = c.len_utf16();
        if used + width > units {
            return index;
        }
        used += width;
    }
    s.len()
}

/// UTF-16 offset of the byte offset `byte` in `s`.
pub fn utf16_at_byte(s: &str, byte: usize) -> usize {
    js::len(&s[..byte.min(s.len())])
}

/// `s.slice(start, end)` in UTF-16 units, with both ends clamped.
pub fn slice_utf16(s: &str, start: usize, end: usize) -> &str {
    let from = byte_at_utf16(s, start);
    let to = byte_at_utf16(s, end.max(start));
    &s[from..to]
}

/// `s.slice(start)` in UTF-16 units.
pub fn slice_utf16_from(s: &str, start: usize) -> &str {
    &s[byte_at_utf16(s, start)..]
}

/// Skip JavaScript `\s` characters from `from`, returning the byte offset of
/// the first other character.
pub fn skip_space(s: &str, from: usize) -> usize {
    s[from..]
        .char_indices()
        .find(|(_, c)| !js::is_space(*c))
        .map_or(s.len(), |(index, _)| from + index)
}

/// Match `^\s*\/(?:name|...)(?=\s|$)\s*` with ASCII case folding, as the
/// TypeScript `/i` regexes did. Returns the byte offset just past the
/// trailing whitespace.
pub fn leading_command(text: &str, names: &[&str]) -> Option<usize> {
    let start = skip_space(text, 0);
    let rest = text[start..].strip_prefix('/')?;
    let after_slash = start + 1;
    for name in names {
        let Some(head) = rest.get(..name.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(name) {
            continue;
        }
        let end = after_slash + name.len();
        let boundary = text[end..].chars().next().is_none_or(js::is_space);
        if boundary {
            return Some(skip_space(text, end));
        }
    }
    None
}

/// Match `^\s*\/name\s*$` with ASCII case folding.
pub fn standalone_command(text: &str, name: &str) -> bool {
    leading_command(text, &[name]).is_some_and(|end| end == text.len())
}

/// `text.replace(/\s+/g, " ")` with JavaScript `\s`.
pub fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
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
    out
}

/// `text.replace(/\r\n?/g, "\n")`.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `JSON.stringify(value)` for a string.
pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_leading_commands_at_a_word_boundary() {
        assert_eq!(leading_command("  /Draft  hello", &["draft"]), Some(10));
        assert_eq!(leading_command("/draft", &["draft"]), Some(6));
        assert_eq!(leading_command("/drafts", &["draft"]), None);
        assert_eq!(leading_command("x /draft", &["draft"]), None);
        assert_eq!(
            leading_command("/monocode x", &["mono", "monocode"]),
            Some(10)
        );
        assert!(standalone_command(" /MCP ", "mcp"));
        assert!(!standalone_command("/mcp x", "mcp"));
    }

    #[test]
    fn slices_by_utf16_units() {
        let text = "a😀b";
        assert_eq!(slice_utf16(text, 0, 1), "a");
        assert_eq!(slice_utf16(text, 1, 3), "😀");
        assert_eq!(slice_utf16_from(text, 3), "b");
        assert_eq!(utf16_at_byte(text, text.len()), 4);
        assert_eq!(collapse_space(" a \n\t b "), " a b ");
    }
}
