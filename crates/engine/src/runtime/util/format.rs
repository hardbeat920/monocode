//! Native document formatting for the file types supported by the editor.
//! Cursor offsets use UTF-16 units, as in the persisted editor API.

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

/// Format a supported document and map its UTF-16 cursor offset through the edits.
/// Invalid source and unsupported or oversized files return `None`.
pub fn format_text(path: &str, source: &str, cursor_offset: usize) -> Option<Formatted> {
    if monocode_core::js::len(source) > MAX_FORMAT_CHARS {
        return None;
    }
    let formatted = format_source(path, source, 0)?;
    let cursor_offset = map_cursor(source, &formatted, cursor_offset);
    Some(Formatted {
        formatted,
        cursor_offset,
    })
}

fn format_source(path: &str, source: &str, depth: usize) -> Option<String> {
    if depth > 8 || monocode_core::js::len(source) > MAX_FORMAT_CHARS {
        return None;
    }
    match parser_for_path(path)? {
        ParserName::Babel | ParserName::Typescript => {
            let config = dprint_plugin_typescript::configuration::ConfigurationBuilder::new()
                .deno()
                .line_width(80)
                .build();
            dprint_plugin_typescript::format_text(dprint_plugin_typescript::FormatTextOptions {
                path: std::path::Path::new(path),
                extension: None,
                text: source.into(),
                config: &config,
                external_formatter: None,
            })
            .ok()
            .map(|result| result.unwrap_or_else(|| source.to_owned()))
        }
        ParserName::Json => {
            let config = dprint_plugin_json::configuration::ConfigurationBuilder::new()
                .line_width(80)
                .indent_width(2)
                .build();
            dprint_plugin_json::format_text(std::path::Path::new(path), source, &config)
                .ok()
                .map(|result| result.unwrap_or_else(|| source.to_owned()))
        }
        ParserName::Css => malva::format_text(source, malva::Syntax::Css, &Default::default()).ok(),
        ParserName::Html => markup_fmt::format_text(
            source,
            markup_fmt::Language::Html,
            &Default::default(),
            |code, hints| {
                let path = format!("embedded.{}", hints.ext);
                Ok(format_source(&path, code, depth + 1)
                    .map(std::borrow::Cow::Owned)
                    .unwrap_or_else(|| std::borrow::Cow::Borrowed(code)))
            },
        )
        .ok(),
        ParserName::Markdown | ParserName::Mdx => {
            let config = dprint_plugin_markdown::configuration::ConfigurationBuilder::new()
                .line_width(80)
                .build();
            dprint_plugin_markdown::format_text(source, &config, |language, code, _| {
                let extension = match language.to_ascii_lowercase().as_str() {
                    "javascript" | "js" | "jsx" => "jsx",
                    "typescript" | "ts" => "ts",
                    "tsx" => "tsx",
                    "json" => "json",
                    "css" => "css",
                    "html" => "html",
                    _ => return Ok(None),
                };
                Ok(format_source(
                    &format!("embedded.{extension}"),
                    code,
                    depth + 1,
                ))
            })
            .ok()
            .map(|result| result.unwrap_or_else(|| source.to_owned()))
        }
    }
}

fn map_cursor(source: &str, formatted: &str, cursor: usize) -> usize {
    let cursor = cursor.min(source.encode_utf16().count());
    if source == formatted {
        return cursor;
    }
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(100))
        .diff_chars(source, formatted);
    let mut before = 0;
    let mut after = 0;
    for change in diff.iter_all_changes() {
        let length = change.value().encode_utf16().count();
        match change.tag() {
            similar::ChangeTag::Insert => after += length,
            similar::ChangeTag::Delete => {
                if before + length > cursor {
                    return after;
                }
                before += length;
            }
            similar::ChangeTag::Equal => {
                if before + length > cursor {
                    return after + cursor - before;
                }
                before += length;
                after += length;
            }
        }
    }
    after
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
        assert!(format_text("a.ts", "let x", 0).is_some());
    }

    #[test]
    fn formats_supported_documents_without_an_external_process() {
        for (path, source, expected) in [
            ("a.js", "const x={a:1}", "const x ="),
            ("a.tsx", "const x: number=1", "const x: number = 1;"),
            (
                "a.json",
                "{\"number\":10000000000000000000001}",
                "10000000000000000000001",
            ),
            ("a.css", "a{color:red}", "color: red;"),
            (
                "a.html",
                "<div class=container></div>",
                "class=\"container\"",
            ),
            (
                "a.md",
                "#   Heading\n\n```ts\nconst x=1\n```",
                "const x = 1;",
            ),
            (
                "a.mdx",
                "#   Heading\n\n<Widget title=\"x\" />\n",
                "<Widget title=\"x\" />",
            ),
        ] {
            let formatted = format_text(path, source, 0).unwrap_or_else(|| panic!("{path}"));
            assert!(
                formatted.formatted.contains(expected),
                "{path}: {}",
                formatted.formatted
            );
            let twice = format_text(path, &formatted.formatted, formatted.cursor_offset).unwrap();
            assert_eq!(
                twice.formatted, formatted.formatted,
                "{path} must be idempotent"
            );
        }
    }

    #[test]
    fn rejects_invalid_and_oversized_source() {
        assert!(format_text("a.ts", "const = {", 0).is_none());
        assert!(format_text("a.json", "{", 0).is_none());
        assert!(format_text("a.css", "a{", 0).is_none());
        assert!(format_text("a.html", "<div>", 0).is_none());
        assert!(format_text("a.ts", &"x".repeat(MAX_FORMAT_CHARS + 1), 0).is_none());
    }

    #[test]
    fn cursor_stays_with_the_same_text_after_whitespace_edits() {
        let source = "const x=\"😀\";const y=1";
        let cursor = source[..source.find('y').unwrap()].encode_utf16().count();
        let result = format_text("a.ts", source, cursor).unwrap();
        let expected = result.formatted[..result.formatted.find('y').unwrap()]
            .encode_utf16()
            .count();
        assert_eq!(result.cursor_offset, expected);
    }
}
