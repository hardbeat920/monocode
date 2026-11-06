//! What a lead keeps of the subagents it drives: the list behind its sidebar
//! progress, the strip above its composer and the phone's lead link. It is
//! saved under `orchestration:{lead}`, so existing lists keep working.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub tasks: Vec<SummaryTask>,
    /// The lead is working, or still waiting on a subagent.
    pub live: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryTask {
    pub session: Option<i64>,
    pub title: String,
    pub provider: String,
    pub model: Option<String>,
    /// queued, running, completed, failed, blocked, stopped or interrupted.
    pub status: String,
}
impl SummaryTask {
    /// The status as shown, given whether the subagent needs input.
    pub fn label(&self, summary: &Summary, needs_input: bool) -> &'static str {
        if needs_input {
            return "Needs input";
        }
        if !summary.live && matches!(self.status.as_str(), "running" | "queued") {
            return "Saved";
        }
        match self.status.as_str() {
            "queued" => "Queued",
            "running" => "Working",
            "completed" => "Done",
            "saved" => "Saved",
            "failed" => "Failed",
            "blocked" => "Needs review",
            "stopped" => "Stopped",
            "interrupted" => "Interrupted",
            _ => "Queued",
        }
    }
}
