//! Port of src/features/sessions/model/linkPreview.ts: find the web link in a
//! user message, recognize GitHub issues and pull requests, and fetch page
//! metadata once per URL.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use monocode_core::{inbox::WorkItemKind, js};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

/// `GithubWorkItemLink`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubWorkItemLink {
    pub kind: WorkItemKind,
    pub repo: String,
    pub number: i64,
}

/// `UserLink`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLink {
    pub url: String,
    pub host: String,
    pub display_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_work_item: Option<GithubWorkItemLink>,
}

/// `UserMessageLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageLink {
    pub link: UserLink,
    pub before_text: String,
    pub after_text: String,
}

/// `LinkPreviewMetadata`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreviewMetadata {
    pub title: Option<String>,
    pub favicon_data_url: Option<String>,
}

static WEB_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s<>"']+"#).unwrap());

/// `parseUserMessageLink`: the first web URL, with the rest of the message
/// kept around it.
pub fn parse_user_message_link(text: &str) -> Option<UserMessageLink> {
    let found = WEB_URL.find(text)?;
    let value = trim_url_punctuation(found.as_str());
    let link = parse_http_url(value)?;
    Some(UserMessageLink {
        link,
        before_text: text[..found.start()].to_string(),
        after_text: text[found.start() + value.len()..].to_string(),
    })
}

/// `parseStandaloneHttpUrl`: the stricter rule where the whole text must be
/// one URL.
pub fn parse_standalone_http_url(text: &str) -> Option<UserLink> {
    let value = js::trim(text);
    if value.is_empty() || value.chars().any(js::is_space) {
        return None;
    }
    parse_http_url(value)
}

fn parse_http_url(value: &str) -> Option<UserLink> {
    let parsed = Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "https" | "http")
        || !parsed.username().is_empty()
        || parsed
            .password()
            .is_some_and(|password| !password.is_empty())
    {
        return None;
    }
    let hostname = parsed.host_str().filter(|host| !host.is_empty())?;
    let host = hostname
        .strip_suffix('.')
        .unwrap_or(hostname)
        .to_lowercase();
    let display_host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let path = parsed.path();
    let search = parsed
        .query()
        .filter(|query| !query.is_empty())
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let hash = parsed
        .fragment()
        .filter(|fragment| !fragment.is_empty())
        .map(|fragment| format!("#{fragment}"))
        .unwrap_or_default();
    let suffix = format!("{}{search}{hash}", if path == "/" { "" } else { path });
    let work_item = github_work_item(&parsed, &display_host);
    Some(UserLink {
        url: parsed.as_str().to_string(),
        host: display_host.clone(),
        display_url: format!("{display_host}{suffix}"),
        github_work_item: work_item,
    })
}

fn github_work_item(url: &Url, display_host: &str) -> Option<GithubWorkItemLink> {
    if display_host != "github.com" {
        return None;
    }
    let parts: Vec<&str> = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let resource = parts[2].to_lowercase();
    if resource != "pull" && resource != "issues" {
        return None;
    }
    let number_text = parts[3];
    let mut digits = number_text.bytes();
    if !digits
        .next()
        .is_some_and(|first| (b'1'..=b'9').contains(&first))
        || !digits.all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let number: i64 = number_text.parse().ok()?;
    if number > 9_007_199_254_740_991 {
        return None;
    }
    let owner = decode_path_part(parts[0]);
    let name = decode_path_part(parts[1]);
    let invalid = |part: &str| part.is_empty() || part.chars().any(|c| js::is_space(c) || c == '/');
    if invalid(&owner) || invalid(&name) {
        return None;
    }
    Some(GithubWorkItemLink {
        kind: if resource == "pull" {
            WorkItemKind::Pr
        } else {
            WorkItemKind::Issue
        },
        repo: format!("{owner}/{name}"),
        number,
    })
}

/// `decodeURIComponent`, or an empty string where it would throw.
fn decode_path_part(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3);
            let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) else {
                return String::new();
            };
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `trimUrlPunctuation`: drop sentence punctuation and unbalanced closing
/// brackets from the end of a URL.
fn trim_url_punctuation(value: &str) -> &str {
    let mut end = value.len();
    while end > 0 && matches!(value.as_bytes()[end - 1], b'.' | b',' | b'!' | b';' | b':') {
        end -= 1;
    }
    for (opening, closing) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while value[..end].ends_with(closing) {
            let head = &value[..end];
            if head.matches(closing).count() <= head.matches(opening).count() {
                break;
            }
            end -= 1;
        }
    }
    &value[..end]
}

type MetadataLoad = Shared<BoxFuture<'static, Result<LinkPreviewMetadata, String>>>;
type Fetcher =
    Arc<dyn Fn(String) -> BoxFuture<'static, Result<LinkPreviewMetadata, String>> + Send + Sync>;

/// `metadataCache` and `fetchLinkPreviewMetadata`. A failed fetch leaves the
/// cache, so the next request tries again.
#[derive(Clone)]
pub struct LinkPreviews {
    cache: Arc<Mutex<HashMap<String, MetadataLoad>>>,
    fetcher: Fetcher,
}

impl Default for LinkPreviews {
    fn default() -> Self {
        Self::new(Arc::new(|url| {
            smol::unblock(move || {
                let metadata = monocode_integrations::link_preview::fetch_link_preview(url)?;
                serde_json::to_value(metadata)
                    .and_then(serde_json::from_value)
                    .map_err(|error| error.to_string())
            })
            .boxed()
        }))
    }
}

impl LinkPreviews {
    /// A cache over any fetcher, such as a fake in tests.
    pub fn new(fetcher: Fetcher) -> Self {
        Self {
            cache: Arc::default(),
            fetcher,
        }
    }

    /// `fetchLinkPreviewMetadata`.
    pub fn fetch_link_preview_metadata(&self, url: &str) -> MetadataLoad {
        if let Some(cached) = self.cache.lock().get(url) {
            return cached.clone();
        }
        let fetch = (self.fetcher)(url.to_string());
        let cache = self.cache.clone();
        let key = url.to_string();
        let pending = async move {
            let result = fetch.await;
            if result.is_err() {
                cache.lock().remove(&key);
            }
            result
        }
        .boxed()
        .shared();
        self.cache.lock().insert(url.to_string(), pending.clone());
        pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // parseStandaloneHttpUrl
    #[test]
    fn normalizes_a_standalone_web_url_for_display() {
        assert_eq!(
            parse_standalone_http_url("  https://www.example.com/docs/start?q=one#intro  "),
            Some(UserLink {
                url: "https://www.example.com/docs/start?q=one#intro".into(),
                host: "example.com".into(),
                display_url: "example.com/docs/start?q=one#intro".into(),
                github_work_item: None,
            })
        );
    }

    #[test]
    fn keeps_a_root_url_compact() {
        assert_eq!(
            parse_standalone_http_url("http://example.com/")
                .unwrap()
                .display_url,
            "example.com"
        );
    }

    #[test]
    fn does_not_turn_prose_or_credentialed_urls_into_preview_cards() {
        assert_eq!(
            parse_standalone_http_url("take a look at https://example.com"),
            None
        );
        assert_eq!(
            parse_standalone_http_url("https://person:secret@example.com"),
            None
        );
        assert_eq!(parse_standalone_http_url("file:///tmp/example.html"), None);
    }

    // parseUserMessageLink
    #[test]
    fn extracts_a_url_followed_by_a_comment() {
        assert_eq!(
            parse_user_message_link("https://github.com/hardbeat920/monocode/pull/226 check this"),
            Some(UserMessageLink {
                link: UserLink {
                    url: "https://github.com/hardbeat920/monocode/pull/226".into(),
                    host: "github.com".into(),
                    display_url: "github.com/hardbeat920/monocode/pull/226".into(),
                    github_work_item: Some(GithubWorkItemLink {
                        kind: WorkItemKind::Pr,
                        repo: "hardbeat920/monocode".into(),
                        number: 226,
                    }),
                },
                before_text: String::new(),
                after_text: " check this".into(),
            })
        );
    }

    #[test]
    fn recognizes_github_issues_and_links_to_a_pr_subpage() {
        assert_eq!(
            parse_user_message_link("https://github.com/acme/widgets/issues/42#issuecomment-1")
                .unwrap()
                .link
                .github_work_item,
            Some(GithubWorkItemLink {
                kind: WorkItemKind::Issue,
                repo: "acme/widgets".into(),
                number: 42
            })
        );
        assert_eq!(
            parse_user_message_link("https://www.github.com/acme/widgets/pull/73/files")
                .unwrap()
                .link
                .github_work_item,
            Some(GithubWorkItemLink {
                kind: WorkItemKind::Pr,
                repo: "acme/widgets".into(),
                number: 73
            })
        );
    }

    #[test]
    fn leaves_other_github_urls_as_normal_web_links() {
        assert_eq!(
            parse_user_message_link("https://github.com/acme/widgets/actions")
                .unwrap()
                .link
                .github_work_item,
            None
        );
        assert_eq!(
            parse_user_message_link("https://github.com/acme/widgets/issues/0")
                .unwrap()
                .link
                .github_work_item,
            None
        );
    }

    #[test]
    fn preserves_prose_around_a_url_and_drops_sentence_punctuation() {
        let result =
            parse_user_message_link("Please review (https://example.com/docs), thanks").unwrap();
        assert_eq!(result.before_text, "Please review (");
        assert_eq!(result.after_text, "), thanks");
        assert_eq!(result.link.url, "https://example.com/docs");
    }

    #[test]
    fn returns_none_when_there_is_no_valid_web_url() {
        assert_eq!(parse_user_message_link("Nothing to preview here"), None);
    }

    #[test]
    fn caches_successful_fetches_and_retries_failures() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let previews = LinkPreviews::new(Arc::new(move |url: String| {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if url.contains("fail") && attempt < 2 {
                    Err("offline".to_string())
                } else {
                    Ok(LinkPreviewMetadata {
                        title: Some(url),
                        favicon_data_url: None,
                    })
                }
            }
            .boxed()
        }));
        smol::block_on(async {
            assert!(
                previews
                    .fetch_link_preview_metadata("https://a.test")
                    .await
                    .is_ok()
            );
            assert!(
                previews
                    .fetch_link_preview_metadata("https://a.test")
                    .await
                    .is_ok()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(
                previews
                    .fetch_link_preview_metadata("https://fail.test")
                    .await
                    .is_err()
            );
            assert!(
                previews
                    .fetch_link_preview_metadata("https://fail.test")
                    .await
                    .is_ok()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 3);
        });
    }
}
