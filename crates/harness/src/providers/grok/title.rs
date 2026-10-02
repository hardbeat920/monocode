//! Port of src/integrations/harness/providers/grok/grokTitle.ts.

use crate::core::registry::TitleInput;
use crate::core::session_title::{
    GeneratedSessionTitle, build_thread_title_prompt, parse_generated_session_title,
};

use super::text::{GrokText, GrokTextPrompt};

const TITLE_TIMEOUT_MS: i64 = 45_000;

/// `generateGrokSessionTitle`. A failed run yields no title.
pub async fn generate_grok_session_title(
    text: &GrokText,
    input: &TitleInput,
) -> Option<GeneratedSessionTitle> {
    let output = text
        .run_prompt(GrokTextPrompt {
            cwd: input.cwd.clone(),
            prompt: build_thread_title_prompt(&input.message),
            timeout_ms: Some(TITLE_TIMEOUT_MS),
            ..GrokTextPrompt::default()
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
