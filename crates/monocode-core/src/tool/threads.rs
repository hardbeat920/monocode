use super::{tasks::Panel, ToolCall, ToolState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadRun {
    pub id: Option<i64>,
    pub title: String,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub prompt: String,
}

impl ToolCall {
    /// The tool's name without the MonoCode MCP server's prefix.
    fn monocode_tool(&self) -> &str {
        self.name
            .strip_prefix("mcp__monocode__")
            .or_else(|| self.name.strip_prefix("monocode_"))
            .or_else(|| self.name.strip_prefix("monocode:"))
            .unwrap_or(&self.name)
    }

    /// The plan an accepted `orchestration_plan` call submitted.
    pub fn orchestration_plan(&self) -> Option<crate::orchestration::Plan> {
        if self.monocode_tool() != crate::orchestration::TOOL
            || matches!(self.state, ToolState::Failed | ToolState::Cancelled)
        {
            return None;
        }
        let input: Value = serde_json::from_str(&self.input).ok()?;
        crate::orchestration::from_input(&input).ok()
    }

    pub fn thread_action(&self) -> Option<&str> {
        match self.monocode_tool() {
            "thread_start" => Some("start"),
            "thread_send" => Some("send"),
            "thread_wait" => Some("wait"),
            "thread_read" => Some("read"),
            "thread_stop" => Some("stop"),
            "thread_close" => Some("close"),
            "thread_list" => Some("list"),
            _ => None,
        }
    }

    pub fn thread_result(&self) -> Value {
        fn decode(mut value: Value, depth: usize) -> Value {
            if depth == 0 {
                return value;
            }
            if let Some(text) = value.as_str() {
                return serde_json::from_str(text)
                    .map(|v| decode(v, depth - 1))
                    .unwrap_or(value);
            }
            if let Some(text) = value["content"]
                .as_array()
                .and_then(|a| a.iter().find_map(|b| b["text"].as_str()))
            {
                return serde_json::from_str(text)
                    .map(|v| decode(v, depth - 1))
                    .unwrap_or(value);
            }
            if value["result"].is_object() {
                return decode(value["result"].take(), depth - 1);
            }
            value
        }
        decode(serde_json::from_str(&self.output).unwrap_or_default(), 4)
    }

    pub(super) fn thread_panel(&self) -> Option<Panel> {
        if !matches!(self.thread_action(), Some("start" | "send" | "stop")) {
            return None;
        }
        let input: Value = serde_json::from_str(&self.input).unwrap_or_default();
        let result = self.thread_result();
        Some(Panel::Thread(ThreadRun {
            id: result["threadId"]
                .as_i64()
                .or_else(|| input["threadId"].as_i64()),
            title: input["title"].as_str().unwrap_or("Child thread").into(),
            harness: result["harness"]
                .as_str()
                .or_else(|| input["harness"].as_str())
                .map(str::to_owned),
            model: result["model"]
                .as_str()
                .or_else(|| input["model"].as_str())
                .map(str::to_owned),
            prompt: input["prompt"].as_str().unwrap_or_default().into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_thread_calls_preserve_identity_through_mcp_envelopes() {
        let mut call = ToolCall::new("start", "mcp__monocode__thread_start");
        call.input =
            json!({"title":"Review", "prompt":"Check ownership", "harness":"codex"}).to_string();
        call.output = json!({"content":[{"type":"text","text":json!({"threadId":7,"model":"gpt"}).to_string()}]}).to_string();
        let Some(Panel::Thread(run)) = call.panel() else {
            panic!("missing thread panel")
        };
        assert_eq!(run.id, Some(7));
        assert_eq!(run.title, "Review");
        assert_eq!(run.model.as_deref(), Some("gpt"));
        assert!(ToolCall::new("x", "mcp__other__thread_start")
            .thread_action()
            .is_none());
        assert_eq!(
            ToolCall::new("x", "monocode:thread_wait").thread_action(),
            Some("wait")
        );
    }

    #[test]
    fn thread_start_reads_its_result_after_the_input_refresh() {
        // Claude refreshes agent input at tool_use, before the result exists.
        let mut call = ToolCall::new("start", "mcp__monocode__thread_start");
        call.input = json!({"title":"Drafts", "prompt":"Share drafts"}).to_string();
        call.refresh_agent_input();
        call.output = json!({"threadId":7,"harness":"claude","model":"opus"}).to_string();
        call.state = ToolState::Completed;
        let Some(Panel::Thread(run)) = call.panel() else {
            panic!("missing thread panel")
        };
        assert_eq!(run.id, Some(7));
        assert_eq!(run.harness.as_deref(), Some("claude"));
        assert_eq!(run.model.as_deref(), Some("opus"));
    }
}
