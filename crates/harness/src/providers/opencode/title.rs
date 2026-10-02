//! Port of src/integrations/harness/providers/opencode/opencodeTitle.ts: the
//! first-turn tab title from the text backend.

use super::text::OpenCodeText;
use crate::core::registry::{TextPromptInput, TitleInput};
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateOpenCodeSessionTitle`. Failures read as no title.
pub async fn generate_open_code_session_title(
    text: &OpenCodeText,
    input: &TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run(TextPromptInput {
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
