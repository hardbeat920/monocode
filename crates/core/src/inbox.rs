//! Port of the inbox types a session refers to, from
//! src/features/inbox/model/githubTasks.ts, inboxAsk.ts, and
//! linkedWorkItemActivity.ts.

use serde::{Deserialize, Serialize};

use crate::block::Extra;

/// `InboxProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InboxProvider {
    #[serde(rename = "github")]
    Github,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "jira")]
    Jira,
    #[serde(rename = "gitlab")]
    Gitlab,
    #[serde(rename = "azuredevops")]
    AzureDevops,
}

/// `InboxKind`: a GitHub issue or pull request, or a tracker issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InboxKind {
    #[serde(rename = "issue")]
    Issue,
    #[serde(rename = "pr")]
    Pr,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "jira")]
    Jira,
}

/// `GithubTaskKind`, also the kind of a `LinkedWorkItem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkItemKind {
    #[serde(rename = "issue")]
    Issue,
    #[serde(rename = "pr")]
    Pr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubLabel {
    pub name: String,
    pub color: String,
}

/// Inbox issue or pull request chip shown above the composer. In-memory and
/// one-shot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxComposerCard {
    pub provider: InboxProvider,
    pub kind: InboxKind,
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub labels: Vec<GithubLabel>,
    pub prompt: String,
}

/// `composeInboxMessage`: the card's prompt followed by the user's note.
pub fn compose_inbox_message(card: Option<&InboxComposerCard>, text: &str) -> String {
    let prompt = card.map(|card| crate::js::trim(&card.prompt)).unwrap_or("");
    let note = crate::js::trim(text);
    if prompt.is_empty() {
        return note.to_string();
    }
    if note.is_empty() {
        return prompt.to_string();
    }
    format!("{prompt}\n\n{note}")
}

/// A temporary Inbox conversation. Stored in the `sessions.inbox_ask` column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxAskContext {
    pub key: String,
    pub title: String,
    pub url: String,
    pub provider: InboxProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkedWorkItemActivityKind {
    #[serde(rename = "comment")]
    Comment,
    #[serde(rename = "review")]
    Review,
    #[serde(rename = "review_comment")]
    ReviewComment,
    #[serde(rename = "commit")]
    Commit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedWorkItemActivityEntry {
    pub id: String,
    pub kind: LinkedWorkItemActivityKind,
    pub author: String,
    pub text: String,
    pub created_at: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedWorkItemActivityCounts {
    pub comments: i64,
    pub reviews: i64,
    pub commits: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkedWorkItemUpdateStatus {
    #[serde(rename = "loading")]
    Loading,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "error")]
    Error,
}

/// New linked-item activity shown when an updated linked session is opened.
/// In-memory and one-shot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedWorkItemUpdateCard {
    pub kind: WorkItemKind,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub since: i64,
    pub updated_at: i64,
    pub status: LinkedWorkItemUpdateStatus,
    pub counts: LinkedWorkItemActivityCounts,
    pub entries: Vec<LinkedWorkItemActivityEntry>,
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_the_card_prompt_and_note() {
        let card = InboxComposerCard {
            provider: InboxProvider::Github,
            kind: InboxKind::Issue,
            identifier: "#1".into(),
            title: "Bug".into(),
            url: "https://x".into(),
            source: "o/r".into(),
            labels: vec![],
            prompt: "Fix #1 ".into(),
        };
        assert_eq!(compose_inbox_message(Some(&card), " now "), "Fix #1\n\nnow");
        assert_eq!(compose_inbox_message(Some(&card), ""), "Fix #1");
        assert_eq!(compose_inbox_message(None, " x "), "x");
    }

    #[test]
    fn provider_ids_match_the_typescript() {
        assert_eq!(
            serde_json::to_string(&InboxProvider::AzureDevops).unwrap(),
            "\"azuredevops\""
        );
    }
}
