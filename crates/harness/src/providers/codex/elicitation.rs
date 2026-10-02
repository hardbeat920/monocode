//! Port of src/integrations/harness/providers/codex/codexElicitation.ts: MCP
//! elicitation requests that fit the Allow and Deny approval UI.

use serde_json::{Map, Value};

use super::json::{as_record, string_field};

/// A confirmation the approval UI can show: its title, and the form content
/// to send back when the user allows it.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfirmation {
    pub title: String,
    pub content: Map<String, Value>,
}

const SCHEMA_KEYS: [&str; 7] = [
    "type",
    "properties",
    "required",
    "title",
    "description",
    "$schema",
    "additionalProperties",
];

const FIELD_KEYS: [&str; 4] = ["type", "title", "description", "default"];

/// `codexMcpConfirmation`. Only confirmations can be shown faithfully by the
/// Allow and Deny UI, so anything else returns `None`.
pub fn codex_mcp_confirmation(params: &Value) -> Option<McpConfirmation> {
    let rec = as_record(Some(params));
    let schema = as_record(rec.and_then(|rec| rec.get("requestedSchema")));
    let properties = as_record(schema.and_then(|schema| schema.get("properties")));
    let mode = rec.and_then(|rec| rec.get("mode")).and_then(Value::as_str);
    let schema = schema?;
    let properties = properties?;
    if !matches!(mode, Some("form" | "openai/form" | "openaiForm"))
        || schema.get("type").and_then(Value::as_str) != Some("object")
        || schema
            .keys()
            .any(|key| !SCHEMA_KEYS.contains(&key.as_str()))
    {
        return None;
    }

    let required: &[Value] = match schema.get("required") {
        None | Some(Value::Null) => &[],
        Some(Value::Array(list)) => list,
        Some(_) => return None,
    };
    if required
        .iter()
        .any(|key| !key.as_str().is_some_and(|key| properties.contains_key(key)))
        || properties.len() > 1
    {
        return None;
    }

    let mut detail: Option<String> = None;
    let mut content = Map::new();
    if let Some((key, value)) = properties.iter().next() {
        let field = as_record(Some(value))?;
        if field.get("type").and_then(Value::as_str) != Some("boolean")
            || field
                .keys()
                .any(|name| !FIELD_KEYS.contains(&name.as_str()))
        {
            return None;
        }
        let parts: Vec<&str> = [
            Some(string_field(Some(field), "title").unwrap_or(key)),
            string_field(Some(field), "description"),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
        detail = Some(parts.join(" \u{2014} "));
        content.insert(key.clone(), Value::Bool(true));
    }

    let message = string_field(rec, "message").unwrap_or("Approve request");
    let server = string_field(rec, "serverName").unwrap_or("MCP");
    let suffix = match detail.as_deref() {
        Some(detail) if !detail.is_empty() => format!(" \u{2014} {detail}"),
        _ => String::new(),
    };
    Some(McpConfirmation {
        title: format!("{server}: {message}{suffix}"),
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn confirmation() -> Value {
        json!({
            "mode": "form",
            "serverName": "example",
            "message": "Confirm access",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "approved": { "type": "boolean", "title": "Read this source?", "default": false },
                },
                "required": ["approved"],
                "additionalProperties": false,
            },
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut params = confirmation();
        params[key] = value;
        params
    }

    #[test]
    fn supports_a_boolean_confirmation_in_each_form_mode() {
        for mode in ["form", "openai/form", "openaiForm"] {
            let result = codex_mcp_confirmation(&with("mode", json!(mode))).unwrap();
            assert_eq!(
                result.title,
                "example: Confirm access \u{2014} Read this source?"
            );
            assert_eq!(Value::Object(result.content), json!({ "approved": true }));
        }
    }

    #[test]
    fn keeps_empty_confirmations_compatible() {
        let result = codex_mcp_confirmation(&with(
            "requestedSchema",
            json!({ "type": "object", "properties": {}, "required": [] }),
        ))
        .unwrap();
        assert!(result.content.is_empty());
    }

    #[test]
    fn does_not_invent_answers_for_unsupported_schemas() {
        let mut all_of = confirmation()["requestedSchema"].clone();
        all_of["allOf"] = json!([{ "required": ["missing"] }]);
        for schema in [
            json!({ "type": "object", "properties": {}, "required": ["missing"] }),
            json!({ "type": "object", "properties": {}, "required": "approved" }),
            json!({ "type": "object", "properties": { "approved": { "type": "string" } } }),
            json!({ "type": "object", "properties": { "approved": { "type": "boolean", "const": false } } }),
            json!({ "type": "object", "properties": { "approved": { "type": "boolean" }, "name": { "type": "string" } } }),
            all_of,
        ] {
            assert_eq!(
                codex_mcp_confirmation(&with("requestedSchema", schema.clone())),
                None,
                "{schema}"
            );
        }
    }

    #[test]
    fn does_not_treat_browser_authorization_as_a_confirmation() {
        assert_eq!(codex_mcp_confirmation(&with("mode", json!("url"))), None);
    }
}
