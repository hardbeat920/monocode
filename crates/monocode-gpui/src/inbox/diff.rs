//! Provider patches use the same virtualized native review as local changes.
use crate::diff_view::DiffFile;
use gpui::SharedString;
use monocode_core::inbox::{Diff, patch};
use std::sync::Arc;

pub(super) fn models(diff: &Diff) -> Vec<DiffFile> {
    let sections = patch::sections(&diff.patch);
    diff.files
        .iter()
        .enumerate()
        .map(|(ix, file)| {
            let section = patch::section_for(&sections, &file.path, ix);
            let binary = patch::is_binary(section);
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
                blocks: Arc::new(patch::blocks(section)),
                can_stage: false,
                can_discard: false,
                can_stage_hunk: false,
            }
        })
        .collect()
}
