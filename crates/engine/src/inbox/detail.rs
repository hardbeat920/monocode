//! `InboxItemDetail`: the data side of the item detail pane, ported from
//! `InboxDetail` and `GithubPrActions` in src/features/inbox/ui/InboxView.tsx.
//! It loads the description, the comment thread, and the pull request diff
//! for any provider, posts comments and replies, and runs pull request
//! actions. A cached answer shows at once while a fresh one loads.

use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{Context, Task};

use super::client::{InboxClient, Pending};
use super::types::{
    GithubPrAction, InboxItem, InboxProvider, PrDiff, WorkItemDetails, WorkItemKind,
    WorkItemThread, work_kind,
};

/// `InboxReplyTarget`: the comment a reply answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxReplyTarget {
    /// Linear replies go under this comment id.
    pub id: String,
    /// GitHub replies go into this review thread.
    pub thread_id: String,
}

/// A posted comment: the reloaded thread, or why it could not reload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostedComment {
    pub thread: Option<WorkItemThread>,
    pub reload_error: Option<String>,
}

/// `inboxShowsFullFileDiff`: only GitHub pull requests offer full files.
pub fn inbox_shows_full_file_diff(item: &InboxItem) -> bool {
    item.provider == InboxProvider::Github && item.kind == super::types::InboxKind::Pr
}

fn jira_key(item: &InboxItem) -> String {
    item.identifier.clone().unwrap_or_default()
}

fn repository_kind(item: &InboxItem, provider: InboxProvider) -> Option<WorkItemKind> {
    (item.provider == provider)
        .then(|| work_kind(item.kind))
        .flatten()
}

fn rejected<T: Clone + Send + Sync + 'static>(message: &str) -> Pending<T> {
    super::client::ready(Err(message.to_string()))
}

impl InboxClient {
    /// The cached description of any item.
    pub fn peek_item_details(&self, item: &InboxItem) -> Option<WorkItemDetails> {
        match item.provider {
            InboxProvider::Linear => {
                self.peek_linear_issue_details(item.id.as_deref().unwrap_or(""))
            }
            InboxProvider::Jira => self.peek_jira_issue_details(&jira_key(item)),
            InboxProvider::Gitlab => {
                let kind = repository_kind(item, InboxProvider::Gitlab)?;
                self.peek_gitlab_work_item_details(&item.repo, kind, item.number)
            }
            InboxProvider::AzureDevops => {
                let kind = repository_kind(item, InboxProvider::AzureDevops)?;
                self.peek_azure_dev_ops_work_item_details(&item.repo, kind, item.number)
            }
            InboxProvider::Github => {
                let kind = repository_kind(item, InboxProvider::Github)?;
                self.peek_github_work_item_details(&item.repo, kind, item.number)
            }
        }
    }

    /// Fetch the description of any item.
    pub fn item_details(&self, item: &InboxItem) -> Pending<WorkItemDetails> {
        match item.provider {
            InboxProvider::Linear => match item.id.as_deref().filter(|id| !id.is_empty()) {
                Some(id) => self.linear_issue_details(id),
                None => rejected("Missing Linear issue"),
            },
            InboxProvider::Jira => {
                let key = jira_key(item);
                if key.is_empty() {
                    rejected("Missing Jira issue")
                } else {
                    self.jira_issue_details(&key)
                }
            }
            InboxProvider::Gitlab => match repository_kind(item, InboxProvider::Gitlab) {
                Some(kind) => self.gitlab_work_item_details(&item.repo, kind, item.number),
                None => rejected("Unknown inbox item"),
            },
            InboxProvider::AzureDevops => match repository_kind(item, InboxProvider::AzureDevops) {
                Some(kind) => self.azure_dev_ops_work_item_details(&item.repo, kind, item.number),
                None => rejected("Unknown inbox item"),
            },
            InboxProvider::Github => match repository_kind(item, InboxProvider::Github) {
                Some(kind) => {
                    self.github_work_item_details(&item.project_path, &item.repo, kind, item.number)
                }
                None => rejected("Unknown inbox item"),
            },
        }
    }

    /// The cached comment thread of any item.
    pub fn peek_item_thread(&self, item: &InboxItem) -> Option<WorkItemThread> {
        match item.provider {
            InboxProvider::Linear => {
                self.peek_linear_issue_thread(item.id.as_deref().unwrap_or(""))
            }
            InboxProvider::Jira => self.peek_jira_issue_thread(&jira_key(item)),
            InboxProvider::Gitlab => {
                let kind = repository_kind(item, InboxProvider::Gitlab)?;
                self.peek_gitlab_work_item_thread(&item.repo, kind, item.number)
            }
            InboxProvider::AzureDevops => {
                let kind = repository_kind(item, InboxProvider::AzureDevops)?;
                self.peek_azure_dev_ops_work_item_thread(&item.repo, kind, item.number)
            }
            InboxProvider::Github => {
                let kind = repository_kind(item, InboxProvider::Github)?;
                self.peek_github_work_item_thread(&item.repo, kind, item.number)
            }
        }
    }

    /// Fetch the comment thread of any item. `None` when the item has none
    /// (a GitHub card of an unknown kind).
    pub fn item_thread(&self, item: &InboxItem, force: bool) -> Option<Pending<WorkItemThread>> {
        Some(match item.provider {
            InboxProvider::Linear => {
                self.linear_issue_thread(item.id.as_deref().unwrap_or(""), force)
            }
            InboxProvider::Jira => self.jira_issue_thread(&jira_key(item), force),
            InboxProvider::Gitlab => {
                let kind = repository_kind(item, InboxProvider::Gitlab)?;
                self.gitlab_work_item_thread(&item.repo, kind, item.number, force)
            }
            InboxProvider::AzureDevops => {
                let kind = repository_kind(item, InboxProvider::AzureDevops)?;
                self.azure_dev_ops_work_item_thread(&item.repo, kind, item.number, force)
            }
            InboxProvider::Github => {
                let kind = repository_kind(item, InboxProvider::Github)?;
                self.github_work_item_thread(
                    &item.project_path,
                    &item.repo,
                    kind,
                    item.number,
                    force,
                )
            }
        })
    }

    /// The cached diff of a pull or merge request.
    pub fn peek_item_diff(&self, item: &InboxItem, full_file: bool) -> Option<PrDiff> {
        match item.provider {
            InboxProvider::Gitlab => self.peek_gitlab_mr_diff(&item.repo, item.number),
            InboxProvider::AzureDevops => self.peek_azure_dev_ops_mr_diff(&item.repo, item.number),
            _ => self.peek_github_pr_diff(&item.repo, item.number, full_file),
        }
    }

    /// Fetch the diff of a pull or merge request.
    pub fn item_diff(&self, item: &InboxItem, full_file: bool) -> Pending<PrDiff> {
        match item.provider {
            InboxProvider::Gitlab => self.gitlab_mr_diff(&item.repo, item.number),
            InboxProvider::AzureDevops => self.azure_dev_ops_mr_diff(&item.repo, item.number),
            _ => self.github_pr_diff(&item.project_path, &item.repo, item.number, full_file),
        }
    }

    /// `postComment`: post, then reload the thread. A failed post rejects; a
    /// failed reload resolves with its error.
    pub fn post_item_comment(
        &self,
        item: &InboxItem,
        body: &str,
        reply_to: Option<&InboxReplyTarget>,
    ) -> BoxFuture<'static, Result<PostedComment, String>> {
        let client = self.clone();
        let item = item.clone();
        let post: Pending<String> = match item.provider {
            InboxProvider::Linear => self.linear_issue_comment(
                item.id.as_deref().unwrap_or(""),
                body,
                reply_to.map(|reply| reply.id.as_str()),
            ),
            InboxProvider::Jira => {
                self.jira_issue_comment(item.id.as_deref().unwrap_or(""), &jira_key(&item), body)
            }
            InboxProvider::Gitlab => match repository_kind(&item, InboxProvider::Gitlab) {
                Some(kind) => self.gitlab_work_item_comment(&item.repo, kind, item.number, body),
                None => rejected("Unknown inbox item"),
            },
            InboxProvider::AzureDevops => {
                match repository_kind(&item, InboxProvider::AzureDevops) {
                    Some(kind) => {
                        self.azure_dev_ops_work_item_comment(&item.repo, kind, item.number, body)
                    }
                    None => rejected("Unknown inbox item"),
                }
            }
            InboxProvider::Github => match repository_kind(&item, InboxProvider::Github) {
                Some(kind) => self.github_work_item_comment(
                    &item.project_path,
                    &item.repo,
                    kind,
                    item.number,
                    body,
                    reply_to.map(|reply| reply.thread_id.as_str()),
                ),
                None => rejected("Unknown inbox item"),
            },
        };
        async move {
            post.await?;
            let Some(reload) = client.item_thread(&item, true) else {
                return Ok(PostedComment::default());
            };
            Ok(match reload.await {
                Ok(thread) => PostedComment {
                    thread: Some(thread),
                    reload_error: None,
                },
                Err(error) => PostedComment {
                    thread: None,
                    reload_error: Some(error),
                },
            })
        }
        .boxed()
    }
}

/// One loadable value: what is on screen, whether a load runs, and its
/// error.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loadable<T> {
    pub value: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
}

/// The detail pane of one inbox item.
pub struct InboxItemDetail {
    client: InboxClient,
    item: InboxItem,
    revision: u64,
    details: Loadable<WorkItemDetails>,
    thread: Loadable<WorkItemThread>,
    diff: Loadable<PrDiff>,
    diff_full_file: Option<bool>,
    reply_to: Option<InboxReplyTarget>,
    posting: bool,
    post_error: Option<String>,
    action_busy: bool,
    action_error: Option<String>,
    action_notice: Option<String>,
}

/// The fields that pick which item the pane loads.
fn identity(
    item: &InboxItem,
) -> (
    InboxProvider,
    super::types::InboxKind,
    Option<String>,
    Option<String>,
    i64,
    String,
    String,
) {
    (
        item.provider,
        item.kind,
        item.id.clone(),
        item.identifier.clone(),
        item.number,
        item.project_path.clone(),
        item.repo.clone(),
    )
}

impl InboxItemDetail {
    pub fn new(client: InboxClient, item: InboxItem, cx: &mut Context<Self>) -> Self {
        let mut detail = Self {
            client,
            item,
            revision: 0,
            details: Loadable::default(),
            thread: Loadable::default(),
            diff: Loadable::default(),
            diff_full_file: None,
            reply_to: None,
            posting: false,
            post_error: None,
            action_busy: false,
            action_error: None,
            action_notice: None,
        };
        detail.load(cx);
        detail
    }

    pub fn item(&self) -> &InboxItem {
        &self.item
    }

    pub fn details(&self) -> &Loadable<WorkItemDetails> {
        &self.details
    }

    pub fn thread(&self) -> &Loadable<WorkItemThread> {
        &self.thread
    }

    pub fn diff(&self) -> &Loadable<PrDiff> {
        &self.diff
    }

    pub fn reply_to(&self) -> Option<&InboxReplyTarget> {
        self.reply_to.as_ref()
    }

    pub fn posting(&self) -> bool {
        self.posting
    }

    pub fn post_error(&self) -> Option<&str> {
        self.post_error.as_deref()
    }

    pub fn action_busy(&self) -> bool {
        self.action_busy
    }

    pub fn action_error(&self) -> Option<&str> {
        self.action_error.as_deref()
    }

    /// "Merge queued or auto-merge enabled." after a merge that did not land.
    pub fn action_notice(&self) -> Option<&str> {
        self.action_notice.as_deref()
    }

    /// `details.reviewDecision || thread.reviewDecision`.
    pub fn review_decision(&self) -> String {
        let from_details = self
            .details
            .value
            .as_ref()
            .and_then(|details| details.review_decision.as_deref())
            .map(monocode_core::js::trim)
            .unwrap_or("");
        if !from_details.is_empty() {
            return from_details.to_string();
        }
        self.thread
            .value
            .as_ref()
            .map(|thread| monocode_core::js::trim(&thread.review_decision).to_string())
            .unwrap_or_default()
    }

    /// Show another item, or the same one with fresh fields. A different
    /// identity reloads everything.
    pub fn set_item(&mut self, item: InboxItem, cx: &mut Context<Self>) {
        let reload = identity(&item) != identity(&self.item);
        self.item = item;
        if reload {
            self.diff = Loadable::default();
            self.diff_full_file = None;
            self.load(cx);
        }
        cx.notify();
    }

    /// The pane's `revision` prop: reload the description, thread, and an
    /// open diff.
    pub fn set_revision(&mut self, revision: u64, cx: &mut Context<Self>) {
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        self.load(cx);
        if let Some(full_file) = self.diff_full_file {
            self.load_diff_now(full_file, cx);
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let cached = self.client.peek_item_details(&self.item);
        let pending = self.client.item_details(&self.item);
        start_load(&mut self.details, cached, Some(pending), cx, |this| {
            &mut this.details
        });
        let cached = self.client.peek_item_thread(&self.item);
        let pending = self.client.item_thread(&self.item, false);
        start_load(&mut self.thread, cached, pending, cx, |this| {
            &mut this.thread
        });
    }

    /// The Code tab is showing: load the diff, with full files or hunks.
    pub fn show_diff(&mut self, full_file: bool, cx: &mut Context<Self>) {
        let is_pr = !self.item.is_tracker() && self.item.kind == super::types::InboxKind::Pr;
        if !is_pr {
            return;
        }
        let full_file = inbox_shows_full_file_diff(&self.item) && full_file;
        if self.diff_full_file == Some(full_file) {
            return;
        }
        self.load_diff_now(full_file, cx);
    }

    fn load_diff_now(&mut self, full_file: bool, cx: &mut Context<Self>) {
        self.diff_full_file = Some(full_file);
        let cached = self.client.peek_item_diff(&self.item, full_file);
        let pending = self.client.item_diff(&self.item, full_file);
        start_load(&mut self.diff, cached, Some(pending), cx, |this| {
            &mut this.diff
        });
    }

    /// Reply to a comment, or `None` for a new top-level comment.
    pub fn set_reply_to(&mut self, reply_to: Option<InboxReplyTarget>, cx: &mut Context<Self>) {
        self.reply_to = reply_to;
        cx.notify();
    }

    /// `postComment`.
    pub fn post_comment(&mut self, body: &str, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        self.posting = true;
        self.post_error = None;
        cx.notify();
        let post = self
            .client
            .post_item_comment(&self.item, body, self.reply_to.as_ref());
        cx.spawn(async move |this, cx| {
            let result = post.await;
            this.update(cx, |this, cx| {
                this.posting = false;
                let outcome = match result {
                    Ok(PostedComment {
                        thread,
                        reload_error,
                    }) => {
                        this.reply_to = None;
                        if let Some(thread) = thread {
                            this.thread.value = Some(thread);
                        }
                        this.post_error = reload_error;
                        Ok(())
                    }
                    Err(error) => {
                        this.post_error = Some(error.clone());
                        Err(error)
                    }
                };
                cx.notify();
                outcome
            })
            .map_err(|error| error.to_string())?
        })
    }

    /// `runAction`: merge or change a GitHub pull request. Resolves with the
    /// updated card for the list.
    pub fn run_pr_action(
        &mut self,
        action: GithubPrAction,
        cx: &mut Context<Self>,
    ) -> Task<Result<InboxItem, String>> {
        if self.action_busy {
            return Task::ready(Err("An action is already running".into()));
        }
        self.action_busy = true;
        self.action_error = None;
        self.action_notice = None;
        cx.notify();
        let pending = self.client.github_pr_action(
            &self.item.project_path,
            &self.item.repo,
            self.item.number,
            action,
        );
        cx.spawn(async move |this, cx| {
            let result = pending.await;
            this.update(cx, |this, cx| {
                this.action_busy = false;
                let outcome = match result {
                    Ok(next) => {
                        let merging = matches!(
                            action,
                            GithubPrAction::Merge | GithubPrAction::Squash | GithubPrAction::Rebase
                        );
                        this.action_notice = (merging
                            && monocode_core::js::trim(&next.state).to_lowercase() != "merged")
                            .then(|| "Merge queued or auto-merge enabled.".to_string());
                        let mut updated = this.item.merge_github(&next);
                        updated.provider = InboxProvider::Github;
                        this.item = updated.clone();
                        Ok(updated)
                    }
                    Err(error) => {
                        this.action_error = Some(error.clone());
                        Err(error)
                    }
                };
                cx.notify();
                outcome
            })
            .map_err(|error| error.to_string())?
        })
    }
}

/// The load pattern every detail effect shares: show a cached value at once,
/// keep it when a refresh fails, and ignore answers a newer load replaced.
fn start_load<T: Clone + Send + Sync + 'static>(
    slot: &mut Loadable<T>,
    cached: Option<T>,
    pending: Option<Pending<T>>,
    cx: &mut Context<InboxItemDetail>,
    select: fn(&mut InboxItemDetail) -> &mut Loadable<T>,
) {
    slot.generation += 1;
    let generation = slot.generation;
    let had_cache = cached.is_some();
    match cached {
        Some(value) => {
            slot.value = Some(value);
            slot.loading = false;
            slot.error = None;
        }
        None => {
            slot.value = None;
            slot.loading = pending.is_some();
            slot.error = None;
        }
    }
    cx.notify();
    let Some(pending) = pending else {
        return;
    };
    cx.spawn(async move |this, cx| {
        let result = pending.await;
        let _ = this.update(cx, |this, cx| {
            let slot = select(this);
            if slot.generation != generation {
                return;
            }
            match result {
                Ok(value) => {
                    slot.value = Some(value);
                    slot.error = None;
                }
                Err(error) if !had_cache => slot.error = Some(error),
                Err(_) => {}
            }
            slot.loading = false;
            cx.notify();
        });
    })
    .detach();
}
