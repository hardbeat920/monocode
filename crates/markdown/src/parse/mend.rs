//! Repairs half-streamed markdown for display, in the spirit of Streamdown's
//! `remend` (which AgentMarkdown.tsx gets through Streamdown 2.5).
//!
//! While a block streams, an unclosed `**bold`, `*em`, `` `code ``, `~~strike`,
//! or `[link](partial-url` parses as literal text, and the closing marker's
//! arrival then restyles the run and reflows the line. Appending synthetic
//! closers to the display parse keeps the styling stable from the first
//! character after the opener. Only the display tree sees the mended text;
//! the canonical tree is untouched, so a marker that never closes settles to
//! its literal form when the stream ends.
//!
//! A last line that is only a block marker still waiting for content (`-`,
//! `1.`, `#`, a setext-looking `-` or `=`, one or two fence characters, or a
//! partial table delimiter row) is hidden, so the paragraph above does not
//! flash into a heading or gain a stray marker for one chunk.
//!
//! The delimiter scanner is adapted from zeronsh/comet's `markdown/mend.rs`
//! (MIT, Copyright (c) 2026 Wing). It is approximate on purpose, the same
//! trade-off remend makes: it prefers stable output close to the final parse
//! over exact CommonMark delimiter rules, because the next append or the
//! final settle repairs any misjudgment.

/// Destination for a link whose URL is still streaming. The renderer styles
/// it as a link but does not make it clickable.
pub const PENDING_LINK_URL: &str = "monocode:pending-link";

/// One unclosed emphasis delimiter run (`*`, `_`, or `~~`).
struct OpenDelim {
    ch: char,
    len: usize,
    /// Char index just past the run: closer order and the content guard.
    pos: usize,
}

/// Mend a streaming block's source. Returns `None` when nothing needs repair,
/// which is the common case.
pub fn close_hanging(text: &str) -> Option<String> {
    let body = drop_pending_marker_line(text);
    let closers = closers_for(body);
    if closers.is_empty() {
        return (body.len() != text.len()).then(|| body.to_string());
    }
    // Insert before trailing whitespace: a closer after a space is not
    // right-flanking and would not close.
    let end = body.trim_end().len();
    if let Some(url_cut) = closers.url_cut {
        return Some(format!("{}]({PENDING_LINK_URL})", &body[..url_cut]));
    }
    Some(format!("{}{}{}", &body[..end], closers.text, &body[end..]))
}

struct Closers {
    text: String,
    /// When the text ends inside a link URL: the byte offset of the `]`.
    url_cut: Option<usize>,
}

impl Closers {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.url_cut.is_none()
    }
}

fn closers_for(text: &str) -> Closers {
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    let n = cs.len();
    let at = |i: usize| cs.get(i).map(|&(_, c)| c);

    let mut delims: Vec<OpenDelim> = Vec::new();
    let mut brackets: Vec<usize> = Vec::new();
    // Open inline code span: (backtick run length, content char index).
    let mut code: Option<(usize, usize)> = None;
    // Char index of the last character that justifies closing an opener.
    let mut last_content: Option<usize> = None;
    let mut pending_url: Option<usize> = None;

    let mut i = 0;
    while i < n {
        let c = cs[i].1;
        if code.is_none() && c == '\\' {
            if i + 1 < n {
                last_content = Some(i + 1);
            }
            i += 2;
            continue;
        }
        if c == '`' {
            let run = run_len(&cs, i);
            match code {
                Some((open, _)) if run == open => code = None,
                Some(_) => last_content = Some(i + run - 1),
                None => code = Some((run, i + run)),
            }
            i += run;
            continue;
        }
        if code.is_some() {
            last_content = Some(i);
            i += 1;
            continue;
        }
        match c {
            '*' | '_' | '~' => {
                let run = run_len(&cs, i);
                delim(&mut delims, &cs, c, run, i, &mut last_content);
                i += run;
            }
            '[' => {
                brackets.push(i);
                i += 1;
            }
            ']' => {
                if let Some(open) = brackets.pop() {
                    // Emphasis opened inside a finished `[…]` stays literal,
                    // as the final parse decides too.
                    delims.retain(|d| d.pos < open);
                    if at(i + 1) == Some('(') {
                        let mut j = i + 2;
                        let mut depth = 0usize;
                        loop {
                            match at(j) {
                                Some('(') => depth += 1,
                                Some(')') if depth == 0 => break,
                                Some(')') => depth -= 1,
                                Some(_) => {}
                                None => {
                                    pending_url = Some(i);
                                    break;
                                }
                            }
                            j += 1;
                        }
                        if pending_url.is_some() {
                            break;
                        }
                        last_content = Some(j);
                        i = j + 1;
                        continue;
                    }
                }
                last_content = Some(i);
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                last_content = Some(i);
                i += 1;
            }
        }
    }

    if let Some(close) = pending_url {
        return Closers {
            text: String::new(),
            url_cut: Some(cs[close].0),
        };
    }

    // Closers innermost first (descending open position).
    let mut pending: Vec<(usize, String)> = Vec::new();
    if let Some((ticks, cpos)) = code
        && last_content.is_some_and(|lc| lc >= cpos)
    {
        pending.push((cpos, "`".repeat(ticks)));
    }
    for d in &delims {
        if last_content.is_some_and(|lc| lc >= d.pos) {
            pending.push((d.pos, d.ch.to_string().repeat(d.len)));
        }
    }
    if let Some(&open) = brackets.last()
        && last_content.is_some_and(|lc| lc > open)
    {
        pending.push((open, format!("]({PENDING_LINK_URL})")));
    }
    pending.sort_by_key(|closer| std::cmp::Reverse(closer.0));
    Closers {
        text: pending.into_iter().map(|(_, s)| s).collect(),
        url_cut: None,
    }
}

fn run_len(cs: &[(usize, char)], i: usize) -> usize {
    let c = cs[i].1;
    cs[i..].iter().take_while(|&&(_, x)| x == c).count()
}

/// Match or open one delimiter run against the innermost same-char opener.
fn delim(
    delims: &mut Vec<OpenDelim>,
    cs: &[(usize, char)],
    c: char,
    run: usize,
    i: usize,
    last_content: &mut Option<usize>,
) {
    let end = i + run;
    // GFM strikethrough is `~` or `~~`; longer tilde runs are literal.
    if c == '~' && run > 2 {
        *last_content = Some(end - 1);
        return;
    }
    let prev = i.checked_sub(1).map(|p| cs[p].1);
    let next = cs.get(end).map(|&(_, c)| c);
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    // Intraword `_` never delimits; intraword single `*` is treated the same
    // so `2*3` does not flash italic.
    if word(prev) && word(next) && (c == '_' || (c == '*' && run == 1)) {
        *last_content = Some(end - 1);
        return;
    }
    let can_close = prev.is_some_and(|c| !c.is_whitespace());
    let can_open = next.is_some_and(|c| !c.is_whitespace());
    let mut rest = run;
    if can_close && let Some(k) = delims.iter().rposition(|d| d.ch == c) {
        let take = rest.min(delims[k].len);
        delims[k].len -= take;
        rest -= take;
        let keep = if delims[k].len == 0 { k } else { k + 1 };
        delims.truncate(keep);
    }
    if rest > 0 {
        // A lone `~` only opens as the remainder of `~~`.
        if can_open && (c != '~' || rest == 2) {
            delims.push(OpenDelim {
                ch: c,
                len: rest,
                pos: end,
            });
        } else {
            *last_content = Some(end - 1);
        }
    }
}

/// The text without its last line when that line is a block marker still
/// waiting for content under a non-blank line.
fn drop_pending_marker_line(text: &str) -> &str {
    let Some(nl) = text.rfind('\n') else {
        return text;
    };
    let last = &text[nl + 1..];
    let above = text[..nl].rsplit('\n').next().unwrap_or("");
    if above.trim().is_empty() {
        return text;
    }
    if is_pending_marker(last.trim(), above.trim()) {
        &text[..nl]
    } else {
        text
    }
}

fn is_pending_marker(line: &str, above: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    // Strip block quote markers so quoted lines follow the same rules.
    let line = line.trim_start_matches(['>', ' ']);
    let above = above.trim_start_matches(['>', ' ']);
    if line.is_empty() {
        return false;
    }
    let all = |c: char| line.chars().all(|x| x == c);
    let n = line.chars().count();
    // Setext candidates and bullets: `-`, `--`, `=`, `==`, `*`, `+`.
    if n <= 2 && (all('-') || all('=')) {
        return true;
    }
    if line == "*" || line == "+" {
        return true;
    }
    // Empty ATX heading.
    if n <= 6 && all('#') {
        return true;
    }
    // One or two fence characters.
    if n <= 2 && (all('`') || all('~')) {
        return true;
    }
    // An ordered marker with nothing after it.
    if let Some(digits) = line.strip_suffix(['.', ')'])
        && !digits.is_empty()
        && digits.len() <= 9
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        return true;
    }
    // A partial table delimiter row under a header row.
    if above.starts_with('|')
        && line.starts_with(['|', ':', '-'])
        && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn mends(input: &str, expected: &str) {
        assert_eq!(close_hanging(input).as_deref(), Some(expected), "{input:?}");
    }

    #[track_caller]
    fn stays(input: &str) {
        assert_eq!(close_hanging(input), None, "{input:?}");
    }

    #[test]
    fn balanced_text_needs_nothing() {
        stays("plain words, no markers");
        stays("a **b** and *c* and `d` and ~~e~~");
        stays("[docs](https://x.dev) done");
        stays("");
    }

    #[test]
    fn bold_and_italic_close() {
        mends("**bold", "**bold**");
        mends("some *em", "some *em*");
        mends("a __b", "a __b__");
        mends("a _b", "a _b_");
        mends("***both", "***both***");
    }

    #[test]
    fn half_streamed_closers_complete() {
        mends("**bold*", "**bold**");
        mends("__b_", "__b__");
        mends("~~gone~", "~~gone~~");
    }

    #[test]
    fn nested_closers_come_innermost_first() {
        mends("**a *b", "**a *b***");
        mends("*a **b", "*a **b***");
        mends("_a **b", "_a **b**_");
    }

    #[test]
    fn bare_openers_stay_literal_until_content() {
        stays("**");
        stays("text **");
        stays("text ** ");
        stays("*");
        stays("~~");
        stays("`");
    }

    #[test]
    fn closers_go_before_trailing_whitespace() {
        mends("**bold ", "**bold** ");
        mends("*em\n", "*em*\n");
    }

    #[test]
    fn intraword_and_escapes_are_literal() {
        stays("2*3 equals 6");
        stays("snake_case_name");
        stays("20~~~25 degrees");
        stays(r"\*not emphasis");
        stays(r"a \** b");
    }

    #[test]
    fn list_markers_are_not_openers() {
        stays("* item one");
        stays("- a\n* b");
    }

    #[test]
    fn inline_code_closes_and_shields_markers() {
        mends("`code", "`code`");
        mends("call `a ** b", "call `a ** b`");
        stays("`done` after");
    }

    #[test]
    fn links_mend_to_pending_destination() {
        mends("[docs](https://x.dev/lo", "[docs](monocode:pending-link)");
        mends("[docs](", "[docs](monocode:pending-link)");
        mends("see [do", "see [do](monocode:pending-link)");
        mends("![alt](https://x/i.p", "![alt](monocode:pending-link)");
        stays("see [");
        stays("[x] task-like");
        stays("[a](https://x.dev/(y)) done");
        mends("[**a", "[**a**](monocode:pending-link)");
        mends("**a [b", "**a [b](monocode:pending-link)**");
    }

    #[test]
    fn pending_marker_lines_hide() {
        mends("para\n-", "para");
        mends("para\n--", "para");
        mends("para\n=", "para");
        mends("para\n1.", "para");
        mends("para\n12)", "para");
        mends("para\n#", "para");
        mends("para\n``", "para");
        mends("| a | b |\n| --- | -", "| a | b |");
        mends("**b\n-", "**b**");
        mends("- item\n  -", "- item");
        mends("> quote\n> -", "> quote");
        stays("para\n---");
        stays("-");
        stays("\n-");
        stays("para\n\n-");
        stays("para\n1. item");
        stays("para\n- item");
    }
}
