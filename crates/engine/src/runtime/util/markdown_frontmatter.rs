//! Port of src/shared/lib/markdownFrontmatter.ts.

use std::sync::LazyLock;

use regex::Regex;

/// `MarkdownDocumentParts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownDocumentParts<'a> {
    pub metadata: Option<String>,
    pub body: &'a str,
}

static OPENING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\x{FEFF}?---[ \t]*\r?\n").unwrap());
static CLOSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?:---|\.\.\.)[ \t]*(?:\r?\n|$)").unwrap());

/// `splitMarkdownFrontmatter`: split a leading YAML frontmatter block
/// without interpreting it. An unfinished header stays in the body.
pub fn split_markdown_frontmatter(text: &str) -> MarkdownDocumentParts<'_> {
    let Some(opening) = OPENING.find(text) else {
        return MarkdownDocumentParts {
            metadata: None,
            body: text,
        };
    };
    let remaining = &text[opening.end()..];
    let Some(closing) = CLOSING.find(remaining) else {
        return MarkdownDocumentParts {
            metadata: None,
            body: text,
        };
    };
    let header = &remaining[..closing.start()];
    let header = header
        .strip_suffix("\r\n")
        .or_else(|| header.strip_suffix('\n'))
        .unwrap_or(header);
    MarkdownDocumentParts {
        metadata: Some(header.to_string()),
        body: &remaining[closing.end()..],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_leading_header() {
        let parts = split_markdown_frontmatter("---\ntitle: x\n---\n# Body\n");
        assert_eq!(parts.metadata.as_deref(), Some("title: x"));
        assert_eq!(parts.body, "# Body\n");
        let crlf = split_markdown_frontmatter("\u{FEFF}---\r\na: 1\r\n...\r\nrest");
        assert_eq!(crlf.metadata.as_deref(), Some("a: 1"));
        assert_eq!(crlf.body, "rest");
    }

    #[test]
    fn keeps_unfinished_or_missing_headers_in_the_body() {
        let open = "---\ntitle: x\n";
        assert_eq!(split_markdown_frontmatter(open).metadata, None);
        assert_eq!(split_markdown_frontmatter(open).body, open);
        assert_eq!(split_markdown_frontmatter("# Plain").body, "# Plain");
        let empty = split_markdown_frontmatter("---\n---\nbody");
        assert_eq!(empty.metadata.as_deref(), Some(""));
        assert_eq!(empty.body, "body");
    }
}
