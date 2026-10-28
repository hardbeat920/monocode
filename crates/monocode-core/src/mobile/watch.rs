//! Watch frames for a phone following a conversation: every turn's ID in
//! order, plus only the turns that changed since the phone's last frame.
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// What the phone last received for a watch, to send only what changed since.
#[derive(Clone, Debug, Default)]
pub struct Baseline {
    busy: bool,
    older: bool,
    order: Vec<String>,
    turns: HashMap<String, String>,
}

/// Paths use JSON array indexes; append offsets count UTF-16 code units.
fn edits_between(before: &Value, after: &Value, path: &mut Vec<Value>, out: &mut Vec<Value>) {
    if before == after {
        return;
    }
    match (before, after) {
        (Value::String(old), Value::String(new)) if new.starts_with(old.as_str()) => {
            out.push(json!({
                "path": path.clone(),
                "from": old.encode_utf16().count(),
                "append": &new[old.len()..],
            }));
        }
        (Value::Array(old), Value::Array(new)) => {
            for (i, value) in new.iter().enumerate() {
                path.push(json!(i));
                edits_between(old.get(i).unwrap_or(&Value::Null), value, path, out);
                path.pop();
            }
            for i in (new.len()..old.len()).rev() {
                path.push(json!(i));
                out.push(json!({"path": path.clone(), "remove": true}));
                path.pop();
            }
        }
        (Value::Object(old), Value::Object(new)) => {
            for key in old.keys().filter(|key| !new.contains_key(*key)) {
                path.push(json!(key));
                out.push(json!({"path": path.clone(), "remove": true}));
                path.pop();
            }
            for (key, value) in new {
                path.push(json!(key));
                edits_between(old.get(key).unwrap_or(&Value::Null), value, path, out);
                path.pop();
            }
        }
        _ => out.push(json!({"path": path.clone(), "value": after})),
    }
}

/// The next frame for a watched conversation. While a reply streams that is
/// the one live turn, not the whole transcript. Capable phones opt into field
/// edits and text suffixes instead of a growing turn. None when nothing changed.
pub fn update(
    baseline: Option<&Baseline>,
    busy: bool,
    older: bool,
    turns: &[Value],
    incremental: bool,
) -> Option<(Value, Baseline)> {
    let order: Vec<String> = turns
        .iter()
        .map(|turn| turn["id"].as_str().unwrap_or_default().to_string())
        .collect();
    let serialized: HashMap<String, String> = order
        .iter()
        .zip(turns)
        .map(|(id, turn)| (id.clone(), turn.to_string()))
        .collect();
    let changed: Vec<(&String, &Value)> = order
        .iter()
        .zip(turns)
        .filter(|(id, _)| baseline.and_then(|b| b.turns.get(*id)) != serialized.get(*id))
        .collect();
    if let Some(baseline) = baseline {
        if baseline.busy == busy
            && baseline.older == older
            && changed.is_empty()
            && baseline.order == order
        {
            return None;
        }
    }
    let mut patches = Vec::new();
    let mut replaced = Vec::new();
    for (id, turn) in changed {
        let previous = incremental
            .then(|| baseline.and_then(|b| b.turns.get(id)))
            .flatten()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        let Some(previous) = previous else {
            replaced.push(turn.clone());
            continue;
        };
        let mut edits = Vec::new();
        edits_between(&previous, turn, &mut Vec::new(), &mut edits);
        let patch = json!({"id": id, "edits": edits});
        // Small replacements are cheaper as a snapshot; long text sends its suffix.
        if patch.to_string().len() >= serialized[id].len() {
            replaced.push(turn.clone());
        } else {
            patches.push(patch);
        }
    }
    let mut frame = Map::new();
    frame.insert("busy".into(), json!(busy));
    frame.insert("older".into(), json!(older));
    frame.insert("order".into(), json!(order));
    frame.insert("turns".into(), Value::Array(replaced));
    if !patches.is_empty() {
        frame.insert("patches".into(), Value::Array(patches));
    }
    Some((
        Value::Object(frame),
        Baseline {
            busy,
            older,
            order,
            turns: serialized,
        },
    ))
}

/// Where a watch's turns start: at turn `from` while it is still there, so the
/// window only grows as the conversation does; otherwise `tail` from the end.
/// Returns the start, the new `from`, and whether there is older history.
pub fn window(
    turn_ids: &[String],
    from: Option<&str>,
    tail: usize,
) -> (usize, Option<String>, bool) {
    let start = from
        .and_then(|from| turn_ids.iter().position(|id| id == from))
        .unwrap_or_else(|| turn_ids.len().saturating_sub(tail));
    (start, turn_ids.get(start).cloned(), start > 0)
}

/// Up to `limit` turns just before turn `before`, for paging in history.
pub fn before(
    turn_ids: &[String],
    before: &str,
    limit: usize,
) -> Result<(usize, usize, bool), String> {
    let end = turn_ids
        .iter()
        .position(|id| id == before)
        .ok_or("This part of the conversation is no longer available")?;
    let start = end.saturating_sub(limit);
    Ok((start, end, start > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(id: &str, text: &str) -> Value {
        json!({"id": id, "items": [{"type": "text", "id": "a", "text": text}]})
    }

    #[test]
    fn unchanged_conversations_send_nothing_and_new_turns_send_only_themselves() {
        let (first, baseline) = update(None, true, false, &[turn("1", "hi")], false).unwrap();
        assert_eq!(first["turns"].as_array().unwrap().len(), 1);
        assert!(update(Some(&baseline), true, false, &[turn("1", "hi")], false).is_none());
        let (next, _) = update(
            Some(&baseline),
            false,
            false,
            &[turn("1", "hi"), turn("2", "yo")],
            false,
        )
        .unwrap();
        assert_eq!(next["order"], json!(["1", "2"]));
        assert_eq!(next["turns"][0]["id"], "2");
        assert_eq!(next["busy"], false);
    }

    #[test]
    fn incremental_watches_receive_streamed_text_as_utf16_suffixes() {
        let long = "é".repeat(200);
        let (_, baseline) = update(None, true, false, &[turn("1", &long)], true).unwrap();
        let grown = format!("{long} more");
        let (frame, _) = update(Some(&baseline), true, false, &[turn("1", &grown)], true).unwrap();
        assert!(frame["turns"].as_array().unwrap().is_empty());
        let edit = &frame["patches"][0]["edits"][0];
        assert_eq!(edit["path"], json!(["items", 0, "text"]));
        assert_eq!(edit["from"], 200);
        assert_eq!(edit["append"], " more");
    }

    #[test]
    fn windows_grow_from_their_first_turn_and_pages_end_before_the_named_turn() {
        let ids: Vec<String> = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(window(&ids, None, 3), (7, Some("7".into()), true));
        assert_eq!(window(&ids, Some("2"), 3), (2, Some("2".into()), true));
        assert_eq!(window(&ids, Some("gone"), 20), (0, Some("0".into()), false));
        assert_eq!(before(&ids, "5", 3).unwrap(), (2, 5, true));
        assert!(before(&ids, "missing", 3).is_err());
    }
}
