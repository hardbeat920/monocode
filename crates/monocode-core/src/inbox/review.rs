//! A reviewer's pending comments on a pull request: written while reading the
//! diff, kept on this machine, and sent to the provider together as one review.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Which side of the diff a comment is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// A deleted line, numbered in the old file.
    Left,
    /// An added or unchanged line, numbered in the new file.
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingComment {
    pub id: u64,
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub text: String,
}

impl PendingComment {
    /// "retry.rs:42", the file name and line.
    pub fn label(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        format!("{name}:{}", self.line)
    }
}

/// The comments for a pull request, as kept in settings.
pub fn setting_key(item_key: &str) -> String {
    format!("pr-review:{item_key}")
}

pub fn parse(json: &str) -> Vec<PendingComment> {
    serde_json::from_str(json).unwrap_or_default()
}

pub fn serialize(comments: &[PendingComment]) -> String {
    serde_json::to_string(comments).unwrap_or_else(|_| "[]".into())
}

/// The next unused comment id.
pub fn next_id(comments: &[PendingComment]) -> u64 {
    comments.iter().map(|c| c.id).max().map_or(1, |id| id + 1)
}

/// The body of GitHub's "create a review" request. It only comments: it never
/// approves or requests changes.
pub fn github_payload(comments: &[PendingComment], summary: &str) -> Value {
    json!({
        "event": "COMMENT",
        "body": summary.trim(),
        "comments": comments.iter().map(|comment| json!({
            "path": comment.path,
            "line": comment.line,
            "side": match comment.side {
                Side::Left => "LEFT",
                Side::Right => "RIGHT",
            },
            "body": comment.text.trim(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: u64, side: Side) -> PendingComment {
        PendingComment {
            id,
            path: "src/retry.rs".into(),
            line: 42,
            side,
            text: " Why jitter? ".into(),
        }
    }

    #[test]
    fn the_review_comments_without_approving() {
        let payload = github_payload(&[comment(1, Side::Right), comment(2, Side::Left)], " ");
        assert_eq!(payload["event"], "COMMENT");
        assert_eq!(payload["body"], "");
        assert_eq!(
            payload["comments"][0],
            json!({"path": "src/retry.rs", "line": 42, "side": "RIGHT", "body": "Why jitter?"})
        );
        assert_eq!(payload["comments"][1]["side"], "LEFT");
    }

    #[test]
    fn comments_round_trip_through_settings_and_bad_data_is_empty() {
        let comments = vec![comment(3, Side::Right)];
        assert_eq!(parse(&serialize(&comments)), comments);
        assert!(parse("not json").is_empty());
        assert_eq!(next_id(&comments), 4);
        assert_eq!(next_id(&[]), 1);
    }

    #[test]
    fn a_comment_is_labelled_by_file_and_line() {
        assert_eq!(comment(1, Side::Right).label(), "retry.rs:42");
        assert_eq!(
            setting_key("github:acme/app#7"),
            "pr-review:github:acme/app#7"
        );
    }
}
