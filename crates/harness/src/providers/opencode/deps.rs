//! The preview and stream helpers this provider takes from
//! `monocode_core::reducer` (ports of preview.ts and streamText.ts). Every
//! call goes through this file, which adapts the provider's call shapes to
//! the reducer's.

use monocode_core::block::{ToolPreview, ToolPreviewKind};
use monocode_core::reducer::{preview, stream_text};
use serde_json::{Map, Value};

/// `streamTextDelta`: body text from a stream. Whitespace is real content,
/// not a missing field.
pub fn stream_text_delta(value: Option<&Value>) -> String {
    stream_text::stream_text_delta(value).to_string()
}

/// `extractToolPreview(update, tool)`.
pub fn extract_tool_preview(
    update: &Map<String, Value>,
    tool: &Map<String, Value>,
) -> Option<ToolPreview> {
    preview::extract_tool_preview(update, tool)
}

/// `extractShellCommand(...values)`.
pub fn extract_shell_command(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| preview::extract_shell_command(&[value]))
}

/// `extractSkillName(...values)`.
pub fn extract_skill_name(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| preview::extract_skill_name(&[value]))
}

/// The options object `composeToolTitle` takes.
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

/// `composeToolTitle(opts)`.
pub fn compose_tool_title(opts: &ComposeToolTitle<'_>) -> String {
    preview::compose_tool_title(&preview::ToolTitleInput {
        kind: opts.kind,
        title: opts.title,
        path: opts.path,
        query: opts.query,
        command: opts.command,
        skill: opts.skill,
        preview_kind: opts.preview_kind,
        cwd: None,
    })
}
