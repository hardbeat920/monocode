//! Session-scoped attachment previews. A host attachment path is never a
//! desktop filesystem path, even when both machines happen to use `/Users`.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const PREFIX: &str = "remote-attachment://";
const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 512 * 1024;

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn decode(value: &str) -> Option<String> {
    url::form_urlencoded::parse(format!("id={value}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
}

pub fn is_attachment_source(value: &str) -> bool {
    value.starts_with(PREFIX)
}

pub struct AttachmentSource {
    pub environment: String,
    pub session: String,
    pub id: String,
    pub size: usize,
}
pub fn parse_source(value: &str) -> Option<AttachmentSource> {
    let (path, size) = value.strip_prefix(PREFIX)?.split_once("?size=")?;
    let size = size
        .parse::<usize>()
        .ok()
        .filter(|size| *size <= MAX_ATTACHMENT_BYTES)?;
    let mut parts = path.split('/');
    let environment = decode(parts.next()?)?;
    let session = decode(parts.next()?)?;
    let id = decode(parts.next()?)?;
    if parts.next().is_some() || environment.is_empty() || session.is_empty() || id.is_empty() {
        return None;
    }
    Some(AttachmentSource {
        environment,
        session,
        id,
        size,
    })
}

pub fn attachment_source_size(value: &str) -> Option<usize> {
    parse_source(value).map(|source| source.size)
}

/// Apply to a projection clone, retaining the host's original sync snapshot.
/// Invalid attachment references lose their host path as well.
pub fn qualify_snapshot(snapshot: &mut Value, environment: &str, session_id: &str) {
    if let Some(blocks) = snapshot["session"]["blocks"].as_array_mut() {
        for block in blocks {
            if block["role"] == "user" {
                block["nativeRemoteAttachments"] = Value::Bool(true);
                if !block["attachments"].is_array() {
                    block["attachments"] = json!([]);
                }
            }
            if let Some(files) = block["attachments"].as_array_mut() {
                for file in files {
                    if file["data"].as_str().is_some_and(is_attachment_source) {
                        // Only this desktop creates transport source URIs.
                        // Embedded host data remains ordinary base64 content.
                        if let Some(object) = file.as_object_mut() {
                            object.remove("data");
                        }
                    }
                    let source = file["id"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .zip(
                            file["size"]
                                .as_u64()
                                .filter(|size| *size <= MAX_ATTACHMENT_BYTES as u64),
                        )
                        .filter(|_| !environment.is_empty() && !session_id.is_empty())
                        .map(|(id, size)| {
                            format!(
                                "{PREFIX}{}/{}/{}?size={size}",
                                encode(environment),
                                encode(session_id),
                                encode(id)
                            )
                        });
                    file["path"] = source.map(Value::String).unwrap_or(Value::Null);
                    if let Some(object) = file.as_object_mut() {
                        object.remove("previewUrl");
                    }
                }
            }
        }
    }
}

pub fn read_attachment_source(value: &str) -> Result<Vec<u8>, String> {
    let source = parse_source(value).ok_or("Invalid remote image attachment")?;
    let connections = super::default_connections()?;
    let machine = connections.machine_for_environment(&source.environment)?;
    download(source.size, |offset| {
        connections.request(
            &machine.id,
            "attachments.read",
            json!({"sessionId":source.session,"id":source.id,"offset":offset}),
        )
    })
}

fn download(
    size: usize,
    mut request: impl FnMut(usize) -> Result<Value, String>,
) -> Result<Vec<u8>, String> {
    if size > MAX_ATTACHMENT_BYTES {
        return Err("Image is larger than 20 MB.".into());
    }
    let mut bytes = Vec::with_capacity(size);
    while bytes.len() < size {
        let chunk = request(bytes.len())?;
        let encoded = chunk["data"]
            .as_str()
            .filter(|data| data.len() <= MAX_CHUNK_BYTES.div_ceil(3) * 4)
            .ok_or("Invalid image transfer")?;
        let offset = chunk["offset"]
            .as_u64()
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or("Invalid image transfer")?;
        let data = STANDARD
            .decode(encoded)
            .map_err(|_| "Invalid image transfer")?;
        if chunk["size"].as_u64() != Some(size as u64)
            || offset <= bytes.len()
            || offset > size
            || data.len() > MAX_CHUNK_BYTES
            || data.len() != offset - bytes.len()
            || (offset < size && data.len() % 3 != 0)
        {
            return Err("Invalid image transfer".into());
        }
        bytes.extend_from_slice(&data);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_never_treats_host_paths_as_local_desktop_images() {
        let mut snapshot = json!({"session":{"blocks":[{"role":"user","text":"image","attachments":[
            {"id":"image/id","size":3,"kind":"image","path":"/Users/me/secret.png","previewUrl":"file:///Users/me/secret.png"},
            {"id":"broken","path":"/Users/me/secret.png"}
        ]}]}});
        qualify_snapshot(&mut snapshot, "host/id", "session/id");
        let file = &snapshot["session"]["blocks"][0]["attachments"][0];
        assert!(!file["path"].as_str().unwrap().contains("/Users/"));
        assert!(file.get("previewUrl").is_none());
        let source = parse_source(file["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            (source.environment, source.session, source.id, source.size),
            ("host/id".into(), "session/id".into(), "image/id".into(), 3)
        );
        assert!(snapshot["session"]["blocks"][0]["attachments"][1]["path"].is_null());
    }

    #[test]
    fn remote_user_links_cannot_trigger_legacy_local_attachment_previews() {
        let mut snapshot = json!({"session":{"blocks":[{"role":"user","text":"A host message\n\n[secret.png](</Users/me/secret.png>)"}]}});
        qualify_snapshot(&mut snapshot, "host", "session");
        let (history, _) = crate::store::Store::project_remote_history(&snapshot).unwrap();
        assert_eq!(history.rows.len(), 2);
        assert_eq!(history.rows[1].role, crate::transcript::attachments::ROLE);
        assert_eq!(history.rows[1].text, "[]");
    }

    #[test]
    fn remote_plan_projection_keeps_its_card_between_the_source_messages() {
        let mut snapshot = json!({"session":{"blocks":[
            {"id":"user","role":"user","text":"Plan it"},
            {"id":"approved","role":"plan","text":"# Exact plan\n\n- Step","streaming":false,"plan":{"status":"ready","future":"preserved"}},
            {"id":"answer","role":"assistant","text":"Ready for review"}
        ]}});
        qualify_snapshot(&mut snapshot, "host", "session");
        let before = snapshot.clone();
        let (history, _) = crate::store::Store::project_remote_history(&snapshot).unwrap();
        assert_eq!(snapshot, before);
        assert_eq!(history.rows[2].role, "remote-plan");
        let plan: Value = serde_json::from_str(&history.rows[2].text).unwrap();
        assert_eq!(plan["id"], "approved");
        assert_eq!(plan["text"], "# Exact plan\n\n- Step");
        assert_eq!(plan["status"], "ready");
        assert_eq!(history.rows[3].role, "assistant");
    }
    #[test]
    fn bounded_chunked_downloads_reject_changed_sizes_and_nonprogressing_offsets() {
        let data = download(5, |offset| {
            Ok(if offset == 0 {
                json!({"size":5,"offset":3,"data":STANDARD.encode(b"abc")})
            } else {
                json!({"size":5,"offset":5,"data":STANDARD.encode(b"de")})
            })
        })
        .unwrap();
        assert_eq!(data, b"abcde");
        assert!(download(3, |_| Ok(json!({"size":4,"offset":3,"data":"YWJj"}))).is_err());
        assert!(download(3, |_| Ok(json!({"size":3,"offset":0,"data":""}))).is_err());
        assert!(download(4, |_| Ok(json!({"size":4,"offset":2,"data":"YWI="}))).is_err());
        assert!(download(MAX_ATTACHMENT_BYTES + 1, |_| unreachable!()).is_err());
    }
}
