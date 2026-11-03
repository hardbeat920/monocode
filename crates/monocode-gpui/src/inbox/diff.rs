//! Provider patches use the same virtualized native review as local changes.
use crate::diff_view::DiffFile;
use gpui::SharedString;
use monocode_core::{
    diff::{Block, Line, LineKind},
    inbox::Diff,
};
use std::sync::Arc;

fn patch_path(section: &str) -> Option<String> {
    section
        .lines()
        .find_map(|line| line.strip_prefix("+++ "))
        .filter(|path| *path != "/dev/null")
        .map(|path| {
            let path = serde_json::from_str::<String>(path).unwrap_or_else(|_| path.to_string());
            path.strip_prefix("b/").unwrap_or(&path).to_string()
        })
}
fn hunk_start(value: &str) -> u32 {
    value
        .trim_start_matches(['+', '-'])
        .split(',')
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}
fn blocks(section: &str) -> Vec<Block> {
    let (mut old, mut new) = (0, 0);
    let mut lines = vec![];
    let mut in_hunk = false;
    for raw in section.lines() {
        if raw.starts_with("@@ ") {
            let mut parts = raw.split_whitespace().skip(1);
            old = hunk_start(parts.next().unwrap_or("-1"));
            new = hunk_start(parts.next().unwrap_or("+1"));
            in_hunk = true;
            lines.push(Line {
                kind: LineKind::Hunk,
                text: raw.into(),
                old_number: None,
                new_number: None,
                chunk: None,
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (kind, old_number, new_number) = match raw.as_bytes().first() {
            Some(b'+') => {
                let n = new;
                new += 1;
                (LineKind::Add, None, Some(n))
            }
            Some(b'-') => {
                let n = old;
                old += 1;
                (LineKind::Del, Some(n), None)
            }
            Some(b' ') => {
                let (o, n) = (old, new);
                old += 1;
                new += 1;
                (LineKind::Context, Some(o), Some(n))
            }
            _ => continue,
        };
        lines.push(Line {
            kind,
            text: raw[1..].into(),
            old_number,
            new_number,
            chunk: None,
        });
    }
    if lines.is_empty() {
        vec![]
    } else {
        vec![Block::Hunk { lines, chunk: None }]
    }
}
pub(super) fn models(diff: &Diff) -> Vec<DiffFile> {
    let sections = diff.patch.split("diff --git ").skip(1).collect::<Vec<_>>();
    diff.files
        .iter()
        .enumerate()
        .map(|(ix, file)| {
            let section = sections
                .iter()
                .find(|section| patch_path(section).as_deref() == Some(file.path.as_str()))
                .copied()
                .or_else(|| sections.get(ix).copied())
                .unwrap_or("");
            let binary = section.contains("Binary files ") || section.contains("GIT binary patch");
            DiffFile {
                id: SharedString::from(format!("inbox-diff-{}", file.path)),
                path: file.path.clone(),
                label: file.path.clone(),
                binary,
                too_large: diff.truncated && section.is_empty(),
                empty_message: if section.is_empty() && !diff.truncated {
                    Some("No textual diff".into())
                } else {
                    None
                },
                additions: file.additions.max(0).min(u32::MAX as i64) as u32,
                deletions: file.deletions.max(0).min(u32::MAX as i64) as u32,
                blocks: Arc::new(blocks(section)),
                can_stage: false,
                can_discard: false,
                can_stage_hunk: false,
            }
        })
        .collect()
}
