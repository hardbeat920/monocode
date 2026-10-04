//! The inbox's side of the Tauri boundary. The TypeScript models called
//! `invoke(command, args)`; `InboxBackend::invoke` keeps the same command
//! names and JSON arguments, so the ported tests can assert the exact calls.
//! `LiveInboxBackend` runs each command against monocode-git (the `gh`
//! calls) and monocode-integrations (GitLab, Azure DevOps, Jira, Linear,
//! inbox media).

use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::{Value, json};

/// One blocking command per call, returned as a future so the caller picks
/// the thread. A fake can hold the future open to test races.
pub trait InboxBackend: Send + Sync {
    /// `invoke(command, args)`: the command's JSON result, or its error text.
    fn invoke(&self, command: &str, args: Value) -> BoxFuture<'static, Result<Value, String>>;

    /// `invoke<ArrayBuffer>("fetch_inbox_media", { url })`.
    fn fetch_media(&self, url: &str) -> BoxFuture<'static, Result<Vec<u8>, String>>;
}

/// The commands against the real `gh` CLI and tracker APIs. Tracker
/// credentials live under `data_dir`, as in the Tauri app.
pub struct LiveInboxBackend {
    data_dir: PathBuf,
}

impl LiveInboxBackend {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self { data_dir })
    }
}

impl InboxBackend for LiveInboxBackend {
    fn invoke(&self, command: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        let data_dir = self.data_dir.clone();
        let command = command.to_string();
        Box::pin(async move { dispatch(&data_dir, &command, &args) })
    }

    fn fetch_media(&self, url: &str) -> BoxFuture<'static, Result<Vec<u8>, String>> {
        let url = url.to_string();
        Box::pin(async move { monocode_integrations::inbox_media::fetch_inbox_media(url) })
    }
}

fn to_json<T: Serialize>(value: Result<T, String>) -> Result<Value, String> {
    value.and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
}

fn string(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn number(args: &Value, key: &str) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or_default()
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn limit(args: &Value) -> Option<u32> {
    args.get("limit")
        .and_then(Value::as_u64)
        .and_then(|limit| u32::try_from(limit).ok())
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn dispatch(data_dir: &std::path::Path, command: &str, args: &Value) -> Result<Value, String> {
    use monocode_git::fs as git;
    use monocode_integrations::{azure_devops as ado, gitlab, jira, linear};
    let a = args;
    match command {
        "git_github_status" => to_json(Ok(git::git_github_status())),
        "github_monocode_star_status" => to_json(Ok(git::github_monocode_star_status())),
        "github_star_monocode" => git::github_star_monocode().map(|()| Value::Null),
        "git_github_repo" => to_json(git::git_github_repo(string(a, "cwd"))),
        "git_github_repositories" => to_json(git::git_github_repositories(string(a, "cwd"))),
        "git_github_work_items" => to_json(git::git_github_work_items(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "kind"),
            flag(a, "assignedToMe"),
            string(a, "state"),
            string(a, "search"),
            limit(a),
        )),
        "git_github_work_item" => to_json(git::git_github_work_item(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "git_github_work_item_details" => to_json(git::git_github_work_item_details(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "git_github_work_item_thread" => to_json(git::git_github_work_item_thread(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "git_github_work_item_comment" => to_json(git::git_github_work_item_comment(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
            string(a, "body"),
            string(a, "inReplyTo"),
        )),
        "git_github_pr_action" => to_json(git::git_github_pr_action(
            string(a, "cwd"),
            string(a, "repo"),
            number(a, "number"),
            string(a, "action"),
        )),
        "git_github_pr_diff" => to_json(git::git_github_pr_diff(
            string(a, "cwd"),
            string(a, "repo"),
            number(a, "number"),
            Some(flag(a, "fullContext")),
        )),
        "git_github_pr_checks" => to_json(git::git_github_pr_checks(
            string(a, "cwd"),
            string(a, "repo"),
            number(a, "number"),
        )),
        "git_github_check_details" => to_json(git::git_github_check_details(
            string(a, "cwd"),
            string(a, "repo"),
            string(a, "jobId"),
        )),
        "git_pr_status" => to_json(git::git_pr_status(string(a, "cwd"))),

        "gitlab_status" => to_json(gitlab::gitlab_status(data_dir)),
        "gitlab_set_config" => to_json(gitlab::gitlab_set_config(
            data_dir,
            string(a, "url"),
            string(a, "token"),
        )),
        "gitlab_repo" => to_json(gitlab::gitlab_repo(data_dir, string(a, "cwd"))),
        "gitlab_list_work_items" => to_json(gitlab::gitlab_list_work_items(
            data_dir,
            string(a, "cwd"),
            string(a, "kind"),
            flag(a, "assignedToMe"),
            string(a, "state"),
            limit(a),
        )),
        "gitlab_list_todos" => to_json(gitlab::gitlab_list_todos(
            data_dir,
            string(a, "kind"),
            limit(a),
        )),
        "gitlab_work_item_details" => to_json(gitlab::gitlab_work_item_details(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "gitlab_work_item_thread" => to_json(gitlab::gitlab_work_item_thread(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "gitlab_work_item_comment" => to_json(gitlab::gitlab_work_item_comment(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
            string(a, "body"),
        )),
        "gitlab_mr_diff" => to_json(gitlab::gitlab_mr_diff(
            data_dir,
            string(a, "repo"),
            number(a, "number"),
        )),

        "azure_devops_status" => to_json(ado::azure_devops_status(data_dir)),
        "azure_devops_set_config" => to_json(ado::azure_devops_set_config(
            data_dir,
            string(a, "url"),
            string(a, "token"),
        )),
        "azure_devops_repo" => to_json(ado::azure_devops_repo(data_dir, string(a, "cwd"))),
        "azure_devops_list_work_items" => to_json(ado::azure_devops_list_work_items(
            data_dir,
            string(a, "cwd"),
            string(a, "kind"),
            flag(a, "assignedToMe"),
            string(a, "state"),
            limit(a),
        )),
        "azure_devops_list_todos" => to_json(ado::azure_devops_list_todos(
            data_dir,
            string(a, "kind"),
            limit(a),
        )),
        "azure_devops_work_item_details" => to_json(ado::azure_devops_work_item_details(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "azure_devops_work_item_thread" => to_json(ado::azure_devops_work_item_thread(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
        )),
        "azure_devops_work_item_comment" => to_json(ado::azure_devops_work_item_comment(
            data_dir,
            string(a, "repo"),
            string(a, "kind"),
            number(a, "number"),
            string(a, "body"),
        )),
        "azure_devops_mr_diff" => to_json(ado::azure_devops_mr_diff(
            data_dir,
            string(a, "repo"),
            number(a, "number"),
        )),

        "jira_status" => to_json(jira::jira_status(data_dir)),
        "jira_set_config" => to_json(jira::jira_set_config(
            data_dir,
            string(a, "site"),
            string(a, "email"),
            string(a, "token"),
        )),
        "jira_list_projects" => to_json(jira::jira_list_projects(data_dir)),
        "jira_list_issues" => to_json(jira::jira_list_issues(
            data_dir,
            flag(a, "assignedToMe"),
            string(a, "state"),
            strings(a, "projectIds"),
            limit(a),
        )),
        "jira_issue_details" => to_json(jira::jira_issue_details(data_dir, string(a, "key"))),
        "jira_issue_thread" => to_json(jira::jira_issue_thread(data_dir, string(a, "key"))),
        "jira_issue_comment" => to_json(jira::jira_issue_comment(
            data_dir,
            string(a, "key"),
            string(a, "body"),
        )),

        "linear_status" => to_json(linear::linear_status(data_dir)),
        "linear_set_token" => to_json(linear::linear_set_token(data_dir, string(a, "token"))),
        "linear_list_teams" => to_json(linear::linear_list_teams(data_dir)),
        "linear_list_issues" => to_json(linear::linear_list_issues(
            data_dir,
            flag(a, "assignedToMe"),
            string(a, "state"),
            strings(a, "teamIds"),
            limit(a),
        )),
        "linear_issue_details" => to_json(linear::linear_issue_details(data_dir, string(a, "id"))),
        "linear_issue_thread" => to_json(linear::linear_issue_thread(data_dir, string(a, "id"))),
        "linear_issue_comment" => to_json(linear::linear_issue_comment(
            data_dir,
            string(a, "id"),
            string(a, "body"),
            string(a, "parentId"),
        )),

        _ => Err(format!("Unknown inbox command: {command}")),
    }
}

/// `{ ...args, limit: undefined }` drops the key, as `invoke` would.
pub(crate) fn args_with_limit(mut args: Value, limit: Option<u32>) -> Value {
    if let (Some(limit), Some(object)) = (limit, args.as_object_mut()) {
        object.insert("limit".into(), json!(limit));
    }
    args
}

#[cfg(test)]
pub(crate) mod fake {
    //! A scripted backend: a handler answers each command, and every call is
    //! recorded with its arguments, like the `vi.mocked(invoke)` mocks.

    use std::collections::VecDeque;
    use std::sync::Arc;

    use futures::channel::oneshot;
    use futures::future::BoxFuture;
    use parking_lot::Mutex;
    use serde_json::Value;

    use super::InboxBackend;

    type Handler = Box<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;
    type MediaHandler = Box<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

    /// A held answer: `release` (or drop) lets the call finish with the
    /// handler's value, `resolve` and `reject` choose the answer.
    pub struct Deferred {
        sender: Option<oneshot::Sender<Option<Result<Value, String>>>>,
    }

    impl Deferred {
        pub fn resolve(mut self, value: Value) {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(Some(Ok(value)));
            }
        }

        pub fn reject(mut self, error: &str) {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(Some(Err(error.to_string())));
            }
        }
    }

    impl Drop for Deferred {
        fn drop(&mut self) {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(None);
            }
        }
    }

    struct Hold {
        command: String,
        receiver: oneshot::Receiver<Option<Result<Value, String>>>,
    }

    pub struct FakeBackend {
        handler: Mutex<Handler>,
        calls: Mutex<Vec<(String, Value)>>,
        holds: Mutex<VecDeque<Hold>>,
        media: Mutex<Vec<String>>,
        media_handler: Mutex<Option<MediaHandler>>,
    }

    impl FakeBackend {
        pub fn new(
            handler: impl Fn(&str, &Value) -> Result<Value, String> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                handler: Mutex::new(Box::new(handler)),
                calls: Mutex::new(Vec::new()),
                holds: Mutex::new(VecDeque::new()),
                media: Mutex::new(Vec::new()),
                media_handler: Mutex::new(None),
            })
        }

        /// Answer media fetches with raw bytes instead of the handler's
        /// JSON, for files too large to script as JSON arrays.
        pub fn set_media(
            &self,
            handler: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
        ) {
            *self.media_handler.lock() = Some(Box::new(handler));
        }

        /// Hold the next call to `command` until the returned answer settles.
        pub fn hold_next(&self, command: &str) -> Deferred {
            let (sender, receiver) = oneshot::channel();
            self.holds.lock().push_back(Hold {
                command: command.to_string(),
                receiver,
            });
            Deferred {
                sender: Some(sender),
            }
        }

        pub fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().clone()
        }

        /// Arguments of every call to `command`, in order.
        pub fn calls_to(&self, command: &str) -> Vec<Value> {
            self.calls
                .lock()
                .iter()
                .filter(|(name, _)| name == command)
                .map(|(_, args)| args.clone())
                .collect()
        }

        pub fn count(&self, command: &str) -> usize {
            self.calls_to(command).len()
        }

        pub fn clear_calls(&self) {
            self.calls.lock().clear();
        }

        pub fn media_calls(&self) -> Vec<String> {
            self.media.lock().clone()
        }
    }

    impl InboxBackend for FakeBackend {
        fn invoke(&self, command: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
            self.calls.lock().push((command.to_string(), args.clone()));
            let answer = (self.handler.lock())(command, &args);
            let hold = {
                let mut holds = self.holds.lock();
                let index = holds.iter().position(|hold| hold.command == command);
                index.and_then(|index| holds.remove(index))
            };
            match hold {
                None => Box::pin(async move { answer }),
                Some(hold) => Box::pin(async move {
                    match hold.receiver.await {
                        Ok(Some(chosen)) => chosen,
                        _ => answer,
                    }
                }),
            }
        }

        fn fetch_media(&self, url: &str) -> BoxFuture<'static, Result<Vec<u8>, String>> {
            self.media.lock().push(url.to_string());
            if let Some(handler) = self.media_handler.lock().as_ref() {
                let answer = handler(url);
                return Box::pin(async move { answer });
            }
            let answer =
                (self.handler.lock())("fetch_inbox_media", &serde_json::json!({ "url": url }));
            Box::pin(async move {
                answer.and_then(|value| {
                    serde_json::from_value::<Vec<u8>>(value).map_err(|error| error.to_string())
                })
            })
        }
    }
}
