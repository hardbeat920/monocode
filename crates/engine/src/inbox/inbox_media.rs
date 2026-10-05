//! Port of src/features/inbox/model/inboxMedia.ts: which remote image and
//! video URLs in issue bodies the inbox may load, the content sniffing, and
//! the fetch cache. The host fetcher in monocode-integrations checks the
//! host again.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use futures::FutureExt;
use monocode_core::js;

use super::client::{InboxClient, Pending, ready};

/// `MEDIA_CACHE_BYTES`. Each file can be up to 25 MB, so the cache keeps
/// recent bytes under this total instead of every image and video shown.
pub const MEDIA_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// `mediaCache` and `mediaRequests`: fetched files, least recently used
/// first, and the requests still in flight.
#[derive(Default)]
pub(crate) struct MediaCache {
    entries: VecDeque<(String, Arc<Vec<u8>>)>,
    bytes: usize,
    requests: HashMap<String, Pending<Arc<Vec<u8>>>>,
}

impl MediaCache {
    /// A cached file. A hit moves it to the most recently used end.
    fn get(&mut self, key: &str) -> Option<Arc<Vec<u8>>> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        let entry = self.entries.remove(index)?;
        let bytes = entry.1.clone();
        self.entries.push_back(entry);
        Some(bytes)
    }

    /// `rememberMedia`: keep `bytes`, then drop the least recently used
    /// files until the total fits the budget. A file larger than the whole
    /// budget is not kept and evicts nothing.
    fn remember(&mut self, key: String, bytes: Arc<Vec<u8>>) {
        if bytes.len() > MEDIA_CACHE_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|(entry, _)| *entry == key)
            && let Some((_, previous)) = self.entries.remove(index)
        {
            self.bytes -= previous.len();
        }
        self.bytes += bytes.len();
        self.entries.push_back((key, bytes));
        while self.bytes > MEDIA_CACHE_BYTES {
            let Some((_, oldest)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= oldest.len();
        }
    }
}

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
    /// `fetchInboxMedia`: cached bytes, or one shared request per URL. A
    /// failure caches nothing, so the next render retries.
    pub fn fetch_inbox_media(&self, url: &str) -> Pending<Arc<Vec<u8>>> {
        let key = js::trim(url).to_string();
        // Hold the lock until the request is in place, so a fast answer
        // cannot settle before it is registered.
        let mut state = self.state();
        if let Some(bytes) = state.media.get(&key) {
            return ready(Ok(bytes));
        }
        if let Some(pending) = state.media.requests.get(&key) {
            return pending.clone();
        }
        let fetch = self.backend().fetch_media(&key);
        let client = self.clone();
        let cache_key = key.clone();
        let pending = self.spawn_pending(async move {
            let result = fetch.map(|result| result.map(Arc::new)).await;
            let mut state = client.state();
            state.media.requests.remove(&cache_key);
            if let Ok(bytes) = &result {
                state.media.remember(cache_key, bytes.clone());
            }
            result
        });
        state.media.requests.insert(key, pending.clone());
        pending
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gpui::TestAppContext;
    use serde_json::json;

    use super::*;
    use crate::inbox::backend::fake::FakeBackend;
    use crate::inbox::client::test_support::{client, settle, unexpected};

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

    const MB: usize = 1024 * 1024;

    fn url(index: usize) -> String {
        format!("https://github.com/user-attachments/assets/cache-{index}")
    }

    /// A client whose media requests answer with `size(url, call)` zero
    /// bytes, where `call` counts every request from 0.
    fn sized(
        cx: &TestAppContext,
        size: impl Fn(&str, usize) -> Result<usize, String> + Send + Sync + 'static,
    ) -> (InboxClient, Arc<FakeBackend>) {
        let (client, backend) = client(cx, |command, _| unexpected(command));
        let calls = AtomicUsize::new(0);
        backend.set_media(move |url| {
            size(url, calls.fetch_add(1, Ordering::SeqCst)).map(|size| vec![0; size])
        });
        (client, backend)
    }

    fn fetch(cx: &mut TestAppContext, client: &InboxClient, index: usize) -> Arc<Vec<u8>> {
        settle(cx, client.fetch_inbox_media(&url(index))).unwrap()
    }

    #[gpui::test]
    fn shares_a_request_in_flight_and_serves_repeats_from_cache(cx: &mut TestAppContext) {
        let (client, backend) = sized(cx, |_, _| Ok(16));
        let first = client.fetch_inbox_media(&url(100));
        let second = client.fetch_inbox_media(&url(100));
        let first = settle(cx, first).unwrap();
        let second = settle(cx, second).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 100), &first));
        assert_eq!(backend.media_calls().len(), 1);
    }

    #[gpui::test]
    fn keeps_cached_bytes_under_a_total_budget(cx: &mut TestAppContext) {
        let (client, backend) = sized(cx, |_, _| Ok(10 * MB));
        for index in 0..6 {
            fetch(cx, &client, index);
        }
        assert_eq!(backend.media_calls().len(), 6);

        // The newest files are still cached. The oldest were dropped.
        fetch(cx, &client, 5);
        assert_eq!(backend.media_calls().len(), 6);
        fetch(cx, &client, 0);
        assert_eq!(backend.media_calls().len(), 7);
    }

    #[gpui::test]
    fn refreshes_recency_on_cache_hits_before_evicting(cx: &mut TestAppContext) {
        let (client, backend) = sized(cx, |_, _| Ok(10 * MB));
        let first = fetch(cx, &client, 0);
        let second = fetch(cx, &client, 1);
        let third = fetch(cx, &client, 2);

        assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &first));
        let fourth = fetch(cx, &client, 3);

        assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &first));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 2), &third));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 3), &fourth));
        assert_eq!(backend.media_calls().len(), 4);
        assert!(!Arc::ptr_eq(&fetch(cx, &client, 1), &second));
        assert_eq!(backend.media_calls().len(), 5);
    }

    #[gpui::test]
    fn evicts_older_mixed_size_entries_to_fit_the_byte_budget(cx: &mut TestAppContext) {
        for older in [0, 1] {
            let (client, backend) = sized(cx, |src, _| {
                let sizes = [4 * MB, 8 * MB, 20 * MB, 12 * MB];
                Ok((0..sizes.len())
                    .find(|index| url(*index) == src)
                    .map_or(0, |index| sizes[index]))
            });
            let small = fetch(cx, &client, 0);
            let medium = fetch(cx, &client, 1);
            let large = fetch(cx, &client, 2);

            // All three entries fit at exactly 32 MiB.
            assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &small));
            assert!(Arc::ptr_eq(&fetch(cx, &client, 1), &medium));
            assert!(Arc::ptr_eq(&fetch(cx, &client, 2), &large));
            assert_eq!(backend.media_calls().len(), 3);

            // Adding 12 MiB must evict both the 4 MiB and 8 MiB entries.
            let newest = fetch(cx, &client, 3);
            assert!(Arc::ptr_eq(&fetch(cx, &client, 2), &large));
            assert!(Arc::ptr_eq(&fetch(cx, &client, 3), &newest));
            assert_eq!(backend.media_calls().len(), 4);

            let previous = if older == 0 { &small } else { &medium };
            assert!(!Arc::ptr_eq(&fetch(cx, &client, older), previous));
            assert_eq!(backend.media_calls().len(), 5);
        }
    }

    #[gpui::test]
    fn evicts_when_one_new_byte_exceeds_the_exact_budget(cx: &mut TestAppContext) {
        let (client, backend) = sized(cx, |_, call| Ok(if call == 2 { 1 } else { 16 * MB }));
        let first = fetch(cx, &client, 0);
        let second = fetch(cx, &client, 1);

        assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &first));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 1), &second));
        assert_eq!(backend.media_calls().len(), 2);

        let tiny = fetch(cx, &client, 2);
        assert!(Arc::ptr_eq(&fetch(cx, &client, 1), &second));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 2), &tiny));
        assert_eq!(backend.media_calls().len(), 3);
        assert!(!Arc::ptr_eq(&fetch(cx, &client, 0), &first));
        assert_eq!(backend.media_calls().len(), 4);
    }

    #[gpui::test]
    fn returns_oversized_responses_without_caching_or_evicting(cx: &mut TestAppContext) {
        let (client, backend) = sized(cx, |_, call| {
            Ok(if call == 0 {
                16 * MB
            } else {
                MEDIA_CACHE_BYTES + 1
            })
        });
        let cached = fetch(cx, &client, 0);
        let oversized = fetch(cx, &client, 1);

        assert_eq!(oversized.len(), 32 * MB + 1);
        assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &cached));
        assert_eq!(backend.media_calls().len(), 2);
        let repeat = fetch(cx, &client, 1);
        assert_eq!(repeat.len(), 32 * MB + 1);
        assert!(!Arc::ptr_eq(&repeat, &oversized));
        assert!(Arc::ptr_eq(&fetch(cx, &client, 0), &cached));
        assert_eq!(backend.media_calls().len(), 3);
    }

    #[gpui::test]
    fn retries_after_a_failed_request(cx: &mut TestAppContext) {
        let (client, _) = sized(cx, |_, call| {
            if call == 0 {
                Err("offline".into())
            } else {
                Ok(8)
            }
        });
        assert_eq!(
            settle(cx, client.fetch_inbox_media(&url(200))),
            Err("offline".to_string())
        );
        assert_eq!(fetch(cx, &client, 200).len(), 8);
    }
}
