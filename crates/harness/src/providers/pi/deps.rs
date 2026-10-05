//! The helpers this provider takes from `monocode_core::reducer` and
//! `crate::core`, in one place.

use serde_json::Value;

pub use crate::core::native_commands::{
    NativeCommand, NativeSubcommand, native_command_invocation,
};
pub use monocode_core::reducer::{extract_tool_preview, join_stream_text, title_from_tool_input};

/// `Record<string, unknown>`.
pub type Rec = monocode_core::reducer::Record;

/// `streamTextDelta`: body text from a stream. Whitespace is real content.
pub fn stream_text_delta(value: Option<&Value>) -> String {
    monocode_core::reducer::stream_text_delta(value).to_string()
}
