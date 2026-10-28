//! AgentMarkdown's existing Shiki Dark+ palette applied to native text runs.
use std::{ops::Range, sync::OnceLock};
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, FontStyle, StyleModifier, Theme, ThemeItem},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
pub(crate) struct HighlightRequest<'a> {
    pub source: &'a str,
    pub path: Option<&'a str>,
    pub fence_tag: Option<&'a str>,
}
pub(crate) struct HighlightSpan {
    pub range: Range<usize>,
    pub color: u32,
}
pub(crate) struct HighlightedDocument {
    pub lines: Vec<Vec<HighlightSpan>>,
}
fn color(value: &str) -> Option<Color> {
    let value = value.strip_prefix('#')?;
    let rgb = u32::from_str_radix(value, 16).ok()?;
    Some(Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 255,
    })
}
fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../assets/dark-plus.json")).unwrap();
        let mut theme = Theme::default();
        theme.settings.foreground = color("#D4D4D4");
        for token in data["tokenColors"].as_array().into_iter().flatten() {
            let scopes: Vec<_> = if let Some(s) = token["scope"].as_str() {
                vec![s]
            } else {
                token["scope"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.as_str())
                    .collect()
            };
            for scope in scopes {
                if let Ok(scope) = scope.parse() {
                    let mut font = FontStyle::empty();
                    let font_style = token["settings"]["fontStyle"].as_str().unwrap_or("");
                    if font_style.contains("italic") {
                        font |= FontStyle::ITALIC;
                    }
                    if font_style.contains("bold") {
                        font |= FontStyle::BOLD;
                    }
                    theme.scopes.push(ThemeItem {
                        scope,
                        style: StyleModifier {
                            foreground: token["settings"]["foreground"].as_str().and_then(color),
                            font_style: (!font.is_empty()).then_some(font),
                            ..Default::default()
                        },
                    });
                }
            }
        }
        theme
    })
}
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAX: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Highlights a document a line at a time, carrying the grammar's state from
/// line to line, so a long file can be coloured in pieces exactly as whole.
pub(crate) struct LineHighlighter {
    inner: HighlightLines<'static>,
}
impl LineHighlighter {
    pub(crate) fn new(path: Option<&str>, fence_tag: Option<&str>) -> Self {
        let syntax = syntaxes();
        let language = fence_tag.or(path).unwrap_or("");
        let ext = match language {
            "rust" => "rs",
            "javascript" => "js",
            "typescript" | "tsx" => "ts",
            "python" => "py",
            "bash" | "shell" => "sh",
            "markdown" => "md",
            other => other,
        };
        let definition = syntax
            .find_syntax_by_extension(ext)
            .or_else(|| syntax.find_syntax_by_token(language))
            .or_else(|| {
                matches!(ext, "ts" | "tsx" | "jsx" | "plaintext" | "text" | "")
                    .then(|| syntax.find_syntax_by_extension("js"))
                    .flatten()
            })
            .unwrap_or_else(|| syntax.find_syntax_plain_text());
        Self {
            inner: HighlightLines::new(definition, theme()),
        }
    }
    /// One line, with or without its line ending; spans cover its text only.
    pub(crate) fn line(&mut self, line: &str) -> Result<Vec<HighlightSpan>, ()> {
        let owned;
        let line = if line.ends_with('\n') {
            line
        } else {
            owned = format!("{line}\n");
            &owned
        };
        let text_end = line.trim_end_matches(['\n', '\r']).len();
        let mut spans = Vec::new();
        let mut offset = 0;
        for (style, text) in self
            .inner
            .highlight_line(line, syntaxes())
            .map_err(|_| ())?
        {
            let end = (offset + text.len()).min(text_end);
            if offset < end {
                spans.push(HighlightSpan {
                    range: offset..end,
                    color: (u32::from(style.foreground.r) << 16)
                        | (u32::from(style.foreground.g) << 8)
                        | u32::from(style.foreground.b),
                });
            }
            offset += text.len();
        }
        Ok(spans)
    }
}

pub(crate) fn highlight(request: HighlightRequest<'_>) -> Result<HighlightedDocument, ()> {
    if request.source.len() > 500_000 {
        return Err(());
    }
    let mut highlighter = LineHighlighter::new(request.path, request.fence_tag);
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(request.source) {
        lines.push(highlighter.line(line)?);
    }
    Ok(HighlightedDocument { lines })
}
