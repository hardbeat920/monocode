//! Port of src/integrations/harness/providers/pi/piTitle.ts: an LLM tab
//! title for the first turn, through the isolated text generator.

use crate::core::registry::{TextPromptInput, TitleInput};
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

use super::text::PiText;

const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateSessionTitle`. Failures log and return `None`.
pub async fn generate_session_title(
    text: &PiText,
    input: TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run_text_prompt(TextPromptInput {
            cwd: input.cwd.clone(),
            prompt: build_thread_title_prompt(&input.message),
            timeout_ms: Some(TITLE_TIMEOUT_MS),
            ..TextPromptInput::default()
        })
        .await;
    match output {
        Ok(output) => parse_generated_session_title(&output, &input.message),
        Err(error) => {
            log::debug!("[monocode] session title {error}");
            None
        }
    }
}
