//! MonoCode Host v1 session sync. Values retain provider-specific block fields.
use super::Connections;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const HOST_PROTOCOL_VERSION: u64 = 1;

pub fn supported_providers() -> &'static [&'static str] {
    &[
        "codex",
        "claude",
        "cursor",
        "grok",
        "opencode",
        "pi",
        "omp",
        "fx",
        "hermes",
        "antigravity",
        "devin",
    ]
}

/// The native common controls use the same provider-specific keys as the
/// headless host adapters. Other advertised controls retain their exact IDs.
pub fn wire_model_settings(
    provider: &str,
    settings: &crate::agent::Settings,
) -> HashMap<String, String> {
    let mut values = settings.model_settings.clone();
    if let Some(effort) = &settings.effort {
        values.insert(
            if provider == "codex" {
                "reasoningEffort"
            } else {
                "effort"
            }
            .into(),
            effort.clone(),
        );
    }
    if provider == "codex" {
        if let Some(tier) = &settings.service_tier {
            values.insert("serviceTier".into(), tier.clone());
        } else if settings.fast {
            values.insert("serviceTier".into(), "fast".into());
        }
    } else {
        values.insert("fast".into(), settings.fast.to_string());
    }
    if let Some(thinking) = settings.thinking {
        values.insert("thinking".into(), thinking.to_string());
    }
    values
}

/// Canonical configuration used to distinguish a local unsent choice from a
/// host update made by another desktop.
pub fn settings_configuration(provider: &str, settings: &crate::agent::Settings) -> Value {
    let model = settings.model.as_deref().map(|model| {
        if model.starts_with(&format!("{provider}:")) {
            model.to_owned()
        } else {
            format!("{provider}:{model}")
        }
    });
    json!({"model":model,"modelSettings":wire_model_settings(provider, settings),"runtimeMode":settings.access.id()})
}

/// Host attachment references require RFC 4122 UUIDs, unlike command IDs.
pub fn new_attachment_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
/// A transfer can exceed the per-response RPC cap while retaining a memory bound.
pub const MAX_SESSION_SYNC_UNITS: usize = 256 * 1024 * 1024;
const MAX_SESSION_SYNC_BYTES: usize = 512 * 1024 * 1024;
const MAX_SESSION_SYNC_CHUNKS: usize = 4096;

/// Applies a delta only to its exact base revision. Callers can recover by loading
/// a snapshot when a revision or block is missing.
pub fn apply_session_sync(known: Option<&Value>, sync: &Value) -> Result<Value, String> {
    match sync.get("kind").and_then(Value::as_str) {
        Some("snapshot") => sync
            .get("value")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| "Invalid session snapshot".into()),
        Some("unchanged" | "delta") => {
            let known = known.ok_or("Session sync base does not match")?;
            let delta = sync["kind"] == "delta";
            let base = sync
                .get(if delta { "base" } else { "revision" })
                .and_then(Value::as_u64);
            if base.is_none() || known.get("revision").and_then(Value::as_u64) != base {
                return Err("Session sync base does not match".into());
            }
            if !delta {
                return Ok(known.clone());
            }
            let previous = known
                .pointer("/session/blocks")
                .and_then(Value::as_array)
                .ok_or("Session sync is missing its blocks")?;
            let changed = sync
                .get("blocks")
                .and_then(Value::as_array)
                .ok_or("Session sync is missing its blocks")?;
            let mut blocks = HashMap::new();
            for block in previous.iter().chain(changed) {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("Session sync has an invalid block")?;
                blocks.insert(id, block);
            }
            let ids = sync
                .get("blockIds")
                .and_then(Value::as_array)
                .ok_or("Session sync is missing its block order")?;
            let mut ordered = Vec::with_capacity(ids.len());
            for id in ids {
                let id = id.as_str().ok_or("Session sync has an invalid block ID")?;
                ordered.push((*blocks.get(id).ok_or("Session sync is missing a block")?).clone());
            }
            let mut next = sync
                .get("value")
                .filter(|value| value.is_object())
                .cloned()
                .ok_or("Session sync is missing its session")?;
            next.get_mut("session")
                .and_then(Value::as_object_mut)
                .ok_or("Session sync is missing its session")?
                .insert("blocks".into(), Value::Array(ordered));
            Ok(next)
        }
        _ => Err("Invalid session sync response".into()),
    }
}

/// Fetches one immutable sync transfer. Offsets and the reported length count
/// UTF-16 units, as in the JavaScript host, rather than UTF-8 bytes or characters.
pub fn read_session_sync(
    session_id: &str,
    revision: Option<u64>,
    mut request: impl FnMut(&str, Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let response = request(
        "sessions.sync",
        json!({ "sessionId": session_id, "revision": revision }),
    )?;
    if response.get("kind").and_then(Value::as_str) != Some("chunked") {
        return Ok(response);
    }
    let transfer = response
        .get("transfer")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Invalid session transfer")?;
    let length = response
        .get("length")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|length| *length > 0 && *length <= MAX_SESSION_SYNC_UNITS)
        .ok_or("Session transfer has an invalid length")?;
    let mut offset = 0;
    let mut text = String::with_capacity(length.min(4 * 1024 * 1024));
    let mut chunks = 0;
    while offset < length {
        chunks += 1;
        if chunks > MAX_SESSION_SYNC_CHUNKS {
            return Err("Session transfer has too many pieces".into());
        }
        let chunk = request(
            "sessions.syncChunk",
            json!({
                "sessionId": session_id, "transfer": transfer, "offset": offset,
            }),
        )?;
        let data = chunk
            .get("data")
            .and_then(Value::as_str)
            .filter(|data| !data.is_empty())
            .ok_or("Session transfer ended early")?;
        offset = offset
            .checked_add(data.encode_utf16().count())
            .ok_or("Session transfer has an unexpected length")?;
        if offset > length {
            return Err("Session transfer has an unexpected length".into());
        }
        if text.len().saturating_add(data.len()) > MAX_SESSION_SYNC_BYTES {
            return Err("Session transfer is too large".into());
        }
        text.push_str(data);
    }
    serde_json::from_str(&text).map_err(|_| "Invalid session transfer data".into())
}

impl Connections {
    pub fn load_session(
        &self,
        machine_id: &str,
        session_id: &str,
        known: Option<&Value>,
    ) -> Result<Value, String> {
        let sync = |revision| {
            read_session_sync(session_id, revision, |method, params| {
                self.request(machine_id, method, params)
            })
        };
        let update = sync(
            known
                .and_then(|value| value.get("revision"))
                .and_then(Value::as_u64),
        )?;
        match apply_session_sync(known, &update) {
            Ok(value) => Ok(value),
            Err(_) => apply_session_sync(None, &sync(None)?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Value {
        json!({ "projectId": "project", "revision": 4, "status": "running", "updatedAt": 0,
            "session": { "id": "session", "busy": true, "blocks": [
                { "id": "user", "role": "user", "text": "Do it", "custom": true },
                { "id": "reply", "role": "assistant", "text": "Work", "streaming": true }
            ] }
        })
    }

    #[test]
    fn delta_keeps_unchanged_blocks_and_host_order() {
        let known = known();
        let sync = json!({ "kind": "delta", "base": 4,
            "value": { "revision": 6, "status": "idle", "session": { "id": "session", "busy": false } },
            "blockIds": ["user", "reply", "done"],
            "blocks": [ { "id": "reply", "role": "assistant", "text": "Work done" },
                { "id": "done", "role": "system", "text": "Finished" } ]
        });
        let next = apply_session_sync(Some(&known), &sync).unwrap();
        assert_eq!(next["revision"], 6);
        assert_eq!(next["session"]["busy"], false);
        assert_eq!(next["session"]["blocks"][0], known["session"]["blocks"][0]);
        assert_eq!(next["session"]["blocks"][1]["text"], "Work done");
        assert_eq!(next["session"]["blocks"][2]["id"], "done");
    }

    #[test]
    fn incompatible_deltas_and_unchanged_revisions_require_snapshot() {
        let known = known();
        assert_eq!(
            apply_session_sync(Some(&known), &json!({ "kind": "unchanged", "revision": 4 }))
                .unwrap(),
            known
        );
        assert!(
            apply_session_sync(Some(&known), &json!({ "kind": "unchanged", "revision": 3 }))
                .is_err()
        );
        assert!(apply_session_sync(None, &json!({ "kind": "unchanged", "revision": 4 })).is_err());
        assert!(apply_session_sync(
            Some(&known),
            &json!({ "kind": "delta", "base": 4,
            "value": { "revision": 5, "session": {} }, "blockIds": ["unknown"], "blocks": [] })
        )
        .is_err());
        assert_eq!(
            apply_session_sync(None, &json!({ "kind": "snapshot", "value": known })).unwrap(),
            known
        );
    }

    #[test]
    fn transfer_offsets_count_utf16_and_preserve_non_ascii() {
        let data = json!({ "kind": "snapshot", "value": { "text": "日本語 🖥 𝄞" } }).to_string();
        let pieces = [
            &data[..data.find('🖥').unwrap()],
            "🖥",
            &data[data.find('🖥').unwrap() + '🖥'.len_utf8()..],
        ];
        let mut index = 0;
        let mut units = 0;
        let sync = read_session_sync("session", Some(8), |method, params| {
            if method == "sessions.sync" {
                assert_eq!(params["revision"], 8);
                return Ok(json!({ "kind": "chunked", "transfer": "transfer", "length": data.encode_utf16().count() }));
            }
            assert_eq!(params["offset"], units);
            assert_eq!(params["transfer"], "transfer");
            let piece = pieces[index];
            units += piece.encode_utf16().count();
            index += 1;
            Ok(json!({ "data": piece }))
        }).unwrap();
        assert_eq!(sync["value"]["text"], "日本語 🖥 𝄞");
        assert_eq!(index, 3);
    }

    #[test]
    fn transfers_reject_empty_oversized_and_inconsistent_data() {
        for (length, data) in [
            (4_u64, ""),
            (1, "🖥"),
            (0, ""),
            (MAX_SESSION_SYNC_UNITS as u64 + 1, ""),
        ] {
            let mut called = 0;
            assert!(read_session_sync("session", None, |_, _| {
                called += 1;
                if called == 1 {
                    Ok(json!({ "kind": "chunked", "transfer": "t", "length": length }))
                } else {
                    Ok(json!({ "data": data }))
                }
            })
            .is_err());
        }
    }
}
