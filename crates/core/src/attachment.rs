//! Port of the attachment types in src/features/sessions/model/session.ts and
//! the pure helpers in src/features/sessions/model/attachments.ts.
//!
//! The clipboard, file picker, and file reads in attachments.ts stay with the
//! engine. `mime_from_name`, `kind_from_mime`, and `skip_name` are public so
//! it can build an `Attachment` from a path.

use serde::{Deserialize, Serialize};

use crate::block::Extra;
use crate::js;
use crate::paths::basename;

/// `MAX_ATTACHMENTS`.
pub const MAX_ATTACHMENTS: usize = 20;
/// `MAX_EMBED_BYTES`: vision images up to this size travel inline as base64.
pub const MAX_EMBED_BYTES: i64 = 20 * 1024 * 1024;

/// Stands in for a turn that arrived with files but no words. It goes on the
/// wire only; the transcript keeps the empty text.
pub const ATTACHMENT_ONLY_PROMPT: &str = "The user attached these files without saying anything. Use the conversation above to work out what they want done with them, then do that. If the conversation gives you nothing to go on, ask.";

/// A copied folder. No harness can open one, so it travels as its path.
pub const FOLDER_MIME: &str = "inode/directory";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AttachmentKind {
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "audio")]
    Audio,
    #[default]
    #[serde(rename = "file")]
    File,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Live transcript only. `persistable_attachment` drops it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_from_path: Option<bool>,
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub kind: AttachmentKind,
    pub size: i64,
    /// Absolute path when the file lives on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Base64 payload for vision images (and pasted blobs) sent to the harness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Object URL for in-session thumbnails. Not persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Attachment {
    fn path_str(&self) -> Option<&str> {
        self.path.as_deref().filter(|path| !path.is_empty())
    }
}

/// A content block in a prompt sent to a harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PromptContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image", rename_all = "camelCase")]
    Image {
        mime_type: String,
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        uri: Option<String>,
    },
    #[serde(rename = "resource_link", rename_all = "camelCase")]
    ResourceLink {
        uri: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<i64>,
    },
}

/// `promptText`: the turn's text, or a stand-in when files arrived without any.
pub fn prompt_text(text: &str, attachments: &[Attachment]) -> String {
    let trimmed = js::trim(text);
    if !trimmed.is_empty() || attachments.is_empty() {
        return trimmed.to_string();
    }
    ATTACHMENT_ONLY_PROMPT.to_string()
}

/// `persistableAttachment`: the fields worth saving with the transcript.
pub fn persistable_attachment(file: &Attachment) -> Attachment {
    Attachment {
        id: file.id.clone(),
        name: file.name.clone(),
        mime_type: file.mime_type.clone(),
        kind: file.kind,
        size: file.size,
        path: file.path_str().map(str::to_string),
        ..Attachment::default()
    }
}

/// `displayAttachments`.
pub fn display_attachments(files: &[Attachment]) -> Vec<Attachment> {
    files
        .iter()
        .map(|file| {
            let mut next = persistable_attachment(file);
            if file.path_str().is_some() {
                next.copy_from_path = Some(true);
            }
            next.preview_url = file.preview_url.clone().filter(|url| !url.is_empty());
            next.data = file.data.clone().filter(|data| !data.is_empty());
            next
        })
        .collect()
}

/// `attachmentPreviewSrc`.
pub fn attachment_preview_src(file: &Attachment) -> Option<String> {
    if let Some(url) = file.preview_url.as_deref().filter(|url| !url.is_empty()) {
        return Some(url.to_string());
    }
    match file.data.as_deref() {
        Some(data) if !data.is_empty() && file.kind == AttachmentKind::Image => {
            Some(format!("data:{};base64,{}", file.mime_type, data))
        }
        _ => None,
    }
}

/// `mergeAttachments`: append new files, skipping duplicates by path or id,
/// up to `MAX_ATTACHMENTS`.
pub fn merge_attachments(existing: &[Attachment], incoming: &[Attachment]) -> Vec<Attachment> {
    let mut next = existing.to_vec();
    for file in incoming {
        let duplicate = next.iter().any(|item| {
            matches!((item.path_str(), file.path_str()), (Some(a), Some(b)) if a == b)
                || item.id == file.id
        });
        if duplicate {
            continue;
        }
        next.push(file.clone());
        if next.len() >= MAX_ATTACHMENTS {
            break;
        }
    }
    next
}

/// `promptBlocks`. Fails like `attachmentPath` when a file has no path.
pub fn prompt_blocks(
    text: &str,
    attachments: &[Attachment],
) -> Result<Vec<PromptContentBlock>, String> {
    let mut blocks = Vec::new();
    let body = prompt_text(text, attachments);
    if !body.is_empty() {
        blocks.push(PromptContentBlock::Text { text: body });
    }
    for file in attachments {
        blocks.push(content_block_for(file)?);
    }
    Ok(blocks)
}

/// `attachmentPath`: require a deliverable source instead of silently
/// dropping an attachment.
pub fn attachment_path(file: &Attachment) -> Result<&str, String> {
    match file.path.as_deref() {
        Some(path) if !js::trim(path).is_empty() => Ok(path),
        _ => Err(format!(
            "Cannot attach {}: no local file path is available. Attach the file again.",
            serde_json::to_string(&file.name).unwrap_or_default()
        )),
    }
}

/// `attachmentPathText`: harnesses without file blocks can ask their tools
/// to read this path.
pub fn attachment_path_text(file: &Attachment) -> Result<String, String> {
    let path = serde_json::to_string(attachment_path(file)?).unwrap_or_default();
    if is_attachment_folder(file) {
        return Ok(format!(
            "Attached folder (list or read the files inside from this path): {path}"
        ));
    }
    Ok(format!("Attached file (read from disk): {path}"))
}

/// `isAttachmentFolder`.
pub fn is_attachment_folder(file: &Attachment) -> bool {
    file.mime_type == FOLDER_MIME
}

fn content_block_for(file: &Attachment) -> Result<PromptContentBlock, String> {
    // No harness can open a folder, so it travels as a path for the agent's own
    // tools rather than a resource link nothing can read.
    if is_attachment_folder(file) {
        return Ok(PromptContentBlock::Text {
            text: attachment_path_text(file)?,
        });
    }
    if let Some(data) = file.data.as_deref().filter(|data| !data.is_empty())
        && is_vision_image(&file.mime_type)
    {
        return Ok(PromptContentBlock::Image {
            mime_type: normalize_image_mime(&file.mime_type),
            data: data.to_string(),
            uri: file.path_str().map(file_uri),
        });
    }
    Ok(PromptContentBlock::ResourceLink {
        uri: file_uri(attachment_path(file)?),
        name: file.name.clone(),
        mime_type: Some(file.mime_type.clone()),
        size: Some(file.size),
    })
}

/// MIME types providers typically send as vision input.
const VISION_MIME: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
];

/// `isVisionImage`.
pub fn is_vision_image(mime_type: &str) -> bool {
    VISION_MIME.contains(&mime_type.to_lowercase().as_str())
}

/// `normalizeImageMime`.
pub fn normalize_image_mime(mime_type: &str) -> String {
    let mime = mime_type.to_lowercase();
    if mime == "image/jpg" {
        return "image/jpeg".into();
    }
    mime
}

/// `MIME_BY_EXT`.
fn mime_by_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "avif" => "image/avif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "json" => "application/json",
        "yaml" | "yml" => "text/yaml",
        "toml" => "application/toml",
        "rtf" => "application/rtf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "ts" | "tsx" | "jsx" | "rs" | "go" | "java" | "kt" | "swift" | "c" | "h" | "cc" | "cpp"
        | "hpp" | "cs" | "rb" | "php" | "sh" | "zsh" | "bash" => "text/plain",
        "js" | "mjs" | "cjs" => "text/javascript",
        "css" => "text/css",
        "py" => "text/x-python",
        "sql" => "application/sql",
        "graphql" => "application/graphql",
        _ => return None,
    })
}

fn is_text_ext(ext: &str) -> bool {
    [
        "txt", "md", "rst", "log", "cfg", "ini", "env", "lock", "gradle", "cmake", "mk", "vue",
        "svelte", "astro", "scss", "sass", "less", "lua", "r", "jl", "ex", "exs", "erl", "hs",
        "ml", "clj", "scala", "groovy", "dart", "nim", "zig", "proto", "graphqls",
    ]
    .contains(&ext)
}

/// `mimeFromName`: MIME type from a file name's extension.
pub fn mime_from_name(name: &str) -> String {
    let ext = extension(name);
    if ext.is_empty() {
        return "application/octet-stream".into();
    }
    mime_by_ext(&ext)
        .unwrap_or(if is_text_ext(&ext) {
            "text/plain"
        } else {
            "application/octet-stream"
        })
        .to_string()
}

/// `mimeFromFile`: prefer the extension, then the type the browser reported.
pub fn mime_from_file(name: &str, reported_type: &str) -> String {
    let from_name = mime_from_name(name);
    if !reported_type.is_empty() && reported_type != "application/octet-stream" {
        if from_name != "application/octet-stream" {
            return from_name;
        }
        return reported_type.to_string();
    }
    from_name
}

/// `kindFromMime`.
pub fn kind_from_mime(mime_type: &str) -> AttachmentKind {
    if mime_type.starts_with("image/") {
        AttachmentKind::Image
    } else if mime_type.starts_with("audio/") {
        AttachmentKind::Audio
    } else {
        AttachmentKind::File
    }
}

/// `skipName`: OS metadata files the composer never attaches.
pub fn skip_name(name: &str) -> bool {
    matches!(
        basename(name).to_lowercase().as_str(),
        ".ds_store" | "thumbs.db" | "desktop.ini"
    )
}

/// `fallbackName`: a name for a pasted blob that has none.
pub fn fallback_name(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "image.png",
        "image/jpeg" | "image/jpg" => "image.jpg",
        "image/gif" => "image.gif",
        "image/webp" => "image.webp",
        _ if mime_type.starts_with("image/") => "image",
        _ if mime_type.starts_with("audio/") => "audio",
        _ => "attachment",
    }
}

/// `fileUri`: a `file://` URI with each path segment percent-encoded.
pub fn file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let abs = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    let encoded: Vec<String> = abs.split('/').map(js::encode_uri_component).collect();
    format!("file://{}", encoded.join("/"))
}

fn extension(name: &str) -> String {
    let base = basename(name).to_lowercase();
    match base.rfind('.') {
        Some(dot) if dot > 0 && dot != base.len() - 1 => base[dot + 1..].to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(id: &str, name: &str, path: Option<&str>) -> Attachment {
        Attachment {
            id: id.into(),
            name: name.into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 4,
            path: path.map(str::to_string),
            ..Attachment::default()
        }
    }

    // mergeAttachments
    #[test]
    fn keeps_previously_attached_images_when_adding_more() {
        let first = attachment("a", "one.png", None);
        let second = attachment("b", "two.png", None);
        let ids: Vec<String> = merge_attachments(&[first], &[second])
            .into_iter()
            .map(|file| file.id)
            .collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn skips_the_same_path_twice() {
        let first = attachment("a", "shot.png", Some("/tmp/shot.png"));
        let again = attachment("b", "shot.png", Some("/tmp/shot.png"));
        assert_eq!(
            merge_attachments(std::slice::from_ref(&first), &[again]),
            vec![first]
        );
    }

    #[test]
    fn stops_at_the_attachment_limit() {
        let incoming: Vec<Attachment> = (0..30)
            .map(|i| attachment(&i.to_string(), "x.png", None))
            .collect();
        assert_eq!(merge_attachments(&[], &incoming).len(), MAX_ATTACHMENTS);
    }

    #[test]
    fn substitutes_a_prompt_for_attachment_only_turns() {
        let file = attachment("a", "one.png", Some("/tmp/one.png"));
        assert_eq!(prompt_text("  hi  ", &[]), "hi");
        assert_eq!(prompt_text("", &[]), "");
        assert_eq!(prompt_text(" ", &[file]), ATTACHMENT_ONLY_PROMPT);
    }

    #[test]
    fn builds_prompt_blocks_for_images_links_and_folders() {
        let image = Attachment {
            data: Some("AAAA".into()),
            mime_type: "image/JPG".into(),
            ..attachment("a", "a.jpg", Some("/tmp/my file.jpg"))
        };
        let pdf = Attachment {
            mime_type: "application/pdf".into(),
            kind: AttachmentKind::File,
            ..attachment("b", "b.pdf", Some("/tmp/b.pdf"))
        };
        let folder = Attachment {
            mime_type: FOLDER_MIME.into(),
            ..attachment("c", "src", Some("/repo/src"))
        };
        let blocks = prompt_blocks("look", &[image, pdf, folder]).unwrap();
        assert_eq!(
            serde_json::to_value(&blocks).unwrap(),
            serde_json::json!([
                { "type": "text", "text": "look" },
                { "type": "image", "mimeType": "image/jpeg", "data": "AAAA", "uri": "file:///tmp/my%20file.jpg" },
                { "type": "resource_link", "uri": "file:///tmp/b.pdf", "name": "b.pdf", "mimeType": "application/pdf", "size": 4 },
                { "type": "text", "text": "Attached folder (list or read the files inside from this path): \"/repo/src\"" }
            ])
        );
    }

    #[test]
    fn refuses_an_attachment_without_a_path() {
        let file = Attachment {
            mime_type: "application/pdf".into(),
            ..attachment("a", "a.pdf", None)
        };
        assert_eq!(
            prompt_blocks("x", &[file]).unwrap_err(),
            "Cannot attach \"a.pdf\": no local file path is available. Attach the file again."
        );
    }

    #[test]
    fn persists_only_the_saved_fields() {
        let file = Attachment {
            data: Some("AAAA".into()),
            preview_url: Some("blob:x".into()),
            copy_from_path: Some(true),
            ..attachment("a", "a.png", Some("/tmp/a.png"))
        };
        assert_eq!(
            serde_json::to_value(persistable_attachment(&file)).unwrap(),
            serde_json::json!({ "id": "a", "name": "a.png", "mimeType": "image/png", "kind": "image", "size": 4, "path": "/tmp/a.png" })
        );
        let shown = display_attachments(std::slice::from_ref(&file));
        assert_eq!(shown[0].copy_from_path, Some(true));
        assert_eq!(shown[0].data.as_deref(), Some("AAAA"));
    }

    #[test]
    fn maps_names_to_mime_types() {
        assert_eq!(mime_from_name("/a/b/Photo.JPEG"), "image/jpeg");
        assert_eq!(mime_from_name("notes.rst"), "text/plain");
        assert_eq!(
            mime_from_name("archive.unknown"),
            "application/octet-stream"
        );
        assert_eq!(mime_from_name(".env"), "application/octet-stream");
        assert_eq!(mime_from_name("trailing."), "application/octet-stream");
        assert_eq!(mime_from_file("clip", "image/png"), "image/png");
        assert_eq!(mime_from_file("a.txt", "application/x-thing"), "text/plain");
        assert_eq!(kind_from_mime("audio/wav"), AttachmentKind::Audio);
        assert!(skip_name("/Users/me/.DS_Store"));
        assert!(!skip_name("readme.md"));
        assert_eq!(fallback_name("image/jpg"), "image.jpg");
    }
}
