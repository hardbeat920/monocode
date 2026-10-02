//! Port of src/integrations/harness/providers/grok/grokGit.ts: commit
//! messages, pull request text, and branch names from the text runner.
//!
//! The TypeScript read the repository through the `gitStagedContext` and
//! `gitRangeContext` Tauri commands. This crate cannot depend on
//! `monocode-git`, so the app supplies them through [`GitContextSource`],
//! the same shape the Claude adapter takes.

use std::sync::{Arc, LazyLock};

use anyhow::{Result, anyhow};
use regex::Regex;

use monocode_core::js;

use crate::core::git_text::{
    CommitMessagePromptInput, PrContent, PrContentPromptInput, build_branch_name_prompt,
    build_commit_message_prompt, build_pr_content_prompt, format_commit_message, parse_branch_name,
    parse_commit_message, parse_pr_content,
};
use crate::core::json_text::first_line;
use crate::core::registry::GeneratedPrContent;
use crate::core::task::{AbortSignal, BoxFuture};

use super::text::{GrokText, GrokTextPrompt};

const GIT_TIMEOUT_MS: i64 = 60_000;

/// `GitStagedContext`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStagedContext {
    pub branch: Option<String>,
    pub summary: String,
    pub patch: String,
}

/// `GitRangeContext`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitRangeContext {
    pub base: String,
    pub head: String,
    pub commit_summary: String,
    pub diff_summary: String,
    pub diff_patch: String,
}

/// The two git reads commit and pull request text need. The app implements
/// this over `monocode_git::fs::git_staged_context` and `git_range_context`,
/// off the UI thread.
pub trait GitContextSource: Send + Sync {
    fn staged_context(&self, cwd: &str) -> BoxFuture<'static, Result<GitStagedContext>>;
    fn range_context(&self, cwd: &str) -> BoxFuture<'static, Result<GitRangeContext>>;
}

pub type SharedGitSource = Arc<dyn GitContextSource>;

fn git_source(git: Option<&SharedGitSource>) -> Result<&SharedGitSource> {
    git.ok_or_else(|| anyhow!("Git context is not available"))
}

fn throw_if_aborted(signal: Option<&AbortSignal>) -> Result<()> {
    signal.map_or(Ok(()), AbortSignal::throw_if_aborted)
}

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// `generateGrokCommitMessage`.
pub async fn generate_grok_commit_message(
    text: &GrokText,
    git: Option<&SharedGitSource>,
    cwd: &str,
    signal: Option<AbortSignal>,
) -> Result<String> {
    throw_if_aborted(signal.as_ref())?;
    let context = git_source(git)?.staged_context(cwd).await?;
    throw_if_aborted(signal.as_ref())?;
    let output = text
        .run_prompt(GrokTextPrompt {
            cwd: cwd.to_string(),
            prompt: build_commit_message_prompt(&CommitMessagePromptInput {
                branch: context.branch,
                staged_summary: context.summary,
                staged_patch: context.patch,
                include_branch: false,
            }),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            signal,
            ..GrokTextPrompt::default()
        })
        .await?;
    if let Some(parsed) = parse_commit_message(&output) {
        return Ok(format_commit_message(&parsed));
    }
    let squashed = WHITESPACE.replace_all(js::trim(&output), " ");
    let snippet = js::slice_prefix(&squashed, 240);
    if snippet.is_empty() {
        Err(anyhow!(
            "Could not generate a commit message. Grok Build returned no text."
        ))
    } else {
        Err(anyhow!(
            "Could not generate a commit message. Model replied: {snippet}"
        ))
    }
}

/// `generateGrokPrContent`. A failed run falls back to the commit summary.
pub async fn generate_grok_pr_content(
    text: &GrokText,
    git: Option<&SharedGitSource>,
    cwd: &str,
) -> Result<Option<GeneratedPrContent>> {
    let range = git_source(git)?.range_context(cwd).await?;
    let output = text
        .run_prompt(GrokTextPrompt {
            cwd: cwd.to_string(),
            prompt: build_pr_content_prompt(&PrContentPromptInput {
                base_branch: range.base.clone(),
                head_branch: range.head.clone(),
                commit_summary: range.commit_summary.clone(),
                diff_summary: range.diff_summary.clone(),
                diff_patch: range.diff_patch.clone(),
            }),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            ..GrokTextPrompt::default()
        })
        .await;
    let parsed: Option<PrContent> = match output {
        Ok(output) => parse_pr_content(&output),
        Err(error) => {
            log::debug!("[monocode] pr content {error:#}");
            None
        }
    };
    let first = js::trim(first_line(&range.commit_summary));
    let title = match parsed.as_ref().map(|parsed| parsed.title.as_str()) {
        Some(title) if !title.is_empty() => title.to_string(),
        _ if !first.is_empty() => first.to_string(),
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

/// `generateGrokBranchName`. A failed run yields no name.
pub async fn generate_grok_branch_name(
    text: &GrokText,
    cwd: &str,
    message: &str,
) -> Option<String> {
    let output = text
        .run_prompt(GrokTextPrompt {
            cwd: cwd.to_string(),
            prompt: build_branch_name_prompt(message),
            timeout_ms: Some(GIT_TIMEOUT_MS),
            ..GrokTextPrompt::default()
        })
        .await;
    match output {
        Ok(output) => parse_branch_name(&output),
        Err(error) => {
            log::debug!("[monocode] branch name {error:#}");
            None
        }
    }
}
