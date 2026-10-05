//! The decisions in src/features/quick-composer/ui/useQuickAttachments.ts
//! that do not need a window: which incoming files a collection keeps, what
//! a paste does, and which files are captures to release.

use monocode_core::Attachment;
use monocode_core::attachment::MAX_ATTACHMENTS;
use monocode_view_composer::composer::model::clipboard::is_file_reference_text;

/// What one collection keeps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collected {
    pub accepted: Vec<Attachment>,
    /// Duplicates and files over the limit. Their previews are revoked.
    pub rejected: Vec<Attachment>,
    /// "You can attach up to 20 files." when the limit dropped files.
    pub error: Option<String>,
}

/// The dedupe and capacity check of `collect`: a file is dropped when the
/// draft is full or already has the same id or path.
pub fn accept_incoming(current: &[Attachment], incoming: Vec<Attachment>) -> Collected {
    let total = incoming.len();
    let mut accepted: Vec<Attachment> = Vec::new();
    let mut rejected = Vec::new();
    for file in incoming {
        let duplicate = current.iter().chain(accepted.iter()).any(|item| {
            item.id == file.id
                || item
                    .path
                    .as_deref()
                    .is_some_and(|path| !path.is_empty() && file.path.as_deref() == Some(path))
        });
        if current.len() + accepted.len() >= MAX_ATTACHMENTS || duplicate {
            rejected.push(file);
            continue;
        }
        accepted.push(file);
    }
    let error = (total > accepted.len() && current.len() + accepted.len() >= MAX_ATTACHMENTS)
        .then(|| format!("You can attach up to {MAX_ATTACHMENTS} files."));
    Collected {
        accepted,
        rejected,
        error,
    }
}

/// `releaseCaptures`: the paths of files on disk.
pub fn capture_paths(files: &[Attachment]) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| file.path.clone().filter(|path| !path.is_empty()))
        .collect()
}

/// What a paste does (`onPaste`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteAction {
    /// The prompt inserts the text as usual.
    Text,
    /// Attach the files the clipboard carried.
    Files,
    /// Read the native clipboard now: a screenshot, or a file a file manager
    /// copied. `restore_text` puts a file reference back into the prompt
    /// when nothing attached.
    Native { restore_text: bool },
    /// A collection is running. Capture the clipboard now, so a later change
    /// cannot replace it, and attach it when the running one ends.
    Queue,
}

/// `onPaste`. `supported` is "this provider takes attachments and the
/// composer is not busy"; `can_collect` is "and no collection is running".
pub fn paste_action(
    has_files: bool,
    text: &str,
    supported: bool,
    can_collect: bool,
) -> PasteAction {
    if has_files {
        return if supported {
            PasteAction::Files
        } else {
            PasteAction::Text
        };
    }
    // Prose and whitespace alike are the prompt's to insert, and cost no
    // clipboard read, spinner, or cleared error.
    if !supported || (!text.is_empty() && !is_file_reference_text(text)) {
        return PasteAction::Text;
    }
    if !can_collect {
        // A file reference can still be inserted as text. A screenshot
        // cannot, so capture it now even though attaching must wait.
        return if text.is_empty() {
            PasteAction::Queue
        } else {
            PasteAction::Text
        };
    }
    PasteAction::Native {
        restore_text: is_file_reference_text(text),
    }
}

#[cfg(test)]
mod tests {
    use monocode_core::AttachmentKind;

    use super::*;

    fn file(id: &str, path: Option<&str>) -> Attachment {
        Attachment {
            id: id.into(),
            name: format!("{id}.png"),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 4,
            path: path.map(str::to_string),
            ..Attachment::default()
        }
    }

    #[test]
    fn drops_repeat_selections_by_id_or_path() {
        let current = vec![file("a", Some("/tmp/image.png"))];
        let collected = accept_incoming(
            &current,
            vec![
                file("b", Some("/tmp/image.png")),
                file("a", None),
                file("c", Some("/tmp/other.png")),
                file("c", Some("/tmp/third.png")),
            ],
        );
        assert_eq!(collected.accepted.len(), 1);
        assert_eq!(
            collected.accepted[0].path.as_deref(),
            Some("/tmp/other.png")
        );
        assert_eq!(collected.rejected.len(), 3);
        assert_eq!(collected.error, None);
    }

    #[test]
    fn stops_at_the_limit_and_says_so() {
        let current: Vec<Attachment> = (0..19)
            .map(|i| file(&format!("f{i}"), Some(&format!("/tmp/{i}"))))
            .collect();
        let collected = accept_incoming(
            &current,
            vec![file("x", Some("/tmp/x")), file("y", Some("/tmp/y"))],
        );
        assert_eq!(collected.accepted.len(), 1);
        assert_eq!(
            collected.error.as_deref(),
            Some("You can attach up to 20 files.")
        );
    }

    #[test]
    fn paste_decisions_follow_the_typescript() {
        // A copied image becomes a chip.
        assert_eq!(paste_action(true, "", true, true), PasteAction::Files);
        // Prose and whitespace are inserted.
        assert_eq!(paste_action(false, "hello", true, true), PasteAction::Text);
        assert_eq!(paste_action(false, "   ", true, true), PasteAction::Text);
        // A file URI is attached instead of being left in the prompt.
        assert_eq!(
            paste_action(false, "file:///tmp/image.png", true, true),
            PasteAction::Native { restore_text: true }
        );
        // An empty text paste carried an image only the native clipboard has.
        assert_eq!(
            paste_action(false, "", true, true),
            PasteAction::Native {
                restore_text: false
            }
        );
        // While a collection runs, a screenshot is captured for later and a
        // file URI is left to the prompt.
        assert_eq!(paste_action(false, "", true, false), PasteAction::Queue);
        assert_eq!(
            paste_action(false, "file:///tmp/image.png", true, false),
            PasteAction::Text
        );
        // Unsupported providers never read the clipboard.
        assert_eq!(paste_action(false, "", false, true), PasteAction::Text);
        assert_eq!(paste_action(true, "", false, true), PasteAction::Text);
    }

    #[test]
    fn only_files_on_disk_are_released() {
        let files = [
            file("a", Some("/tmp/a")),
            file("b", None),
            file("c", Some("")),
        ];
        assert_eq!(capture_paths(&files), ["/tmp/a"]);
    }
}
