//! Turns that were running when MonoCode last stopped, so a relaunch can
//! settle their rows and continue them under the same policy.
use super::*;
use crate::agent::AccessMode;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "in-flight:";

/// The policy a running turn was launched with. Composer modes reset after
/// Send, so a continuation must not read them from the conversation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InFlight {
    pub cwd: String,
    pub started_at: u64,
    pub plan: bool,
    pub orchestrator: bool,
    pub operator: bool,
    pub access: AccessMode,
    /// False for automation runs, orchestration workers and Mono turns: their
    /// owners have their own recovery, so a relaunch only settles the rows.
    pub resume: bool,
}

impl Store {
    pub fn mark_in_flight(&self, id: i64, turn: &InFlight) -> Result<(), String> {
        let value = serde_json::to_string(turn).map_err(|e| e.to_string())?;
        self.save_setting(&format!("{PREFIX}{id}"), &value)
    }
    pub fn clear_in_flight(&self, id: i64) -> Result<(), String> {
        let key = format!("{PREFIX}{id}");
        if self.setting(&key)?.is_none_or(|value| value.is_empty()) {
            return Ok(());
        }
        self.save_setting(&key, "")
    }
    /// Oldest first, so restored chats continue in the order they started.
    pub fn in_flight(&self) -> Result<Vec<(i64, InFlight)>, String> {
        let mut turns: Vec<_> = self
            .settings_with_prefix(PREFIX)?
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .filter_map(|(id, value)| Some((id, serde_json::from_str(&value).ok()?)))
            .collect();
        turns.sort_by_key(|(id, turn): &(i64, InFlight)| (turn.started_at, *id));
        Ok(turns)
    }
    /// Settle the tools a stopped process left running and note the
    /// interruption once, so a reload never shows a spinner for dead work.
    pub fn seal_interrupted_turn(&mut self, id: i64) -> Result<Interrupted, String> {
        let mut transcript = self.load(id)?;
        let Some(prompt) = transcript
            .rows
            .iter()
            .rposition(|row| row.is_prompt() && row.first)
        else {
            return Ok(Interrupted::default());
        };
        let mut start = transcript.rows.len();
        let mut tool_activity = false;
        for (ix, row) in transcript.rows.iter_mut().enumerate().skip(prompt) {
            if row.role != "tool" {
                continue;
            }
            tool_activity = true;
            let Ok(mut tool) = serde_json::from_str::<crate::tool::ToolCall>(&row.text) else {
                continue;
            };
            if tool.state == crate::tool::ToolState::Running {
                tool.state = crate::tool::ToolState::Cancelled;
                if tool.output.is_empty() {
                    tool.output = INTERRUPTED_BY_QUIT.into();
                }
                row.text = serde_json::to_string(&tool).map_err(|e| e.to_string())?;
                start = start.min(ix);
            }
        }
        let text: String = transcript.rows[prompt..]
            .iter()
            .enumerate()
            .take_while(|(ix, row)| *ix == 0 || !row.first)
            .map(|(_, row)| row.text.as_str())
            .collect();
        let attachments = transcript.rows[prompt + 1..]
            .iter()
            .find(|row| row.first)
            .filter(|row| row.role == crate::transcript::attachments::ROLE)
            .and_then(|row| crate::transcript::attachments::paths(row).ok())
            .unwrap_or_default();
        let noted = transcript.rows.last().is_some_and(|row| {
            row.role == "interjection"
                && serde_json::from_str::<crate::transcript::Interjection>(&row.text)
                    .is_ok_and(|note| note.text == TURN_INTERRUPTED)
        });
        if !noted {
            let note = crate::transcript::Interjection {
                text: TURN_INTERRUPTED.into(),
                custom_type: "custom".into(),
                severity: Some("warning".into()),
            };
            let at = transcript.begin("interjection");
            transcript.append(&serde_json::to_string(&note).map_err(|e| e.to_string())?);
            transcript.rows[at].sent_at = Some(crate::time::now_ms());
            start = start.min(at);
        }
        if start < transcript.rows.len() {
            self.write_rows(id, start, &transcript.rows[start..])?;
        }
        Ok(Interrupted {
            prompt: Some(text).filter(|text| !text.trim().is_empty()),
            attachments,
            tool_activity,
        })
    }
}

use crate::process::INTERRUPTED_BY_QUIT;
pub const TURN_INTERRUPTED: &str = "Turn interrupted when MonoCode quit.";

/// What an interrupted turn had done, to decide how it may continue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interrupted {
    /// The request that opened the turn.
    pub prompt: Option<String>,
    pub attachments: Vec<std::path::PathBuf>,
    /// Re-sending a request that already ran tools could repeat side effects.
    pub tool_activity: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_survives_restart_until_cleared() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("native.db");
        let store = Store::open(&path).unwrap();
        let first = store.create("codex", "/project").unwrap();
        let second = store.create("claude", "/project").unwrap();
        let plan = InFlight {
            cwd: "/project".into(),
            started_at: 20,
            plan: true,
            access: AccessMode::ReadOnly,
            resume: true,
            ..Default::default()
        };
        store.mark_in_flight(second.id, &plan).unwrap();
        store
            .mark_in_flight(
                first.id,
                &InFlight {
                    started_at: 10,
                    ..plan.clone()
                },
            )
            .unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let turns = store.in_flight().unwrap();
        assert_eq!(
            turns.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![first.id, second.id]
        );
        assert_eq!(turns[1].1, plan);
        store.clear_in_flight(first.id).unwrap();
        store.clear_in_flight(first.id).unwrap();
        assert_eq!(store.in_flight().unwrap().len(), 1);
    }

    #[test]
    fn sealing_settles_running_tools_and_notes_the_interruption_once() {
        use crate::tool::{ToolCall, ToolState};
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let mut transcript = Transcript::default();
        transcript.begin("user");
        transcript.append("Earlier");
        transcript.begin("assistant");
        transcript.append("Done");
        transcript.begin("user");
        transcript.append(&"Fix the build. ".repeat(200));
        transcript.begin("assistant");
        transcript.append("Looking");
        let tool = transcript.begin("tool");
        let mut call = ToolCall::new("1", "Bash");
        call.state = ToolState::Running;
        transcript.rows[tool].text = serde_json::to_string(&call).unwrap();
        store.write_rows(session.id, 0, &transcript.rows).unwrap();

        let interrupted = store.seal_interrupted_turn(session.id).unwrap();
        assert_eq!(
            interrupted.prompt.as_deref(),
            Some("Fix the build. ".repeat(200).as_str())
        );
        assert!(interrupted.tool_activity);
        let rows = store.load(session.id).unwrap().rows;
        let call: ToolCall = serde_json::from_str(&rows[tool].text).unwrap();
        assert_eq!(call.state, ToolState::Cancelled);
        assert_eq!(call.output, INTERRUPTED_BY_QUIT);
        assert_eq!(rows.last().unwrap().role, "interjection");

        store.seal_interrupted_turn(session.id).unwrap();
        assert_eq!(store.load(session.id).unwrap().rows, rows);
    }
}
