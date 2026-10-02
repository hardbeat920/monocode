//! Port of src/shared/lib/jsonText.ts: pulling a JSON object out of model
//! output.

use monocode_core::js;
use serde_json::{Map, Value};

/// `limitSection`: cut `value` to `max_chars` UTF-16 units and mark the cut.
pub fn limit_section(value: &str, max_chars: usize) -> String {
    if js::len(value) <= max_chars {
        return value.to_string();
    }
    format!("{}\n\n[truncated]", js::slice_prefix(value, max_chars))
}

/// `extractJsonObject`: the first balanced `{...}` in `raw`, skipping braces
/// inside strings.
pub fn extract_json_object(raw: &str) -> Option<&str> {
    let trimmed = js::trim(raw);
    let start = trimmed.find('{')?;
    let mut depth = 0i64;
    let mut in_string = false;
    let mut escaping = false;
    for (offset, ch) in trimmed[start..].char_indices() {
        if in_string {
            if escaping {
                escaping = false;
            } else if ch == '\\' {
                escaping = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&trimmed[start..start + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

const GIT_TEXT_KEYS: [&str; 5] = ["subject", "title", "message", "body", "branch"];

/// `parseJsonObject`: the first JSON object in `raw` with a git text field,
/// else the first JSON object at all.
pub fn parse_json_object(raw: &str) -> Option<Map<String, Value>> {
    let trimmed = js::trim(raw);
    let mut search_from = 0;
    let mut fallback = None;
    while search_from < trimmed.len() {
        let Some(found) = trimmed[search_from..].find('{') else {
            break;
        };
        let start = search_from + found;
        let Some(json) = extract_json_object(&trimmed[start..]) else {
            break;
        };
        search_from = start + 1;
        // Models often mention `{` in preamble; keep scanning for real JSON.
        let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        if GIT_TEXT_KEYS
            .iter()
            .any(|key| rec.get(*key).is_some_and(Value::is_string))
        {
            return Some(rec);
        }
        if fallback.is_none() {
            fallback = Some(rec);
        }
    }
    fallback
}

/// `stringField`: the string at `key`, or empty.
pub fn string_field<'a>(rec: &'a Map<String, Value>, key: &str) -> &'a str {
    rec.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_long_sections() {
        assert_eq!(limit_section("abc", 3), "abc");
        assert_eq!(limit_section("abcdef", 3), "abc\n\n[truncated]");
    }

    #[test]
    fn extracts_balanced_objects_past_string_braces() {
        assert_eq!(
            extract_json_object(r#"Sure: {"a": "}{", "b": {"c": 1}} done"#),
            Some(r#"{"a": "}{", "b": {"c": 1}}"#)
        );
        assert_eq!(extract_json_object("no json"), None);
        assert_eq!(extract_json_object("{ open"), None);
    }

    #[test]
    fn prefers_objects_with_git_text() {
        let rec = parse_json_object(r#"Use {braces} then {"x": 1} and {"title": "Fix"}"#).unwrap();
        assert_eq!(string_field(&rec, "title"), "Fix");
        let fallback = parse_json_object(r#"{"x": 1}"#).unwrap();
        assert_eq!(fallback.get("x"), Some(&Value::from(1)));
        assert_eq!(string_field(&fallback, "missing"), "");
        assert!(parse_json_object("nothing").is_none());
    }
}
