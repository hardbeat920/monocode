//! Port of src/features/connections/model/remoteAttachmentPreviews.ts.
//!
//! Preview bytes live only in desktop snapshots, not in every host database
//! write. Unchanged attachments reuse their previous data across delta
//! syncs, and one download runs per image even when several loads ask.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared, join_all};
use monocode_core::{Attachment, AttachmentKind};
use monocode_remote::host::protocol::HostSession;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Value, json};

use super::transport::RemoteFuture;

/// Images above this size are not previewed.
const MAX_PREVIEW_BYTES: i64 = 20 * 1024 * 1024;

/// One `attachments.read` answer.
#[derive(Debug, Clone, Deserialize)]
struct Chunk {
    data: String,
    offset: i64,
    size: i64,
}

/// `attachments.read` with `{ sessionId, id, offset }`.
pub type ChunkReader = Arc<dyn Fn(Value) -> RemoteFuture<Value> + Send + Sync>;

type Download = Shared<BoxFuture<'static, Result<String, String>>>;

/// `downloads`: image downloads in progress, keyed by machine, session, and
/// attachment.
#[derive(Clone, Default)]
pub struct Downloads(Arc<Mutex<HashMap<String, Download>>>);

/// `atob`, which accepts input with or without padding.
fn atob(data: &str) -> Result<Vec<u8>, String> {
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    LENIENT
        .decode(data.trim())
        .map_err(|_| "Invalid image transfer".to_string())
}

/// `data` or `previewUrl` set to a non-empty string.
fn truthy(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|value| !value.is_empty())
}

async fn download(
    read: ChunkReader,
    session_id: String,
    id: String,
    size: i64,
) -> Result<String, String> {
    let mut pieces = String::new();
    let mut offset = 0;
    while offset < size {
        let value = read(json!({ "sessionId": session_id, "id": id, "offset": offset })).await?;
        let chunk: Chunk =
            serde_json::from_value(value).map_err(|_| "Invalid image transfer".to_string())?;
        let invalid = chunk.size != size
            || chunk.offset <= offset
            || chunk.offset > size
            || atob(&chunk.data)?.len() as i64 != chunk.offset - offset
            || (chunk.offset < size && (chunk.offset - offset) % 3 != 0);
        if invalid {
            return Err("Invalid image transfer".into());
        }
        pieces.push_str(&chunk.data);
        offset = chunk.offset;
    }
    Ok(pieces)
}

/// One attachment with its preview bytes, or `None` when it stays as it is.
async fn preview(
    machine_id: &str,
    session_id: &str,
    file: &Attachment,
    previous: Option<Option<String>>,
    read: &ChunkReader,
    downloads: &Downloads,
) -> Option<Attachment> {
    if file.kind != AttachmentKind::Image
        || truthy(&file.data)
        || truthy(&file.preview_url)
        || file.size > MAX_PREVIEW_BYTES
    {
        return None;
    }
    let data = match previous.flatten() {
        Some(data) => data,
        None => {
            let key = serde_json::to_string(&[machine_id, session_id, file.id.as_str()])
                .unwrap_or_default();
            let shared = {
                let mut map = downloads.0.lock();
                map.entry(key.clone())
                    .or_insert_with(|| {
                        let map = downloads.0.clone();
                        let run = download(
                            read.clone(),
                            session_id.to_string(),
                            file.id.clone(),
                            file.size,
                        );
                        async move {
                            let result = run.await;
                            map.lock().remove(&key);
                            result
                        }
                        .boxed()
                        .shared()
                    })
                    .clone()
            };
            // Missing images must not hide the conversation or mark its host
            // offline.
            shared.await.ok()?
        }
    };
    Some(Attachment {
        data: Some(data),
        ..file.clone()
    })
}

/// `withRemoteAttachmentPreviews`: fill in image bytes for the transcript.
/// Returns `snapshot` itself when nothing changed.
pub async fn with_remote_attachment_previews(
    machine_id: &str,
    snapshot: Arc<HostSession>,
    known: Option<&HostSession>,
    read: ChunkReader,
    downloads: &Downloads,
) -> Arc<HostSession> {
    let mut previous: HashMap<&str, Option<String>> = HashMap::new();
    if let Some(known) = known.filter(|known| known.session.id == snapshot.session.id) {
        for file in known
            .session
            .blocks
            .iter()
            .flat_map(|block| block.attachments.iter().flatten())
        {
            previous.insert(file.id.as_str(), file.data.clone());
        }
    }
    let session_id = snapshot.session.id.as_str();
    let blocks = join_all(snapshot.session.blocks.iter().map(|block| {
        let previous = &previous;
        let read = &read;
        async move {
            let attachments = block.attachments.as_deref().unwrap_or_default();
            if attachments.is_empty() {
                return None;
            }
            let next = join_all(attachments.iter().map(|file| {
                preview(
                    machine_id,
                    session_id,
                    file,
                    previous.get(file.id.as_str()).cloned(),
                    read,
                    downloads,
                )
            }))
            .await;
            if next.iter().all(Option::is_none) {
                return None;
            }
            Some(
                next.into_iter()
                    .zip(attachments)
                    .map(|(next, file)| next.unwrap_or_else(|| file.clone()))
                    .collect::<Vec<_>>(),
            )
        }
    }))
    .await;
    if blocks.iter().all(Option::is_none) {
        return snapshot;
    }
    let mut next = (*snapshot).clone();
    for (block, attachments) in next.session.blocks.iter_mut().zip(blocks) {
        if let Some(attachments) = attachments {
            block.attachments = Some(attachments);
        }
    }
    Arc::new(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn snapshot() -> Arc<HostSession> {
        Arc::new(
            serde_json::from_value(json!({
                "projectId": "project",
                "revision": 1,
                "updatedAt": 0,
                "status": "idle",
                "session": {
                    "id": "session",
                    "cwd": "/repo",
                    "harness": "codex",
                    "model": "codex:test",
                    "modelSettings": {},
                    "runtimeMode": "supervised",
                    "title": "Image",
                    "blocks": [{
                        "id": "turn",
                        "role": "user",
                        "text": "Look",
                        "attachments": [{
                            "id": "image",
                            "name": "shot.png",
                            "mimeType": "image/png",
                            "kind": "image",
                            "size": 5,
                            "path": "/host/image"
                        }]
                    }]
                }
            }))
            .unwrap(),
        )
    }

    fn btoa(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    // remoteAttachmentPreviews.test.ts
    #[test]
    fn reopens_image_previews_from_chunks_and_reuses_bytes_on_subsequent_syncs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let read: ChunkReader = Arc::new(move |params: Value| {
            counter.fetch_add(1, Ordering::SeqCst);
            let reply = if params["offset"] == 0 {
                json!({ "offset": 3, "size": 5, "data": btoa("abc") })
            } else {
                json!({ "offset": 5, "size": 5, "data": btoa("de") })
            };
            Box::pin(async move { Ok(reply) })
        });
        let downloads = Downloads::default();
        let first = futures::executor::block_on(with_remote_attachment_previews(
            "machine",
            snapshot(),
            None,
            read.clone(),
            &downloads,
        ));
        let data = |session: &HostSession| {
            session.session.blocks[0].attachments.as_ref().unwrap()[0]
                .data
                .clone()
        };
        assert_eq!(data(&first), Some(btoa("abcde")));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let next = futures::executor::block_on(with_remote_attachment_previews(
            "machine",
            snapshot(),
            Some(&first),
            read,
            &downloads,
        ));
        assert_eq!(data(&next), Some(btoa("abcde")));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(downloads.0.lock().is_empty());
    }

    // remoteAttachmentPreviews.test.ts
    #[test]
    fn keeps_the_transcript_available_when_an_image_is_missing() {
        let value = snapshot();
        let read: ChunkReader =
            Arc::new(|_| Box::pin(async { Err("Image no longer available".to_string()) }));
        let result = futures::executor::block_on(with_remote_attachment_previews(
            "machine",
            value.clone(),
            None,
            read,
            &Downloads::default(),
        ));
        assert!(Arc::ptr_eq(&result, &value));
    }

    #[test]
    fn rejects_chunks_that_do_not_advance_by_whole_base64_groups() {
        let read: ChunkReader = Arc::new(|_| {
            Box::pin(async { Ok(json!({ "offset": 2, "size": 5, "data": btoa("ab") })) })
        });
        let value = snapshot();
        let result = futures::executor::block_on(with_remote_attachment_previews(
            "machine",
            value.clone(),
            None,
            read,
            &Downloads::default(),
        ));
        assert!(Arc::ptr_eq(&result, &value));
    }
}
