//! Provider-neutral tool presentation, persisted as a native `tool` row.
use serde::{Deserialize, Serialize};
mod presentation;
pub mod tasks;
pub use presentation::{visual_tool, CommandStep, ToolPresentation, VisualStep};

/// The app's own step while a conversation carries over from another model.
pub const HANDOFF: &str = "providerHandoff";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: String,
    pub output: String,
    pub state: ToolState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<tasks::Panel>,
    /// Descriptive provider/desktop metadata, separate from actual arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<serde_json::Value>,
}
impl ToolCall {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input: String::new(),
            output: String::new(),
            state: ToolState::Running,
            panel: None,
            display: None,
        }
    }
    pub fn is_compaction(&self) -> bool {
        self.name == "contextCompaction"
    }
    pub fn is_handoff(&self) -> bool {
        self.name == HANDOFF
    }
    /// Session milestones read as a status line rather than a tool step.
    pub fn is_milestone(&self) -> bool {
        self.is_compaction() || self.is_handoff()
    }
    pub fn append_output(&mut self, delta: &str) {
        self.output.push_str(delta);
        self.bound_output();
    }
    pub fn bound_output(&mut self) {
        // Keep a useful tail without retaining unbounded command output in the UI.
        const LIMIT: usize = 64 * 1024;
        if self.output.len() > LIMIT {
            let mut at = self.output.len() - LIMIT;
            while !self.output.is_char_boundary(at) {
                at += 1;
            }
            self.output = format!("… earlier output omitted …\n{}", &self.output[at..]);
        }
    }
    /// One of Monocode's own visual tools, under any provider's naming.
    pub fn visual_step(&self) -> Option<VisualStep> {
        visual_tool(&self.name)
    }
    pub fn summary(&self) -> (String, String) {
        match self.panel() {
            Some(tasks::Panel::Tasks(list)) => return ("Tasks".into(), list.progress()),
            Some(tasks::Panel::Agent(run)) => return ("Agent".into(), run.title().into()),
            None => {}
        }
        let shown = self.presentation();
        (shown.action, shown.target)
    }
}

impl ToolCall {
    /// Provider metadata affects display only. The persisted arguments stay intact.
    pub fn presentation(&self) -> ToolPresentation {
        presentation::present(self, None)
    }
    pub fn presentation_in(&self, cwd: Option<&str>) -> ToolPresentation {
        presentation::present(self, cwd)
    }
    /// Show the provider's actual input rather than the desktop adapter envelope.
    pub fn input_detail(&self) -> String {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.input) else {
            return self.input.clone();
        };
        let raw = if value["__desktop"].is_object() {
            value
                .get("rawInput")
                .filter(|raw| !raw.is_null())
                .unwrap_or(&value["__desktop"])
        } else {
            &value
        };
        raw.as_str().map(str::to_owned).unwrap_or_else(|| {
            serde_json::to_string_pretty(raw).unwrap_or_else(|_| self.input.clone())
        })
    }
}

/// Strip a recognized shell launcher for display without executing its script.
pub fn shell_command(command: &str) -> String {
    presentation::shell_command(command)
}
