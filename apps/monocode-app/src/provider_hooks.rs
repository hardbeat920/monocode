//! What the provider adapters ask of the app that `HarnessContext` does not
//! carry: git context for commit and pull request text (over
//! `monocode-git`), Codex's generated images, and Cursor's own session
//! stores (over `monocode-store`). Each read blocks on git or SQLite, so it
//! runs on smol's blocking pool and resolves a future.
//!
//! Claude, Cursor, Grok, OpenCode, and Codex each define their own copy of
//! the git context trait. [`MonoGit`] implements all five.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use futures::FutureExt as _;
use futures::future::BoxFuture;
use monocode_harness::providers::{claude, codex, cursor, grok, opencode};
use serde::de::DeserializeOwned;

/// Runs a blocking call off the caller's thread.
fn blocking<T: Send + 'static>(
    run: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> BoxFuture<'static, Result<T, String>> {
    smol::unblock(run).boxed()
}

fn anyhow_future<T: Send + 'static>(
    future: BoxFuture<'static, Result<T, String>>,
) -> BoxFuture<'static, Result<T>> {
    future
        .map(|result| result.map_err(|error| anyhow!(error)))
        .boxed()
}

/// A store value converted through its JSON shape. The store keeps its
/// fields private, and the harness types match its JSON.
fn through_json<T: DeserializeOwned>(value: impl serde::Serialize) -> Result<T, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| error.to_string())
}

/// `gitStagedContext` and `gitRangeContext` over `monocode_git::fs`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MonoGit;

/// The staged diff, or the unstaged diff against HEAD when nothing is staged.
fn staged(cwd: String) -> BoxFuture<'static, Result<monocode_git::fs::GitStagedContext, String>> {
    blocking(move || monocode_git::fs::git_staged_context(cwd))
}

/// Commits and diff between the default branch and HEAD.
fn range(cwd: String) -> BoxFuture<'static, Result<monocode_git::fs::GitRangeContext, String>> {
    blocking(move || monocode_git::fs::git_range_context(cwd))
}

/// Implements one provider's copy of the git context trait. Every copy has
/// the same two structs with the same fields.
macro_rules! impl_git_source {
    ($($module:ident)::+) => {
        impl $($module)::+::GitContextSource for MonoGit {
            fn staged_context(
                &self,
                cwd: &str,
            ) -> BoxFuture<'static, Result<$($module)::+::GitStagedContext>> {
                anyhow_future(
                    staged(cwd.to_string())
                        .map(|result| {
                            result.map(|context| $($module)::+::GitStagedContext {
                                branch: context.branch,
                                summary: context.summary,
                                patch: context.patch,
                            })
                        })
                        .boxed(),
                )
            }

            fn range_context(
                &self,
                cwd: &str,
            ) -> BoxFuture<'static, Result<$($module)::+::GitRangeContext>> {
                anyhow_future(
                    range(cwd.to_string())
                        .map(|result| {
                            result.map(|context| $($module)::+::GitRangeContext {
                                base: context.base,
                                head: context.head,
                                commit_summary: context.commit_summary,
                                diff_summary: context.diff_summary,
                                diff_patch: context.diff_patch,
                            })
                        })
                        .boxed(),
                )
            }
        }
    };
}

impl_git_source!(claude::git);
impl_git_source!(cursor::git);
impl_git_source!(grok::git);
impl_git_source!(opencode::git);

impl codex::GitContexts for MonoGit {
    fn staged_context(
        &self,
        cwd: String,
    ) -> BoxFuture<'static, Result<codex::GitStagedContext, String>> {
        staged(cwd)
            .map(|result| {
                result.map(|context| codex::GitStagedContext {
                    branch: context.branch,
                    summary: context.summary,
                    patch: context.patch,
                })
            })
            .boxed()
    }

    fn range_context(
        &self,
        cwd: String,
    ) -> BoxFuture<'static, Result<codex::GitRangeContext, String>> {
        range(cwd)
            .map(|result| {
                result.map(|context| codex::GitRangeContext {
                    base: context.base,
                    head: context.head,
                    commit_summary: context.commit_summary,
                    diff_summary: context.diff_summary,
                    diff_patch: context.diff_patch,
                })
            })
            .boxed()
    }
}

/// `saveGeneratedImage` and `deleteGeneratedImages` under the data dir.
#[derive(Debug, Clone)]
pub struct GeneratedImageStore {
    pub data_dir: PathBuf,
}

/// `GeneratedImageAsset`'s JSON shape in `monocode-git`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageAsset {
    path: String,
    mime_type: String,
    size: i64,
}

impl codex::GeneratedImages for GeneratedImageStore {
    fn save(
        &self,
        data: String,
        name: String,
    ) -> BoxFuture<'static, Result<codex::GeneratedImageAsset, String>> {
        let data_dir = self.data_dir.clone();
        blocking(move || {
            let asset = monocode_git::fs::save_generated_image(&data_dir, data, name)?;
            let asset: ImageAsset = through_json(asset)?;
            Ok(codex::GeneratedImageAsset {
                path: asset.path,
                mime_type: asset.mime_type,
                size: asset.size,
            })
        })
    }

    fn delete(&self, paths: Vec<String>) -> BoxFuture<'static, Result<(), String>> {
        let data_dir = self.data_dir.clone();
        blocking(move || monocode_git::fs::delete_generated_images(&data_dir, paths))
    }
}

/// Cursor's `cursor_subagent_runs` and `cursor_tool_calls` over
/// `monocode_store::cursor_store`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorSessionStore;

impl cursor::CursorStore for CursorSessionStore {
    fn subagent_runs(
        &self,
        session_id: String,
        tool_call_ids: Vec<String>,
        known_revisions: HashMap<String, String>,
    ) -> BoxFuture<'static, Result<Vec<cursor::StoredCursorSubagentRun>>> {
        anyhow_future(blocking(move || {
            let runs = monocode_store::cursor_store::cursor_subagent_runs(
                session_id,
                tool_call_ids,
                Some(known_revisions),
            )?;
            through_json(runs)
        }))
    }

    fn tool_calls(
        &self,
        session_id: String,
        tool_call_ids: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<cursor::StoredCursorToolCall>>> {
        anyhow_future(blocking(move || {
            let calls = monocode_store::cursor_store::cursor_tool_calls(session_id, tool_call_ids)?;
            through_json(calls)
        }))
    }
}
