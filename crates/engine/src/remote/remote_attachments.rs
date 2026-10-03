//! Port of src/features/connections/model/remoteAttachments.ts: copy local
//! attachment bytes to the host before a turn or draft refers to them.

use monocode_core::Attachment;
use monocode_remote::host::protocol::RemoteAttachment;
use serde::Deserialize;
use serde_json::json;

use super::client::RemoteClient;

const MAX_BYTES: i64 = 20 * 1024 * 1024;
/// Keep each request well below the host's 4 MiB JSON limit.
const CHUNK_CHARS: usize = 4 * ((512 * 1024) / 3);

#[derive(Deserialize)]
struct UploadReply {
    offset: i64,
}

/// `uploadRemoteAttachments`: upload each file in base64 pieces and return
/// the host's references to them.
pub async fn upload_remote_attachments(
    client: &RemoteClient,
    machine_id: &str,
    attachments: &[Attachment],
) -> Result<Vec<RemoteAttachment>, String> {
    if attachments.len() > 20 {
        return Err("Too many attachments".into());
    }
    let mut uploaded = Vec::new();
    for file in attachments {
        if file.size > MAX_BYTES {
            return Err(format!(
                "{} is too large to send to a remote machine (20 MB maximum)",
                file.name
            ));
        }
        let data = match (
            &file.data,
            file.path.as_deref().filter(|path| !path.is_empty()),
        ) {
            (Some(data), _) => Some(data.clone()),
            (None, Some(path)) => Some(client.read_file_base64(path.to_string()).await?),
            (None, None) => None,
        };
        let Some(data) = data else {
            return Err(format!("Cannot read {} for remote upload", file.name));
        };
        let mut offset = 0;
        if data.is_empty() {
            client
                .request(
                    machine_id,
                    "attachments.upload",
                    json!({ "id": file.id, "offset": 0, "size": file.size, "data": "" }),
                )
                .await?;
        }
        // Base64 is ASCII, so byte offsets are character offsets.
        for chunk in data.as_bytes().chunks(CHUNK_CHARS) {
            let chunk = std::str::from_utf8(chunk).map_err(|error| error.to_string())?;
            let reply = client
                .request(
                    machine_id,
                    "attachments.upload",
                    json!({ "id": file.id, "offset": offset, "size": file.size, "data": chunk }),
                )
                .await?;
            offset = serde_json::from_value::<UploadReply>(reply)
                .map_err(|_| format!("Could not finish uploading {}", file.name))?
                .offset;
        }
        if offset != file.size {
            return Err(format!("Could not finish uploading {}", file.name));
        }
        uploaded.push(RemoteAttachment {
            id: file.id.clone(),
            name: file.name.clone(),
            mime_type: file.mime_type.clone(),
            kind: file.kind,
            size: file.size,
        });
    }
    Ok(uploaded)
}
