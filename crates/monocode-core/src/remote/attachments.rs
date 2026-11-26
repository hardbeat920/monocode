//! Session-scoped attachment previews. A host attachment path is never a
//! desktop filesystem path, even when both machines happen to use `/Users`.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const PREFIX: &str = "remote-attachment://";
const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 512 * 1024;
const MAX_HTTP_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_HTTP_IMAGE_REDIRECTS: usize = 5;

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

fn percent_decode_path(input: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let raw = input.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let (Some(hi), Some(lo)) = (hex(raw[i + 1]), hex(raw[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decodes a `file://` URI (preserving UTF-8, spaces, `+`, and Windows drives)
/// or returns a raw filesystem path unchanged.
pub fn decode_file_url_or_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("file:") {
        if let Ok(parsed) = url::Url::parse(trimmed) {
            if parsed.scheme() == "file" {
                let mut decoded = percent_decode_path(parsed.path());
                if let Some(host) = parsed.host_str() {
                    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                        decoded = format!("//{host}{decoded}");
                    }
                }
                let bytes = decoded.as_bytes();
                if bytes.len() >= 3
                    && bytes[0] == b'/'
                    && bytes[1].is_ascii_alphabetic()
                    && bytes[2] == b':'
                {
                    decoded.remove(0);
                }
                return (!decoded.is_empty()).then_some(decoded);
            }
        }
        let rest = trimmed.get(5..).unwrap_or("").trim_start_matches("//");
        let rest = if rest.len() >= 10 && rest[..10].eq_ignore_ascii_case("localhost/") {
            &rest[9..]
        } else {
            rest
        };
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let decoded = percent_decode_path(rest);
        return (!decoded.is_empty()).then_some(decoded);
    }
    Some(trimmed.to_owned())
}

pub fn detect_image_mime(bytes: &[u8], hinted: Option<&str>, path_or_url: &str) -> Option<String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png".into());
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("image/jpeg".into());
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif".into());
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp".into());
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp".into());
    }
    if bytes.starts_with(b"\x00\x00\x01\x00") {
        return Some("image/x-icon".into());
    }
    let head_len = bytes.len().min(512);
    if let Ok(head) = std::str::from_utf8(&bytes[..head_len]) {
        let trimmed = head.trim_start();
        if trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && trimmed.contains("<svg"))
        {
            return Some("image/svg+xml".into());
        }
    }
    if let Some(hint) = hinted {
        let clean = hint
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if clean.starts_with("image/") {
            return Some(clean);
        }
    }
    let clean_path = path_or_url.split(['?', '#']).next().unwrap_or(path_or_url);
    let ext = clean_path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png".into()),
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "gif" => Some("image/gif".into()),
        "webp" => Some("image/webp".into()),
        "bmp" => Some("image/bmp".into()),
        "ico" => Some("image/x-icon".into()),
        "svg" => Some("image/svg+xml".into()),
        _ => None,
    }
}

pub fn read_remote_image(
    environment: &str,
    cwd: &str,
    path: &str,
) -> Result<(Vec<u8>, String), String> {
    let connections = super::default_connections()?;
    let machine = connections.machine_for_environment(environment)?;
    let host_path = if path.starts_with('/') || (path.len() >= 2 && path.as_bytes()[1] == b':') {
        path.to_owned()
    } else {
        format!(
            "{}/{}",
            cwd.trim_end_matches('/'),
            path.trim_start_matches("./")
        )
    };
    let qualified = super::remote_path(environment, &host_path);
    let encoded = connections.request(
        &machine.id,
        "workspace.run",
        json!({"command": "read_binary_file", "args": {"path": qualified}}),
    )?;
    let encoded = encoded
        .as_str()
        .ok_or_else(|| "Invalid remote image response".to_string())?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("Invalid remote image content: {e}"))?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err("Image is larger than 20 MB.".into());
    }
    let mime = detect_image_mime(&bytes, None, path)
        .ok_or_else(|| "Remote file is not a supported image".to_string())?;
    Ok((bytes, mime))
}

pub fn read_http_image(url: &str) -> Result<(Vec<u8>, String), String> {
    use std::io::Read;
    use std::time::Duration;
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.len() > 8192 {
        return Err("Invalid image URL".into());
    }
    let mut current = url::Url::parse(trimmed).map_err(|_| "Invalid image URL".to_string())?;
    if !matches!(current.scheme(), "http" | "https") {
        return Err("Unsupported image URL scheme".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirects(0)
        .build();
    for _ in 0..=MAX_HTTP_IMAGE_REDIRECTS {
        let response = match agent
            .get(current.as_str())
            .set("Accept", "image/*,*/*;q=0.1")
            .set("User-Agent", "MonoCode")
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response))
                if matches!(status, 301 | 302 | 303 | 307 | 308) =>
            {
                response
            }
            Err(ureq::Error::Status(status, _)) => {
                return Err(format!("Image request failed ({status})"));
            }
            Err(e) => return Err(format!("Failed to fetch image: {e}")),
        };
        if matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("Location")
                .ok_or_else(|| "Image redirect is missing a Location header".to_string())?;
            let next = current
                .join(location)
                .map_err(|_| "Invalid redirect URL".to_string())?;
            if !matches!(next.scheme(), "http" | "https") {
                return Err("Unsupported redirect URL scheme".into());
            }
            current = next;
            continue;
        }
        let hinted = response.header("Content-Type").map(str::to_owned);
        if let Some(ref ct) = hinted {
            let lower = ct
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if lower.starts_with("text/html")
                || lower.starts_with("text/javascript")
                || lower == "application/javascript"
                || lower == "application/xhtml+xml"
            {
                return Err("URL did not return an image".into());
            }
        }
        if let Some(len) = response
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
        {
            if len > MAX_HTTP_IMAGE_BYTES {
                return Err("Image is larger than 10 MB.".into());
            }
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_HTTP_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed to read image: {e}"))?;
        if bytes.len() as u64 > MAX_HTTP_IMAGE_BYTES {
            return Err("Image is larger than 10 MB.".into());
        }
        let mime = detect_image_mime(&bytes, hinted.as_deref(), current.as_str())
            .ok_or_else(|| "URL did not return a supported image".to_string())?;
        return Ok((bytes, mime));
    }
    Err("Too many image redirects".into())
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

    #[test]
    fn decode_file_urls_preserves_utf8_spaces_plus_and_windows_drives() {
        assert_eq!(
            decode_file_url_or_path("file:///Users/me/%E6%B5%8B%E8%AF%95%20dir/img+1.jpg")
                .as_deref(),
            Some("/Users/me/测试 dir/img+1.jpg")
        );
        assert_eq!(
            decode_file_url_or_path("file://localhost/tmp/a%20b.png").as_deref(),
            Some("/tmp/a b.png")
        );
        assert_eq!(
            decode_file_url_or_path("file:///C:/Users/me/my%20shot.png").as_deref(),
            Some("C:/Users/me/my shot.png")
        );
        assert_eq!(
            decode_file_url_or_path("./relative/pic.webp").as_deref(),
            Some("./relative/pic.webp")
        );
    }

    #[test]
    fn detect_image_mime_prefers_magic_bytes_over_extension() {
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1a\n1234", None, "file.jpg").as_deref(),
            Some("image/png")
        );
        assert_eq!(
            detect_image_mime(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>", None, "x")
                .as_deref(),
            Some("image/svg+xml")
        );
        assert!(detect_image_mime(b"<html>nope</html>", Some("text/html"), "x").is_none());
    }
}
