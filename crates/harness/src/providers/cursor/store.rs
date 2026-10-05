//! Port of src/integrations/harness/providers/cursor/cursorStore.ts.
//!
//! Cursor's ACP events leave out tool arguments and subagent activity that
//! Cursor writes to its own SQLite stores. The TypeScript read them through
//! the `cursor_tool_calls` and `cursor_subagent_runs` Tauri commands. The
//! harness must not depend on `monocode-store`, so the engine supplies a
//! [`CursorStore`] over `monocode_store::cursor_store`. Those functions block
//! on SQLite, so an implementation runs them off the UI thread and resolves
//! the future when they finish. The store's structs serialize to the JSON
//! shapes below, so `serde_json::to_value` then `from_value` converts them.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use monocode_core::block::AgentStepKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::task::BoxFuture;

/// `StoredCursorToolCall`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCursorToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub args: Value,
}

/// One step of `StoredCursorSubagentRun.steps`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCursorSubagentStep {
    pub id: String,
    pub kind: AgentStepKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// `StoredCursorSubagentRun`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCursorSubagentRun {
    pub tool_call_id: String,
    pub agent_id: String,
    pub revision: String,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    pub steps: Vec<StoredCursorSubagentStep>,
}

/// Read access to Cursor's local session stores.
pub trait CursorStore: Send + Sync {
    /// `cursor_subagent_runs`: the children a parent session spawned through
    /// these tool calls. A run whose revision matches `known_revisions` may be
    /// left out.
    fn subagent_runs(
        &self,
        session_id: String,
        tool_call_ids: Vec<String>,
        known_revisions: HashMap<String, String>,
    ) -> BoxFuture<'static, Result<Vec<StoredCursorSubagentRun>>>;

    /// `cursor_tool_calls`: the stored arguments of these tool calls.
    fn tool_calls(
        &self,
        session_id: String,
        tool_call_ids: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<StoredCursorToolCall>>>;
}

/// A store with nothing in it, for hosts without Cursor's files.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCursorStore;

impl CursorStore for NoCursorStore {
    fn subagent_runs(
        &self,
        _session_id: String,
        _tool_call_ids: Vec<String>,
        _known_revisions: HashMap<String, String>,
    ) -> BoxFuture<'static, Result<Vec<StoredCursorSubagentRun>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn tool_calls(
        &self,
        _session_id: String,
        _tool_call_ids: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<StoredCursorToolCall>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// The two `readStored*` functions. A headless child backend runs Cursor on
/// another machine, so its stores are not here and both reads come back empty.
#[derive(Clone)]
pub struct StoreReader {
    store: Arc<dyn CursorStore>,
    headless: bool,
}

impl StoreReader {
    pub fn new(store: Arc<dyn CursorStore>, headless: bool) -> Self {
        Self { store, headless }
    }

    /// `readStoredCursorSubagentRuns`.
    pub async fn read_stored_cursor_subagent_runs(
        &self,
        session_id: &str,
        tool_call_ids: Vec<String>,
        known_revisions: HashMap<String, String>,
    ) -> Result<Vec<StoredCursorSubagentRun>> {
        if self.headless {
            return Ok(Vec::new());
        }
        self.store
            .subagent_runs(session_id.to_string(), tool_call_ids, known_revisions)
            .await
    }

    /// `readStoredCursorToolCalls`.
    pub async fn read_stored_cursor_tool_calls(
        &self,
        session_id: &str,
        tool_call_ids: Vec<String>,
    ) -> Result<Vec<StoredCursorToolCall>> {
        if self.headless {
            return Ok(Vec::new());
        }
        self.store
            .tool_calls(session_id.to_string(), tool_call_ids)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_store_json_shapes() {
        let run: StoredCursorSubagentRun = serde_json::from_value(json!({
            "toolCallId": "spawn", "agentId": "child", "revision": "7",
            "agentType": null, "model": "gpt", "prompt": null,
            "steps": [
                { "id": "s1", "kind": "message", "text": "hi" },
                { "id": "s2", "kind": "tool", "text": "", "toolName": "Shell",
                  "args": { "command": "ls" }, "status": "completed", "output": "a" }
            ]
        }))
        .unwrap();
        assert_eq!(run.steps[1].kind, AgentStepKind::Tool);
        assert_eq!(run.agent_type, None);
        let call: StoredCursorToolCall = serde_json::from_value(
            json!({ "toolCallId": "c", "toolName": "Read", "args": { "path": "a" } }),
        )
        .unwrap();
        assert_eq!(call.args, json!({ "path": "a" }));
    }

    #[test]
    fn a_headless_reader_never_asks_the_store() {
        struct Panics;
        impl CursorStore for Panics {
            fn subagent_runs(
                &self,
                _: String,
                _: Vec<String>,
                _: HashMap<String, String>,
            ) -> BoxFuture<'static, Result<Vec<StoredCursorSubagentRun>>> {
                panic!("headless reads must not reach the store")
            }
            fn tool_calls(
                &self,
                _: String,
                _: Vec<String>,
            ) -> BoxFuture<'static, Result<Vec<StoredCursorToolCall>>> {
                panic!("headless reads must not reach the store")
            }
        }
        let reader = StoreReader::new(Arc::new(Panics), true);
        smol::block_on(async {
            assert!(
                reader
                    .read_stored_cursor_tool_calls("s", vec!["c".into()])
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                reader
                    .read_stored_cursor_subagent_runs("s", vec!["c".into()], HashMap::new())
                    .await
                    .unwrap()
                    .is_empty()
            );
        });
    }
}
