//! Port of src/integrations/harness/providers/cursor/cursorTitle.ts: a
//! sidebar title from one Cursor text prompt.

use std::sync::Arc;

use crate::core::registry::{TextPromptInput, TitleInput};
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

use super::text::TextRunner;

const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateCursorSessionTitle`: a title, or `None` on any failure.
pub async fn generate_cursor_session_title(
    text: &Arc<TextRunner>,
    input: &TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run_cursor_text_prompt(TextPromptInput {
            cwd: input.cwd.clone(),
            prompt: build_thread_title_prompt(&input.message),
            timeout_ms: Some(TITLE_TIMEOUT_MS),
            ..Default::default()
        })
        .await;
    match output {
        Ok(output) => parse_generated_session_title(&output, &input.message),
        Err(error) => {
            log::debug!("[monocode] session title {error:#}");
            None
        }
    }
}
