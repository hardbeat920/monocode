//! Port of src/integrations/harness/providers/codex/codexGit.ts: commit
//! messages, pull request text, and branch names from the Codex text runner.

use std::sync::Arc;

use anyhow::{Result, anyhow};

use monocode_core::js;

use crate::core::git_text::{
    CommitMessagePromptInput, PrContentPromptInput, build_branch_name_prompt,
    build_commit_message_prompt, build_pr_content_prompt, format_commit_message, parse_branch_name,
    parse_commit_message, parse_pr_content,
};
use crate::core::registry::{GeneratedPrContent, TextPromptInput};
use crate::core::task::{AbortSignal, BoxFuture};

use super::text::CodexText;

const GIT_TIMEOUT_MS: i64 = 90_000;

/// `gitStagedContext`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStagedContext {
    pub branch: Option<String>,
    pub summary: String,
    pub patch: String,
}

/// `gitRangeContext`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRangeContext {
    pub base: String,
    pub head: String,
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

/// `gitStagedContext` and `gitRangeContext` from src/platform/tauri/fs.ts.
/// The app implements this over monocode-git's functions of the same names.
pub trait GitContexts: Send + Sync {
    fn staged_context(&self, cwd: String) -> BoxFuture<'static, Result<GitStagedContext, String>>;
    fn range_context(&self, cwd: String) -> BoxFuture<'static, Result<GitRangeContext, String>>;
}

fn git_or_error(git: Option<&Arc<dyn GitContexts>>) -> Result<&Arc<dyn GitContexts>> {
    git.ok_or_else(|| anyhow!("Git context is not available"))
}

/// `generateCodexCommitMessage`.
pub async fn generate_codex_commit_message(
    text: &CodexText,
    git: Option<&Arc<dyn GitContexts>>,
    cwd: &str,
    signal: Option<AbortSignal>,
) -> Result<String> {
    if let Some(signal) = &signal {
        signal.throw_if_aborted()?;
    }
    let context = git_or_error(git)?
        .staged_context(cwd.to_string())
        .await
        .map_err(|error| anyhow!(error))?;
    if let Some(signal) = &signal {
        signal.throw_if_aborted()?;
    }
    let output = text
        .run_text_prompt(TextPromptInput {
            cwd: cwd.to_string(),
            prompt: build_commit_message_prompt(&CommitMessagePromptInput {
                branch: context.branch,
                staged_summary: context.summary,
                staged_patch: context.patch,
                include_branch: false,
            }),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            signal,
            ..Default::default()
        })
        .await?;
    if let Some(parsed) = parse_commit_message(&output) {
        return Ok(format_commit_message(&parsed));
    }
    let snippet = collapse_space(js::trim(&output));
    let snippet = js::slice_prefix(&snippet, 240);
    if snippet.is_empty() {
        Err(anyhow!(
            "Could not generate a commit message. Codex returned no text."
        ))
    } else {
        Err(anyhow!(
            "Could not generate a commit message. Model replied: {snippet}"
        ))
    }
}

/// `text.replace(/\s+/g, " ")`.
fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if js::is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `generateCodexPrContent`.
pub async fn generate_codex_pr_content(
    text: &CodexText,
    git: Option<&Arc<dyn GitContexts>>,
    cwd: &str,
) -> Result<Option<GeneratedPrContent>> {
    let range = git_or_error(git)?
        .range_context(cwd.to_string())
        .await
        .map_err(|error| anyhow!(error))?;
    let output = text
        .run_text_prompt(TextPromptInput {
            cwd: cwd.to_string(),
            prompt: build_pr_content_prompt(&PrContentPromptInput {
                base_branch: range.base.clone(),
                head_branch: range.head.clone(),
                commit_summary: range.commit_summary.clone(),
                diff_summary: range.diff_summary.clone(),
                diff_patch: range.diff_patch.clone(),
            }),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            ..Default::default()
        })
        .await;
    let parsed = match output {
        Ok(output) => parse_pr_content(&output),
        Err(error) => {
            log::debug!("[monocode] pr content {error:#}");
            None
        }
    };
    let first_line = js::trim(range.commit_summary.split('\n').next().unwrap_or(""));
    let title = match parsed.as_ref().map(|parsed| parsed.title.as_str()) {
        Some(title) if !title.is_empty() => title.to_string(),
        _ if !first_line.is_empty() => first_line.to_string(),
        _ => format!("Update {}", range.head),
    };
    let body = match parsed.as_ref().map(|parsed| parsed.body.as_str()) {
        Some(body) if !body.is_empty() => body.to_string(),
        _ => js::trim(&range.commit_summary).to_string(),
    };
    Ok(Some(GeneratedPrContent {
        title,
        body,
        base: range.base,
        head: range.head,
    }))
}

/// `generateCodexBranchName`.
pub async fn generate_codex_branch_name(
    text: &CodexText,
    cwd: &str,
    message: &str,
) -> Result<Option<String>> {
    let output = text
        .run_text_prompt(TextPromptInput {
            cwd: cwd.to_string(),
            prompt: build_branch_name_prompt(message),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            ..Default::default()
        })
        .await;
    match output {
        Ok(output) => Ok(parse_branch_name(&output)),
        Err(error) => {
            log::debug!("[monocode] branch name {error:#}");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace_like_the_regex() {
        assert_eq!(collapse_space("a \n\t b  c"), "a b c");
    }
}
