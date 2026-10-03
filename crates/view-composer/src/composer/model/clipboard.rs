//! Port of src/platform/tauri/clipboard.ts.
//!
//! The webview copied a message as `text/html` with the files embedded in a
//! `data-monocode-files` attribute. GPUI's clipboard has no HTML flavor, so
//! the native app carries the same file list as the clipboard string's JSON
//! metadata. The limits and validation are the TypeScript's.
//!
//! The paste flow itself (GPUI clipboard first, then `monocode-platform`'s
//! pasteboard for copied paths and screenshots) lives in the composer view,
//! because it needs the host to turn paths and bytes into attachments.

use base64::Engine as _;
use gpui::ClipboardItem;
use monocode_core::attachment::{
    AttachmentKind, MAX_ATTACHMENTS, MAX_EMBED_BYTES, is_attachment_folder,
};
use monocode_core::{Attachment, js};
use serde::{Deserialize, Serialize};

/// A file inside a copied message: the TypeScript's `CopiedFile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopiedFile {
    pub name: String,
    pub mime_type: String,
    /// Base64 bytes.
    pub data: String,
}

/// The JSON metadata on a copied message's clipboard string.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopiedMessage {
    pub monocode_files: Vec<CopiedFile>,
}

/// A pasted file as bytes: what a browser `File` held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardFile {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

const MAX_CLIPBOARD_METADATA_CHARS: usize = 64 * 1024;
const MAX_CLIPBOARD_BASE64_CHARS: usize = (MAX_EMBED_BYTES as usize).div_ceil(3) * 4;
const MAX_CLIPBOARD_PAYLOAD_CHARS: usize =
    MAX_CLIPBOARD_BASE64_CHARS * 3 + MAX_CLIPBOARD_METADATA_CHARS;

/// What `readClipboardImage` throws when there is nothing to read. An empty
/// clipboard is a no-op, not an error.
pub const NO_CLIPBOARD_IMAGE: &str = "The clipboard does not contain an image.";

/// The name a pasted screenshot gets.
pub const CLIPBOARD_IMAGE_NAME: &str = "clipboard-image.png";

/// `textWithFolderPaths`: folder paths the plain text does not already list,
/// one per line.
pub fn text_with_folder_paths(text: &str, paths: &[String]) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let extra: Vec<&str> = paths
        .iter()
        .map(String::as_str)
        .filter(|path| !lines.contains(path))
        .collect();
    if extra.is_empty() {
        return text.to_string();
    }
    let suffix = extra.join("\n");
    if text.is_empty() {
        return suffix;
    }
    if text.ends_with('\n') {
        format!("{text}{suffix}")
    } else {
        format!("{text}\n{suffix}")
    }
}

/// `copyMessage`: the clipboard item for a message and its files.
/// `read_base64` reads a file on disk (`read_file_base64`). Folders travel
/// as paths in the text; restored images with no live bytes are skipped.
pub fn copy_message(
    text: &str,
    attachments: &[Attachment],
    mut read_base64: impl FnMut(&str) -> Result<String, String>,
) -> Result<ClipboardItem, String> {
    let mut files = Vec::new();
    let mut folder_paths = Vec::new();
    for attachment in attachments {
        // A folder has no bytes to copy, and asking for them fails the whole
        // copy. The path is the reference, so it goes in the text instead.
        if is_attachment_folder(attachment) {
            if let Some(path) = attachment.path.as_deref().map(js::trim)
                && !path.is_empty()
            {
                folder_paths.push(path.to_string());
            }
            continue;
        }
        if attachment.kind == AttachmentKind::Image
            && attachment.data.is_none()
            && attachment.preview_url.is_none()
            && attachment.copy_from_path.is_none()
        {
            continue;
        }
        let data = match (&attachment.data, &attachment.path) {
            (Some(data), _) => Ok(data.clone()),
            (None, Some(path)) => read_base64(path),
            (None, None) => Err("File content is no longer available.".to_string()),
        };
        let data =
            data.map_err(|reason| format!("Could not copy {}: {reason}", attachment.name))?;
        files.push(CopiedFile {
            name: attachment.name.clone(),
            mime_type: attachment.mime_type.clone(),
            data,
        });
    }
    let payload = text_with_folder_paths(text, &folder_paths);
    if files.is_empty() {
        if payload.is_empty() {
            return Err("No copyable content is available.".into());
        }
        return Ok(ClipboardItem::new_string(payload));
    }
    Ok(ClipboardItem::new_string_with_json_metadata(
        payload,
        CopiedMessage {
            monocode_files: files,
        },
    ))
}

/// `messageFilesFromClipboard`: the files a MonoCode copy carried, read from
/// the clipboard string's metadata. Only embedded bytes are accepted, and the
/// payload must fit one turn.
pub fn message_files_from_metadata(metadata: &str) -> Option<Vec<ClipboardFile>> {
    if metadata.is_empty() || metadata.len() > MAX_CLIPBOARD_PAYLOAD_CHARS {
        return None;
    }
    let message: CopiedMessage = serde_json::from_str(metadata).ok()?;
    let files = message.monocode_files;
    if files.is_empty() || files.len() > MAX_ATTACHMENTS {
        return None;
    }
    let mut decoded_bytes: i64 = 0;
    for file in &files {
        let padding = if file.data.ends_with("==") {
            2
        } else if file.data.ends_with('=') {
            1
        } else {
            0
        };
        decoded_bytes += (file.data.len() as i64 * 3) / 4 - padding;
        if decoded_bytes > MAX_EMBED_BYTES {
            return None;
        }
    }
    files
        .into_iter()
        .map(|file| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(file.data.as_bytes())
                .ok()?;
            Some(ClipboardFile {
                name: file.name,
                mime_type: file.mime_type,
                bytes,
            })
        })
        .collect()
}

/// `isFileReferenceText`: a file manager that also publishes plain text
/// publishes the path as a `file://` URI, so that is the only shape treated
/// as a reference. Anything else is a text paste.
pub fn is_file_reference_text(text: &str) -> bool {
    js::trim(text).to_lowercase().starts_with("file:")
}

/// The error when every copied path failed to attach.
pub fn nothing_to_attach_message(paths: usize) -> String {
    let which = if paths == 1 {
        "that path"
    } else {
        "those paths"
    };
    format!("Nothing to attach from {which} — the file may have been moved, renamed, or deleted.")
}

/// The warning when more files were copied than one turn carries.
pub fn too_many_copied_files_message(attached: usize, copied: usize) -> String {
    format!("Attached {attached} of {copied} copied files. A turn carries up to {MAX_ATTACHMENTS}.")
}

/// The paths for one batch of `attachmentsFromClipboardPaths`: the next
/// `MAX_ATTACHMENTS` after `consumed`.
pub fn clipboard_path_batch(paths: &[String], consumed: usize) -> &[String] {
    let end = (consumed + MAX_ATTACHMENTS).min(paths.len());
    &paths[consumed.min(end)..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(id: &str, name: &str, mime: &str, kind: AttachmentKind) -> Attachment {
        Attachment {
            id: id.into(),
            name: name.into(),
            mime_type: mime.into(),
            kind,
            size: 3,
            ..Attachment::default()
        }
    }

    fn metadata(item: &ClipboardItem) -> Option<String> {
        item.metadata().cloned()
    }

    #[test]
    fn copies_a_message_with_multiple_attachments_and_preserves_names_and_bytes_on_paste() {
        let mut png = attachment("1", "screen.png", "image/png", AttachmentKind::Image);
        png.data = Some("YWJj".into());
        let mut pdf = attachment("2", "report.pdf", "application/pdf", AttachmentKind::File);
        pdf.path = Some("/tmp/report.pdf".into());
        let item = copy_message("Look at these", &[png, pdf], |path| {
            assert_eq!(path, "/tmp/report.pdf");
            Ok("ZGVm".into())
        })
        .unwrap();
        assert_eq!(item.text().as_deref(), Some("Look at these"));
        let files = message_files_from_metadata(&metadata(&item).unwrap()).unwrap();
        assert_eq!(
            files,
            vec![
                ClipboardFile {
                    name: "screen.png".into(),
                    mime_type: "image/png".into(),
                    bytes: b"abc".to_vec()
                },
                ClipboardFile {
                    name: "report.pdf".into(),
                    mime_type: "application/pdf".into(),
                    bytes: b"def".to_vec()
                },
            ]
        );
    }

    #[test]
    fn rejects_clipboard_files_whose_decoded_bytes_exceed_the_attachment_limit() {
        let chunk = "A".repeat((MAX_EMBED_BYTES as usize / 3) * 4 + 8);
        let payload = serde_json::to_string(&CopiedMessage {
            monocode_files: vec![CopiedFile {
                name: "big.bin".into(),
                mime_type: "application/octet-stream".into(),
                data: chunk,
            }],
        })
        .unwrap();
        assert_eq!(message_files_from_metadata(&payload), None);
    }

    #[test]
    fn skips_restored_images_without_live_content_while_keeping_the_text() {
        let restored = attachment("1", "old.png", "image/png", AttachmentKind::Image);
        let item = copy_message("Caption", &[restored], |_| panic!("no read")).unwrap();
        assert_eq!(item.text().as_deref(), Some("Caption"));
        assert_eq!(metadata(&item), None);
    }

    #[test]
    fn keeps_the_existing_clipboard_when_a_message_has_no_copyable_content() {
        let restored = attachment("1", "old.png", "image/png", AttachmentKind::Image);
        assert_eq!(
            copy_message("", &[restored], |_| panic!("no read"))
                .err()
                .as_deref(),
            Some("No copyable content is available.")
        );
    }

    #[test]
    fn copies_a_folder_path_including_when_the_message_is_only_that_folder() {
        let mut folder = attachment("1", "src", "inode/directory", AttachmentKind::File);
        folder.path = Some("/repo/src".into());
        let item = copy_message("", std::slice::from_ref(&folder), |_| panic!("no read")).unwrap();
        assert_eq!(item.text().as_deref(), Some("/repo/src"));
        let item = copy_message("See\n/repo/src", &[folder.clone()], |_| panic!()).unwrap();
        assert_eq!(item.text().as_deref(), Some("See\n/repo/src"));
        let item = copy_message("See", &[folder], |_| panic!()).unwrap();
        assert_eq!(item.text().as_deref(), Some("See\n/repo/src"));
    }

    #[test]
    fn reports_a_file_it_cannot_read() {
        let mut pdf = attachment("2", "report.pdf", "application/pdf", AttachmentKind::File);
        pdf.path = Some("/gone.pdf".into());
        assert_eq!(
            copy_message("x", &[pdf], |_| Err("missing".into()))
                .err()
                .as_deref(),
            Some("Could not copy report.pdf: missing")
        );
    }

    #[test]
    fn treats_only_file_uris_as_file_references() {
        assert!(is_file_reference_text("  FILE:///Users/me/a.png\n"));
        assert!(!is_file_reference_text("https://example.com/a.png"));
        assert!(!is_file_reference_text("/Users/me/a.png"));
    }

    #[test]
    fn batches_clipboard_paths_by_the_turn_quota() {
        let paths: Vec<String> = (0..45).map(|i| format!("/f{i}")).collect();
        assert_eq!(clipboard_path_batch(&paths, 0).len(), MAX_ATTACHMENTS);
        assert_eq!(clipboard_path_batch(&paths, 40).len(), 5);
        assert!(clipboard_path_batch(&paths, 45).is_empty());
        assert_eq!(
            too_many_copied_files_message(20, 45),
            "Attached 20 of 45 copied files. A turn carries up to 20."
        );
        assert!(nothing_to_attach_message(1).contains("that path"));
    }
}
