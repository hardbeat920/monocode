//! Port of src/shared/lib/jsonText.ts: pull a JSON object out of model text.
//! `session_title` and `git_text` need it.

use serde_json::{Map, Value};

use monocode_core::js;

/// `limitSection`. Lengths count UTF-16 code units, as in JavaScript.
pub fn limit_section(value: &str, max_chars: usize) -> String {
    if js::len(value) <= max_chars {
        return value.to_string();
    }
    format!("{}\n\n[truncated]", js::slice_prefix(value, max_chars))
}

/// `extractJsonObject`: the first balanced `{...}` in the trimmed text.
pub fn extract_json_object(raw: &str) -> Option<String> {
    let trimmed = js::trim(raw);
    let start = trimmed.find('{')?;
    let mut depth = 0i64;
    let mut in_string = false;
    let mut escaping = false;
    for (offset, c) in trimmed[start..].char_indices() {
        if in_string {
            if escaping {
                escaping = false;
            } else if c == '\\' {
                escaping = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + 1;
                    return Some(trimmed[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

const GIT_TEXT_KEYS: [&str; 5] = ["subject", "title", "message", "body", "branch"];

/// `parseJsonObject`: the first object that has a git text key, else the
/// first object at all.
pub fn parse_json_object(raw: &str) -> Option<Map<String, Value>> {
    let trimmed = js::trim(raw);
    let mut search_from = 0;
    let mut fallback: Option<Map<String, Value>> = None;
    while search_from < trimmed.len() {
        let Some(found) = trimmed[search_from..].find('{') else {
            break;
        };
        let start = search_from + found;
        let Some(json) = extract_json_object(&trimmed[start..]) else {
            break;
        };
        search_from = start + 1;
        // Models often mention `{` in a preamble, so keep scanning for real JSON.
        let Ok(Value::Object(rec)) = serde_json::from_str::<Value>(&json) else {
            continue;
        };
        if GIT_TEXT_KEYS
            .iter()
            .any(|key| matches!(rec.get(*key), Some(Value::String(_))))
        {
            return Some(rec);
        }
        if fallback.is_none() {
            fallback = Some(rec);
        }
    }
    fallback
}

/// `stringField`.
pub fn string_field(rec: &Map<String, Value>, key: &str) -> String {
    match rec.get(key) {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

/// JavaScript `String(value)` for a parsed JSON value.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number
            .as_f64()
            .map(js::number_to_string)
            .unwrap_or_else(|| number.to_string()),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// The first line of `text`, as `text.split(/\r?\n/g)[0]`.
pub fn first_line(text: &str) -> &str {
    let line = text.split('\n').next().unwrap_or("");
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_first_balanced_object() {
        assert_eq!(
            extract_json_object(" note {\"a\": \"}\", \"b\": {\"c\": 1}} tail").as_deref(),
            Some("{\"a\": \"}\", \"b\": {\"c\": 1}}")
        );
        assert_eq!(extract_json_object("no json"), None);
        assert_eq!(extract_json_object("{ unclosed"), None);
    }

    #[test]
    fn prefers_an_object_with_a_git_text_key() {
        let rec =
            parse_json_object("use {braces} then {\"x\":1} and {\"subject\":\"Fix\"}").unwrap();
        assert_eq!(string_field(&rec, "subject"), "Fix");
        let fallback = parse_json_object("{\"x\":1}").unwrap();
        assert_eq!(fallback.get("x"), Some(&Value::from(1)));
    }

    #[test]
    fn limits_long_sections() {
        assert_eq!(limit_section("abc", 3), "abc");
        assert_eq!(limit_section("abcd", 3), "abc\n\n[truncated]");
    }

    #[test]
    fn stringifies_like_javascript() {
        assert_eq!(js_string(&serde_json::json!(3)), "3");
        assert_eq!(js_string(&serde_json::json!(null)), "null");
        assert_eq!(js_string(&serde_json::json!(["a", null, 2])), "a,,2");
        assert_eq!(js_string(&serde_json::json!({})), "[object Object]");
    }
}
