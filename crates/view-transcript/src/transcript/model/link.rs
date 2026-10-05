//! Port of `parseUserMessageLink` and `parseStandaloneHttpUrl` in
//! src/features/sessions/model/linkPreview.ts: the first web link in a user
//! message, shown as a compact chip.

/// `GithubWorkItemLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubWorkItem {
    /// `true` for a pull request, `false` for an issue.
    pub pull_request: bool,
    pub repo: String,
    pub number: i64,
}

/// `UserLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLink {
    pub url: String,
    pub host: String,
    pub display_url: String,
    pub github_work_item: Option<GithubWorkItem>,
}

/// `UserMessageLink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageLink {
    pub link: UserLink,
    pub before_text: String,
    pub after_text: String,
}

/// `/https?:\/\/[^\s<>"']+/i`: the first URL's byte range.
fn find_url(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let start = [lower.find("http://"), lower.find("https://")]
        .into_iter()
        .flatten()
        .min()?;
    let end = text[start..]
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
        .map(|offset| start + offset)
        .unwrap_or(text.len());
    Some((start, end))
}

/// `trimUrlPunctuation`.
fn trim_url_punctuation(value: &str) -> &str {
    let mut end = value.trim_end_matches(['.', ',', '!', ';', ':']).len();
    for (opening, closing) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while value[..end].ends_with(closing)
            && value[..end].matches(closing).count() > value[..end].matches(opening).count()
        {
            end -= closing.len_utf8();
        }
    }
    &value[..end]
}

fn decode_path_part(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = bytes
                .get(i + 1..i + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        if bytes[i] == b'%' {
            return String::new();
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `parseHttpUrl`: a web URL without credentials, normalized for display.
fn parse_http_url(value: &str) -> Option<UserLink> {
    let (scheme, rest) = value.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let host_port = authority.to_ascii_lowercase();
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host.to_string(), Some(port.to_string()))
        }
        _ => (host_port.clone(), None),
    };
    let host = host.trim_end_matches('.').to_string();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let default_port = (scheme == "http" && port.as_deref() == Some("80"))
        || (scheme == "https" && port.as_deref() == Some("443"));
    let authority = match &port {
        Some(port) if !default_port => format!("{host}:{port}"),
        _ => host.clone(),
    };
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = if path_end == 0 {
        "/"
    } else {
        &tail[..path_end]
    };
    let rest_tail = &tail[path_end..];
    let display_host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let suffix = format!("{}{rest_tail}", if path == "/" { "" } else { path });
    let github_work_item = github_work_item(path, &display_host);
    Some(UserLink {
        url: format!("{scheme}://{authority}{path}{rest_tail}"),
        host: display_host.clone(),
        display_url: format!("{display_host}{suffix}"),
        github_work_item,
    })
}

/// `githubWorkItem`: a link to a GitHub pull request or issue.
fn github_work_item(path: &str, display_host: &str) -> Option<GithubWorkItem> {
    if display_host != "github.com" {
        return None;
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 4 {
        return None;
    }
    let resource = parts[2].to_lowercase();
    if resource != "pull" && resource != "issues" {
        return None;
    }
    let number_text = parts[3];
    if number_text.starts_with('0')
        || number_text.is_empty()
        || !number_text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let number: i64 = number_text
        .parse()
        .ok()
        .filter(|n: &i64| *n <= 9_007_199_254_740_991)?;
    let owner = decode_path_part(parts[0]);
    let name = decode_path_part(parts[1]);
    let bad =
        |part: &str| part.is_empty() || part.contains(char::is_whitespace) || part.contains('/');
    if bad(&owner) || bad(&name) {
        return None;
    }
    Some(GithubWorkItem {
        pull_request: resource == "pull",
        repo: format!("{owner}/{name}"),
        number,
    })
}

/// `parseUserMessageLink`: the first web URL and the text around it.
pub fn parse_user_message_link(text: &str) -> Option<UserMessageLink> {
    let (start, end) = find_url(text)?;
    let value = trim_url_punctuation(&text[start..end]);
    let link = parse_http_url(value)?;
    Some(UserMessageLink {
        link,
        before_text: text[..start].to_string(),
        after_text: text[start + value.len()..].to_string(),
    })
}

/// `parseStandaloneHttpUrl`.
pub fn parse_standalone_http_url(text: &str) -> Option<UserLink> {
    let value = monocode_core::js::trim(text);
    if value.is_empty() || value.contains(char::is_whitespace) {
        return None;
    }
    parse_http_url(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_a_standalone_web_url_for_display() {
        let link = parse_standalone_http_url("  https://www.example.com/docs/start?q=one#intro  ")
            .unwrap();
        assert_eq!(link.url, "https://www.example.com/docs/start?q=one#intro");
        assert_eq!(link.host, "example.com");
        assert_eq!(link.display_url, "example.com/docs/start?q=one#intro");
        assert_eq!(
            parse_standalone_http_url("http://example.com/")
                .unwrap()
                .display_url,
            "example.com"
        );
    }

    #[test]
    fn does_not_turn_prose_or_credentialed_urls_into_previews() {
        assert!(parse_standalone_http_url("take a look at https://example.com").is_none());
        assert!(parse_standalone_http_url("https://person:secret@example.com").is_none());
        assert!(parse_standalone_http_url("file:///tmp/example.html").is_none());
    }

    #[test]
    fn extracts_a_url_followed_by_a_comment() {
        let parsed =
            parse_user_message_link("https://github.com/hardbeat920/monocode/pull/226 check this")
                .unwrap();
        assert_eq!(
            parsed.link.url,
            "https://github.com/hardbeat920/monocode/pull/226"
        );
        assert_eq!(
            parsed.link.display_url,
            "github.com/hardbeat920/monocode/pull/226"
        );
        assert_eq!(
            parsed.link.github_work_item,
            Some(GithubWorkItem {
                pull_request: true,
                repo: "hardbeat920/monocode".into(),
                number: 226
            })
        );
        assert_eq!(parsed.before_text, "");
        assert_eq!(parsed.after_text, " check this");
    }

    #[test]
    fn recognizes_github_issues_and_pr_subpages() {
        let issue =
            parse_user_message_link("https://github.com/acme/widgets/issues/42#issuecomment-1")
                .unwrap();
        assert_eq!(issue.link.github_work_item.unwrap().number, 42);
        let files =
            parse_user_message_link("https://www.github.com/acme/widgets/pull/73/files").unwrap();
        assert!(files.link.github_work_item.unwrap().pull_request);
        assert!(
            parse_user_message_link("https://github.com/acme/widgets/actions")
                .unwrap()
                .link
                .github_work_item
                .is_none()
        );
        assert!(
            parse_user_message_link("https://github.com/acme/widgets/issues/0")
                .unwrap()
                .link
                .github_work_item
                .is_none()
        );
    }

    #[test]
    fn preserves_prose_and_drops_sentence_punctuation() {
        let parsed = parse_user_message_link("Please check https://example.com/docs.").unwrap();
        assert_eq!(parsed.before_text, "Please check ");
        assert_eq!(parsed.link.display_url, "example.com/docs");
        assert_eq!(parsed.after_text, ".");
        let wrapped = parse_user_message_link("(see https://example.com/a_(b))").unwrap();
        assert_eq!(wrapped.link.display_url, "example.com/a_(b)");
        assert_eq!(wrapped.after_text, ")");
    }
}
