//! Antigravity's ACP server announces `invoke_subagent` with an empty input,
//! so the stream never says what each child is for. Its conversation store
//! keeps the call's JSON arguments and, once dispatched, the child
//! conversation ids in the same order. MonoCode only opens that store
//! read-only, and an unreadable store just leaves rows unnamed.

use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::dirs_home;
use crate::sqlite_readonly;

/// Start of the JSON arguments in an `invoke_subagent` step.
const ARGS: &[u8] = b"{\"Subagents\":[";

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySubagent {
    conversation_id: String,
    role: String,
    type_name: Option<String>,
}

#[tauri::command]
pub async fn antigravity_subagents(session_id: String) -> Result<Vec<AntigravitySubagent>, String> {
    if !is_uuid(session_id.as_bytes()) {
        return Err("Invalid session id".into());
    }
    let home = dirs_home().ok_or("Home directory is unavailable")?;
    let path = PathBuf::from(home)
        .join(".gemini/antigravity-acp/conversations")
        .join(format!("{session_id}.db"));
    tauri::async_runtime::spawn_blocking(move || {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let connection = sqlite_readonly::open(&path)?;
        connection
            .busy_timeout(std::time::Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        read_subagents(&connection, &session_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn read_subagents(
    connection: &Connection,
    session_id: &str,
) -> rusqlite::Result<Vec<AntigravitySubagent>> {
    // Only invoke_subagent steps carry the args; let SQLite skip the rest.
    // instr() does a byte search when both operands are blobs.
    let mut statement = connection
        .prepare("SELECT step_payload FROM steps WHERE instr(step_payload, ?1) > 0 ORDER BY idx")?;
    let mut rows = statement.query([ARGS])?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let payload: Vec<u8> = row.get(0)?;
        found.extend(subagents_in_payload(&payload, session_id));
    }
    Ok(found)
}

fn subagents_in_payload(payload: &[u8], session_id: &str) -> Vec<AntigravitySubagent> {
    let Some(start) = payload.windows(ARGS.len()).position(|w| w == ARGS) else {
        return Vec::new();
    };
    let Some(len) = json_object_len(&payload[start..]) else {
        return Vec::new();
    };
    let Ok(args) = serde_json::from_slice::<Value>(&payload[start..start + len]) else {
        return Vec::new();
    };
    let Some(specs) = args.get("Subagents").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = Vec::new();
    for window in payload[start + len..].windows(36) {
        if !is_uuid(window) {
            continue;
        }
        let id = String::from_utf8_lossy(window).into_owned();
        if id != session_id && !ids.contains(&id) {
            ids.push(id);
        }
    }
    // Children not dispatched yet have no id; a partial list would misname them.
    if ids.len() != specs.len() {
        return Vec::new();
    }
    ids.into_iter()
        .zip(specs)
        .filter_map(|(conversation_id, spec)| {
            let role = spec.get("Role")?.as_str()?.trim();
            (!role.is_empty()).then(|| AntigravitySubagent {
                conversation_id,
                role: role.to_string(),
                type_name: spec
                    .get("TypeName")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Length of the JSON object at the start of `bytes`, string-aware.
fn json_object_len(bytes: &[u8]) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (index, &byte) in bytes.iter().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_uuid(bytes: &[u8]) -> bool {
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, &byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: &str = "639c8fd4-acaa-451e-acbf-db8dffbce7cb";
    const FIRST: &str = "c8d11a3c-960b-4b7e-b7bf-1684938d2f31";
    const SECOND: &str = "25764917-cb43-47fd-92ef-54f341a67399";

    fn payload(ids: &[&str]) -> Vec<u8> {
        // Shaped like the stored step: protobuf framing around the call's JSON
        // arguments, then each dispatched child's id (twice) with its paths.
        let mut bytes = b"\x0a\x0ccall_1242438\x12\x0finvoke_subagent\x1a".to_vec();
        bytes.extend_from_slice(
            br#"{"Subagents":[{"Model":"inherit","Prompt":"Read {src} \"app\"","Role":"Frontend App Researcher","TypeName":"research"},{"Prompt":"Read tests","Role":"Test Suite Researcher"}]}"#,
        );
        bytes.extend_from_slice(format!("\x00{PARENT}\x01{PARENT}").as_bytes());
        for id in ids {
            bytes.extend_from_slice(b"R\x91\x04\n$");
            bytes.extend_from_slice(format!("{id}\x12file:///brain/{id}/x").as_bytes());
        }
        bytes
    }

    #[test]
    fn pairs_roles_with_dispatched_children_in_order() {
        assert_eq!(
            subagents_in_payload(&payload(&[FIRST, SECOND]), PARENT),
            vec![
                AntigravitySubagent {
                    conversation_id: FIRST.into(),
                    role: "Frontend App Researcher".into(),
                    type_name: Some("research".into()),
                },
                AntigravitySubagent {
                    conversation_id: SECOND.into(),
                    role: "Test Suite Researcher".into(),
                    type_name: None,
                },
            ]
        );
    }

    #[test]
    fn names_nothing_until_every_child_is_dispatched() {
        assert!(subagents_in_payload(&payload(&[FIRST]), PARENT).is_empty());
        assert!(subagents_in_payload(b"\x0a\x04view_file", PARENT).is_empty());
    }

    #[test]
    fn reads_only_steps_carrying_subagent_args() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_payload BLOB)")
            .unwrap();
        let rows: [Option<Vec<u8>>; 4] = [
            Some(b"\x0a\x04view_file".to_vec()),
            None,
            Some(payload(&[FIRST, SECOND])),
            Some(b"\x0a\x0dSubagents only".to_vec()),
        ];
        for (idx, step) in rows.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO steps VALUES (?1, ?2)",
                    rusqlite::params![idx as i64, step],
                )
                .unwrap();
        }
        let matched: i64 = connection
            .query_row(
                "SELECT count(*) FROM steps WHERE instr(step_payload, ?1) > 0",
                [ARGS],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(matched, 1);
        let found = read_subagents(&connection, PARENT).unwrap();
        let ids: Vec<_> = found.iter().map(|s| s.conversation_id.as_str()).collect();
        assert_eq!(ids, [FIRST, SECOND]);
    }

    #[test]
    fn accepts_only_lowercase_uuids() {
        assert!(is_uuid(PARENT.as_bytes()));
        assert!(!is_uuid(b"../../etc/passwd-aaaa-bbbb-cccccccccc"));
        assert!(!is_uuid(PARENT.to_uppercase().as_bytes()));
    }
}
