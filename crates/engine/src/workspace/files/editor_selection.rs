//! Port of src/features/files/model/editorSelection.ts.

use crate::workspace::chat_context::ChatContextItem;

/// `EditorCodeSelection`: a line range in an open file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCodeSelection {
    pub path: String,
    pub start_line: i64,
    pub end_line: i64,
}

/// `editorSelectionContext`: an editor range as a reference the agent can
/// read on demand, without copying the code.
pub fn editor_selection_context(selection: &EditorCodeSelection) -> ChatContextItem {
    ChatContextItem::Code {
        path: selection.path.replace('\\', "/"),
        start_line: selection.start_line,
        end_line: selection.end_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_the_file_and_line_range_without_copying_its_contents() {
        let item = editor_selection_context(&EditorCodeSelection {
            path: "src/FileEditor.tsx".into(),
            start_line: 12,
            end_line: 13,
        });
        assert_eq!(
            item,
            ChatContextItem::Code {
                path: "src/FileEditor.tsx".into(),
                start_line: 12,
                end_line: 13,
            }
        );
    }

    #[test]
    fn uses_forward_slashes_for_windows_paths() {
        let item = editor_selection_context(&EditorCodeSelection {
            path: "docs\\read me.md".into(),
            start_line: 3,
            end_line: 3,
        });
        assert!(matches!(item, ChatContextItem::Code { path, .. } if path == "docs/read me.md"));
    }
}
