//! Port of the tool label heuristics in
//! src/integrations/harness/providers/cursor/cursor.ts (`toolLabel` through
//! `joinContentParts`). Cursor's ACP events often name a tool only by a call
//! id or a weak verb, so these helpers dig a readable label out of the input,
//! the locations, and the content.

use std::sync::LazyLock;

use monocode_core::js;
use regex::Regex;
use serde_json::Value;

use monocode_core::reducer::is_weak_tool_title;

use super::json::{Rec, as_record, first_nn, nn};

static CALL_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(call[-_]?|tool[-_])[a-z0-9_-]+$").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap()
});
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[_-]+").unwrap());

/// `MAX_TOOL_DETAIL_CHARS`.
pub const MAX_TOOL_DETAIL_CHARS: usize = 8_000;

/// `stringField`: a string with something other than whitespace in it. The
/// value comes back untrimmed.
pub fn string_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    match rec.get(key) {
        Some(Value::String(value)) if !js::trim(value).is_empty() => Some(value),
        _ => None,
    }
}

/// `coerceMaybeString`.
pub fn coerce_maybe_string<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    string_field(rec, key)
}

/// `numberField`: a finite number.
pub fn number_field(rec: &Rec, key: &str) -> Option<f64> {
    rec.get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// `toolLabel`.
pub fn tool_label(update: &Rec, tool: &Rec) -> Option<String> {
    let kind = string_field(update, "kind").or_else(|| string_field(tool, "kind"));
    let named = ["title", "name", "toolName", "tool_name"]
        .iter()
        .find_map(|key| human_field(update, key).or_else(|| human_field(tool, key)))
        .map(str::to_string)
        .or_else(|| meta_label(nn(update, "_meta").or_else(|| nn(tool, "_meta"))));
    let from_input = input_label(raw_input_any(update, tool));
    let from_location = location_label(nn(update, "locations").or_else(|| nn(tool, "locations")))
        .or_else(|| content_path(nn(update, "content").or_else(|| nn(tool, "content"))));

    if let Some(named) = &named
        && !is_weak_name(named)
    {
        return Some(named.clone());
    }
    if from_input.is_some() {
        return from_input;
    }
    if from_location.is_some() {
        return from_location;
    }
    if named.is_some() {
        return named;
    }
    kind_title(kind)
}

/// `update.rawInput ?? tool.rawInput ?? update.raw_input ?? tool.raw_input ??
/// update.input ?? tool.input`.
pub fn raw_input_any<'a>(update: &'a Rec, tool: &'a Rec) -> Option<&'a Value> {
    ["rawInput", "raw_input", "input"]
        .iter()
        .find_map(|key| nn(update, key).or_else(|| nn(tool, key)))
}

/// `toolOutput`: what the call produced. A step that opens an error control
/// wants the reason it failed, and the request it was making is already its
/// title, so the input fallback in [`tool_detail`] belongs to a top-level row
/// and not to this.
pub fn tool_output(update: &Rec, tool: &Rec) -> Option<String> {
    let mut content = text_from_content(update.get("content"), "\n");
    if content.is_empty() {
        content = text_from_content(tool.get("content"), "\n");
    }
    if !js::trim(&content).is_empty() {
        return Some(cap_tool_detail(&content));
    }
    let output = nn(update, "rawOutput").or_else(|| nn(tool, "rawOutput"));
    if let Some(Value::String(output)) = output
        && !js::trim(output).is_empty()
    {
        return Some(cap_tool_detail(output));
    }
    let output_text = text_from_content(output, "");
    if !js::trim(&output_text).is_empty() {
        return Some(cap_tool_detail(&output_text));
    }
    None
}

/// `toolDetail`.
pub fn tool_detail(update: &Rec, tool: &Rec) -> Option<String> {
    tool_output(update, tool).or_else(|| {
        input_label(
            nn(update, "rawInput")
                .or_else(|| nn(tool, "rawInput"))
                .or_else(|| nn(update, "input"))
                .or_else(|| nn(tool, "input")),
        )
    })
}

/// `capToolDetail`.
pub fn cap_tool_detail(value: &str) -> String {
    let text = js::trim(value);
    if js::len(text) <= MAX_TOOL_DETAIL_CHARS {
        return text.to_string();
    }
    format!("{}\n…", js::slice_prefix(text, MAX_TOOL_DETAIL_CHARS))
}

/// `inputLabel`.
pub fn input_label(value: Option<&Value>) -> Option<String> {
    if let Some(Value::String(raw)) = value
        && !js::trim(raw).is_empty()
    {
        let text = js::trim(raw);
        if looks_like_call_id(text) {
            return None;
        }
        if text.starts_with('{') || text.starts_with('[') {
            return match serde_json::from_str::<Value>(text) {
                Ok(parsed) => input_label(Some(&parsed)),
                Err(_) => Some(text.to_string()),
            };
        }
        return Some(text.to_string());
    }
    let raw = as_record(value)?;

    if let Some(command) = string_field(raw, "command") {
        return Some(command.to_string());
    }

    let from = string_field(raw, "old_path").or_else(|| string_field(raw, "from"));
    let to = string_field(raw, "new_path")
        .or_else(|| string_field(raw, "to"))
        .or_else(|| string_field(raw, "destination"));
    if let (Some(from), Some(to)) = (from, to) {
        return Some(format!("{} → {}", short_path(from), short_path(to)));
    }

    let path = [
        "path",
        "filePath",
        "file_path",
        "targetFile",
        "target_file",
        "relative_workspace_path",
        "uri",
        "url",
    ]
    .iter()
    .find_map(|key| string_field(raw, key));
    if let Some(path) = path {
        return Some(short_path(path));
    }

    let query = [
        "query",
        "pattern",
        "glob",
        "glob_pattern",
        "globPattern",
        "search_term",
        "searchTerm",
    ]
    .iter()
    .find_map(|key| string_field(raw, key));
    let name = human_field(raw, "name").or_else(|| human_field(raw, "toolName"));
    if let (Some(name), Some(query)) = (name, query) {
        return Some(format!("{name} {query}"));
    }
    if let Some(query) = query {
        return Some(query.to_string());
    }

    let nested = input_label(first_nn(raw, &["arguments", "args", "input", "params"]));
    match (name, nested) {
        (Some(name), Some(nested)) => Some(format!("{name} {nested}")),
        (None, Some(nested)) => Some(nested),
        (Some(name), None) => Some(name.to_string()),
        (None, None) => first_string_arg(raw),
    }
}

/// `firstStringArg`.
// TODO(port): JavaScript walks keys in insertion order. serde_json is built
// without `preserve_order`, so this picks the first qualifying key in sorted
// order instead.
fn first_string_arg(raw: &Rec) -> Option<String> {
    for (key, value) in raw {
        if matches!(key.as_str(), "name" | "toolName" | "kind" | "type") {
            continue;
        }
        if let Value::String(value) = value
            && !js::trim(value).is_empty()
            && !looks_like_call_id(value)
        {
            let text = js::trim(value);
            if js::len(text) <= 200 {
                return Some(text.to_string());
            }
        }
    }
    None
}

/// `contentPath` in cursor.ts. A path found directly in an array item comes
/// back as written; a nested or single-record path is shortened.
pub fn content_path(content: Option<&Value>) -> Option<String> {
    let Some(Value::Array(items)) = content else {
        let rec = as_record(content)?;
        return string_field(rec, "path").map(short_path);
    };
    for item in items {
        let Value::Object(rec) = item else {
            continue;
        };
        let path = match string_field(rec, "path") {
            Some(path) => Some(path.to_string()),
            None => content_path(nn(rec, "content").or_else(|| nn(rec, "diff"))),
        };
        if path.is_some() {
            return path;
        }
    }
    None
}

/// `locationLabel`.
pub fn location_label(locations: Option<&Value>) -> Option<String> {
    let Some(Value::Array(items)) = locations else {
        return None;
    };
    items.iter().find_map(|item| {
        let rec = item.as_object()?;
        let path = string_field(rec, "path")
            .or_else(|| string_field(rec, "uri"))
            .or_else(|| string_field(rec, "file"))?;
        Some(short_path(path))
    })
}

/// `metaLabel`.
pub fn meta_label(meta: Option<&Value>) -> Option<String> {
    let rec = as_record(meta)?;
    human_field(rec, "toolName")
        .or_else(|| human_field(rec, "name"))
        .or_else(|| human_field(rec, "displayName"))
        .map(str::to_string)
}

/// `humanField`: a string field that is not just a call id.
pub fn human_field<'a>(rec: &'a Rec, key: &str) -> Option<&'a str> {
    string_field(rec, key).filter(|value| !looks_like_call_id(value))
}

/// `kindTitle`.
pub fn kind_title(kind: Option<&str>) -> Option<String> {
    let kind = kind?;
    if js::trim(kind).is_empty() {
        return None;
    }
    let key = js::trim(kind).to_lowercase();
    let title = match key.as_str() {
        "read" => "Read",
        "edit" => "Edit",
        "delete" => "Delete",
        "move" => "Move",
        "search" => "Find",
        "execute" | "shell" | "bash" => "Shell",
        "skill" => "Skill",
        "think" => "Think",
        "fetch" => "Fetch",
        "other" => return None,
        _ => {
            let stripped = key.strip_prefix('_').unwrap_or(&key);
            return Some(SEPARATORS.replace_all(stripped, " ").into_owned());
        }
    };
    Some(title.to_string())
}

/// `isWeakName`.
pub fn is_weak_name(value: &str) -> bool {
    is_weak_tool_title(value)
}

/// `looksLikeCallId`.
pub fn looks_like_call_id(value: &str) -> bool {
    let text = js::trim(value);
    CALL_ID.is_match(text) || UUID.is_match(text)
}

/// `shortPath`: the last two path segments, unless the path has whitespace.
pub fn short_path(path: &str) -> String {
    if path.chars().any(js::is_space) {
        return path.to_string();
    }
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 2 {
        let joined = parts.join("/");
        return if joined.is_empty() {
            path.to_string()
        } else {
            joined
        };
    }
    parts[parts.len() - 2..].join("/")
}

/// `textFromContent`.
pub fn text_from_content(content: Option<&Value>, separator: &str) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(rec)) => {
            if let Some(Value::String(text)) = rec.get("text") {
                return text.clone();
            }
            match nn(rec, "content") {
                Some(nested) => text_from_content(Some(nested), separator),
                None => String::new(),
            }
        }
        Some(Value::Array(items)) => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| text_from_content(Some(item), separator))
                .filter(|part| !part.is_empty())
                .collect();
            join_content_parts(&parts, separator)
        }
        _ => String::new(),
    }
}

/// `joinContentParts`: add the separator only where the parts do not already
/// meet at whitespace.
pub fn join_content_parts(parts: &[String], separator: &str) -> String {
    let mut joined = String::new();
    for part in parts {
        if joined.is_empty() {
            joined = part.clone();
            continue;
        }
        let boundary_already_present = separator.is_empty()
            || joined.chars().next_back().is_some_and(js::is_space)
            || part.chars().next().is_some_and(js::is_space);
        if !boundary_already_present {
            joined.push_str(separator);
        }
        joined.push_str(part);
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(value: Value) -> Rec {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn prefers_a_strong_name_then_input_then_location() {
        let named = rec(json!({ "title": "Grep auth", "rawInput": { "path": "/a/b/c.ts" } }));
        assert_eq!(tool_label(&named, &named).as_deref(), Some("Grep auth"));
        let weak = rec(json!({ "title": "Read", "rawInput": { "path": "/a/b/c.ts" } }));
        assert_eq!(tool_label(&weak, &weak).as_deref(), Some("b/c.ts"));
        let located = rec(json!({ "title": "call_123", "locations": [{ "path": "/x/y/z.rs" }] }));
        assert_eq!(tool_label(&located, &located).as_deref(), Some("y/z.rs"));
        let weak_only = rec(json!({ "title": "Edit" }));
        assert_eq!(tool_label(&weak_only, &weak_only).as_deref(), Some("Edit"));
        let kind_only = rec(json!({ "kind": "_web_search" }));
        assert_eq!(
            tool_label(&kind_only, &kind_only).as_deref(),
            Some("web search")
        );
        let other = rec(json!({ "kind": "other" }));
        assert_eq!(tool_label(&other, &other), None);
    }

    #[test]
    fn labels_inputs_by_command_move_path_and_query() {
        assert_eq!(
            input_label(Some(&json!({ "command": "ls -la" }))).as_deref(),
            Some("ls -la")
        );
        assert_eq!(
            input_label(Some(&json!({ "from": "/a/b/c.ts", "to": "/a/b/d.ts" }))).as_deref(),
            Some("b/c.ts → b/d.ts")
        );
        assert_eq!(
            input_label(Some(&json!({ "name": "rg", "pattern": "todo" }))).as_deref(),
            Some("rg todo")
        );
        assert_eq!(
            input_label(Some(&json!("{\"glob\":\"*.ts\"}"))).as_deref(),
            Some("*.ts")
        );
        assert_eq!(
            input_label(Some(&json!("{not json"))).as_deref(),
            Some("{not json")
        );
        assert_eq!(input_label(Some(&json!("call_abc"))), None);
        assert_eq!(
            input_label(Some(&json!({ "name": "mcp", "args": { "query": "q" } }))).as_deref(),
            Some("mcp q")
        );
        assert_eq!(
            input_label(Some(&json!({ "kind": "x", "note": "hello" }))).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn shortens_paths_unless_they_have_spaces() {
        assert_eq!(short_path("/repo/src/a.ts"), "src/a.ts");
        assert_eq!(short_path("a.ts"), "a.ts");
        assert_eq!(short_path("/"), "/");
        assert_eq!(short_path("My Docs/a b.txt"), "My Docs/a b.txt");
    }

    #[test]
    fn recognizes_call_ids() {
        assert!(looks_like_call_id("call_abc123"));
        assert!(looks_like_call_id("tool-9"));
        assert!(looks_like_call_id("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!looks_like_call_id("Read file"));
    }

    #[test]
    fn joins_content_parts_at_whitespace() {
        let parts = vec!["one".to_string(), "two".to_string(), " three".to_string()];
        assert_eq!(join_content_parts(&parts, "\n"), "one\ntwo three");
        assert_eq!(join_content_parts(&parts, ""), "onetwo three");
        assert_eq!(
            text_from_content(
                Some(&json!([{ "type": "text", "text": "a" }, { "content": { "text": "b" } }])),
                "\n"
            ),
            "a\nb"
        );
    }

    #[test]
    fn caps_output_and_falls_back_to_raw_output() {
        let long = "x".repeat(MAX_TOOL_DETAIL_CHARS + 5);
        let update = rec(json!({ "content": [{ "type": "text", "text": long }] }));
        let capped = tool_output(&update, &update).unwrap();
        assert!(capped.ends_with("\n…"));
        let raw = rec(json!({ "rawOutput": { "content": "done" } }));
        assert_eq!(tool_output(&raw, &raw).as_deref(), Some("done"));
        let input_only = rec(json!({ "rawInput": { "command": "npm test" } }));
        assert_eq!(tool_output(&input_only, &input_only), None);
        assert_eq!(
            tool_detail(&input_only, &input_only).as_deref(),
            Some("npm test")
        );
    }

    #[test]
    fn content_path_keeps_direct_array_paths() {
        assert_eq!(
            content_path(Some(&json!([{ "path": "/a/b/c.ts" }]))).as_deref(),
            Some("/a/b/c.ts")
        );
        assert_eq!(
            content_path(Some(&json!([{ "diff": { "path": "/a/b/c.ts" } }]))).as_deref(),
            Some("b/c.ts")
        );
    }
}
