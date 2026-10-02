//! The transcript reducer, preview normalization, and shell intent. Port of src/integrations/harness/core/apply.ts, preview.ts, shellIntent.ts, and streamText.ts.

pub mod apply;
mod js_regex;
pub mod preview;
pub mod shell_intent;
pub mod stream_text;

pub use apply::{
    ReducerEnv, SystemEnv, UserTurnExtra, append_steer_user, append_steer_user_mut, append_user,
    append_user_mut, apply_harness_event, apply_harness_event_mut, apply_harness_events,
    apply_harness_events_mut, now_ms, promote_last_assistant_to_plan,
    promote_last_assistant_to_plan_mut, stop_streaming, stop_streaming_mut,
};
pub use preview::{
    MAX_LINE_CHARS, MAX_PREVIEW_LINES, Record, ToolTitleInput, agent_tool_title,
    compose_tool_title, context_lines, extract_search_query, extract_shell_command,
    extract_skill_name, extract_tool_preview, format_agent_type, is_agent_tool, is_agent_tool_name,
    is_edit_tool, is_execute_tool, is_file_tool, is_read_tool, is_search_tool, is_skill_tool,
    is_weak_tool_title, merge_tool_preview, stub_file_preview, title_from_tool_input,
};
pub use shell_intent::{
    ShellIntent, ShellVerb, format_shell_intent, infer_shell_intent, rewrite_readable_title,
    unwrap_shell_command,
};
pub use stream_text::{
    join_stream_text, join_stream_text_into, snapshot_remainder, stream_text_delta,
};
