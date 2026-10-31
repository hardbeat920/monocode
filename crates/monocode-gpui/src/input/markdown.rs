//! Live markdown styling for the composer. The text is never rewritten: the
//! scanner only reports which byte ranges to draw bold, italic, monospace and
//! so on, keeping every offset the native selection and IME code relies on.
use std::ops::Range;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
    /// Syntax characters (`**`, `#`, fences) drawn quietly.
    pub dim: bool,
    /// Heading text and list markers draw in the accent color.
    pub accent: bool,
    pub quote: bool,
}

impl Style {
    pub fn merge(self, other: Style) -> Style {
        Style {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            code: self.code || other.code,
            strike: self.strike || other.strike,
            link: self.link || other.link,
            dim: self.dim || other.dim,
            accent: self.accent || other.accent,
            quote: self.quote || other.quote,
        }
    }
}

pub(super) type Spans = Vec<(Range<usize>, Style)>;

const DIM: Style = Style {
    bold: false,
    italic: false,
    code: false,
    strike: false,
    link: false,
    dim: true,
    accent: false,
    quote: false,
};

pub(super) fn spans(text: &str) -> Spans {
    let mut out = Spans::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for line in text.split('\n') {
        block(line, offset, &mut fence, &mut out);
        offset += line.len() + 1;
    }
    out
}

/// Styles at ascending offsets in one pass. Looking each offset up against
/// every span is quadratic in the draft's length, which long drafts feel on
/// every layout and paint.
pub(super) struct Sweep {
    pending: std::vec::IntoIter<(Range<usize>, Style)>,
    active: Vec<(usize, Style)>,
}

impl Sweep {
    pub fn new(spans: &Spans) -> Self {
        let mut sorted = spans.clone();
        sorted.sort_by_key(|(range, _)| range.start);
        Sweep {
            pending: sorted.into_iter(),
            active: Vec::new(),
        }
    }
    /// The merged style at `at`; offsets must not decrease between calls.
    pub fn at(&mut self, at: usize) -> Style {
        self.active.retain(|(end, _)| *end > at);
        while let Some((range, style)) = self.pending.as_slice().first().cloned() {
            if range.start > at {
                break;
            }
            self.pending.next();
            if range.end > at {
                self.active.push((range.end, style));
            }
        }
        self.active
            .iter()
            .fold(Style::default(), |all, (_, style)| all.merge(*style))
    }
}

/// Whether `at` sits on a line inside an open fenced code block, using the
/// same fence rules as [`spans`].
pub(super) fn in_fence(text: &str, at: usize) -> bool {
    let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let mut fence: Option<char> = None;
    for line in text[..line_start].split('\n') {
        let body = line.trim_start_matches([' ', '\t']);
        if let Some(mark) = ["```", "~~~"].iter().find(|m| body.starts_with(**m)) {
            let ch = mark.chars().next().unwrap();
            match fence {
                None => fence = Some(ch),
                Some(open) if open == ch => fence = None,
                Some(_) => {}
            }
        }
    }
    fence.is_some()
}

fn block(line: &str, base: usize, fence: &mut Option<char>, out: &mut Spans) {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - body.len();
    let whole = base..base + line.len();
    if let Some(mark) = ["```", "~~~"].iter().find(|m| body.starts_with(**m)) {
        let ch = mark.chars().next().unwrap();
        match *fence {
            None => *fence = Some(ch),
            Some(open) if open == ch => *fence = None,
            Some(_) => {}
        }
        out.push((
            whole,
            Style {
                code: true,
                dim: true,
                ..Style::default()
            },
        ));
        return;
    }
    if fence.is_some() {
        out.push((
            whole,
            Style {
                code: true,
                ..Style::default()
            },
        ));
        return;
    }
    let hashes = body.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && (body.len() == hashes || body[hashes..].starts_with(' ')) {
        let marker = base + indent..base + indent + hashes;
        out.push((
            marker,
            Style {
                dim: true,
                ..Style::default()
            },
        ));
        let text = base + indent + hashes..base + line.len();
        out.push((
            text.clone(),
            Style {
                bold: true,
                accent: true,
                ..Style::default()
            },
        ));
        inline(&line[indent + hashes..], text.start, out);
        return;
    }
    if is_rule(body) {
        out.push((whole, DIM));
        return;
    }
    let mut start = indent;
    if let Some(rest) = body.strip_prefix('>') {
        let width = 1 + usize::from(rest.starts_with(' '));
        out.push((base + indent..base + indent + width, DIM));
        out.push((
            base + indent + width..base + line.len(),
            Style {
                quote: true,
                italic: true,
                ..Style::default()
            },
        ));
        start = indent + width;
    } else if let Some(width) = list_marker(body) {
        out.push((
            base + indent..base + indent + width,
            Style {
                accent: true,
                ..Style::default()
            },
        ));
        start = indent + width;
    }
    inline(&line[start..], base + start, out);
}

/// Width of a leading `- `, `* `, `+ `, `1. ` or `1) ` marker, including its space.
fn list_marker(body: &str) -> Option<usize> {
    if ["- ", "* ", "+ "].iter().any(|b| body.starts_with(b)) {
        return Some(2);
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    let tail = &body[digits..];
    (digits > 0 && digits <= 9 && (tail.starts_with(". ") || tail.starts_with(") ")))
        .then_some(digits + 2)
}

fn is_rule(body: &str) -> bool {
    let body = body.trim_end();
    body.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|c| body.chars().all(|b| b == *c || b == ' '))
        && body.chars().filter(|c| !c.is_whitespace()).count() >= 3
}

/// Index of the next `delim` at or after `from` that is not backslash-escaped.
fn find(s: &str, from: usize, delim: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i < s.len() {
        if bytes[i] == b'\\' {
            i += 1 + s[i + 1..].chars().next().map_or(0, char::len_utf8);
            continue;
        }
        if bytes[i..].starts_with(delim.as_bytes()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn inline(s: &str, base: usize, out: &mut Spans) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let ch = bytes[i];
        if ch == b'\\' {
            i += 1 + s[i + 1..].chars().next().map_or(0, char::len_utf8);
            continue;
        }
        if ch == b'`' {
            let ticks = rest.bytes().take_while(|b| *b == b'`').count();
            let fence = "`".repeat(ticks);
            if let Some(end) = find(s, i + ticks, &fence).filter(|end| *end > i + ticks) {
                out.push((
                    base + i..base + end + ticks,
                    Style {
                        code: true,
                        ..Style::default()
                    },
                ));
                out.push((base + i..base + i + ticks, DIM));
                out.push((base + end..base + end + ticks, DIM));
                i = end + ticks;
                continue;
            }
            i += ticks;
            continue;
        }
        if let Some(delim) = ["**", "__", "~~"].iter().find(|d| rest.starts_with(**d)) {
            let word_edge = *delim == "__"
                && s[..i]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            let inner = i + 2;
            let close = find(s, inner, delim).filter(|end| {
                *end > inner
                    && !s[inner..].starts_with(char::is_whitespace)
                    && !s[..*end].ends_with(char::is_whitespace)
            });
            if let (Some(end), false) = (close, word_edge) {
                let style = if *delim == "~~" {
                    Style {
                        strike: true,
                        ..Style::default()
                    }
                } else {
                    Style {
                        bold: true,
                        ..Style::default()
                    }
                };
                out.push((base + inner..base + end, style));
                out.push((base + i..base + inner, DIM));
                out.push((base + end..base + end + 2, DIM));
                inline(&s[inner..end], base + inner, out);
                i = end + 2;
                continue;
            }
            i += 2;
            continue;
        }
        if ch == b'*' || ch == b'_' {
            let delim = if ch == b'*' { "*" } else { "_" };
            let word_edge = ch == b'_'
                && s[..i]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            let inner = i + 1;
            let mut from = inner;
            let close = loop {
                match find(s, from, delim) {
                    // Skip `**` / `__` pairs belonging to nested bold.
                    Some(end) if s[end + 1..].starts_with(delim) => from = end + 2,
                    other => break other,
                }
            }
            .filter(|end| {
                *end > inner
                    && !s[inner..].starts_with(char::is_whitespace)
                    && !s[..*end].ends_with(char::is_whitespace)
                    && (ch == b'*' || !s[end + 1..].starts_with(char::is_alphanumeric))
            });
            if let (Some(end), false) = (close, word_edge) {
                out.push((
                    base + inner..base + end,
                    Style {
                        italic: true,
                        ..Style::default()
                    },
                ));
                out.push((base + i..base + inner, DIM));
                out.push((base + end..base + end + 1, DIM));
                inline(&s[inner..end], base + inner, out);
                i = end + 1;
                continue;
            }
            i += 1;
            continue;
        }
        if ch == b'['
            && let Some(mid) = find(s, i + 1, "](").filter(|mid| *mid > i + 1)
            && let Some(end) = find(s, mid + 2, ")")
        {
            out.push((
                base + i + 1..base + mid,
                Style {
                    link: true,
                    ..Style::default()
                },
            ));
            out.push((base + i..base + i + 1, DIM));
            out.push((base + mid..base + end + 1, DIM));
            inline(&s[i + 1..mid], base + i + 1, out);
            i = end + 1;
            continue;
        }
        i += s[i..].chars().next().map_or(1, char::len_utf8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(spans: &Spans, text: &str, needle: &str) -> Style {
        let start = text.find(needle).unwrap();
        spans
            .iter()
            .filter(|(range, _)| range.contains(&start))
            .fold(Style::default(), |all, (_, style)| all.merge(*style))
    }

    #[test]
    fn headings_emphasis_code_and_links_are_styled() {
        let text = "# Title\n**bold** and *it* and `code` and ~~gone~~\n[docs](http://x.y)";
        let spans = spans(text);
        assert!(at(&spans, text, "Title").bold && at(&spans, text, "Title").accent);
        assert!(at(&spans, text, "# ").dim);
        assert!(at(&spans, text, "bold").bold);
        assert!(at(&spans, text, "**bold").dim);
        assert!(at(&spans, text, "it*").italic);
        assert!(at(&spans, text, "code").code);
        assert!(at(&spans, text, "gone").strike);
        assert!(at(&spans, text, "docs").link);
        assert!(at(&spans, text, "http").dim);
    }

    #[test]
    fn blocks_fences_quotes_and_lists_are_styled() {
        let text = "```rs\nlet a = *b*;\n```\n> quote\n- item\n2. next\n---";
        let spans = spans(text);
        assert!(at(&spans, text, "let").code);
        assert!(!at(&spans, text, "let").italic);
        assert!(at(&spans, text, "quote").quote);
        assert!(at(&spans, text, "- item").accent);
        assert!(at(&spans, text, "2. next").accent);
        assert!(at(&spans, text, "---").dim);
    }

    #[test]
    fn plain_prose_stays_plain() {
        for text in [
            "snake_case_name",
            "2 * 3 * 4",
            "#hashtag",
            "a ** b",
            "50% of [x]",
            "**open",
        ] {
            let styled = spans(text);
            assert!(
                styled.iter().all(|(_, s)| *s == Style::default()),
                "{text}: {styled:?}"
            );
        }
        let text = "bold **a*b** end";
        assert!(at(&spans(text), text, "a*b").bold);
    }

    #[test]
    fn nested_emphasis_and_multibyte_text_keep_valid_boundaries() {
        let text = "**héllo *wörld* 🙂** ok";
        let spans = spans(text);
        for (range, _) in &spans {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
        let style = at(&spans, text, "wörld");
        assert!(style.bold && style.italic);
    }
}
