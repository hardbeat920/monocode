//! Port of host/attachments.ts.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::{GeneralPurpose, GeneralPurposeConfig};
use monocode_core::{Attachment, AttachmentKind};
use serde::Serialize;
use serde_json::{Map, Value};

use super::js;
use super::protocol::RemoteAttachment;
use super::store::HostStore;

pub const MAX_REMOTE_ATTACHMENT_BYTES: i64 = 20 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 512 * 1024;

/// Node's base64 decoder ignores stray bits in the last symbol.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

/// `/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i`.
pub fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        14 => (b'1'..=b'8').contains(byte),
        19 => matches!(byte.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'),
        _ => byte.is_ascii_hexdigit(),
    })
}

pub fn attachment_path(store: &HostStore, id: &str) -> Result<PathBuf, String> {
    if !is_uuid(id) {
        return Err("Invalid attachment ID".into());
    }
    Ok(store.attachment_dir.join(id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UploadProgress {
    pub offset: i64,
}

fn is_base64_text(text: &str) -> bool {
    let body = text.trim_end_matches('=');
    text.len() - body.len() <= 2
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
}

fn create_dir_private(path: &std::path::Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// An offset makes a repeated chunk safe when its HTTP response was lost.
pub fn write_attachment_chunk(
    store: &HostStore,
    input: &Map<String, Value>,
) -> Result<UploadProgress, String> {
    let path = attachment_path(store, &js::string(input.get("id")))?;
    let offset = js::safe_integer(input.get("offset"));
    let size = js::safe_integer(input.get("size"));
    let encoded = input.get("data").and_then(Value::as_str);
    let (Some(offset), Some(size), Some(encoded)) = (offset, size, encoded) else {
        return Err("Invalid attachment chunk".into());
    };
    if offset < 0
        || !(0..=MAX_REMOTE_ATTACHMENT_BYTES).contains(&size)
        || encoded.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4
        || (!encoded.is_empty() && (!is_base64_text(encoded) || encoded.len() % 4 != 0))
    {
        return Err("Invalid attachment chunk".into());
    }
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| "Invalid attachment chunk".to_string())?;
    if bytes.len() > MAX_CHUNK_BYTES || offset + bytes.len() as i64 > size {
        return Err("Invalid attachment chunk size".into());
    }
    create_dir_private(&store.attachment_dir).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true);
    if offset == 0 {
        options.append(true).create(true);
    } else {
        options.write(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len() as i64;
    let end = offset + bytes.len() as i64;
    if length == offset {
        file.seek(SeekFrom::Start(offset as u64))
            .and_then(|_| file.write_all(&bytes))
            .map_err(|error| error.to_string())?;
    } else if length >= end {
        let mut existing = vec![0; bytes.len()];
        file.seek(SeekFrom::Start(offset as u64))
            .and_then(|_| file.read_exact(&mut existing))
            .map_err(|error| error.to_string())?;
        if existing != bytes {
            return Err("Attachment retry does not match uploaded bytes".into());
        }
    } else {
        return Err("Attachment chunks are out of order".into());
    }
    Ok(UploadProgress { offset: end })
}

pub fn parse_remote_attachments(value: Option<&Value>) -> Result<Vec<RemoteAttachment>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(entries) = value.as_array().filter(|entries| entries.len() <= 20) else {
        return Err("Invalid attachments".into());
    };
    entries
        .iter()
        .map(|entry| {
            let invalid = || "Invalid attachment".to_string();
            let fields = entry.as_object().ok_or_else(invalid)?;
            let text = |key: &str, max: usize| {
                fields
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|text| {
                        !monocode_core::js::trim(text).is_empty()
                            && monocode_core::js::len(text) <= max
                    })
                    .map(str::to_string)
            };
            let id = js::string(fields.get("id"));
            let kind = match fields
                .get("kind")
                .map(|kind| js::string(Some(kind)))
                .as_deref()
            {
                Some("image") => AttachmentKind::Image,
                Some("audio") => AttachmentKind::Audio,
                Some("file") => AttachmentKind::File,
                _ => return Err(invalid()),
            };
            let size = js::safe_integer(fields.get("size"))
                .filter(|size| (0..=MAX_REMOTE_ATTACHMENT_BYTES).contains(size))
                .ok_or_else(invalid)?;
            let (true, Some(name), Some(mime_type)) =
                (is_uuid(&id), text("name", 255), text("mimeType", 128))
            else {
                return Err(invalid());
            };
            Ok(RemoteAttachment {
                id,
                name,
                mime_type,
                kind,
                size,
            })
        })
        .collect()
}

pub fn resolve_attachments(
    store: &HostStore,
    refs: &[RemoteAttachment],
) -> Result<Vec<Attachment>, String> {
    refs.iter()
        .map(|reference| {
            let path = attachment_path(store, &reference.id)?;
            let size = fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len();
            if size as i64 != reference.size {
                return Err(format!("Attachment {} is incomplete", reference.name));
            }
            Ok(Attachment {
                id: reference.id.clone(),
                name: reference.name.clone(),
                mime_type: reference.mime_type.clone(),
                kind: reference.kind,
                size: reference.size,
                path: Some(path.to_string_lossy().into_owned()),
                ..Default::default()
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachmentChunk {
    pub data: String,
    pub offset: i64,
    pub size: i64,
}

/// Reads only an attachment already accepted into this session. Paths
/// supplied by the client are never used, and each response stays below the
/// RPC cap.
pub fn read_attachment_chunk(
    store: &HostStore,
    input: &Map<String, Value>,
) -> Result<AttachmentChunk, String> {
    let session = store.session(&js::string(input.get("sessionId")))?;
    let wanted = input.get("id").and_then(Value::as_str);
    let attachment = session
        .session
        .blocks
        .iter()
        .flat_map(|block| block.attachments.iter().flatten())
        .find(|file| Some(file.id.as_str()) == wanted)
        .filter(|file| file.kind == AttachmentKind::Image)
        .ok_or("Image attachment not found")?;
    let offset = js::safe_integer(input.get("offset"))
        .filter(|offset| (0..=attachment.size).contains(offset))
        .ok_or("Invalid attachment offset")?;
    let path = attachment_path(store, &attachment.id)?;
    let mut file = fs::File::open(&path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() as i64 != attachment.size {
        return Err("Attachment is incomplete".into());
    }
    // Non-final chunks are divisible by three, so the client can join base64.
    let wanted = (3 * (MAX_CHUNK_BYTES / 3)).min((attachment.size - offset) as usize);
    let mut bytes = vec![0; wanted];
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| error.to_string())?;
    let mut read = 0;
    while read < wanted {
        match file.read(&mut bytes[read..]) {
            Ok(0) => break,
            Ok(count) => read += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(AttachmentChunk {
        data: base64::engine::general_purpose::STANDARD.encode(&bytes[..read]),
        offset: offset + read as i64,
        size: attachment.size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::tests::temporary;
    use serde_json::json;

    #[test]
    fn accepts_ordered_chunks_and_an_identical_retry_while_rejecting_changes() {
        let directory = temporary("remote-upload-test-");
        let store = HostStore::open(&directory.path().join("host.db")).unwrap();
        let id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let chunk = |offset: i64, data: &str| {
            let input = json!({
                "id": id,
                "offset": offset,
                "size": 6,
                "data": base64::engine::general_purpose::STANDARD.encode(data),
            });
            write_attachment_chunk(&store, input.as_object().unwrap())
        };
        assert_eq!(chunk(0, "abc").unwrap(), UploadProgress { offset: 3 });
        assert_eq!(chunk(0, "abc").unwrap(), UploadProgress { offset: 3 });
        assert!(chunk(0, "xyz").unwrap_err().contains("does not match"));
        assert!(chunk(4, "ef").unwrap_err().contains("out of order"));
        assert_eq!(chunk(3, "def").unwrap(), UploadProgress { offset: 6 });
        let escape = json!({ "id": "../escape", "offset": 0, "size": 1, "data": "YQ==" });
        assert!(
            write_attachment_chunk(&store, escape.as_object().unwrap())
                .unwrap_err()
                .contains("Invalid attachment ID")
        );
        assert_eq!(fs::read(store.attachment_dir.join(id)).unwrap(), b"abcdef");
    }

    #[test]
    fn validates_attachment_references() {
        let valid = json!([{
            "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
            "name": "notes.txt",
            "mimeType": "text/plain",
            "kind": "file",
            "size": 5
        }]);
        assert_eq!(parse_remote_attachments(Some(&valid)).unwrap()[0].size, 5);
        assert!(parse_remote_attachments(None).unwrap().is_empty());
        for bad in [
            json!({}),
            json!([{ "id": "x", "name": "a", "mimeType": "t", "kind": "file", "size": 1 }]),
            json!([{ "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd", "name": " ", "mimeType": "t", "kind": "file", "size": 1 }]),
            json!([{ "id": "dddddddd-dddd-4ddd-8ddd-dddddddddddd", "name": "a", "mimeType": "t", "kind": "video", "size": 1 }]),
        ] {
            assert!(parse_remote_attachments(Some(&bad)).is_err(), "{bad}");
        }
        assert!(is_uuid("DDDDDDDD-DDDD-4DDD-8DDD-DDDDDDDDDDDD"));
        assert!(!is_uuid("dddddddd-dddd-9ddd-8ddd-dddddddddddd"));
    }
}
