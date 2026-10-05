//! What the source control views ask of the rest of the app: the agent that
//! writes commit messages and pull request text, native confirms and alerts,
//! opening URLs, and the signals other features listen to.
//!
//! Every field is optional. Unset agent callbacks disable generation; the
//! other defaults use GPUI's native prompt and `open_url`.

use std::rc::Rc;

use gpui::{App, PromptButton, PromptLevel, Task, Window};
use monocode_core::HarnessId;
use monocode_core::appearance::ChangesView;

use crate::model::diff_comment::DiffCommentItem;

/// `generateCommitMessage(cwd, textHarness, signal)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessageRequest {
    pub cwd: String,
    pub text_harness: Option<HarnessId>,
}

/// `generatePrContent(cwd, textHarness)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrContentRequest {
    pub cwd: String,
    pub text_harness: Option<HarnessId>,
}

/// Pull request text for `git_pr_create`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrContent {
    pub title: String,
    pub body: String,
    pub base: String,
    pub head: String,
}

/// Generates a commit message. The view drops the task to cancel, the way
/// the React view aborted its signal, and ignores whatever a dropped task
/// would have returned. An implementation that runs an agent should stop
/// it when its task is dropped.
pub type GenerateCommitMessage =
    Rc<dyn Fn(CommitMessageRequest, &mut App) -> Task<Result<String, String>>>;

/// Generates pull request text. `Ok(None)` means there was nothing to write.
pub type GeneratePrContent =
    Rc<dyn Fn(PrContentRequest, &mut App) -> Task<Result<Option<PrContent>, String>>>;

/// A yes or no question (`ask` from the Tauri dialog plugin).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub message: String,
    /// The confirming button's label. The native default is "Yes".
    pub ok_label: Option<String>,
}

pub type ConfirmHandler = Rc<dyn Fn(Confirm, &mut Window, &mut App) -> Task<bool>>;
pub type AlertHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;
pub type OpenUrlHandler = Rc<dyn Fn(&str, &mut App)>;
/// `invalidateWatchedFiles(paths?)`: `None` means every open file.
pub type FilesChangedHandler = Rc<dyn Fn(Option<&[String]>, &mut App)>;
/// `recordInboxSelfActivity({ provider: "github", kind: "pr", number, projectPath })`.
pub type PrActivityHandler = Rc<dyn Fn(&str, i64, &mut App)>;
/// `requestAddToChat(item)`.
pub type AddToChatHandler = Rc<dyn Fn(DiffCommentItem, &mut App)>;
/// `saveChangesView(view)`.
pub type SaveChangesViewHandler = Rc<dyn Fn(ChangesView, &mut App)>;
/// A git mutation made here, for the rest of the app (`notifyGitChanged`).
pub type GitChangedHandler = Rc<dyn Fn(&mut App)>;

#[derive(Clone, Default)]
pub struct ScmHooks {
    pub generate_commit_message: Option<GenerateCommitMessage>,
    pub generate_pr_content: Option<GeneratePrContent>,
    pub confirm: Option<ConfirmHandler>,
    pub alert: Option<AlertHandler>,
    pub open_url: Option<OpenUrlHandler>,
    pub files_changed: Option<FilesChangedHandler>,
    pub pr_activity: Option<PrActivityHandler>,
    pub add_to_chat: Option<AddToChatHandler>,
    pub save_changes_view: Option<SaveChangesViewHandler>,
    pub git_changed: Option<GitChangedHandler>,
}

impl ScmHooks {
    /// `confirmNative`: a warning prompt titled MonoCode.
    pub fn confirm(
        &self,
        message: String,
        ok_label: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<bool> {
        if let Some(confirm) = &self.confirm {
            return confirm(
                Confirm {
                    message,
                    ok_label: ok_label.map(str::to_string),
                },
                window,
                cx,
            );
        }
        let buttons = match ok_label {
            Some(label) => [
                PromptButton::Ok(label.to_string().into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            None => [
                PromptButton::Ok("Yes".into()),
                PromptButton::Cancel("No".into()),
            ],
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "MonoCode",
            Some(&message),
            &buttons,
            cx,
        );
        cx.foreground_executor()
            .spawn(async move { matches!(answer.await, Ok(0)) })
    }

    /// `window.alert`.
    pub fn alert(&self, message: String, window: &mut Window, cx: &mut App) {
        if let Some(alert) = &self.alert {
            alert(message, window, cx);
            return;
        }
        let answer = window.prompt(
            PromptLevel::Critical,
            "MonoCode",
            Some(&message),
            &[PromptButton::Ok("OK".into())],
            cx,
        );
        cx.foreground_executor()
            .spawn(async move {
                let _ = answer.await;
            })
            .detach();
    }

    /// `openUrl`.
    pub fn open_url(&self, url: &str, cx: &mut App) {
        match &self.open_url {
            Some(open) => open(url, cx),
            None => cx.open_url(url),
        }
    }

    pub fn files_changed(&self, paths: Option<&[String]>, cx: &mut App) {
        if let Some(handler) = &self.files_changed {
            handler(paths, cx);
        }
    }

    pub fn pr_activity(&self, cwd: &str, number: i64, cx: &mut App) {
        if let Some(handler) = &self.pr_activity {
            handler(cwd, number, cx);
        }
    }

    pub fn add_to_chat(&self, item: DiffCommentItem, cx: &mut App) {
        if let Some(handler) = &self.add_to_chat {
            handler(item, cx);
        }
    }

    pub fn save_changes_view(&self, view: ChangesView, cx: &mut App) {
        if let Some(handler) = &self.save_changes_view {
            handler(view, cx);
        }
    }
}
