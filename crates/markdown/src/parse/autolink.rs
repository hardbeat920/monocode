//! GFM literal autolinks (`https://…`, `http://…`, `www.…`), which
//! pulldown-cmark does not implement but remark-gfm does.
//!
//! The scanning rules follow zeronsh/comet's `markdown/parser.rs` (MIT,
//! Copyright (c) 2026 Wing), extended with `www.` links.

use std::sync::Arc;

use super::{Inline, Span};

/// Split plain spans at bare URLs and mark the URL parts as links. Spans that
/// are already links, code, or images pass through.
pub(super) fn autolink(inline: Inline) -> Inline {
    if !inline
        .spans
        .iter()
        .any(|span| linkable(span) && find_url_start(&inline.text[span.range.clone()]).is_some())
    {
        return inline;
    }
    let Inline { text, spans } = inline;
    let mut out = Vec::with_capacity(spans.len() + 2);
    for span in spans {
        if !linkable(&span) {
            out.push(span);
            continue;
        }
        split_span(&text, span, &mut out);
    }
    Inline { text, spans: out }
}

fn linkable(span: &Span) -> bool {
    span.style.link.is_none() && !span.style.code && span.style.image.is_none()
}

fn split_span(text: &str, span: Span, out: &mut Vec<Span>) {
    let base = span.range.start;
    let segment = &text[span.range.clone()];
    let mut cursor = 0;
    let push = |out: &mut Vec<Span>, range: std::ops::Range<usize>, link: Option<Arc<str>>| {
        if range.is_empty() {
            return;
        }
        let mut style = span.style.clone();
        style.link = link;
        out.push(Span {
            range: base + range.start..base + range.end,
            style,
            src: span.src + range.start,
        });
    };
    while let Some(rel) = find_url_start(&segment[cursor..]) {
        let at = cursor + rel;
        let from = &segment[at..];
        let prefix = scheme_len(from);
        let len = bare_url_len(from);
        if len <= prefix {
            // A scheme or `www.` with nothing after it stays text.
            push(out, cursor..at + prefix, None);
            cursor = at + prefix;
            continue;
        }
        push(out, cursor..at, None);
        let url = &from[..len];
        let dest: Arc<str> = if url.starts_with("www.") {
            format!("http://{url}").into()
        } else {
            url.into()
        };
        push(out, at..at + len, Some(dest));
        cursor = at + len;
    }
    push(out, cursor..segment.len(), None);
}

fn scheme_len(text: &str) -> usize {
    if text.starts_with("https://") {
        "https://".len()
    } else if text.starts_with("http://") {
        "http://".len()
    } else {
        "www.".len()
    }
}

/// The first `http://`, `https://`, or `www.` that does not follow a letter or
/// digit (GFM's boundary rule).
fn find_url_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut best: Option<usize> = None;
    for needle in ["http", "www."] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(needle) {
            let at = from + rel;
            let after = &text[at..];
            let is_start = if needle == "http" {
                after.starts_with("http://") || after.starts_with("https://")
            } else {
                // `www.` needs a domain character after it.
                after[4..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
            };
            let boundary = at == 0 || {
                let prev = text[..at].chars().next_back().unwrap_or(' ');
                !prev.is_alphanumeric() && (needle == "http" || !matches!(prev, '/' | '.' | '@'))
            };
            if is_start && boundary {
                best = Some(best.map_or(at, |b| b.min(at)));
                break;
            }
            from = at + needle.len();
            if from >= bytes.len() {
                break;
            }
        }
    }
    best
}

/// Byte length of the bare URL at the start of `text`: up to whitespace or a
/// delimiter that does not appear in pasted URLs, minus trailing punctuation
/// GFM excludes. A closing paren stays only when an opener inside the URL
/// balances it.
fn bare_url_len(text: &str) -> usize {
    let end = text
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
        .map_or(text.len(), |(ix, _)| ix);
    let mut url = &text[..end];
    while let Some(last) = url.chars().next_back() {
        let trim = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '*' | '_' | '~' | '\'' => true,
            ')' => url.matches('(').count() < url.matches(')').count(),
            _ => false,
        };
        if !trim {
            break;
        }
        url = &url[..url.len() - last.len_utf8()];
    }
    url.len()
}

#[cfg(test)]
mod tests {
    use super::super::{Block, parse};

    fn links(source: &str) -> Vec<(String, String)> {
        let doc = parse(source);
        let Block::Paragraph(inline) = &doc.blocks[0].block else {
            panic!("expected a paragraph");
        };
        inline
            .spans
            .iter()
            .filter_map(|span| {
                Some((
                    inline.text[span.range.clone()].to_string(),
                    span.style.link.as_deref()?.to_string(),
                ))
            })
            .collect()
    }

    #[test]
    fn bare_urls_become_links() {
        assert_eq!(
            links("PR: https://github.com/a/b/pull/31\n"),
            vec![(
                "https://github.com/a/b/pull/31".into(),
                "https://github.com/a/b/pull/31".into()
            )]
        );
        assert_eq!(
            links("see https://x.dev/a, then.")[0].1,
            "https://x.dev/a".to_string()
        );
        assert_eq!(
            links("(docs: https://x.dev/Foo_(bar))")[0].1,
            "https://x.dev/Foo_(bar)".to_string()
        );
        assert_eq!(
            links("visit www.example.com.")[0],
            ("www.example.com".into(), "http://www.example.com".into())
        );
    }

    #[test]
    fn non_urls_stay_text() {
        assert!(links("foohttps://x.dev glued").is_empty());
        assert!(links("the https:// scheme").is_empty());
        assert!(links("`https://x.dev` in code").is_empty());
        assert!(links("a www. b").is_empty());
        assert_eq!(
            links("[https://shown.dev](https://real.dev)"),
            vec![("https://shown.dev".into(), "https://real.dev".into())]
        );
    }

    #[test]
    fn autolink_keeps_emphasis_and_sources() {
        let doc = parse("**see https://x.dev now**");
        let Block::Paragraph(inline) = &doc.blocks[0].block else {
            panic!();
        };
        let link = inline
            .spans
            .iter()
            .find(|span| span.style.link.is_some())
            .unwrap();
        assert!(link.style.strong);
        assert_eq!(&inline.text[link.range.clone()], "https://x.dev");
        assert_eq!(link.src, 6);
    }
}
