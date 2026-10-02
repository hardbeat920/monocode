//! Port of src/shared/lib/format.ts: which Prettier parser formats a file.
//!
//! The TypeScript formatted with Prettier's standalone build in the webview.
//! There is no Prettier in Rust, so `format_text` returns `None` (the
//! TypeScript's "cannot format" result) until a formatter is chosen.

use monocode_core::paths::basename;

/// `MAX_FORMAT_CHARS`.
pub const MAX_FORMAT_CHARS: usize = 512 * 1024;

/// `ParserName`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParserName {
    Babel,
    Typescript,
    Json,
    Css,
    Html,
    Markdown,
    Mdx,
}

/// `parserForPath`.
pub fn parser_for_path(path: &str) -> Option<ParserName> {
    let name = basename(path).to_lowercase();
    let extension = name.rfind('.').map(|dot| &name[dot..]).unwrap_or("");
    match extension {
        ".js" | ".jsx" | ".mjs" | ".cjs" => Some(ParserName::Babel),
        ".ts" | ".tsx" | ".mts" | ".cts" => Some(ParserName::Typescript),
        ".json" => Some(ParserName::Json),
        ".css" => Some(ParserName::Css),
        ".html" | ".htm" => Some(ParserName::Html),
        ".md" | ".markdown" => Some(ParserName::Markdown),
        ".mdx" => Some(ParserName::Mdx),
        _ => None,
    }
}

/// The formatted text and the cursor's new offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    pub formatted: String,
    pub cursor_offset: usize,
}

/// `formatText`.
///
/// TODO(port): no formatter yet, so every file reports "cannot format".
pub fn format_text(path: &str, source: &str, _cursor_offset: usize) -> Option<Formatted> {
    if monocode_core::js::len(source) > MAX_FORMAT_CHARS {
        return None;
    }
    parser_for_path(path)?;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_a_parser_by_extension() {
        assert_eq!(parser_for_path("/a/App.TSX"), Some(ParserName::Typescript));
        assert_eq!(parser_for_path("x.mjs"), Some(ParserName::Babel));
        assert_eq!(
            parser_for_path("README.markdown"),
            Some(ParserName::Markdown)
        );
        assert_eq!(parser_for_path("Makefile"), None);
        assert_eq!(format_text("a.ts", "let x", 0), None);
    }
}
