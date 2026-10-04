//! Strict M0 fixture for the native persistence JSON encoding.
//! Reads an owned database copy and compares its original JSON text directly.
//! The output codec matches native persistence, typed value to JSON Value to text.

use std::collections::BTreeMap;

use monocode_core::block::{Block, ModelSettings, TurnMetrics};
use monocode_core::inbox::InboxAskContext;
use monocode_core::session::LinkedWorkItem;
use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};

fn check<T: DeserializeOwned + Serialize>(raw: &str) -> Result<(), String> {
    let typed: T = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    let value = serde_json::to_value(&typed).map_err(|error| error.to_string())?;
    let output = serde_json::to_string(&value).map_err(|error| error.to_string())?;
    if raw.as_bytes() == output.as_bytes() {
        return Ok(());
    }
    let offset = raw
        .bytes()
        .zip(output.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| raw.len().min(output.len()));
    Err(format!(
        "first difference at byte {offset}; original {} bytes, output {} bytes",
        raw.len(),
        output.len()
    ))
}

#[test]
fn persistence_codec_preserves_a_sorted_block_without_rewriting_the_input() {
    check::<Vec<Block>>(r#"[{"durationMs":7,"id":"sample","role":"user","text":"hello"}]"#)
        .unwrap();
}

#[test]
fn strict_comparison_rejects_whitespace_key_order_and_number_spelling_changes() {
    assert!(check::<ModelSettings>(r#"{ "effort":"high" }"#).is_err());
    assert!(check::<ModelSettings>(r#"{"fast":"true","effort":"high"}"#).is_err());
    assert!(check::<TurnMetrics>(r#"{"cacheHitPercent":12}"#).is_err());
    check::<TurnMetrics>(r#"{"cacheHitPercent":12.5}"#).unwrap();
}

#[test]
#[ignore = "requires an owned database copy in MONOCODE_GOLDEN_DB"]
fn every_session_json_column_is_byte_equivalent() {
    let path = std::env::var("MONOCODE_GOLDEN_DB")
        .expect("MONOCODE_GOLDEN_DB must name an owned database copy");
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut columns = conn.prepare("PRAGMA table_info(sessions)").unwrap();
    let columns = columns
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let optional = |column: &str| {
        if columns.iter().any(|name| name == column) {
            column.to_owned()
        } else {
            format!("NULL AS {column}")
        }
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT blocks_json, model_settings, {}, {} FROM sessions ORDER BY id",
            optional("linked_work_item_json"),
            optional("inbox_ask"),
        ))
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .unwrap();
    let mut sessions = 0;
    let mut populated = BTreeMap::<&str, usize>::new();
    let mut failures = BTreeMap::<&str, usize>::new();
    for row in rows {
        let (blocks, settings, linked, inbox) = row.unwrap();
        sessions += 1;
        let mut note = |column, result: Result<(), String>| {
            *populated.entry(column).or_default() += 1;
            if let Err(error) = result {
                let count = failures.entry(column).or_default();
                *count += 1;
                if *count == 1 {
                    eprintln!("{column}: {error}");
                }
            }
        };
        note("blocks_json", check::<Vec<Block>>(&blocks));
        note("model_settings", check::<ModelSettings>(&settings));
        if let Some(raw) = linked.filter(|raw| !raw.is_empty()) {
            note("linked_work_item_json", check::<LinkedWorkItem>(&raw));
        }
        if let Some(raw) = inbox.filter(|raw| !raw.is_empty()) {
            note("inbox_ask", check::<InboxAskContext>(&raw));
        }
    }
    eprintln!(
        "strict JSON bytes: {sessions} sessions; columns {populated:?}; failures {failures:?}"
    );
    assert!(sessions > 0, "the copy has no sessions");
    assert!(failures.is_empty(), "stored JSON bytes changed");
}
