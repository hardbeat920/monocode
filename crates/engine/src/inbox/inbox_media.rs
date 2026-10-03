//! Port of src/features/inbox/model/inboxMedia.ts: which remote image and
//! video URLs in issue bodies the inbox may load, the content sniffing, and
//! the fetch cache. The host fetcher in monocode-integrations checks the
//! host again.

use std::sync::Arc;

use futures::FutureExt;
use monocode_core::js;

use super::client::{InboxClient, Pending};

/// `INBOX_MEDIA_PREFIXES`: the URL prefixes GitHub and Linear put in
/// markdown sources, not the CDNs they redirect to.
pub const INBOX_MEDIA_PREFIXES: [&str; 11] = [
    "https://github.com/user-attachments/",
    "https://www.github.com/user-attachments/",
    "https://user-images.githubusercontent.com/",
    "https://private-user-images.githubusercontent.com/",
    "https://objects.githubusercontent.com/",
    "https://media.githubusercontent.com/",
    "https://camo.githubusercontent.com/",
    "https://avatars.githubusercontent.com/",
    "https://raw.githubusercontent.com/",
    "https://gist.githubusercontent.com/",
    "https://uploads.linear.app/",
];

/// `InboxMediaKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxMediaKind {
    Image,
    Video,
}

/// `InboxMediaType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxMediaType {
    pub kind: InboxMediaKind,
    pub mime: &'static str,
}

fn path_has_dot_dot(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(
            segment.to_lowercase().as_str(),
            ".." | "%2e%2e" | "%2e." | ".%2e"
        )
    })
}

/// `isInboxMediaUrl`: remote image and video URLs GitHub and Linear put in
/// issue bodies.
pub fn is_inbox_media_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(js::trim(value)) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_lowercase();
    if path_has_dot_dot(url.path()) {
        return false;
    }
    if host == "uploads.linear.app" || host.ends_with(".uploads.linear.app") {
        return true;
    }
    if host == "githubusercontent.com" || host.ends_with(".githubusercontent.com") {
        return true;
    }
    if host != "github.com" && host != "www.github.com" {
        return false;
    }
    let path = url.path().to_lowercase();
    if path.starts_with("/user-attachments/") {
        return true;
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    parts.len() >= 4
        && parts[2] == "assets"
        && !parts[3].is_empty()
        && parts[3].bytes().all(|b| b.is_ascii_digit())
}

/// `sniffImageMime` from src/features/files/model/filePreview.ts.
// TODO(port): the workspace package ports filePreview.ts. Use its copy once
// the inbox may depend on that package.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return Some("image/gif");
    }
    if bytes.starts_with(&[0x42, 0x4d]) {
        return Some("image/bmp");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    // RIFF....WEBP: the four size bytes at offset 4 are skipped.
    if bytes.starts_with(b"RIFF") && bytes.get(8..).is_some_and(|rest| rest.starts_with(b"WEBP")) {
        return Some("image/webp");
    }
    // ....ftyp{avif,avis}: an ISO base media box, shared with HEIF and MP4.
    if bytes.get(4..).is_some_and(|rest| rest.starts_with(b"ftyp")) {
        let brand = bytes.get(8..12.min(bytes.len())).unwrap_or_default();
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
    }
    None
}

fn sniff_video_type(bytes: &[u8]) -> Option<InboxMediaType> {
    if bytes.len() >= 12 && bytes[4..].starts_with(b"ftyp") {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return None;
        }
        return Some(InboxMediaType {
            kind: InboxMediaKind::Video,
            mime: if brand == b"qt  " {
                "video/quicktime"
            } else {
                "video/mp4"
            },
        });
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(InboxMediaType {
            kind: InboxMediaKind::Video,
            mime: "video/webm",
        });
    }
    None
}

/// `sniffInboxMedia`.
pub fn sniff_inbox_media(bytes: &[u8]) -> Option<InboxMediaType> {
    if let Some(mime) = sniff_image_mime(bytes) {
        return Some(InboxMediaType {
            kind: InboxMediaKind::Image,
            mime,
        });
    }
    sniff_video_type(bytes)
}

impl InboxClient {
    /// `fetchInboxMedia`: one shared request per URL. A failure leaves the
    /// cache so the next render retries.
    pub fn fetch_inbox_media(&self, url: &str) -> Pending<Arc<Vec<u8>>> {
        let key = js::trim(url).to_string();
        // Hold the lock until the entry is in place, so a fast failure
        // cannot remove it before it exists.
        let mut state = self.state();
        if let Some(cached) = state.media.get(&key) {
            return cached.clone();
        }
        let fetch = self.backend().fetch_media(&key);
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = fetch.map(|result| result.map(Arc::new)).await;
            if result.is_err() {
                client.state().media.remove(&cache_key);
            }
            result
        });
        state.media.insert(key, pending.clone());
        pending
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use serde_json::json;

    use super::*;
    use crate::inbox::client::test_support::{client, settle};

    #[test]
    fn allows_github_and_linear_attachment_hosts() {
        assert!(is_inbox_media_url(
            "https://github.com/user-attachments/assets/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        ));
        assert!(is_inbox_media_url(
            "https://github.com/acme/web/assets/12/aaaaaaaa-bbbb"
        ));
        assert!(is_inbox_media_url(
            "https://user-images.githubusercontent.com/1/shot.png"
        ));
        assert!(is_inbox_media_url(
            "https://uploads.linear.app/org/uuid/file.png"
        ));
    }

    #[test]
    fn rejects_pages_other_hosts_and_traversal() {
        assert!(!is_inbox_media_url("https://github.com/acme/web/issues/1"));
        assert!(!is_inbox_media_url(
            "https://github.com/user-attachments/../login"
        ));
        assert!(!is_inbox_media_url(
            "http://github.com/user-attachments/assets/x"
        ));
        assert!(!is_inbox_media_url("https://evil.example/shot.png"));
        assert!(!is_inbox_media_url(
            "https://github.com.evil.com/user-attachments/assets/x"
        ));
    }

    #[test]
    fn prefixes_stay_on_https_attachment_hosts() {
        assert!(
            INBOX_MEDIA_PREFIXES
                .iter()
                .all(|prefix| prefix.starts_with("https://"))
        );
        assert!(
            INBOX_MEDIA_PREFIXES
                .iter()
                .any(|prefix| prefix.starts_with("https://github.com/user-attachments/"))
        );
    }

    #[test]
    fn keeps_images_and_recognizes_mp4_and_webm() {
        assert_eq!(
            sniff_inbox_media(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
            Some(InboxMediaType {
                kind: InboxMediaKind::Image,
                mime: "image/png"
            })
        );
        assert_eq!(
            sniff_inbox_media(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d
            ]),
            Some(InboxMediaType {
                kind: InboxMediaKind::Video,
                mime: "video/mp4"
            })
        );
        assert_eq!(
            sniff_inbox_media(&[0x1a, 0x45, 0xdf, 0xa3, 1, 2, 3, 4]),
            Some(InboxMediaType {
                kind: InboxMediaKind::Video,
                mime: "video/webm"
            })
        );
    }

    #[test]
    fn does_not_treat_avif_pdf_or_html_as_video() {
        assert_eq!(
            sniff_inbox_media(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66
            ]),
            Some(InboxMediaType {
                kind: InboxMediaKind::Image,
                mime: "image/avif"
            })
        );
        assert_eq!(
            sniff_inbox_media(&[0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37]),
            None
        );
        assert_eq!(sniff_inbox_media(b"<html><script>x()</script>"), None);
    }

    #[gpui::test]
    fn caches_media_and_retries_after_a_failure(cx: &mut TestAppContext) {
        let (client, backend) = client(cx, |_, args| {
            if args["url"] == "https://bad.example/x" {
                Err("Media request failed (404)".into())
            } else {
                Ok(json!([1, 2, 3]))
            }
        });
        let first = settle(cx, client.fetch_inbox_media(" https://good.example/x ")).unwrap();
        assert_eq!(*first, vec![1, 2, 3]);
        settle(cx, client.fetch_inbox_media("https://good.example/x")).unwrap();
        assert!(settle(cx, client.fetch_inbox_media("https://bad.example/x")).is_err());
        assert!(settle(cx, client.fetch_inbox_media("https://bad.example/x")).is_err());
        assert_eq!(
            backend.media_calls(),
            [
                "https://good.example/x",
                "https://bad.example/x",
                "https://bad.example/x"
            ]
        );
    }
}
