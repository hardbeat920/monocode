//! Port of src/features/inbox/model/linkedWorkItemActivity.ts: the one-shot
//! card above the composer that lists what changed on a linked GitHub item
//! since the session last looked, and the prompt that asks the agent to act
//! on it.

use monocode_core::inbox::{
    LinkedWorkItemActivityCounts, LinkedWorkItemActivityEntry, LinkedWorkItemActivityKind,
    LinkedWorkItemUpdateCard, LinkedWorkItemUpdateStatus,
};

use super::github_tasks::github_review_state_label;
use super::linked_session_updates::LinkedSessionUpdate;
use super::text::{clip, one_line};
use super::time::{date_parse, date_parse_or_zero};
use super::types::{WorkItemComment, WorkItemKind, WorkItemThread};

/// `LinkedWorkItemTerminalState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkedWorkItemTerminalState {
    IssueClosed,
    PrMerged,
    PrClosed,
}

/// `pendingLinkedWorkItemUpdateCard`.
pub fn pending_linked_work_item_update_card(
    update: &LinkedSessionUpdate,
) -> LinkedWorkItemUpdateCard {
    LinkedWorkItemUpdateCard {
        kind: update.item.kind,
        repo: update.item.repo.clone(),
        number: update.item.number,
        title: update.item.title.clone(),
        url: update.item.url.clone(),
        state: update.item.state.clone(),
        since: update.since,
        updated_at: update.updated_at,
        status: LinkedWorkItemUpdateStatus::Loading,
        counts: LinkedWorkItemActivityCounts::default(),
        entries: Vec::new(),
        truncated: false,
    }
}

/// `concise`: one line, at most 140 UTF-16 units.
fn concise(value: &str) -> String {
    clip(&one_line(value), 140)
}

fn after(timestamp: &str, since: i64) -> bool {
    date_parse(timestamp).is_some_and(|value| value > since)
}

fn flatten_comments(comments: &[WorkItemComment]) -> Vec<&WorkItemComment> {
    comments
        .iter()
        .flat_map(|comment| std::iter::once(comment).chain(flatten_comments(&comment.replies)))
        .collect()
}

/// `completeLinkedWorkItemUpdateCard`: the comments, reviews, and commits
/// newer than the card's baseline, newest first.
pub fn complete_linked_work_item_update_card(
    card: &LinkedWorkItemUpdateCard,
    thread: &WorkItemThread,
) -> LinkedWorkItemUpdateCard {
    let comments: Vec<&WorkItemComment> = flatten_comments(&thread.comments)
        .into_iter()
        .filter(|comment| after(&comment.created_at, card.since))
        .collect();
    let commits: Vec<_> = thread
        .commits
        .iter()
        .filter(|commit| after(&commit.committed_date, card.since))
        .collect();
    let mut entries: Vec<LinkedWorkItemActivityEntry> = comments
        .iter()
        .map(|comment| {
            let kind = match comment.kind.as_str() {
                "review" => LinkedWorkItemActivityKind::Review,
                "review_comment" => LinkedWorkItemActivityKind::ReviewComment,
                _ => LinkedWorkItemActivityKind::Comment,
            };
            let text = if comment.kind == "review" {
                let label = github_review_state_label(&comment.state);
                let parts: Vec<&str> = [label, comment.body.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect();
                concise(&parts.join(": "))
            } else {
                concise(&comment.body)
            };
            LinkedWorkItemActivityEntry {
                id: comment.id.clone(),
                kind,
                author: comment.author.clone(),
                text,
                created_at: comment.created_at.clone(),
                url: comment.url.clone(),
            }
        })
        .chain(commits.iter().map(|commit| LinkedWorkItemActivityEntry {
            id: commit.oid.clone(),
            kind: LinkedWorkItemActivityKind::Commit,
            author: commit.author.clone(),
            text: concise(&commit.message_headline),
            created_at: commit.committed_date.clone(),
            url: commit.url.clone(),
        }))
        .collect();
    entries.sort_by(|left, right| {
        date_parse_or_zero(&right.created_at).cmp(&date_parse_or_zero(&left.created_at))
    });
    let reviews = comments
        .iter()
        .filter(|comment| comment.kind == "review")
        .count() as i64;
    LinkedWorkItemUpdateCard {
        status: LinkedWorkItemUpdateStatus::Ready,
        counts: LinkedWorkItemActivityCounts {
            comments: comments.len() as i64 - reviews,
            reviews,
            commits: commits.len() as i64,
        },
        entries,
        truncated: thread.truncated,
        ..card.clone()
    }
}

/// `failLinkedWorkItemUpdateCard`.
pub fn fail_linked_work_item_update_card(
    card: &LinkedWorkItemUpdateCard,
) -> LinkedWorkItemUpdateCard {
    LinkedWorkItemUpdateCard {
        status: LinkedWorkItemUpdateStatus::Error,
        ..card.clone()
    }
}

/// `linkedWorkItemTerminalState`: a state that usually means the session
/// can be cleaned up.
pub fn linked_work_item_terminal_state(
    kind: WorkItemKind,
    state: &str,
) -> Option<LinkedWorkItemTerminalState> {
    let state = monocode_core::js::trim(state).to_lowercase();
    if kind == WorkItemKind::Issue {
        return (state == "closed").then_some(LinkedWorkItemTerminalState::IssueClosed);
    }
    match state.as_str() {
        "merged" => Some(LinkedWorkItemTerminalState::PrMerged),
        "closed" => Some(LinkedWorkItemTerminalState::PrClosed),
        _ => None,
    }
}

fn count_label(count: i64, singular: &str) -> String {
    format!("{count} {singular}{}", if count == 1 { "" } else { "s" })
}

/// `linkedWorkItemUpdateSummary`.
pub fn linked_work_item_update_summary(card: &LinkedWorkItemUpdateCard) -> String {
    let parts: Vec<String> = [
        (card.counts.commits, "new commit"),
        (card.counts.reviews, "new review"),
        (card.counts.comments, "new comment"),
    ]
    .into_iter()
    .filter(|(count, _)| *count != 0)
    .map(|(count, singular)| count_label(count, singular))
    .collect();
    if !parts.is_empty() {
        return parts.join(" · ");
    }
    match card.status {
        LinkedWorkItemUpdateStatus::Loading => "Loading change details…".into(),
        LinkedWorkItemUpdateStatus::Error => "Updated on GitHub · details unavailable".into(),
        LinkedWorkItemUpdateStatus::Ready => "Metadata or status changed".into(),
    }
}

/// `linkedWorkItemActivityPrompt`: the message that asks the agent to act
/// on the new activity.
pub fn linked_work_item_activity_prompt(card: &LinkedWorkItemUpdateCard) -> String {
    let kind = if card.kind == WorkItemKind::Pr {
        "pull request"
    } else {
        "issue"
    };
    let instruction = match card.entries.first() {
        Some(latest) if latest.kind == LinkedWorkItemActivityKind::Commit => {
            "Review the new commit and continue the work where needed."
        }
        Some(_) => "Address the new feedback and continue the work where needed.",
        None => "Review the latest update and continue the work where needed.",
    };
    let mut details: Vec<String> = card
        .entries
        .iter()
        .take(12)
        .map(|entry| {
            let actor = if entry.author.is_empty() {
                String::new()
            } else {
                format!(" by @{}", entry.author)
            };
            let kind = match entry.kind {
                LinkedWorkItemActivityKind::ReviewComment => "review comment",
                LinkedWorkItemActivityKind::Review => "review",
                LinkedWorkItemActivityKind::Comment => "comment",
                LinkedWorkItemActivityKind::Commit => "commit",
            };
            let text = if entry.text.is_empty() {
                "No message"
            } else {
                entry.text.as_str()
            };
            format!("- {kind}{actor}: {text}")
        })
        .collect();
    if details.is_empty() {
        details.push(format!(
            "- {}; current state: {}",
            linked_work_item_update_summary(card),
            card.state
        ));
    }
    let mut lines = vec![
        instruction.to_string(),
        String::new(),
        format!("The linked GitHub {kind} has new activity:"),
        String::new(),
        format!("#{} {}", card.number, card.title),
        card.url.clone(),
        String::new(),
    ];
    lines.extend(details);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::time::date_parse;
    use crate::inbox::types::{GithubWorkItem, WorkItemCommit};

    fn update() -> LinkedSessionUpdate {
        LinkedSessionUpdate {
            session_id: "session-1".into(),
            since: date_parse("2026-09-13T10:00:00Z").unwrap(),
            updated_at: date_parse("2026-09-13T12:00:00Z").unwrap(),
            item: GithubWorkItem {
                kind: WorkItemKind::Pr,
                repo: "acme/app".into(),
                number: 42,
                title: "Update sidebar activity".into(),
                url: "https://github.com/acme/app/pull/42".into(),
                state: "open".into(),
                state_reason: None,
                created_at: None,
                updated_at: "2026-09-13T12:00:00Z".into(),
                labels: vec![],
                assignees: vec![],
                draft: false,
            },
        }
    }

    fn thread() -> WorkItemThread {
        WorkItemThread {
            comments: vec![
                WorkItemComment {
                    id: "old".into(),
                    kind: "comment".into(),
                    author: "old-user".into(),
                    body: "Already handled".into(),
                    created_at: "2026-09-13T09:00:00Z".into(),
                    ..Default::default()
                },
                WorkItemComment {
                    id: "review".into(),
                    kind: "review".into(),
                    author: "maya".into(),
                    body: "Please cover the empty state".into(),
                    created_at: "2026-09-13T11:30:00Z".into(),
                    url: "https://github.com/acme/app/pull/42#review".into(),
                    state: "CHANGES_REQUESTED".into(),
                    ..Default::default()
                },
            ],
            commits: vec![WorkItemCommit {
                oid: "abcdef123456".into(),
                message_headline: "Handle linked activity".into(),
                author: "nik".into(),
                committed_date: "2026-09-13T11:00:00Z".into(),
                url: "https://github.com/acme/app/commit/abcdef123456".into(),
            }],
            truncated: false,
            review_decision: "CHANGES_REQUESTED".into(),
            base_ref_name: "main".into(),
            head_ref_name: "activity".into(),
        }
    }

    #[test]
    fn only_includes_activity_newer_than_the_prior_read_baseline() {
        let card = complete_linked_work_item_update_card(
            &pending_linked_work_item_update_card(&update()),
            &thread(),
        );
        assert_eq!(
            card.counts,
            LinkedWorkItemActivityCounts {
                comments: 0,
                reviews: 1,
                commits: 1
            }
        );
        let ids: Vec<&str> = card.entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["review", "abcdef123456"]);
        assert_eq!(
            linked_work_item_update_summary(&card),
            "1 new commit · 1 new review"
        );
    }

    #[test]
    fn builds_an_explicit_agent_action_from_the_update_details() {
        let card = complete_linked_work_item_update_card(
            &pending_linked_work_item_update_card(&update()),
            &thread(),
        );
        let message = linked_work_item_activity_prompt(&card);
        assert!(message.contains("Address the new feedback"));
        assert!(message.contains("The linked GitHub pull request has new activity"));
        assert!(message.contains("review by @maya: Requested changes"));
        assert!(message.contains("commit by @nik: Handle linked activity"));
    }

    #[test]
    fn recognizes_terminal_issue_and_pull_request_states() {
        assert_eq!(
            linked_work_item_terminal_state(WorkItemKind::Issue, "CLOSED"),
            Some(LinkedWorkItemTerminalState::IssueClosed)
        );
        assert_eq!(
            linked_work_item_terminal_state(WorkItemKind::Pr, "merged"),
            Some(LinkedWorkItemTerminalState::PrMerged)
        );
        assert_eq!(
            linked_work_item_terminal_state(WorkItemKind::Pr, "closed"),
            Some(LinkedWorkItemTerminalState::PrClosed)
        );
        assert_eq!(
            linked_work_item_terminal_state(WorkItemKind::Issue, "open"),
            None
        );
    }

    #[test]
    fn summarizes_loading_failed_and_metadata_only_cards() {
        let pending = pending_linked_work_item_update_card(&update());
        assert_eq!(
            linked_work_item_update_summary(&pending),
            "Loading change details…"
        );
        let failed = fail_linked_work_item_update_card(&pending);
        assert_eq!(
            linked_work_item_update_summary(&failed),
            "Updated on GitHub · details unavailable"
        );
        let empty = complete_linked_work_item_update_card(&pending, &WorkItemThread::default());
        assert_eq!(
            linked_work_item_update_summary(&empty),
            "Metadata or status changed"
        );
        assert!(
            linked_work_item_activity_prompt(&empty)
                .ends_with("- Metadata or status changed; current state: open")
        );
    }
}
