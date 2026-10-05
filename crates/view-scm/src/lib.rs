//! Source control views: the changes panel, history graph, branch and
//! worktree pickers, the worktrees settings page, the diff wrappers, the
//! diff comment composer, the branch and worktree dialogs, and the GitHub
//! pull request actions. Port of src/features/source-control/ui.
//!
//! Views take an [`Scm`] handle: a [`GitBackend`] for git calls, the
//! engine's git status registry for the shared diff index, branches, and
//! worktree list, and [`ScmHooks`] for the agent and the rest of the app.

pub mod git;
pub mod hooks;
pub mod model;
pub mod paths;
pub mod scm;
pub mod ui;

#[cfg(test)]
mod view_tests;

pub use git::{GitBackend, GitFileDiffKind, LocalGit};
pub use hooks::ScmHooks;
pub use scm::{Scm, ScmEvent, ScmState};
