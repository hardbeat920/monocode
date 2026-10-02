use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use url::{Host, Url};

const HTTP_TIMEOUT: Duration = Duration::from_secs(7);
const MAX_REDIRECTS: usize = 5;
const MAX_PAGE_BYTES: usize = 512 * 1024;
const MAX_ICON_BYTES: usize = 256 * 1024;
const USER_AGENT: &str = "Mozilla/5.0 (compatible; MonoCode-LinkPreview/1.0)";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreviewMetadata {
    title: Option<String>,
    favicon_data_url: Option<String>,
}

struct FetchedResource {
    final_url: Url,
    content_type: Option<String>,
    bytes: Vec<u8>,
}

/// Fetch metadata in the native host so the webview's deliberately narrow CSP
/// can remain intact. Every hop is checked before a request is made.
#[tauri::command]
pub async fn fetch_link_preview(url: String) -> Result<LinkPreviewMetadata, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_link_preview_sync(&url))
        .await
        .map_err(|error| error.to_string())?
}

fn fetch_link_preview_sync(raw: &str) -> Result<LinkPreviewMetadata, String> {
    let url = parse_public_url(raw)?;
    let page = fetch_resource(url, "text/html,application/xhtml+xml;q=0.9", MAX_PAGE_BYTES)?;
    if !is_html(&page.content_type) {
        return Ok(LinkPreviewMetadata {
            title: None,
            favicon_data_url: None,
        });
    }

    let html = String::from_utf8_lossy(&page.bytes);
    let title = document_title(&html);
    let default_icon = page
        .final_url
        .join("/favicon.ico")
        .map_err(|_| "Could not resolve this site's icon".to_string())?;
    let linked_icon = document_icon_href(&html)
        .and_then(|href| page.final_url.join(&href).ok())
        .filter(is_http_url);

    let favicon_data_url = linked_icon
        .and_then(fetch_icon_data_url)
        .or_else(|| fetch_icon_data_url(default_icon));

    Ok(LinkPreviewMetadata {
        title,
        favicon_data_url,
    })
}

fn fetch_icon_data_url(url: Url) -> Option<String> {
    let icon = fetch_resource(url, "image/*,*/*;q=0.1", MAX_ICON_BYTES).ok()?;
    let mime = image_mime(icon.content_type.as_deref(), &icon.bytes)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(icon.bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn fetch_resource(mut url: Url, accept: &str, max_bytes: usize) -> Result<FetchedResource, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .redirects(0)
        .build();

    for _ in 0..=MAX_REDIRECTS {
        validate_public_target(&url)?;
        let response = match agent
            .get(url.as_str())
            .set("Accept", accept)
            .set("User-Agent", USER_AGENT)
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) if is_redirect(status) => response,
            Err(ureq::Error::Status(status, _)) => {
                return Err(format!("Link preview request failed ({status})"));
            }
            Err(_) => return Err("Could not load link preview".into()),
        };

        if is_redirect(response.status()) {
            let location = response
                .header("Location")
                .ok_or_else(|| "Link preview redirect has no destination".to_string())?;
            url = url
                .join(location)
                .map_err(|_| "Link preview redirect is invalid".to_string())?;
            if !is_http_url(&url) {
                return Err("Link preview redirect is not HTTP".into());
            }
            continue;
        }

        if !(200..300).contains(&response.status()) {
            return Err(format!(
                "Link preview request failed ({})",
                response.status()
            ));
        }
        if let Some(length) = response
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok())
        {
            if length > max_bytes {
                return Err("Link preview response is too large".into());
            }
        }
        let content_type = response.header("Content-Type").map(str::to_string);
        let mut reader = response.into_reader().take(max_bytes as u64 + 1);
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read link preview".to_string())?;
        if bytes.len() > max_bytes {
            return Err("Link preview response is too large".into());
        }
        return Ok(FetchedResource {
            final_url: url,
            content_type,
            bytes,
        });
    }

    Err("Too many link preview redirects".into())
}

fn parse_public_url(raw: &str) -> Result<Url, String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 8192 {
        return Err("Link preview URL is invalid".into());
    }
    let url = Url::parse(value).map_err(|_| "Link preview URL is invalid".to_string())?;
    if !is_http_url(&url)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err("Link preview URL is invalid".into());
    }
    validate_public_target(&url)?;
    Ok(url)
}

fn is_http_url(url: &Url) -> bool {
    url.scheme() == "https" || url.scheme() == "http"
}

fn validate_public_target(url: &Url) -> Result<(), String> {
    if !is_http_url(url) || !url.username().is_empty() || url.password().is_some() {
        return Err("Link preview target is invalid".into());
    }
    let host = match url
        .host()
        .ok_or_else(|| "Link preview target is invalid".to_string())?
    {
        Host::Ipv4(ip) => return public_ip_result(IpAddr::V4(ip)),
        Host::Ipv6(ip) => return public_ip_result(IpAddr::V6(ip)),
        Host::Domain(host) => host.trim_end_matches('.').to_ascii_lowercase(),
    };
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err("Local addresses cannot be previewed".into());
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Link preview target has no port".to_string())?;
    let addresses: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| "Could not resolve link preview host".to_string())?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("Private addresses cannot be previewed".into());
    }
    Ok(())
}

fn public_ip_result(ip: IpAddr) -> Result<(), String> {
    if is_public_ip(ip) {
        Ok(())
    } else {
        Err("Private addresses cannot be previewed".into())
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || segments[0] & 0xffc0 == 0xfec0
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
    {
        return false;
    }
    true
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn is_html(content_type: &Option<String>) -> bool {
    content_type.as_deref().is_none_or(|value| {
        let mime = value.split(';').next().unwrap_or("").trim();
        mime.eq_ignore_ascii_case("text/html") || mime.eq_ignore_ascii_case("application/xhtml+xml")
    })
}

fn image_mime(content_type: Option<&str>, bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some("image/x-icon");
    }
    match content_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("image/vnd.microsoft.icon") | Some("image/x-icon") => Some("image/x-icon"),
        _ => None,
    }
}

fn document_title(html: &str) -> Option<String> {
    for key in ["og:title", "twitter:title"] {
        for tag in opening_tags(html, "meta") {
            let marker = attribute(tag, "property").or_else(|| attribute(tag, "name"));
            if marker
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(key))
            {
                if let Some(value) = attribute(tag, "content").and_then(clean_text) {
                    return Some(value);
                }
            }
        }
    }

    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let opening_end = tag_end(html, start)?;
    let close = lower[opening_end + 1..].find("</title>")? + opening_end + 1;
    clean_text(html[opening_end + 1..close].to_string())
}

fn document_icon_href(html: &str) -> Option<String> {
    opening_tags(html, "link").into_iter().find_map(|tag| {
        let rel = attribute(tag, "rel")?;
        if !rel
            .split_ascii_whitespace()
            .any(|part| part.eq_ignore_ascii_case("icon"))
        {
            return None;
        }
        attribute(tag, "href").filter(|href| !href.trim().is_empty())
    })
}

fn opening_tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut tags = Vec::new();
    let mut offset = 0;
    while let Some(relative) = lower[offset..].find(&needle) {
        let start = offset + relative;
        let after = start + needle.len();
        let boundary = lower.as_bytes().get(after).copied();
        if boundary.is_some_and(|byte| byte.is_ascii_whitespace() || byte == b'/' || byte == b'>') {
            if let Some(end) = tag_end(html, start) {
                tags.push(&html[start..=end]);
                offset = end + 1;
                continue;
            }
        }
        offset = after;
    }
    tags
}

fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (relative, byte) in html.as_bytes()[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(expected), value) if value == expected => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(start + relative),
            _ => {}
        }
    }
    None
}

fn attribute(tag: &str, wanted: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut cursor = 1;
    while cursor < bytes.len() && is_attribute_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    while cursor < bytes.len() {
        while cursor < bytes.len() && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/')
        {
            cursor += 1;
        }
        let name_start = cursor;
        while cursor < bytes.len() && is_attribute_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            cursor += 1;
            continue;
        }
        let name = &tag[name_start..cursor];
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let (value_start, value_end) = match bytes.get(cursor).copied() {
            Some(quote @ (b'\'' | b'"')) => {
                cursor += 1;
                let start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                (start, cursor)
            }
            Some(_) => {
                let start = cursor;
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && bytes[cursor] != b'>'
                {
                    cursor += 1;
                }
                (start, cursor)
            }
            None => return None,
        };
        if name.eq_ignore_ascii_case(wanted) {
            return Some(decode_entities(&tag[value_start..value_end]));
        }
        cursor = cursor.saturating_add(1);
    }
    None
}

fn is_attribute_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn clean_text(value: String) -> Option<String> {
    let mut without_tags = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => without_tags.push(character),
            _ => {}
        }
    }
    let decoded = decode_entities(&without_tags);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(240).collect())
}

fn decode_entities(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('&') {
        result.push_str(&rest[..index]);
        let entity_start = &rest[index + 1..];
        let Some(end) = entity_start.find(';').filter(|end| *end <= 12) else {
            result.push('&');
            rest = entity_start;
            continue;
        };
        let entity = &entity_start[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            value if value.starts_with("#x") || value.starts_with("#X") => {
                u32::from_str_radix(&value[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            value if value.starts_with('#') => value[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(character) = decoded {
            result.push(character);
        } else {
            result.push('&');
            result.push_str(entity);
            result.push(';');
        }
        rest = &entity_start[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_social_title_and_icon_attributes_in_any_order() {
        let html = r#"
          <html><head>
            <title>Fallback &amp; title</title>
            <meta content="A nicer &quot;title&quot;" property="og:title">
            <link href="/assets/icon.png" sizes="32x32" rel="shortcut icon">
          </head></html>
        "#;
        assert_eq!(document_title(html).as_deref(), Some("A nicer \"title\""));
        assert_eq!(
            document_icon_href(html).as_deref(),
            Some("/assets/icon.png")
        );
    }

    #[test]
    fn falls_back_to_the_document_title() {
        let html = "<TITLE>  Example <b>docs</b> &amp; API  </TITLE>";
        assert_eq!(document_title(html).as_deref(), Some("Example docs & API"));
    }

    #[test]
    fn rejects_local_and_special_networks() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "172.20.1.1",
            "192.168.1.1",
            "100.64.0.1",
            "192.0.2.1",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(!is_public_ip(address.parse().unwrap()), "{address}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn accepts_only_plain_http_urls() {
        assert!(parse_public_url("file:///tmp/page.html").is_err());
        assert!(parse_public_url("https://user:secret@example.com").is_err());
        assert!(parse_public_url("http://localhost:3000").is_err());
        assert!(parse_public_url("http://[::1]:3000").is_err());
    }
}
