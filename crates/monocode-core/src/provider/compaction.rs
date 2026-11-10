//! Provider-owned compaction uses the same durable lifecycle as tool calls.
use super::*;

impl Protocol {
    pub(super) fn compaction(
        &mut self,
        id: Option<&str>,
        state: ToolState,
        events: &mut Vec<Event>,
    ) {
        // Any subsequent prose starts after the activity row, even when the
        // provider reuses the previous assistant message id.
        self.text_key = None;
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = id
            .map(str::to_owned)
            .or_else(|| self.active_compaction.clone())
            .unwrap_or_else(|| {
                format!(
                    "native-compaction-{}-{}",
                    crate::time::now_ms(),
                    SEQUENCE.fetch_add(1, Ordering::Relaxed)
                )
            });
        if state == ToolState::Running {
            self.active_compaction = Some(id.clone());
        } else if self.active_compaction.as_deref() == Some(id.as_str()) {
            self.active_compaction = None;
        }
        let tool = self
            .tools
            .entry(id.clone())
            .or_insert_with(|| ToolCall::new(&id, "contextCompaction"));
        tool.state = state;
        tool.output = match tool.state {
            ToolState::Running => {
                "Summarizing the conversation to make room for the next response."
            }
            ToolState::Completed => "The conversation context has been compacted.",
            ToolState::Failed => "The provider could not compact the conversation context.",
            ToolState::Cancelled => "Context compaction was cancelled.",
        }
        .into();
        events.push(Event::Tool(tool.clone()));
    }
}
