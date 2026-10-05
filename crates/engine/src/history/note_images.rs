//! Port of src/features/notes/noteImages.ts: images dropped or pasted into a
//! note, and the markdown that shows them.
//!
//! Saving runs in the `Notes` entity (`Notes::save_images_from_paths` and
//! `Notes::save_images_from_data`). This module keeps the pure parts.
//!
//! Text offsets are UTF-8 byte offsets, the unit GPUI text inputs use. The
//! TypeScript counted UTF-16 code units; the two agree for ASCII text.

use std::sync::LazyLock;

use monocode_core::AttachmentKind;
use monocode_core::attachment::{kind_from_mime, mime_from_name};
use regex::Regex;

pub use monocode_store::notes::NoteImageAsset;

/// `NOTE_IMAGE_PREFIX`.
pub const NOTE_IMAGE_PREFIX: &str = "/note-assets/";
/// The error when a drop holds no image.
pub const NO_IMAGES_ERROR: &str = "Drop a PNG, JPG, GIF, WebP, or SVG image.";
/// The error when every image failed without a message of its own.
pub const NONE_SAVED_ERROR: &str = "None of the dropped images could be added to the note.";

static NEWLINES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\r\n]+").expect("newline regex"));

/// `MarkdownInsertion`: the new text and where the cursor goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownInsertion {
    pub value: String,
    pub cursor: usize,
}

/// A pasted image that has no file yet: a name and base64 data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteImageData {
    pub name: String,
    pub data: String,
}

/// Whether a dropped file is an image by its name, the way
/// `attachmentsFromPaths` sorted dropped files.
pub fn is_image_name(name: &str) -> bool {
    kind_from_mime(&mime_from_name(name)) == AttachmentKind::Image
}

/// `noteImageMarkdown`: escape the name for alt text.
pub fn note_image_markdown(image: &NoteImageAsset) -> String {
    let alt = NEWLINES_RE.replace_all(&image.name, " ");
    let mut escaped = String::with_capacity(alt.len());
    for c in alt.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '[' | ']' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }
    format!("![{escaped}]({})", image.markdown_path)
}

/// `insertNoteImagesMarkdown`: replace the selection with the images as
/// blocks separated by blank lines.
pub fn insert_note_images_markdown(
    value: &str,
    start: usize,
    end: usize,
    images: &[NoteImageAsset],
) -> MarkdownInsertion {
    if images.is_empty() {
        return MarkdownInsertion {
            value: value.to_string(),
            cursor: floor_boundary(value, start.min(value.len())),
        };
    }
    let from = floor_boundary(value, start.min(value.len()));
    let to = floor_boundary(value, end.min(value.len()).max(from));
    let before = &value[..from];
    let after = &value[to..];
    let block = images
        .iter()
        .map(note_image_markdown)
        .collect::<Vec<_>>()
        .join("\n\n");
    let leading = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let trailing = if after.is_empty() || after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let inserted = format!("{leading}{block}");
    MarkdownInsertion {
        value: format!("{before}{inserted}{trailing}{after}"),
        cursor: before.len() + inserted.len(),
    }
}

/// `isNoteImagePath`.
pub fn is_note_image_path(value: &str) -> bool {
    value.starts_with(NOTE_IMAGE_PREFIX)
}

fn floor_boundary(value: &str, mut index: usize) -> usize {
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> NoteImageAsset {
        NoteImageAsset {
            name: "Architecture [draft].png".into(),
            markdown_path: "/note-assets/note-1/123-architecture-draft.png".into(),
        }
    }

    #[test]
    fn escapes_image_names_used_as_alt_text() {
        assert_eq!(
            note_image_markdown(&image()),
            "![Architecture \\[draft\\].png](/note-assets/note-1/123-architecture-draft.png)"
        );
    }

    #[test]
    fn inserts_images_as_blocks_at_the_cursor() {
        assert_eq!(
            insert_note_images_markdown("BeforeAfter", 6, 6, &[image()]),
            MarkdownInsertion {
                value: "Before\n\n![Architecture \\[draft\\].png](/note-assets/note-1/123-architecture-draft.png)\n\nAfter"
                    .into(),
                cursor: 85,
            }
        );
    }

    #[test]
    fn replaces_the_selection_and_separates_multiple_images() {
        let second = NoteImageAsset {
            name: "flow.png".into(),
            markdown_path: "/note-assets/note-1/456-flow.png".into(),
        };
        assert_eq!(
            insert_note_images_markdown("Top\nreplace\nBottom", 4, 12, &[image(), second.clone()]),
            MarkdownInsertion {
                value: [
                    "Top",
                    "",
                    &note_image_markdown(&image()),
                    "",
                    &note_image_markdown(&second),
                    "",
                    "Bottom",
                ]
                .join("\n"),
                cursor: 129,
            }
        );
    }

    #[test]
    fn keeps_the_text_when_there_are_no_images() {
        assert_eq!(
            insert_note_images_markdown("abc", 9, 9, &[]),
            MarkdownInsertion {
                value: "abc".into(),
                cursor: 3,
            }
        );
    }

    #[test]
    fn recognizes_only_note_asset_references() {
        assert!(is_note_image_path(&image().markdown_path));
        assert!(!is_note_image_path("https://example.com/image.png"));
    }

    #[test]
    fn sorts_dropped_files_by_name() {
        assert!(is_image_name("shot.PNG"));
        assert!(is_image_name("diagram.svg"));
        assert!(!is_image_name("notes.md"));
    }
}
