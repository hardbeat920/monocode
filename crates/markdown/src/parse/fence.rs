//! Code fence info strings and file-name helpers.
//!
//! Port of `parseCodeFence`, `languageFromFileName`, `highlightLanguageFor`,
//! and `inlineFileName` from `src/features/sessions/ui/AgentMarkdown.tsx`,
//! plus `isExtensionlessFileName` from `src/shared/lib/paths.ts`.

/// Fence languages that Shiki treats as plain text. AgentMarkdown highlights
/// them with the JavaScript grammar so strings and numbers still get color.
pub const PLAINTEXT_FENCE_LANGUAGES: &[&str] = &["text", "plaintext", "txt", ""];

/// A parsed fence info string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeFence {
    /// The language as written, or derived from a file name.
    pub language: String,
    /// First line number, from `startLine=N` or a `12:20:path` citation.
    pub start_line: Option<u32>,
    /// File name when the fence names a file.
    pub file_name: Option<String>,
    /// File path when the fence names a file.
    pub file_path: Option<String>,
    /// False when the meta string has `noLineNumbers`.
    pub line_numbers: bool,
}

impl CodeFence {
    /// Parse an info string such as `rust`, `ts startLine=4`,
    /// `src/main.rs`, or `12:20:src/main.rs`.
    pub fn parse(info: &str) -> Self {
        let info = info.trim();
        let (raw, meta) = match info.find(char::is_whitespace) {
            Some(ix) => (&info[..ix], info[ix..].trim()),
            None => (info, ""),
        };
        let line_numbers = !has_word(meta, "noLineNumbers");
        let meta_start_line = meta_start_line(meta);

        if let Some((start, path)) = citation(raw) {
            let file_name = last_segment(path).unwrap_or(path).to_string();
            return Self {
                language: language_from_file_name(&file_name),
                start_line: Some(start),
                file_name: Some(file_name),
                file_path: Some(path.to_string()),
                line_numbers,
            };
        }

        if raw.contains('/') || raw.contains('\\') {
            let file_name = last_segment(raw).unwrap_or(raw).to_string();
            return Self {
                language: language_from_file_name(&file_name),
                start_line: meta_start_line,
                file_name: Some(file_name),
                file_path: Some(raw.to_string()),
                line_numbers,
            };
        }

        Self {
            language: raw.to_string(),
            start_line: meta_start_line,
            file_name: None,
            file_path: None,
            line_numbers,
        }
    }

    /// Whether the fence is a Mermaid diagram.
    pub fn is_mermaid(&self) -> bool {
        self.language.eq_ignore_ascii_case("mermaid")
    }

    /// Whether the language is one Shiki would leave uncolored.
    pub fn is_plaintext(&self) -> bool {
        is_plaintext_language(&self.language)
    }

    /// The language to highlight with (`highlightLanguageFor`).
    pub fn highlight_language(&self) -> &str {
        if self.is_plaintext() {
            "js"
        } else {
            &self.language
        }
    }

    /// The label for the code block header: the file path when the fence
    /// names a file, otherwise the language as written.
    pub fn label(&self) -> &str {
        self.file_path.as_deref().unwrap_or(&self.language)
    }
}

pub fn is_plaintext_language(language: &str) -> bool {
    let lower = language.to_ascii_lowercase();
    PLAINTEXT_FENCE_LANGUAGES.contains(&lower.as_str())
}

/// `^(\d+):(\d+):(.+)$`
fn citation(raw: &str) -> Option<(u32, &str)> {
    let (start, rest) = raw.split_once(':')?;
    let (end, path) = rest.split_once(':')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(start) || !digits(end) || path.is_empty() {
        return None;
    }
    Some((start.parse().ok()?, path))
}

/// `\bstartLine=(\d+)`
fn meta_start_line(meta: &str) -> Option<u32> {
    let mut search = meta;
    while let Some(ix) = search.find("startLine=") {
        let boundary = search[..ix]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        let rest = &search[ix + "startLine=".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if boundary && !digits.is_empty() {
            return digits.parse().ok();
        }
        search = rest;
    }
    None
}

fn has_word(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(ix, _)| {
        let before = text[..ix].chars().next_back().is_none_or(|c| !is_word(c));
        let after = text[ix + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c));
        before && after
    })
}

fn last_segment(path: &str) -> Option<&str> {
    path.split(['/', '\\']).rfind(|s| !s.is_empty())
}

/// Map a file name to a highlight language (`languageFromFileName`).
pub fn language_from_file_name(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    if lower == "dockerfile" {
        return "dockerfile".into();
    }
    if lower == "makefile" {
        return "makefile".into();
    }
    let ext = match lower.rfind('.') {
        Some(ix) => &lower[ix + 1..],
        None => lower.as_str(),
    };
    let mapped = match ext {
        "sh" | "zsh" => "bash",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "ts" => "typescript",
        "js" => "javascript",
        "md" => "markdown",
        "yml" => "yaml",
        "cs" => "csharp",
        "cpp" | "cc" | "cxx" => "cpp",
        other => other,
    };
    mapped.to_string()
}

/// `isExtensionlessFileName` from paths.ts.
pub fn is_extensionless_file_name(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "dockerfile" | "makefile" | "gemfile" | "license"
    )
}

/// The file name inline code refers to, if it looks like a file
/// (`inlineFileName`). Hosts use it to decide which inline code spans open a
/// file when clicked.
pub fn inline_file_name(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > 240 || text.chars().any(char::is_whitespace) {
        return None;
    }
    let without_location = strip_location(text);
    let file_name = last_segment(without_location)?;
    let allowed = |c: char| c.is_ascii_alphanumeric() || "_%@+().-".contains(c);
    if !file_name.chars().all(allowed) {
        return None;
    }
    if is_extensionless_file_name(file_name) {
        return Some(file_name.to_string());
    }
    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext)?;
    let mut chars = extension.chars();
    let first = chars.next()?;
    let valid = first.is_ascii_alphabetic()
        && extension.len() <= 12
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-');
    valid.then(|| file_name.to_string())
}

/// Strip a trailing `:12`, `:12:4`, `#L12`, or `#L12-L20`.
fn strip_location(text: &str) -> &str {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(ix) = text.rfind("#L") {
        let rest = &text[ix + 2..];
        let ok = match rest.split_once("-L") {
            Some((a, b)) => digits(a) && digits(b),
            None => digits(rest),
        };
        if ok {
            return &text[..ix];
        }
    }
    if let Some((head, last)) = text.rsplit_once(':')
        && digits(last)
    {
        if let Some((head2, mid)) = head.rsplit_once(':')
            && digits(mid)
        {
            return head2;
        }
        return head;
    }
    text
}

/// Whether the raw source of a fenced code block ends with a closing fence.
pub(crate) fn is_closed(raw: &str) -> bool {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return false;
    };
    let Some((fence_char, fence_len)) = fence_of(strip_container(first)) else {
        return false;
    };
    let body = raw.trim_end_matches(['\n', '\r']);
    if body.lines().nth(1).is_none() {
        return false;
    }
    let Some(last) = body.lines().next_back() else {
        return false;
    };
    let last = strip_container(last).trim_end();
    last.len() >= fence_len && last.chars().all(|c| c == fence_char)
}

/// The fence character and run length that open a fenced block.
fn fence_of(line: &str) -> Option<(char, usize)> {
    let c = line.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let len = line.chars().take_while(|&x| x == c).count();
    (len >= 3).then_some((c, len))
}

/// Strip indentation and block quote markers that precede a fence line
/// inside containers. List indentation is plain whitespace.
fn strip_container(line: &str) -> &str {
    let mut line = line.trim_start();
    while let Some(rest) = line.strip_prefix('>') {
        line = rest.trim_start();
    }
    line
}

/// When the last line of an unclosed fenced block could be the start of the
/// closing fence (only fence characters, shorter than the opening run), the
/// byte length of the code without that line. Used to keep a half-typed
/// closing fence from flashing as code.
pub(crate) fn partial_closing_fence(code: &str, raw_first_line: &str) -> Option<usize> {
    let (fence_char, fence_len) = fence_of(strip_container(raw_first_line))?;
    let (head, last) = match code.rfind('\n') {
        Some(ix) => (ix, &code[ix + 1..]),
        None => (0, code),
    };
    let trimmed = last.trim();
    (!trimmed.is_empty() && trimmed.len() < fence_len && trimmed.chars().all(|c| c == fence_char))
        .then_some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_language() {
        let fence = CodeFence::parse("rust");
        assert_eq!(fence.language, "rust");
        assert!(fence.line_numbers);
        assert_eq!(fence.label(), "rust");
        assert_eq!(fence.highlight_language(), "rust");
    }

    #[test]
    fn plaintext_fences_highlight_as_js() {
        for info in ["", "text", "TXT", "plaintext"] {
            let fence = CodeFence::parse(info);
            assert!(fence.is_plaintext(), "{info}");
            assert_eq!(fence.highlight_language(), "js");
        }
    }

    #[test]
    fn citation_fence() {
        let fence = CodeFence::parse("12:20:src/app/main.rs");
        assert_eq!(fence.start_line, Some(12));
        assert_eq!(fence.file_name.as_deref(), Some("main.rs"));
        assert_eq!(fence.file_path.as_deref(), Some("src/app/main.rs"));
        assert_eq!(fence.language, "rust");
        assert_eq!(fence.label(), "src/app/main.rs");
    }

    #[test]
    fn path_fence_and_meta() {
        let fence = CodeFence::parse("crates/core/lib.ts startLine=40 noLineNumbers");
        assert_eq!(fence.language, "typescript");
        assert_eq!(fence.start_line, Some(40));
        assert!(!fence.line_numbers);
        let fence = CodeFence::parse("py startLine=3");
        assert_eq!(fence.language, "py");
        assert_eq!(fence.start_line, Some(3));
        assert!(fence.line_numbers);
    }

    #[test]
    fn file_name_languages() {
        assert_eq!(language_from_file_name("Dockerfile"), "dockerfile");
        assert_eq!(language_from_file_name("build.zsh"), "bash");
        assert_eq!(language_from_file_name("x.cxx"), "cpp");
        assert_eq!(language_from_file_name("x.go"), "go");
        assert_eq!(language_from_file_name("noext"), "noext");
    }

    #[test]
    fn inline_file_names() {
        assert_eq!(inline_file_name("src/main.rs").as_deref(), Some("main.rs"));
        assert_eq!(inline_file_name("main.rs:12:4").as_deref(), Some("main.rs"));
        assert_eq!(
            inline_file_name("app.tsx#L3-L9").as_deref(),
            Some("app.tsx")
        );
        assert_eq!(inline_file_name("Makefile").as_deref(), Some("Makefile"));
        assert_eq!(inline_file_name("cargo test"), None);
        assert_eq!(inline_file_name("v1.2.3"), None);
        assert_eq!(inline_file_name("foo"), None);
    }

    #[test]
    fn closing_fences() {
        assert!(is_closed("```rust\nfn a() {}\n```\n"));
        assert!(is_closed("```\na\n````"));
        assert!(!is_closed("```rust\nfn a() {}\n"));
        assert!(!is_closed("````\na\n```\n"));
        assert!(!is_closed("```"));
        assert!(is_closed("> ```\n> a\n> ```"));
        assert!(is_closed("~~~\na\n~~~"));
    }

    #[test]
    fn partial_closing_fence_lines() {
        assert_eq!(partial_closing_fence("a\nb\n``", "```rust"), Some(3));
        assert_eq!(partial_closing_fence("a\n`", "```"), Some(1));
        assert_eq!(partial_closing_fence("``", "```"), Some(0));
        assert_eq!(partial_closing_fence("a\nb", "```"), None);
        assert_eq!(partial_closing_fence("a\n``", "~~~"), None);
        assert_eq!(partial_closing_fence("a\n```", "````"), Some(1));
    }
}
