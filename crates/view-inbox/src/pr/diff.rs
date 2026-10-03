//! Port of src/features/inbox/ui/InboxPrDiff.tsx: a pull or merge request's
//! patch in the shared diff view, merged with the provider's file list so
//! files without a textual patch still show, first file open.
//!
//! `mergePrDiff` comes from src/features/source-control/model/prDiff.ts;
//! `parsePrPatch` is `monocode_editor::unified_diff::parse_patch`.

use gpui::{App, AppContext as _, Context, Entity};
use monocode_editor::diff_view::{DiffFile, DiffFileActions, DiffView, InitialExpansion};
use monocode_editor::unified_diff::{
    PatchFile, PatchStatus, UNIFIED_CONTEXT_DEFAULT, blocks_from_lines, file_hunks, parse_patch,
};
use monocode_editor::{ColorScheme, EditorTheme};
use monocode_ui::Theme;

use crate::data::{PrDiff, PrFile};

/// `mergePrDiff`: the provider's file list in its order, each with its
/// parsed patch when there is one, then any parsed file the list missed.
pub fn merge_pr_diff(meta: &[PrFile], parsed: Vec<PatchFile>) -> Vec<PatchFile> {
    let empty = |file: &PrFile| PatchFile {
        path: file.path.clone(),
        previous_path: None,
        status: PatchStatus::Modified,
        binary: false,
        additions: file.additions.max(0) as usize,
        deletions: file.deletions.max(0) as usize,
        lines: Vec::new(),
    };
    if parsed.is_empty() {
        return meta.iter().map(empty).collect();
    }
    if meta.is_empty() {
        return parsed;
    }
    let mut used: Vec<String> = Vec::new();
    let mut out = Vec::with_capacity(meta.len());
    for file in meta {
        match parsed.iter().find(|hit| hit.path == file.path) {
            Some(hit) => {
                out.push(PatchFile {
                    additions: file.additions.max(0) as usize,
                    deletions: file.deletions.max(0) as usize,
                    ..hit.clone()
                });
                used.push(file.path.clone());
            }
            None => out.push(empty(file)),
        }
    }
    for file in parsed {
        if !used.contains(&file.path) {
            out.push(file);
        }
    }
    out
}

/// `toModel`: one diff file. `full_file` shows every line with no folds.
pub fn diff_file(file: PatchFile, truncated: bool, full_file: bool) -> DiffFile {
    let label = match (&file.status, &file.previous_path) {
        (PatchStatus::Renamed, Some(previous)) => format!("{previous} → {}", file.path),
        _ => file.path.clone(),
    };
    let empty_message = (!file.binary && file.lines.is_empty()).then_some(if truncated {
        "Patch unavailable because this change is too large"
    } else {
        "No textual diff"
    });
    let context = if full_file {
        usize::MAX
    } else {
        UNIFIED_CONTEXT_DEFAULT
    };
    let mut diff = blocks_from_lines(file.lines, context);
    diff.additions = file.additions;
    diff.deletions = file.deletions;
    let hunks = file_hunks(&diff);
    DiffFile {
        id: file.path.clone().into(),
        path: file.path.clone().into(),
        label: label.into(),
        previous_path: file.previous_path.map(Into::into),
        status: file.status,
        binary: file.binary,
        too_large: false,
        empty_message: empty_message.map(Into::into),
        diff,
        hunks,
        actions: DiffFileActions::default(),
    }
}

/// The files of a pull request diff.
pub fn pr_diff_files(diff: &PrDiff, full_file: bool) -> Vec<DiffFile> {
    merge_pr_diff(&diff.files, parse_patch(&diff.patch))
        .into_iter()
        .map(|file| diff_file(file, diff.truncated, full_file))
        .collect()
}

/// The editor palette for the current theme.
pub fn editor_theme(cx: &App) -> EditorTheme {
    let theme = Theme::of(cx);
    let scheme = if theme.is_dark() {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    let mut editor = EditorTheme::new(scheme, theme.colors.background_base, theme.colors.content);
    editor.accent = theme.colors.accent;
    editor.mono_font = theme.fonts.mono.clone();
    editor.ui_font = theme.fonts.sans.clone();
    editor
}

/// `InboxPrDiff`: a diff view for `diff`.
pub fn inbox_pr_diff<T>(diff: &PrDiff, full_file: bool, cx: &mut Context<T>) -> Entity<DiffView> {
    let files = pr_diff_files(diff, full_file);
    let truncated = diff.truncated;
    let theme = editor_theme(cx);
    cx.new(|cx| {
        let mut view = DiffView::new(Vec::new(), theme, cx);
        view.set_files(files, InitialExpansion::First, cx);
        view.set_truncated(truncated, None, cx);
        view
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/app.ts b/src/app.ts\nindex 1..2 100644\n--- a/src/app.ts\n+++ b/src/app.ts\n@@ -1,2 +1,2 @@\n-const a = 1;\n+const a = 2;\n const b = 3;\n";

    #[test]
    fn keeps_listed_files_without_a_patch() {
        let diff = PrDiff {
            additions: 3,
            deletions: 1,
            files: vec![
                PrFile {
                    path: "src/app.ts".into(),
                    additions: 1,
                    deletions: 1,
                },
                PrFile {
                    path: "assets/logo.png".into(),
                    additions: 2,
                    deletions: 0,
                },
            ],
            patch: PATCH.into(),
            truncated: true,
        };
        let files = pr_diff_files(&diff, false);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path.as_ref(), "src/app.ts");
        assert!(files[0].empty_message.is_none());
        assert_eq!(
            files[1].empty_message.as_ref().map(|m| m.as_ref()),
            Some("Patch unavailable because this change is too large")
        );
    }

    #[test]
    fn falls_back_to_the_file_list_when_nothing_parses() {
        let merged = merge_pr_diff(
            &[PrFile {
                path: "a.txt".into(),
                additions: 1,
                deletions: 0,
            }],
            Vec::new(),
        );
        assert_eq!(merged.len(), 1);
        assert!(merged[0].lines.is_empty());
    }
}
