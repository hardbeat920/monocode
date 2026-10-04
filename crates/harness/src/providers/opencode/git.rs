//! Port of src/integrations/harness/providers/opencode/opencodeGit.ts:
//! commit messages, pull request text, and branch names from the text
//! backend.
//!
//! The TypeScript read the repository through the `gitStagedContext` and
//! `gitRangeContext` Tauri commands. This crate cannot depend on
//! `monocode-git`, so the caller supplies them through [`GitContextSource`].

use std::sync::Arc;

use anyhow::{Result, bail};
use futures::future::BoxFuture;
use monocode_core::js;

use super::text::TextBackend;
use crate::core::git_text::{
    CommitMessagePromptInput, PrContent, PrContentPromptInput, build_branch_name_prompt,
    build_commit_message_prompt, build_pr_content_prompt, format_commit_message, parse_branch_name,
    parse_commit_message, parse_pr_content,
};
use crate::core::registry::{GeneratedPrContent, TextPromptInput};
use crate::core::task::AbortSignal;

/// `GIT_TIMEOUT_MS`.
pub const GIT_TIMEOUT_MS: i64 = 90_000;

/// `GitStagedContext` (`monocode_git::fs::GitStagedContext`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStagedContext {
    pub branch: Option<String>,
    pub summary: String,
    pub patch: String,
}

/// `GitRangeContext` (`monocode_git::fs::GitRangeContext`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRangeContext {
    pub base: String,
    pub head: String,
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

/// The two git reads commit and pull request text need. The engine
/// implements this over `monocode_git::fs::git_staged_context` and
/// `git_range_context`, off the UI thread.
pub trait GitContextSource: Send + Sync {
    fn staged_context(&self, cwd: &str) -> BoxFuture<'static, Result<GitStagedContext>>;
    fn range_context(&self, cwd: &str) -> BoxFuture<'static, Result<GitRangeContext>>;
}

pub type SharedGitSource = Arc<dyn GitContextSource>;

fn git_source(git: Option<&SharedGitSource>) -> Result<&SharedGitSource> {
    match git {
        Some(git) => Ok(git),
        None => bail!("Git context is not available"),
    }
}

fn throw_if_aborted(signal: Option<&AbortSignal>) -> Result<()> {
    signal.map_or(Ok(()), AbortSignal::throw_if_aborted)
}

fn prompt(cwd: &str, prompt: String, signal: Option<AbortSignal>) -> TextPromptInput {
    TextPromptInput {
        cwd: cwd.to_string(),
        prompt,
        timeout_ms: Some(GIT_TIMEOUT_MS),
        signal,
        ..Default::default()
    }
}

/// `.replace(/\s+/g, " ")`.
fn squash_whitespace(text: &str) -> String {
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

/// `generateOpenCodeCommitMessage`.
pub async fn generate_open_code_commit_message(
    text: &impl TextBackend,
    git: Option<&SharedGitSource>,
    cwd: &str,
    signal: Option<AbortSignal>,
) -> Result<String> {
    throw_if_aborted(signal.as_ref())?;
    let context = git_source(git)?.staged_context(cwd).await?;
    throw_if_aborted(signal.as_ref())?;
    let output = text
        .run_text(prompt(
            cwd,
            build_commit_message_prompt(&CommitMessagePromptInput {
                branch: context.branch,
                staged_summary: context.summary,
                staged_patch: context.patch,
                include_branch: false,
            }),
            signal,
        ))
        .await?;
    if let Some(parsed) = parse_commit_message(&output) {
        return Ok(format_commit_message(&parsed));
    }
    let snippet = squash_whitespace(js::trim(&output));
    let snippet = js::slice_prefix(&snippet, 240);
    if snippet.is_empty() {
        bail!("Could not generate a commit message. OpenCode returned no text.");
    }
    bail!("Could not generate a commit message. Model replied: {snippet}");
}

/// `generateOpenCodePrContent`.
pub async fn generate_open_code_pr_content(
    text: &impl TextBackend,
    git: Option<&SharedGitSource>,
    cwd: &str,
) -> Result<Option<GeneratedPrContent>> {
    let range = git_source(git)?.range_context(cwd).await?;
    let output = text
        .run_text(prompt(
            cwd,
            build_pr_content_prompt(&PrContentPromptInput {
                base_branch: range.base.clone(),
                head_branch: range.head.clone(),
                commit_summary: range.commit_summary.clone(),
                diff_summary: range.diff_summary.clone(),
                diff_patch: range.diff_patch.clone(),
            }),
            None,
        ))
        .await;
    let parsed: Option<PrContent> = match output {
        Ok(output) => parse_pr_content(&output),
        Err(error) => {
            log::debug!("[monocode] pr content {error:#}");
            None
        }
    };
    // `range.commitSummary.split(/\r?\n/)[0]?.trim()`.
    let first_commit = range
        .commit_summary
        .split('\n')
        .next()
        .map(|line| js::trim(line.strip_suffix('\r').unwrap_or(line)).to_string())
        .unwrap_or_default();
    let title = parsed
        .as_ref()
        .map(|parsed| parsed.title.clone())
        .filter(|title| !title.is_empty())
        .or_else(|| (!first_commit.is_empty()).then_some(first_commit))
        .unwrap_or_else(|| format!("Update {}", range.head));
    let body = parsed
        .map(|parsed| parsed.body)
        .filter(|body| !body.is_empty())
        .unwrap_or_else(|| js::trim(&range.commit_summary).to_string());
    Ok(Some(GeneratedPrContent {
        title,
        body,
        base: range.base,
        head: range.head,
    }))
}

/// `generateOpenCodeBranchName`.
pub async fn generate_open_code_branch_name(
    text: &impl TextBackend,
    cwd: &str,
    message: &str,
) -> Option<String> {
    match text
        .run_text(prompt(cwd, build_branch_name_prompt(message), None))
        .await
    {
        Ok(output) => parse_branch_name(&output),
        Err(error) => {
            log::debug!("[monocode] branch name {error:#}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squashes_whitespace_runs_like_the_regex() {
        assert_eq!(squash_whitespace("a \n\t b  c"), "a b c");
    }
}
