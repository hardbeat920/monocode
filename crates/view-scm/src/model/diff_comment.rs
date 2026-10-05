//! Port of `diffCommentContext` in
//! src/features/source-control/model/diffComment.ts and the `comment` arm of
//! `ChatContextItem` (src/features/sessions/model/chatContext.ts).
//! `diffCommentLocation` is `DiffCommentTarget::location` in monocode-editor.

use monocode_editor::unified_diff::{DiffCommentTarget, UnifiedLineKind};
use serde::{Deserialize, Serialize};

/// `DiffLineChange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineChange {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "removed")]
    Removed,
    #[serde(rename = "unchanged")]
    Unchanged,
}

/// `{ kind: "comment", path, line?, change, code, comment }`, the context
/// chip a diff comment adds to the composer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename = "comment")]
pub struct DiffCommentItem {
    pub path: String,
    /// New-file line number. A removed line uses its old-file number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    pub change: DiffLineChange,
    pub code: String,
    pub comment: String,
}

/// `diffCommentContext`: the chip for a comment on one diff line, or `None`
/// for an empty comment.
pub fn diff_comment_context(target: &DiffCommentTarget, comment: &str) -> Option<DiffCommentItem> {
    let body = comment.replace("\r\n", "\n").replace('\r', "\n");
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    Some(DiffCommentItem {
        path: target.path.replace('\\', "/"),
        line: target.line_number().map(|line| line as i64),
        change: match target.line.kind {
            UnifiedLineKind::Add => DiffLineChange::Added,
            UnifiedLineKind::Del => DiffLineChange::Removed,
            _ => DiffLineChange::Unchanged,
        },
        code: target
            .line
            .text
            .strip_suffix('\r')
            .unwrap_or(&target.line.text)
            .to_string(),
        comment: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use monocode_editor::unified_diff::UnifiedLine;

    use super::*;

    fn target(
        path: &str,
        kind: UnifiedLineKind,
        text: &str,
        old: Option<usize>,
        new: Option<usize>,
    ) -> DiffCommentTarget {
        DiffCommentTarget {
            path: path.into(),
            line: UnifiedLine {
                kind,
                text: text.into(),
                old_number: old,
                new_number: new,
                pos: None,
            },
        }
    }

    #[test]
    fn turns_a_comment_on_a_current_line_into_a_context_chip() {
        let t = target(
            "src/auth.ts",
            UnifiedLineKind::Add,
            "const token = readCookie();",
            None,
            Some(42),
        );
        assert_eq!(t.location(), "src/auth.ts:42");
        let item = diff_comment_context(&t, " Please handle a missing cookie. ").unwrap();
        assert_eq!(
            item,
            DiffCommentItem {
                path: "src/auth.ts".into(),
                line: Some(42),
                change: DiffLineChange::Added,
                code: "const token = readCookie();".into(),
                comment: "Please handle a missing cookie.".into(),
            }
        );
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            serde_json::json!({
                "kind": "comment",
                "path": "src/auth.ts",
                "line": 42,
                "change": "added",
                "code": "const token = readCookie();",
                "comment": "Please handle a missing cookie.",
            })
        );
    }

    #[test]
    fn uses_the_old_line_number_for_a_removed_line() {
        let t = target(
            "src/old.ts",
            UnifiedLineKind::Del,
            "legacy();\r",
            Some(8),
            None,
        );
        let item = diff_comment_context(&t, "Keep this behavior.").unwrap();
        assert_eq!(item.line, Some(8));
        assert_eq!(item.change, DiffLineChange::Removed);
        assert_eq!(item.code, "legacy();");
    }

    #[test]
    fn ignores_an_empty_comment() {
        let t = target(
            "README.md",
            UnifiedLineKind::Context,
            "Title",
            Some(1),
            Some(1),
        );
        assert_eq!(diff_comment_context(&t, " \n "), None);
    }
}
