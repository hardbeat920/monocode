//! Helpers this provider takes from code that other agents are still porting
//! (`monocode_core::reducer`'s preview.ts and streamText.ts). Every call goes
//! through this file, so switching to the real functions touches one place.

use monocode_core::block::{ToolPreview, ToolPreviewKind};
use serde_json::{Map, Value};

/// `streamTextDelta`: body text from a stream. Whitespace is real content,
/// not a missing field.
pub fn stream_text_delta(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// `extractToolPreview(update, tool)`.
// TODO(port): call monocode_core::reducer's extract_tool_preview once it lands.
pub fn extract_tool_preview(
    _update: &Map<String, Value>,
    _tool: &Map<String, Value>,
) -> Option<ToolPreview> {
    None
}

/// `extractShellCommand(...values)`.
// TODO(port): call monocode_core::reducer's extract_shell_command once it lands.
pub fn extract_shell_command(_value: Option<&Value>) -> Option<String> {
    None
}

/// `extractSkillName(...values)`.
// TODO(port): call monocode_core::reducer's extract_skill_name once it lands.
pub fn extract_skill_name(_value: Option<&Value>) -> Option<String> {
    None
}

/// The options object `composeToolTitle` takes. The stand-in reads only the
/// title; the real function reads every field.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct ComposeToolTitle<'a> {
    pub kind: Option<&'a str>,
    pub title: Option<&'a str>,
    pub path: Option<&'a str>,
    pub query: Option<&'a str>,
    pub command: Option<&'a str>,
    pub skill: Option<&'a str>,
    pub preview_kind: Option<ToolPreviewKind>,
}

/// `composeToolTitle(opts)`. The stand-in returns the trimmed title, which is
/// what the TypeScript returns for a kind it does not rewrite.
// TODO(port): call monocode_core::reducer's compose_tool_title once it lands.
pub fn compose_tool_title(opts: &ComposeToolTitle<'_>) -> String {
    monocode_core::js::trim(opts.title.unwrap_or_default()).to_string()
}
