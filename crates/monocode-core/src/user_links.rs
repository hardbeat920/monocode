//! The first web link in a user message, following sessions/model/linkPreview.ts.
use std::{ops::Range, sync::LazyLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserLink {
    /// The parsed, normalized address (`URL.href`).
    pub url: String,
    /// Lowercase host without `www.`.
    pub host: String,
    pub display_url: String,
    pub github_work_item: Option<GithubWorkItemLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubWorkItemLink {
    /// "pr" or "issue".
    pub kind: &'static str,
    pub repo: String,
    pub number: u64,
}

/// parseUserMessageLink: the link, and the bytes of `text` it replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserMessageLink {
    pub link: UserLink,
    pub range: Range<usize>,
}

/// Find the first web URL and preserve the rest of the user's message.
pub fn parse_user_message_link(text: &str) -> Option<UserMessageLink> {
    static URL: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?i)https?://[^\s<>"']+"#).unwrap());
    let found = URL.find(text)?;
    let value = trim_url_punctuation(found.as_str());
    let link = parse_http_url(value)?;
    Some(UserMessageLink {
        link,
        range: found.start()..found.start() + value.len(),
    })
}

fn parse_http_url(value: &str) -> Option<UserLink> {
    let parsed = url::Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "https" | "http")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let display_host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    let path = if parsed.path() == "/" {
        ""
    } else {
        parsed.path()
    };
    let search = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
    let hash = parsed
        .fragment()
        .map(|f| format!("#{f}"))
        .unwrap_or_default();
    Some(UserLink {
        url: parsed.as_str().to_owned(),
        display_url: format!("{display_host}{path}{search}{hash}"),
        github_work_item: github_work_item(&parsed, &display_host),
        host: display_host,
    })
}

fn github_work_item(url: &url::Url, display_host: &str) -> Option<GithubWorkItemLink> {
    if display_host != "github.com" {
        return None;
    }
    let parts: Vec<&str> = url.path().split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 4 {
        return None;
    }
    let kind = match parts[2].to_ascii_lowercase().as_str() {
        "pull" => "pr",
        "issues" => "issue",
        _ => return None,
    };
    let digits = parts[3];
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Number.isSafeInteger.
    let number = digits.parse::<u64>().ok().filter(|&n| n < 1 << 53)?;
    let owner = decode_path_part(parts[0])?;
    let name = decode_path_part(parts[1])?;
    let valid =
        |part: &str| !part.is_empty() && !part.chars().any(|c| c.is_whitespace() || c == '/');
    if !valid(&owner) || !valid(&name) {
        return None;
    }
    Some(GithubWorkItemLink {
        kind,
        repo: format!("{owner}/{name}"),
        number,
    })
}

/// decodeURIComponent, which rejects malformed escapes and invalid UTF-8.
fn decode_path_part(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn trim_url_punctuation(value: &str) -> &str {
    let mut value = value.trim_end_matches(['.', ',', '!', ';', ':']);
    for (opening, closing) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while value.ends_with(closing)
            && value.matches(closing).count() > value.matches(opening).count()
        {
            value = &value[..value.len() - 1];
        }
    }
    value
}

/// UserLinkPreview's plainTextSummary: a GitHub body as one run of prose.
pub fn plain_text_summary(value: &str) -> String {
    static STEPS: LazyLock<[(regex::Regex, &str); 5]> = LazyLock::new(|| {
        [
            (r"!\[[^\]]*\]\([^)]*\)", ""),
            (r"\[([^\]]+)\]\([^)]*\)", "$1"),
            (r"<[^>]+>", " "),
            (r"[`*_>#~|-]+", " "),
            (r"\s+", " "),
        ]
        .map(|(re, with)| (regex::Regex::new(re).unwrap(), with))
    });
    let mut text = value.to_owned();
    for (re, with) in STEPS.iter() {
        text = re.replace_all(&text, *with).into_owned();
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_link_keeps_the_text_around_it() {
        let text = "hey lets review this pr\n\nhttps://github.com/hardbeat920/monocode/pull/1142 its for the native one";
        let found = parse_user_message_link(text).unwrap();
        assert_eq!(&text[..found.range.start], "hey lets review this pr\n\n");
        assert_eq!(&text[found.range.end..], " its for the native one");
        assert_eq!(
            found.link.github_work_item,
            Some(GithubWorkItemLink {
                kind: "pr",
                repo: "hardbeat920/monocode".into(),
                number: 1142,
            })
        );
        assert_eq!(found.link.host, "github.com");
    }

    #[test]
    fn trailing_punctuation_and_unbalanced_brackets_stay_text() {
        let text = "(see https://www.Example.com/a_(b)/c?q=1#x).";
        let found = parse_user_message_link(text).unwrap();
        assert_eq!(
            &text[found.range.clone()],
            "https://www.Example.com/a_(b)/c?q=1#x"
        );
        assert_eq!(found.link.host, "example.com");
        assert_eq!(found.link.display_url, "example.com/a_(b)/c?q=1#x");
        assert_eq!(found.link.github_work_item, None);
    }

    #[test]
    fn only_numbered_github_issues_and_pulls_are_work_items() {
        let item = |text: &str| parse_user_message_link(text).and_then(|f| f.link.github_work_item);
        assert_eq!(
            item("https://GitHub.com/a/b/Issues/7/").map(|i| (i.kind, i.number)),
            Some(("issue", 7))
        );
        assert_eq!(
            item("https://github.com/a/b/pull/12/files").map(|i| i.repo),
            Some("a/b".into())
        );
        for text in [
            "https://github.com/a/b/pull/0",
            "https://github.com/a/b/pull/012",
            "https://github.com/a/b/pull/12abc",
            "https://github.com/a/b/commit/12",
            "https://github.com/a/b/pull",
            "https://gist.github.com/a/b/pull/1",
            "https://github.com/a%2Fx/b/pull/1",
            "https://github.com/a%ZZ/b/pull/1",
        ] {
            assert_eq!(item(text), None, "{text}");
        }
        assert_eq!(
            parse_user_message_link("https://user:pw@github.com/a/b/pull/1"),
            None
        );
        assert_eq!(parse_user_message_link("no links here"), None);
    }

    #[test]
    fn summaries_drop_markdown_syntax() {
        assert_eq!(
            plain_text_summary(
                "## Summary\n\n![shot](a.png) Fixes [the bug](https://x.y) in `foo_bar`.\n- <b>one</b>\n> two"
            ),
            "Summary Fixes the bug in foo bar . one two"
        );
    }
}
