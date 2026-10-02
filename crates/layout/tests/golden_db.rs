//! Golden round trip of the stored workspace snapshot against a copy of a
//! real `monocode.db`.
//!
//! Set `MONOCODE_GOLDEN_DB` to the path of a copy, for example one made with
//! `sqlite3 '<live db>' ".backup /tmp/mc/golden.db"`. The test skips when the
//! variable is unset, so CI passes without user data. It opens the copy
//! read-only.
//!
//! For every `workspace_snapshot` row, `snapshot_json` is deserialized into
//! `WorkspaceSnapshot`, serialized again, and compared with the original as
//! `serde_json::Value`, with numbers compared by value (1 equals 1.0). The
//! test also runs the TypeScript sanitizer port over the stored value, which
//! must keep every tab, and hydrates it.

use std::collections::HashMap;

use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs};
use monocode_core::project_providers::ProjectProviders;
use monocode_layout::workspace_snapshot::{
    WorkspaceSnapshot, hydrate_workspace_snapshot_value, parse_workspace_snapshot, unknown_fields,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// Numbers become f64 so 1 and 1.0 compare equal.
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
                .find_map(|(index, (left, right))| {
                    first_diff(left, right, &format!("{path}/{index}"))
                })
        }
        _ => (a != b).then(|| format!("{path}: original {a}, round trip {b}")),
    }
}

#[test]
fn every_workspace_snapshot_round_trips() {
    let Ok(path) = std::env::var("MONOCODE_GOLDEN_DB") else {
        eprintln!("MONOCODE_GOLDEN_DB is unset; skipping the golden snapshot round trip");
        return;
    };
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap_or_else(|e| panic!("open {path}: {e}"));
    let mut stmt = conn
        .prepare("SELECT id, snapshot_json FROM workspace_snapshot ORDER BY id")
        .unwrap();
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let catalog = ModelCatalog::new();
    let prefs = ModelPrefs::default();
    let availability = HarnessAvailability::default();
    let projects = ProjectProviders::default();
    let env = ModelEnv {
        catalog: &catalog,
        prefs: &prefs,
        availability: &availability,
        projects: &projects,
    };

    let mut failures = Vec::new();
    let mut tabs = 0;
    let mut panes = 0;
    let mut files = 0;
    let mut stubs = 0;
    let mut docks = 0;
    let mut unknown = Vec::new();
    for (id, json) in &rows {
        let original: Value =
            serde_json::from_str(json).unwrap_or_else(|e| panic!("row {id}: invalid JSON: {e}"));
        let snapshot: WorkspaceSnapshot = match serde_json::from_value(original.clone()) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                failures.push(format!("row {id}: deserialize: {e}"));
                continue;
            }
        };
        tabs += snapshot.tabs.len();
        for tab in &snapshot.tabs {
            panes += tab.editor_panes.len() + tab.terminal_panes.len();
            files += tab
                .editor_panes
                .iter()
                .chain(&tab.terminal_panes)
                .map(|pane| pane.files.len())
                .sum::<usize>();
        }
        stubs += snapshot.sessions.len();
        docks += snapshot.project_terminals.len();
        unknown.extend(unknown_fields(&snapshot));

        let round_trip = serde_json::to_value(&snapshot).unwrap();
        if let Some(diff) = first_diff(&normalize(&original), &normalize(&round_trip), "") {
            failures.push(format!("row {id}: serde round trip differs at {diff}"));
        }

        // The TypeScript stored sanitized output, so sanitizing again keeps
        // every tab and session stub.
        match parse_workspace_snapshot(&original) {
            None => failures.push(format!("row {id}: parse_workspace_snapshot returned None")),
            Some(parsed) => {
                if parsed.tabs.len() != snapshot.tabs.len() {
                    failures.push(format!(
                        "row {id}: sanitizing kept {} of {} tabs",
                        parsed.tabs.len(),
                        snapshot.tabs.len()
                    ));
                }
                if parsed.sessions.len() != snapshot.sessions.len() {
                    failures.push(format!(
                        "row {id}: sanitizing kept {} of {} session stubs",
                        parsed.sessions.len(),
                        snapshot.sessions.len()
                    ));
                }
                let sanitized = serde_json::to_value(&parsed).unwrap();
                if let Some(diff) = first_diff(&normalize(&original), &normalize(&sanitized), "") {
                    eprintln!("row {id}: sanitized snapshot differs from the stored one at {diff}");
                }
            }
        }

        let resumed =
            hydrate_workspace_snapshot_value(&original, &HashMap::new(), &[], &env, |session| {
                session
            });
        match resumed {
            None => failures.push(format!("row {id}: hydrate returned None")),
            Some(resumed) => assert_eq!(
                resumed.tabs.len(),
                snapshot.tabs.len(),
                "row {id}: hydrated tabs"
            ),
        }
    }

    unknown.sort();
    unknown.dedup();
    eprintln!(
        "{} snapshot rows: {tabs} tabs, {panes} panes, {files} files, {stubs} session stubs, {docks} docks; \
         {} failures; fields kept in extra: {unknown:?}",
        rows.len(),
        failures.len(),
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
