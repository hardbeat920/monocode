//! Large tool output stays in SQLite until the transcript expands its row.
//!
//! Full command output and file reads are most of a transcript's bytes, and
//! the web view keeps every loaded block alive. Loading a session replaces
//! each large `detail` with a `detailOmitted` marker. Saving puts the stored
//! output back, so a client that never fetched it cannot erase it.

use std::borrow::Cow;
use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

/// Output up to this size ships with its block; a fetch would cost more.
pub(crate) const INLINE_DETAIL_BYTES: usize = 512;
const OMITTED: &str = "detailOmitted";

/// Replaces large tool and subagent-step output with a marker.
pub(crate) fn omit_large_details(blocks: &mut Value) {
    for block in blocks.as_array_mut().into_iter().flatten() {
        if let Some(tool) = block.get_mut("tool") {
            omit(tool);
        }
        for step in steps_mut(block) {
            omit(step);
        }
    }
}

fn omit(holder: &mut Value) {
    let Some(fields) = holder.as_object_mut() else {
        return;
    };
    let large = fields
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.len() > INLINE_DETAIL_BYTES);
    if large {
        fields.remove("detail");
        fields.insert(OMITTED.into(), Value::Bool(true));
    }
}

fn steps_mut(block: &mut Value) -> impl Iterator<Item = &mut Value> {
    block
        .pointer_mut("/agentRun/steps")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

fn steps(block: &Value) -> impl Iterator<Item = &Value> {
    block
        .pointer("/agentRun/steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Puts stored output back on every tool and subagent step a client saved
/// without it. That covers blocks loaded with the marker and blocks whose
/// marker was dropped when the client rebuilt them. Output that cannot be
/// found keeps its marker, so it reads as unavailable rather than empty.
pub(crate) fn restore_details<'a>(stored: &Value, blocks: &'a Value) -> Cow<'a, Value> {
    let by_id: HashMap<&str, &Value> = stored
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| has_detail(block.get("tool")) || steps(block).any(|s| has_detail(Some(s))))
        .filter_map(|block| Some((block.get("id")?.as_str()?, block)))
        .collect();
    let mut restored = Cow::Borrowed(blocks);
    let Some(incoming) = blocks.as_array() else {
        return restored;
    };
    for (index, block) in incoming.iter().enumerate() {
        let previous = block
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| by_id.get(id).copied());
        let previous_steps: HashMap<&str, &Value> = previous
            .into_iter()
            .flat_map(steps)
            .filter_map(|step| Some((step.get("id")?.as_str()?, step)))
            .collect();
        let step_previous = |step: &Value| {
            step.get("id")
                .and_then(Value::as_str)
                .and_then(|id| previous_steps.get(id).copied())
        };
        let tool_previous = previous.and_then(|block| block.get("tool"));
        let changes = block
            .get("tool")
            .is_some_and(|tool| needs_restore(tool, tool_previous))
            || steps(block).any(|step| needs_restore(step, step_previous(step)));
        if !changes {
            continue;
        }
        let block = &mut restored.to_mut()[index];
        if let Some(tool) = block.get_mut("tool") {
            restore(tool, tool_previous);
        }
        for step in steps_mut(block) {
            let previous = step_previous(step);
            restore(step, previous);
        }
    }
    restored
}

fn has_detail(holder: Option<&Value>) -> bool {
    holder.is_some_and(|value| value.get("detail").is_some())
}

fn needs_restore(holder: &Value, previous: Option<&Value>) -> bool {
    match holder.get("detail") {
        Some(_) => holder.get(OMITTED).is_some(),
        None => has_detail(previous),
    }
}

fn restore(holder: &mut Value, previous: Option<&Value>) {
    let Some(fields) = holder.as_object_mut() else {
        return;
    };
    if !fields.contains_key("detail") {
        let Some(detail) = previous.and_then(|value| value.get("detail")) else {
            return;
        };
        fields.insert("detail".into(), detail.clone());
    }
    fields.remove(OMITTED);
}

/// The stored output of one tool block, or of one step inside its subagent run.
pub(crate) fn stored_detail(
    conn: &Connection,
    session_id: &str,
    block_id: &str,
    step_id: Option<&str>,
) -> rusqlite::Result<Option<String>> {
    // Extract the one block inside SQLite instead of parsing the transcript.
    let block: Option<String> = conn
        .query_row(
            "SELECT block.value FROM sessions, json_each(sessions.blocks_json) AS block
             WHERE sessions.id = ?1 AND json_extract(block.value, '$.id') = ?2
             LIMIT 1",
            [session_id, block_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(block) = block.and_then(|json| serde_json::from_str::<Value>(&json).ok()) else {
        return Ok(None);
    };
    let holder = match step_id {
        Some(step_id) => {
            steps(&block).find(|step| step.get("id").and_then(Value::as_str) == Some(step_id))
        }
        None => block.get("tool"),
    };
    Ok(holder
        .and_then(|value| value.get("detail"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn restore_from(stored: &Value, blocks: &Value) -> Value {
        restore_details(stored, blocks).into_owned()
    }

    fn store(blocks: &Value) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, blocks_json TEXT NOT NULL)",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, blocks_json) VALUES ('s', ?1)",
            [blocks.to_string()],
        )
        .unwrap();
        conn
    }

    fn transcript() -> Value {
        let large = "x".repeat(INLINE_DETAIL_BYTES + 1);
        json!([
            { "id": "u", "role": "user", "text": "run it" },
            { "id": "t", "role": "tool", "text": "", "tool": { "kind": "execute", "detail": large } },
            { "id": "small", "role": "tool", "text": "", "tool": { "detail": "ok" } },
            { "id": "a", "role": "tool", "text": "", "tool": { "kind": "other" },
              "agentRun": { "name": "review", "steps": [
                { "id": "s1", "kind": "tool", "text": "read", "detail": large },
                { "id": "s2", "kind": "tool", "text": "ls", "detail": "a b" }
              ] } }
        ])
    }

    #[test]
    fn omits_only_large_details() {
        let mut blocks = transcript();
        omit_large_details(&mut blocks);
        assert_eq!(
            blocks[1]["tool"],
            json!({ "kind": "execute", "detailOmitted": true })
        );
        assert_eq!(blocks[2]["tool"]["detail"], "ok");
        assert_eq!(blocks[3]["agentRun"]["steps"][0]["detailOmitted"], true);
        assert!(blocks[3]["agentRun"]["steps"][0].get("detail").is_none());
        assert_eq!(blocks[3]["agentRun"]["steps"][1]["detail"], "a b");
    }

    #[test]
    fn saving_omitted_blocks_keeps_the_stored_output() {
        let full = transcript();
        let mut loaded = full.clone();
        omit_large_details(&mut loaded);
        // The client appends a turn and saves without ever expanding the output.
        loaded
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "u2", "role": "user", "text": "again" }));
        let restored = restore_from(&full, &loaded);
        assert_eq!(
            &restored.as_array().unwrap()[..4],
            &full.as_array().unwrap()[..]
        );
        assert_eq!(restored[4]["id"], "u2");
    }

    #[test]
    fn unrecoverable_output_keeps_its_marker() {
        let mut loaded = transcript();
        omit_large_details(&mut loaded);
        let restored = restore_from(&json!([]), &loaded);
        assert_eq!(restored[1]["tool"]["detailOmitted"], true);
    }

    #[test]
    fn untouched_blocks_are_borrowed() {
        let full = transcript();
        assert!(matches!(restore_details(&full, &full), Cow::Borrowed(_)));
    }

    #[test]
    fn restores_output_when_a_rebuilt_block_dropped_the_marker() {
        let full = transcript();
        let mut loaded = full.clone();
        omit_large_details(&mut loaded);
        // A late status update rebuilt the tool, and a sanitizer rebuilt the step.
        loaded[1]["tool"] = json!({ "kind": "execute", "status": "completed" });
        loaded[3]["agentRun"]["steps"][0] = json!({ "id": "s1", "kind": "tool", "text": "read" });
        let restored = restore_from(&full, &loaded);
        assert_eq!(restored[1]["tool"]["detail"], full[1]["tool"]["detail"]);
        assert_eq!(restored[1]["tool"]["status"], "completed");
        assert_eq!(
            restored[3]["agentRun"]["steps"][0]["detail"],
            full[3]["agentRun"]["steps"][0]["detail"]
        );
    }

    #[test]
    fn fetches_one_tool_or_step_detail() {
        let conn = store(&transcript());
        let large = "x".repeat(INLINE_DETAIL_BYTES + 1);
        assert_eq!(
            stored_detail(&conn, "s", "t", None).unwrap(),
            Some(large.clone())
        );
        assert_eq!(
            stored_detail(&conn, "s", "a", Some("s1")).unwrap(),
            Some(large)
        );
        assert_eq!(stored_detail(&conn, "s", "missing", None).unwrap(), None);
    }
}
