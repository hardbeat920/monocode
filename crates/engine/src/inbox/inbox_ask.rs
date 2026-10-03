//! Port of src/features/inbox/model/inboxAsk.ts: the key of a temporary
//! Inbox conversation and the prompt wrapper that keeps it read-only.
//!
//! The instructions are the same markdown file the TypeScript bundled.
// TODO(port): src/ goes away at cutover. Move src/instructions/inbox.md next
// to this module then.

use monocode_core::inbox::InboxAskContext;
use monocode_core::js;

use super::types::{InboxItem, kind_str, provider_str};

/// The bundled discussion instructions.
pub const INBOX_INSTRUCTIONS: &str = include_str!("../../../../src/instructions/inbox.md");

/// `url.host`: the host name plus a non-default port.
fn url_host(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("");
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// `inboxAskKey`: one conversation per item, whatever local project or URL
/// fragment it was opened from.
pub fn inbox_ask_key(item: &InboxItem) -> String {
    let provider = provider_str(item.provider);
    if item.is_tracker() {
        // A template string prints a missing id as "undefined".
        return format!("{provider}:{}", item.id.as_deref().unwrap_or("undefined"));
    }
    // Items without a usable link must still produce a stable key.
    match url::Url::parse(&item.url) {
        Ok(url) => {
            let path = url.path();
            let path = path.strip_suffix('/').unwrap_or(path);
            format!(
                "{provider}:{}:{}",
                url_host(&url).to_lowercase(),
                path.to_lowercase()
            )
        }
        Err(_) => {
            let repo = js::trim(&item.repo).to_lowercase();
            let repo = if repo.is_empty() {
                "unknown".to_string()
            } else {
                repo
            };
            format!("{provider}:{repo}:{}:{}", kind_str(item.kind), item.number)
        }
    }
}

/// `inboxAskPrompt`: the instructions and the item as reference data ahead
/// of the user's message. Ordinary sessions pass `None` and keep the text.
pub fn inbox_ask_prompt(context: Option<&InboxAskContext>, text: &str) -> String {
    let Some(context) = context else {
        return text.to_string();
    };
    format!(
        "{}\n\nINBOX ITEM (reference data):\n{}\n\nUSER MESSAGE:\n{}",
        js::trim(INBOX_INSTRUCTIONS),
        serde_json::to_string(context).unwrap_or_default(),
        text
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::github_tasks::test_items::{item, with};
    use crate::inbox::types::{InboxKind, InboxProvider};

    fn context() -> InboxAskContext {
        InboxAskContext {
            key: "github:github.com:/acme/app/pull/42".into(),
            title: "Fix login".into(),
            url: "https://github.com/acme/app/pull/42".into(),
            provider: InboxProvider::Github,
            description: None,
            extra: Default::default(),
        }
    }

    fn github(url: &str) -> InboxItem {
        with(item(42, "2026-09-13T12:00:00Z"), |row| row.url = url.into())
    }

    #[test]
    fn identifies_the_item_independently_of_its_local_project_and_url_fragment() {
        let url = context().url;
        let key = inbox_ask_key(&github(&url));
        assert_eq!(key, "github:github.com:/acme/app/pull/42");
        let other = with(github(&format!("{url}/#discussion")), |row| {
            row.project_path = "/other".into()
        });
        assert_eq!(inbox_ask_key(&other), key);
        assert_ne!(
            inbox_ask_key(&github("https://git.example.com/acme/app/pull/42")),
            key
        );
        let linear = with(item(1, ""), |row| {
            row.provider = InboxProvider::Linear;
            row.id = Some("issue-uuid".into());
        });
        assert_eq!(inbox_ask_key(&linear), "linear:issue-uuid");
        let gitlab = with(
            github("https://gitlab.example.com/acme/app/-/merge_requests/42"),
            |row| {
                row.provider = InboxProvider::Gitlab;
            },
        );
        assert_eq!(
            inbox_ask_key(&gitlab),
            "gitlab:gitlab.example.com:/acme/app/-/merge_requests/42"
        );
    }

    #[test]
    fn falls_back_to_a_stable_key_when_the_item_has_no_usable_link() {
        let ado = with(item(12, ""), |row| {
            row.provider = InboxProvider::AzureDevops;
            row.kind = InboxKind::Pr;
            row.repo = "platform/web".into();
            row.url = String::new();
        });
        assert_eq!(inbox_ask_key(&ado), "azuredevops:platform/web:pr:12");
        let bad = with(ado, |row| row.url = "not a url".into());
        assert_eq!(inbox_ask_key(&bad), "azuredevops:platform/web:pr:12");
    }

    #[test]
    fn adds_the_remote_access_instruction_on_follow_ups_and_leaves_ordinary_sessions_alone() {
        assert_eq!(inbox_ask_prompt(None, "Change the file"), "Change the file");
        for text in ["Explain the PR", "What about tests?"] {
            let prompt = inbox_ask_prompt(Some(&context()), text);
            assert!(prompt.contains(&context().url));
            assert!(prompt.contains("Do not clone repositories"));
            assert!(prompt.contains("fetch PR branches into local git"));
            assert!(prompt.contains("gh pr diff"));
            assert!(prompt.ends_with(text));
        }
        assert!(inbox_ask_prompt(Some(&context()), "x").contains(
            r#"INBOX ITEM (reference data):
{"key":"github:github.com:/acme/app/pull/42","title":"Fix login","url":"https://github.com/acme/app/pull/42","provider":"github"}"#
        ));
    }
}
