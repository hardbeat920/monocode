//! What the Claude provider needs from src/integrations/harness/core/child.ts:
//! resolve the CLI, spawn it, watch its stdout lines and exit, write a line,
//! kill it, run it once for `--version`, and find the home directory.
//!
//! The provider talks to this trait rather than to [`Children`] directly, the
//! way the TypeScript tests replaced `../../core/child` with a mock. The app
//! uses [`ChildrenIo`]; tests use a scripted fake that delivers lines on the
//! caller's thread.

use std::sync::Arc;

use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use monocode_core::harness::HarnessId;

use crate::core::child::{BinaryPathChoice, ChildAccount, ChildHandlers, Children};

/// Called with each stdout line of a watched child.
pub type LineHandler = Arc<dyn Fn(String) + Send + Sync>;

/// Called when a watched child exits, with its exit code.
pub type ExitHandler = Arc<dyn Fn(Option<i64>) + Send + Sync>;

/// The Claude profile a child runs under: `{ provider: "claude", id }`, with
/// `default` when the session names no account.
pub fn claude_account(provider_account_id: Option<&str>) -> ChildAccount {
    ChildAccount {
        provider: HarnessId::Claude,
        id: provider_account_id.unwrap_or("default").to_string(),
    }
}

/// Child process access, keyed by a child id: the MonoCode thread id for a
/// session, or a fixed id for the text runner and the catalog probe.
pub trait ClaudeChildIo: Send + Sync {
    /// `resolveClaudeBinary`: the path of the Claude Code CLI to run.
    fn resolve_claude_binary(&self) -> BoxFuture<'static, Result<String>>;

    /// `spawnChild(id, path, args, cwd, account, "claude")`. Resolves once
    /// the child is running.
    fn spawn_child(
        &self,
        child_id: &str,
        command: &str,
        args: Vec<String>,
        cwd: &str,
        account: Option<ChildAccount>,
    ) -> BoxFuture<'static, Result<()>>;

    /// `watchChild`: route this child's stdout lines and exit to the
    /// handlers, in order. Lines that arrived before the watch come first.
    fn watch_child(&self, child_id: &str, on_line: LineHandler, on_exit: ExitHandler);

    /// `unwatchChild`.
    fn unwatch_child(&self, child_id: &str);

    /// `writeChild`: one line to the child's stdin. The newline is added.
    fn write_child(&self, child_id: &str, line: String) -> BoxFuture<'static, Result<()>>;

    /// `killChild`.
    fn kill_child(&self, child_id: &str) -> BoxFuture<'static, Result<()>>;

    /// `execChild(path, args, cwd, "claude")`: run the CLI once, return stdout.
    fn exec_child(
        &self,
        command: &str,
        args: Vec<String>,
        cwd: Option<&str>,
    ) -> BoxFuture<'static, Result<String>>;

    /// `homeDir()` on the machine running the children.
    fn home_dir(&self) -> BoxFuture<'static, Result<String>>;
}

/// A shared [`ClaudeChildIo`].
pub type SharedChildIo = Arc<dyn ClaudeChildIo>;

/// [`ClaudeChildIo`] over the framework's process handle.
#[derive(Clone)]
pub struct ChildrenIo {
    children: Children,
}

impl ChildrenIo {
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

impl ClaudeChildIo for ChildrenIo {
    fn resolve_claude_binary(&self) -> BoxFuture<'static, Result<String>> {
        let children = self.children.clone();
        async move { Ok(children.resolve_claude_binary().await?.path) }.boxed()
    }

    fn spawn_child(
        &self,
        child_id: &str,
        command: &str,
        args: Vec<String>,
        cwd: &str,
        account: Option<ChildAccount>,
    ) -> BoxFuture<'static, Result<()>> {
        let children = self.children.clone();
        let (child_id, command, cwd) = (child_id.to_string(), command.to_string(), cwd.to_string());
        async move {
            children
                .spawn_child(
                    &child_id,
                    &command,
                    args,
                    &cwd,
                    account,
                    Some(HarnessId::Claude),
                )
                .await
        }
        .boxed()
    }

    fn watch_child(&self, child_id: &str, on_line: LineHandler, on_exit: ExitHandler) {
        self.children.watch_child_with(
            child_id,
            ChildHandlers {
                on_line: Box::new(move |line| on_line(line)),
                on_exit: Box::new(move |code| on_exit(code.map(i64::from))),
                on_stderr: None,
            },
        );
    }

    fn unwatch_child(&self, child_id: &str) {
        self.children.unwatch_child(child_id);
    }

    fn write_child(&self, child_id: &str, line: String) -> BoxFuture<'static, Result<()>> {
        let children = self.children.clone();
        let child_id = child_id.to_string();
        async move { children.write_child(&child_id, &line).await }.boxed()
    }

    fn kill_child(&self, child_id: &str) -> BoxFuture<'static, Result<()>> {
        let children = self.children.clone();
        let child_id = child_id.to_string();
        async move { children.kill_child(&child_id).await }.boxed()
    }

    fn exec_child(
        &self,
        command: &str,
        args: Vec<String>,
        cwd: Option<&str>,
    ) -> BoxFuture<'static, Result<String>> {
        let children = self.children.clone();
        let command = command.to_string();
        let cwd = cwd.map(str::to_string);
        async move {
            children
                .exec_child(
                    &command,
                    args,
                    cwd.as_deref(),
                    Some(HarnessId::Claude),
                    BinaryPathChoice::Runtime,
                )
                .await
        }
        .boxed()
    }

    fn home_dir(&self) -> BoxFuture<'static, Result<String>> {
        let children = self.children.clone();
        async move { children.home_dir().await }.boxed()
    }
}
