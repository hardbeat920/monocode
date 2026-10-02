//! Port of src/integrations/harness/providers/codex/codexTitle.ts: a sidebar
//! title from the Codex text runner.

use crate::core::registry::{TextPromptInput, TitleInput};
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

use super::text::CodexText;

const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateCodexSessionTitle`: a title, or `None` on any failure.
pub async fn generate_codex_session_title(
    text: &CodexText,
    input: &TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run_text_prompt(TextPromptInput {
            cwd: input.cwd.clone(),
            provider_account_id: input.provider_account_id.clone(),
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
