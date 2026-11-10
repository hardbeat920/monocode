//! Mirrors claudeCatalog.ts's live list_models mapping and launch ids.
use super::{model, Model, ModelControl};
use serde_json::Value;
use std::collections::HashSet;

pub(super) fn models(rows: &Value) -> Vec<Model> {
    let mut seen = HashSet::new();
    let default = rows
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["value"] == "default")
        .and_then(|row| row["resolvedModel"].as_str())
        .map(split_context)
        .map(|(id, _)| id);
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let value = row["value"].as_str()?.trim();
            if value.is_empty()
                || value == "default"
                || value.starts_with("cc-update-required")
                || row["disabled"] == true
                || row["hidden"] == true
            {
                return None;
            }
            let (value, context) = split_context(value);
            let (resolved, resolved_context) =
                split_context(row["resolvedModel"].as_str().unwrap_or(""));
            let id = if value.starts_with("claude-") || !value.chars().any(|c| c.is_ascii_digit()) {
                value.to_owned()
            } else if resolved.starts_with("claude-") {
                resolved.to_owned()
            } else {
                format!("claude-{value}")
            };
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = picker_name(row, &id, resolved);
            let mut model = model(&id, &name);
            model.default = row["isDefault"] == true
                || default.is_some_and(|id| !id.is_empty() && id == resolved);
            model.alias_target =
                (!resolved.is_empty() && resolved != id).then(|| resolved.to_owned());
            let levels: Vec<_> = row["supportedEffortLevels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .collect();
            if row["supportsEffort"] == true || !levels.is_empty() {
                model.efforts = levels
                    .into_iter()
                    .filter(|level| matches!(*level, "low" | "medium" | "high" | "xhigh" | "max"))
                    .map(str::to_owned)
                    .collect();
                if model.efforts.is_empty() {
                    model.efforts = ["low", "medium", "high", "max"].map(str::to_owned).to_vec();
                }
                model.default_effort = model
                    .efforts
                    .iter()
                    .find(|e| *e == "high")
                    .or(model.efforts.first())
                    .cloned();
                if model.efforts.iter().any(|e| e == "xhigh") {
                    model.efforts.push("ultracode".into());
                }
                model.efforts.push("ultrathink".into());
            } else {
                model.thinking = row["supportsAdaptiveThinking"] == true;
            }
            model.fast = row["supportsFastMode"] == true;
            if context || resolved_context {
                model.controls.push(ModelControl {
                    id: "context".into(),
                    name: "Context".into(),
                    current: "1m".into(),
                    choices: vec![("200k".into(), "200k".into()), ("1m".into(), "1M".into())],
                });
            }
            Some(model)
        })
        .collect()
}

fn split_context(value: &str) -> (&str, bool) {
    let value = value.trim();
    match value.get(value.len().saturating_sub(4)..) {
        Some(suffix) if suffix.eq_ignore_ascii_case("[1m]") => {
            (value[..value.len() - 4].trim(), true)
        }
        _ => (value, false),
    }
}

fn picker_name(row: &Value, fallback: &str, resolved: &str) -> String {
    let display = row["displayName"].as_str().unwrap_or("").trim();
    let head = row["description"]
        .as_str()
        .unwrap_or("")
        .split('·')
        .next()
        .unwrap_or("")
        .trim();
    let name = if display.is_empty() {
        if head.is_empty() {
            fallback
        } else {
            head
        }
    } else if head.len() > display.len() && head.to_lowercase().starts_with(&display.to_lowercase())
    {
        head
    } else {
        display
    };
    let Some(slug) = resolved.strip_prefix("claude-") else {
        return name.into();
    };
    let parts: Vec<_> = slug.split('-').collect();
    let numeric = |part: &str| {
        !part.is_empty()
            && part
                .split('.')
                .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
    };
    let Some(start) = parts
        .iter()
        .position(|part| numeric(part) && part.len() != 8)
    else {
        return name.into();
    };
    if start == 0 {
        return name.into();
    }
    let family = parts[..start].join(" ");
    let version = parts[start..]
        .iter()
        .take_while(|part| numeric(part) && part.len() != 8)
        .copied()
        .collect::<Vec<_>>()
        .join(".");
    let pattern = regex::Regex::new(&format!(
        r"(?i)^(Claude\s+)?({})(.*)$",
        regex::escape(&family)
    ))
    .unwrap();
    let Some(captures) = pattern.captures(name) else {
        return name.into();
    };
    let suffix = captures.get(3).unwrap().as_str();
    let version_suffix = suffix.trim_start();
    let version_suffix = version_suffix
        .strip_prefix('v')
        .or_else(|| version_suffix.strip_prefix('V'))
        .unwrap_or(version_suffix);
    if suffix.starts_with(char::is_whitespace)
        && version_suffix.starts_with(|c: char| c.is_ascii_digit())
    {
        return name.into();
    }
    format!(
        "{}{} {version}{suffix}",
        captures.get(1).map_or("", |m| m.as_str()),
        &captures[2]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_catalog_matches_tauri_names_launch_ids_and_available_rows() {
        let models = models(&json!([
            {"value":"default","resolvedModel":"claude-sonnet-5","displayName":"Default (recommended)"},
            {"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus","supportsEffort":true,"supportedEffortLevels":["low","high","xhigh","max"],"supportsFastMode":true},
            {"value":"fable[1m]","resolvedModel":"claude-fable-5-1","displayName":"Fable","description":"Fable 5.1 · Capable"},
            {"value":"sonnet-5","resolvedModel":"claude-sonnet-5","displayName":"Sonnet"},
            {"value":"haiku","resolvedModel":"claude-haiku-4-5-20251001","displayName":"Haiku","supportsAdaptiveThinking":true},
            {"value":"opus","displayName":"Duplicate"},
            {"value":"cc-update-required-1","displayName":"Update required"},
            {"value":"unavailable","disabled":true},
            {"value":"hidden","hidden":true}
        ]));
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["opus", "fable", "claude-sonnet-5", "haiku"]
        );
        assert_eq!(
            models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            ["Opus 5.5", "Fable 5.1", "Sonnet 5", "Haiku 4.5"]
        );
        assert_eq!(models[0].alias_target.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            models[0].efforts,
            ["low", "high", "xhigh", "max", "ultracode", "ultrathink"]
        );
        assert!(models[0].fast);
        assert!(!models[0].default);
        assert!(models[2].default);
        assert_eq!(models[1].controls[0].current, "1m");
        assert!(models[3].thinking);
    }

    #[test]
    fn preserves_advertised_versions_and_qualifies_generic_names() {
        let models = models(&json!([
            {"value":"opus[1m]","resolvedModel":"claude-opus-5-5","displayName":"Opus (1M context)"},
            {"value":"haiku","resolvedModel":"claude-haiku-4-5-20251001","displayName":"Haiku 4.5"},
            {"value":"next-family","resolvedModel":"claude-next-family-7.2-20300101","displayName":"Next Family — recommended"},
            {"value":"opus-5-5","displayName":"Claude Opus 5.5"}
        ]));
        assert_eq!(
            models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            [
                "Opus 5.5 (1M context)",
                "Haiku 4.5",
                "Next Family 7.2 — recommended",
                "Claude Opus 5.5"
            ]
        );
        assert_eq!(models[3].id, "claude-opus-5-5");
    }
}
