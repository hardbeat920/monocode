//! Port of src/integrations/harness/providers/claude/claudeTitle.ts: a tab
//! title for the first turn, from the text runner.

use crate::core::registry::{TextPromptInput, TitleInput};
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

use super::text::ClaudeText;

/// `TITLE_TIMEOUT_MS`.
pub const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateClaudeSessionTitle`. Any failure reads as no title.
pub async fn generate_claude_session_title(
    text: &ClaudeText,
    input: TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run(TextPromptInput {
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
