//! Golden round trip against a copy of a real `monocode.db`.
//!
//! Set `MONOCODE_GOLDEN_DB` to the path of a copy, for example one made with
//! `sqlite3 '<live db>' ".backup /tmp/mc/golden.db"`. The test skips when the
//! variable is unset, so CI passes without user data. It opens the copy
//! read-only.
//!
//! For every session row, each JSON column is deserialized into its Rust
//! type, serialized again, and compared with the original as
//! `serde_json::Value`, with numbers compared by value (12 equals 12.0).

use std::collections::BTreeMap;

use monocode_core::Extra;
use monocode_core::block::{Block, ModelSettings, ToolPreview};
use monocode_core::inbox::InboxAskContext;
use monocode_core::session::LinkedWorkItem;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Numbers become f64 so 12 and 12.0 compare equal.
fn normalize(value: &Value) -> Value {
    match value {
        Value::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| value.clone()),
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), normalize(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// JSON pointer to the first place two values differ.
fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for key in x.keys().chain(y.keys()) {
                let child = format!("{path}/{key}");
                match (x.get(key), y.get(key)) {
                    (Some(left), Some(right)) => {
                        if let Some(diff) = first_diff(left, right, &child) {
                            return Some(diff);
                        }
                    }
                    (left, right) => {
                        return Some(format!("{child}: original {left:?}, round trip {right:?}"));
                    }
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (left, right))| first_diff(left, right, &format!("{path}/{i}")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: original {a}, round trip {b}")),
    }
}

type Counts = BTreeMap<String, usize>;

fn note(out: &mut Counts, path: &str, extra: &Extra) {
    for key in extra.keys() {
        *out.entry(format!("{path}.{key}")).or_default() += 1;
    }
}

fn note_preview(out: &mut Counts, path: &str, preview: &ToolPreview) {
    note(out, path, &preview.extra);
    for line in preview.lines.iter().flatten() {
        note(out, &format!("{path}.lines"), &line.extra);
    }
}

/// Every field that landed in an `extra` map instead of a modeled field,
/// as `path.key`. A typo in a serde rename would show up here.
fn unmodeled(block: &Block, out: &mut Counts) {
    note(out, "block", &block.extra);
    if let Some(tool) = &block.tool {
        note(out, "tool", &tool.extra);
        if let Some(preview) = &tool.preview {
            note_preview(out, "tool.preview", preview);
        }
    }
    if let Some(run) = &block.agent_run {
        note(out, "agentRun", &run.extra);
        for step in &run.steps {
            note(out, "agentRun.steps", &step.extra);
            if let Some(preview) = &step.preview {
                note_preview(out, "agentRun.steps.preview", preview);
            }
        }
    }
    if let Some(list) = &block.task_list {
        note(out, "taskList", &list.extra);
        for item in &list.items {
            note(out, "taskList.items", &item.extra);
        }
    }
    if let Some(plan) = &block.plan {
        note(out, "plan", &plan.extra);
    }
    if let Some(proposal) = &block.orchestration {
        note(out, "orchestration", &proposal.extra);
        note(out, "orchestration.author", &proposal.author.extra);
        note(out, "orchestration.settings", &proposal.settings.extra);
        for choice in &proposal.settings.choices {
            note(out, "orchestration.settings.choices", &choice.extra);
        }
        for task in &proposal.tasks {
            note(out, "orchestration.tasks", &task.extra);
        }
    }
    for (path, extra) in [
        ("turnModel", block.turn_model.as_ref().map(|m| &m.extra)),
        ("turnMetrics", block.turn_metrics.as_ref().map(|m| &m.extra)),
        ("approval", block.approval.as_ref().map(|m| &m.extra)),
        ("handoff", block.handoff.as_ref().map(|m| &m.extra)),
        (
            "secondOpinion",
            block.second_opinion.as_ref().map(|m| &m.extra),
        ),
        ("noteCard", block.note_card.as_ref().map(|m| &m.extra)),
        (
            "interjection",
            block.interjection.as_ref().map(|m| &m.extra),
        ),
        ("image", block.image.as_ref().map(|m| &m.extra)),
    ] {
        if let Some(extra) = extra {
            note(out, path, extra);
        }
    }
    for file in block.attachments.iter().flatten() {
        note(out, "attachments", &file.extra);
    }
    for thread in block.btw_threads.iter().flatten() {
        note(out, "btwThreads", &thread.extra);
        for message in &thread.messages {
            note(out, "btwThreads.messages", &message.extra);
            for inner in message.blocks.iter().flatten() {
                unmodeled(inner, out);
            }
        }
    }
}

/// Deserialize `raw` into `T`, serialize it again, and compare.
fn check<T: DeserializeOwned + Serialize>(raw: &str) -> Result<T, String> {
    let original: Value = serde_json::from_str(raw).map_err(|e| format!("not JSON: {e}"))?;
    let typed: T = serde_json::from_str(raw).map_err(|e| format!("deserialize: {e}"))?;
    let again = serde_json::to_string(&typed).map_err(|e| format!("serialize: {e}"))?;
    let reparsed: Value = serde_json::from_str(&again).map_err(|e| e.to_string())?;
    match first_diff(&normalize(&original), &normalize(&reparsed), "") {
        Some(diff) => Err(format!("mismatch at {diff}")),
        None => Ok(typed),
    }
}

fn columns(conn: &Connection) -> Vec<String> {
    let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn every_session_round_trips() {
    let Ok(path) = std::env::var("MONOCODE_GOLDEN_DB") else {
        eprintln!("MONOCODE_GOLDEN_DB is unset; skipping the golden round trip");
        return;
    };
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|e| panic!("open {path}: {e}"));
    let present = columns(&conn);
    let optional = |name: &str| {
        if present.iter().any(|column| column == name) {
            name.to_string()
        } else {
            format!("NULL AS {name}")
        }
    };
    let sql = format!(
        "SELECT id, blocks_json, model_settings, {}, {} FROM sessions ORDER BY id",
        optional("linked_work_item_json"),
        optional("inbox_ask"),
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .unwrap();

    let mut sessions = 0;
    let mut blocks = 0;
    let mut extra_fields = Counts::new();
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let (id, blocks_json, model_settings, linked, inbox_ask) = row.unwrap();
        sessions += 1;
        let mut fail = |what: &str, error: String| {
            failures
                .entry(id.clone())
                .or_default()
                .push(format!("{what}: {error}"));
        };
        match check::<Vec<Block>>(&blocks_json) {
            Ok(parsed) => {
                blocks += parsed.len();
                for block in &parsed {
                    unmodeled(block, &mut extra_fields);
                }
            }
            Err(error) => {
                // Narrow the failure to the first block that does not round-trip.
                let values: Vec<Value> = serde_json::from_str(&blocks_json).unwrap_or_default();
                let culprit = values.iter().enumerate().find_map(|(i, block)| {
                    check::<Block>(&block.to_string())
                        .err()
                        .map(|e| format!("block {i}: {e}"))
                });
                fail("blocks_json", culprit.unwrap_or(error));
            }
        }
        if let Err(error) = check::<ModelSettings>(&model_settings) {
            fail("model_settings", error);
        }
        if let Some(raw) = linked.filter(|raw| !raw.is_empty())
            && let Err(error) = check::<LinkedWorkItem>(&raw)
        {
            fail("linked_work_item_json", error);
        }
        if let Some(raw) = inbox_ask.filter(|raw| !raw.is_empty())
            && let Err(error) = check::<InboxAskContext>(&raw)
        {
            fail("inbox_ask", error);
        }
    }

    eprintln!(
        "golden round trip: {sessions} sessions, {blocks} blocks, {} sessions with mismatches, {} distinct unmodeled fields",
        failures.len(),
        extra_fields.len()
    );
    for (field, count) in &extra_fields {
        eprintln!("  kept in extra, not modeled: {field} ({count})");
    }
    for (id, errors) in &failures {
        for error in errors {
            eprintln!("  {id}: {error}");
        }
    }
    assert!(sessions > 0, "the golden database has no sessions");
    assert!(
        failures.is_empty(),
        "{} sessions failed to round-trip",
        failures.len()
    );
}

#[test]
fn normalizes_numbers_before_comparing() {
    let a: Value = serde_json::from_str(r#"{"n":12,"m":[1.5]}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"m":[1.5],"n":12.0}"#).unwrap();
    assert_eq!(normalize(&a), normalize(&b));
    assert!(first_diff(&normalize(&a), &normalize(&b), "").is_none());
}

#[test]
fn reports_a_value_the_types_drop() {
    // An explicit null on an optional field does not survive, so the check
    // must flag it rather than pass silently.
    let error =
        check::<Block>(r#"{"id":"a","role":"user","text":"","streaming":null}"#).unwrap_err();
    assert!(error.contains("/streaming"), "{error}");
    assert!(check::<Block>(r#"{"id":"a","role":"user","text":"","later":null}"#).is_ok());
}
