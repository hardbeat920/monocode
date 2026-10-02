//! Syntax highlighting for fenced code blocks.
//!
//! Replaces Shiki (through `@streamdown/code`). Grammars are the Sublime Text
//! syntaxes from two-face (bat's set) run by syntect, which uses TextMate
//! scopes like Shiki does. Scopes are sorted into [`TokenKind`]s with the
//! selectors of the `github-dark` theme, and [`crate::SyntaxColors`] colors
//! each kind.
//!
//! Highlighting is incremental by line: [`CodeHighlight`] keeps the parser
//! state after the last complete line, so a code block that grows while it
//! streams only pays for its new lines. The partial last line is highlighted
//! from a copy of that state and redone when it grows.
//!
//! The grammar set takes tens of milliseconds to load, so it loads on first
//! use through [`syntaxes_blocking`], which callers run off the UI thread, and
//! [`syntaxes`] returns `None` until then.

use std::ops::Range;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use syntect::highlighting::{
    Color, HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet, SyntaxSetBuilder};

use crate::style::SyntaxColors;

/// A class of syntax token. Each kind maps to one color in [`SyntaxColors`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenKind {
    Plain = 0,
    Comment,
    Keyword,
    String,
    Constant,
    Function,
    Variable,
    Tag,
    Attribute,
    Inserted,
    Deleted,
    Heading,
}

impl TokenKind {
    const ALL: [TokenKind; 12] = [
        TokenKind::Plain,
        TokenKind::Comment,
        TokenKind::Keyword,
        TokenKind::String,
        TokenKind::Constant,
        TokenKind::Function,
        TokenKind::Variable,
        TokenKind::Tag,
        TokenKind::Attribute,
        TokenKind::Inserted,
        TokenKind::Deleted,
        TokenKind::Heading,
    ];

    fn from_code(code: u8) -> Self {
        Self::ALL
            .get(code as usize)
            .copied()
            .unwrap_or(TokenKind::Plain)
    }

    /// The color for this kind.
    pub fn color(self, colors: &SyntaxColors) -> gpui::Hsla {
        match self {
            TokenKind::Plain => colors.plain,
            TokenKind::Comment => colors.comment,
            TokenKind::Keyword => colors.keyword,
            TokenKind::String => colors.string,
            TokenKind::Constant => colors.constant,
            TokenKind::Function => colors.function,
            TokenKind::Variable => colors.variable,
            TokenKind::Tag => colors.tag,
            TokenKind::Attribute => colors.attribute,
            TokenKind::Inserted => colors.inserted,
            TokenKind::Deleted => colors.deleted,
            TokenKind::Heading => colors.heading,
        }
    }
}

/// A colored byte range within one line. Plain text has no token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

/// Scope selectors per kind, from the `github-dark` Shiki theme.
const SELECTORS: &[(&str, TokenKind)] = &[
    (
        "comment, punctuation.definition.comment, string.comment",
        TokenKind::Comment,
    ),
    (
        "constant, entity.name.constant, variable.other.constant, variable.other.enummember, variable.language, support, meta.property-name, support.constant, support.variable",
        TokenKind::Constant,
    ),
    ("entity, entity.name", TokenKind::Function),
    ("entity.other.attribute-name", TokenKind::Attribute),
    (
        "variable.parameter.function, storage.modifier.package, storage.modifier.import, storage.type.java, variable.other, punctuation, meta.embedded, source.groovy.embedded",
        TokenKind::Plain,
    ),
    ("entity.name.tag, markup.quote", TokenKind::Tag),
    ("keyword, storage, storage.type", TokenKind::Keyword),
    (
        "string, punctuation.definition.string, string punctuation.section.embedded source",
        TokenKind::String,
    ),
    ("variable", TokenKind::Variable),
    (
        "markup.heading, markup.heading entity.name",
        TokenKind::Heading,
    ),
    (
        "markup.inserted, meta.diff.header.to-file, punctuation.definition.inserted",
        TokenKind::Inserted,
    ),
    (
        "markup.deleted, meta.diff.header.from-file, punctuation.definition.deleted",
        TokenKind::Deleted,
    ),
];

/// A minimal Mermaid grammar. two-face has none, and Shiki's would need a
/// TextMate-to-Sublime conversion; this covers diagram keywords, arrows,
/// labels, strings, numbers, and `%%` comments.
const MERMAID_SYNTAX: &str = r#"%YAML 1.2
---
name: Mermaid
file_extensions: [mermaid, mmd]
scope: source.mermaid
contexts:
  main:
    - match: '%%.*$'
      scope: comment.line.percentage.mermaid
    - match: '\b(graph|flowchart|sequenceDiagram|classDiagram|stateDiagram-v2|stateDiagram|erDiagram|gantt|pie|journey|gitGraph|mindmap|timeline|quadrantChart|requirementDiagram|sankey-beta|xychart-beta|block-beta|C4Context|C4Container|C4Component)\b'
      scope: keyword.control.diagram.mermaid
    - match: '\b(TD|TB|BT|RL|LR)\b'
      scope: constant.language.direction.mermaid
    - match: '\b(subgraph|end|participant|actor|loop|alt|else|opt|par|and|rect|critical|break|note|over|activate|deactivate|class|state|section|title|dateFormat|axisFormat|excludes|style|classDef|linkStyle|click|direction|autonumber|as|commit|branch|checkout|merge)\b'
      scope: keyword.other.mermaid
    - match: '"'
      scope: punctuation.definition.string.begin.mermaid
      push: double_string
    - match: '\|[^|\n]*\|'
      scope: string.unquoted.label.mermaid
    - match: '(<<-->>|<<->>|-->>|->>|--x|-x|--\)|-\)|==>|-\.->|-\.-|-->|---|~~~|<\|--|\*--|o--|\.\.>|\.\.|--|->|<-|:::)'
      scope: keyword.operator.arrow.mermaid
    - match: '\b\d+(\.\d+)?\b'
      scope: constant.numeric.mermaid
  double_string:
    - meta_scope: string.quoted.double.mermaid
    - match: '"'
      scope: punctuation.definition.string.end.mermaid
      pop: true
"#;

/// The loaded grammar sets and the classification theme.
pub struct Syntaxes {
    extra: SyntaxSet,
    mermaid: SyntaxSet,
    theme: Theme,
}

static SYNTAXES: OnceLock<Syntaxes> = OnceLock::new();

/// The grammar sets, if they have finished loading.
pub fn syntaxes() -> Option<&'static Syntaxes> {
    SYNTAXES.get()
}

/// Load the grammar sets, blocking until they are ready. Run this off the UI
/// thread.
pub fn syntaxes_blocking() -> &'static Syntaxes {
    SYNTAXES.get_or_init(Syntaxes::load)
}

impl Syntaxes {
    fn load() -> Self {
        let extra = two_face::syntax::extra_newlines();
        let mut builder = SyntaxSetBuilder::new();
        if let Ok(definition) =
            syntect::parsing::SyntaxDefinition::load_from_str(MERMAID_SYNTAX, true, None)
        {
            builder.add(definition);
        }
        let mermaid = builder.build();
        Self {
            extra,
            mermaid,
            theme: classification_theme(),
        }
    }

    /// The grammar for a fence language, or `None` to show plain code.
    pub fn find(&self, language: &str) -> Option<(&SyntaxSet, &SyntaxReference)> {
        let lower = language.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return None;
        }
        if lower == "mermaid" || lower == "mmd" {
            let syntax = self.mermaid.syntaxes().first()?;
            return Some((&self.mermaid, syntax));
        }
        let token = match lower.as_str() {
            "js" | "javascript" | "jsx" | "mjs" | "cjs" | "node" => "js",
            "ts" | "typescript" | "mts" | "cts" => "ts",
            "tsx" => "tsx",
            "sh" | "bash" | "zsh" | "shell" | "console" | "shellscript" | "shell-session" => "sh",
            "py" | "python" | "python3" => "py",
            "rb" | "ruby" => "rb",
            "rs" | "rust" => "rs",
            "yml" | "yaml" => "yaml",
            "md" | "markdown" | "mdx" => "md",
            "cs" | "csharp" | "c#" => "cs",
            "cpp" | "c++" | "cc" | "cxx" | "hpp" => "cpp",
            "kt" | "kotlin" | "kts" => "kt",
            "go" | "golang" => "go",
            "json" | "jsonc" | "json5" => "json",
            "diff" | "patch" => "diff",
            "make" | "makefile" => "Makefile",
            "dockerfile" | "docker" => "Dockerfile",
            "objc" | "objective-c" => "m",
            "ps1" | "powershell" | "pwsh" => "ps1",
            "viml" | "vim" => "vim",
            "proto" | "protobuf" => "proto",
            "html" | "xml" | "svg" | "css" | "scss" | "sql" | "toml" | "swift" | "java" | "php"
            | "lua" | "zig" | "c" | "h" | "r" | "scala" | "dart" | "elixir" | "ex" | "erlang"
            | "hs" | "haskell" | "ini" | "nix" | "graphql" | "gql" => lower.as_str(),
            other => other,
        };
        let syntax = self
            .extra
            .find_syntax_by_token(token)
            .or_else(|| self.extra.find_syntax_by_token(&lower))?;
        Some((&self.extra, syntax))
    }

    fn highlighter(&self) -> Highlighter<'_> {
        Highlighter::new(&self.theme)
    }
}

fn kind_color(kind: TokenKind) -> Color {
    Color {
        r: kind as u8,
        g: 0,
        b: 0,
        a: 255,
    }
}

fn classification_theme() -> Theme {
    let scopes = SELECTORS
        .iter()
        .filter_map(|(selectors, kind)| {
            Some(ThemeItem {
                scope: ScopeSelectors::from_str(selectors).ok()?,
                style: StyleModifier {
                    foreground: Some(kind_color(*kind)),
                    background: None,
                    font_style: None,
                },
            })
        })
        .collect();
    Theme {
        name: Some("monocode-classify".into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(kind_color(TokenKind::Plain)),
            ..ThemeSettings::default()
        },
        scopes,
    }
}

/// Incremental highlight state for one code block.
#[derive(Clone)]
pub struct CodeHighlight {
    language: String,
    syntax_name: String,
    /// The code covered by `lines`: complete lines, each ending in `\n`.
    done: String,
    lines: Vec<Arc<[Token]>>,
    /// Tokens for the partial last line, if the code does not end in `\n`.
    tail: Option<(String, Arc<[Token]>)>,
    parse_state: ParseState,
    highlight_state: HighlightState,
}

impl CodeHighlight {
    /// Start highlighting `language`. Returns `None` when no grammar matches.
    pub fn new(syntaxes: &Syntaxes, language: &str) -> Option<Self> {
        let (_, syntax) = syntaxes.find(language)?;
        let highlighter = syntaxes.highlighter();
        Some(Self {
            language: language.to_string(),
            syntax_name: syntax.name.clone(),
            done: String::new(),
            lines: Vec::new(),
            tail: None,
            parse_state: ParseState::new(syntax),
            highlight_state: HighlightState::new(&highlighter, ScopeStack::new()),
        })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// The grammar's display name, such as `Rust`.
    pub fn syntax_name(&self) -> &str {
        &self.syntax_name
    }

    /// Bring the highlight up to date with `code`. Costs O(new lines) when
    /// `code` extends the previous code, and restarts otherwise.
    pub fn update(&mut self, syntaxes: &Syntaxes, code: &str) {
        if !code.starts_with(self.done.as_str())
            && let Some(fresh) = Self::new(syntaxes, &self.language)
        {
            *self = fresh;
        }
        let Some((set, _)) = syntaxes.find(&self.language) else {
            return;
        };
        let highlighter = syntaxes.highlighter();
        let rest = &code[self.done.len()..];
        let mut consumed = 0;
        for line in rest.split_inclusive('\n') {
            if !line.ends_with('\n') {
                break;
            }
            let tokens = highlight_line(
                line,
                &mut self.parse_state,
                &mut self.highlight_state,
                set,
                &highlighter,
            );
            self.lines.push(tokens.into());
            consumed += line.len();
        }
        self.done.push_str(&rest[..consumed]);
        let partial = &rest[consumed..];
        if partial.is_empty() {
            self.tail = None;
            return;
        }
        if self.tail.as_ref().is_some_and(|(text, _)| text == partial) {
            return;
        }
        // Highlight the partial line from a copy of the committed state.
        let mut parse_state = self.parse_state.clone();
        let mut highlight_state = self.highlight_state.clone();
        let mut line = String::with_capacity(partial.len() + 1);
        line.push_str(partial);
        line.push('\n');
        let mut tokens = highlight_line(
            &line,
            &mut parse_state,
            &mut highlight_state,
            set,
            &highlighter,
        );
        for token in &mut tokens {
            token.range.end = token.range.end.min(partial.len());
        }
        tokens.retain(|token| !token.range.is_empty());
        self.tail = Some((partial.to_string(), tokens.into()));
    }

    /// Tokens for line `ix` of the code passed to the last [`Self::update`].
    pub fn line(&self, ix: usize) -> &[Token] {
        if let Some(line) = self.lines.get(ix) {
            return line;
        }
        match &self.tail {
            Some((_, tokens)) if ix == self.lines.len() => tokens,
            _ => &[],
        }
    }

    /// Number of highlighted lines, the partial last line included.
    pub fn line_count(&self) -> usize {
        self.lines.len() + usize::from(self.tail.is_some())
    }
}

fn highlight_line(
    line: &str,
    parse_state: &mut ParseState,
    highlight_state: &mut HighlightState,
    set: &SyntaxSet,
    highlighter: &Highlighter,
) -> Vec<Token> {
    let Ok(ops) = parse_state.parse_line(line, set) else {
        return Vec::new();
    };
    let content_len = line.trim_end_matches(['\n', '\r']).len();
    let mut tokens: Vec<Token> = Vec::new();
    let mut at = 0;
    for (style, piece) in HighlightIterator::new(highlight_state, &ops, line, highlighter) {
        let start = at;
        at += piece.len();
        let end = at.min(content_len);
        if start >= end {
            continue;
        }
        let kind = TokenKind::from_code(style.foreground.r);
        if kind == TokenKind::Plain {
            continue;
        }
        match tokens.last_mut() {
            Some(last) if last.kind == kind && last.range.end == start => last.range.end = end,
            _ => tokens.push(Token {
                range: start..end,
                kind,
            }),
        }
    }
    tokens
}

/// Highlight a whole code block at once.
pub fn highlight_all(syntaxes: &Syntaxes, language: &str, code: &str) -> Option<CodeHighlight> {
    let mut highlight = CodeHighlight::new(syntaxes, language)?;
    highlight.update(syntaxes, code);
    Some(highlight)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(highlight: &CodeHighlight, code: &str, line: usize) -> Vec<(String, TokenKind)> {
        let text = code.split('\n').nth(line).unwrap();
        highlight
            .line(line)
            .iter()
            .map(|token| (text[token.range.clone()].to_string(), token.kind))
            .collect()
    }

    #[test]
    fn rust_keywords_strings_and_comments() {
        let syntaxes = syntaxes_blocking();
        let code = "fn main() {\n    let s = \"hi\"; // greet\n}";
        let highlight = highlight_all(syntaxes, "rust", code).unwrap();
        assert_eq!(highlight.line_count(), 3);
        let first = kinds(&highlight, code, 0);
        assert!(
            first.contains(&("fn".into(), TokenKind::Keyword)),
            "{first:?}"
        );
        assert!(
            first.contains(&("main".into(), TokenKind::Function)),
            "{first:?}"
        );
        let second = kinds(&highlight, code, 1);
        assert!(
            second
                .iter()
                .any(|(t, k)| t.contains("hi") && *k == TokenKind::String)
        );
        assert!(
            second
                .iter()
                .any(|(t, k)| t.contains("greet") && *k == TokenKind::Comment)
        );
    }

    #[test]
    fn languages_resolve_through_aliases() {
        let syntaxes = syntaxes_blocking();
        for language in [
            "ts",
            "tsx",
            "js",
            "bash",
            "zsh",
            "py",
            "json",
            "yaml",
            "toml",
            "go",
            "diff",
            "sql",
            "html",
            "css",
            "c",
            "cpp",
            "swift",
            "java",
            "md",
            "mermaid",
            "Rust",
            "dockerfile",
        ] {
            assert!(syntaxes.find(language).is_some(), "{language}");
        }
        assert!(syntaxes.find("").is_none());
        assert!(syntaxes.find("no-such-language").is_none());
    }

    #[test]
    fn streaming_updates_match_a_full_highlight() {
        let syntaxes = syntaxes_blocking();
        let code = "const a = [1, 2, 3];\n/* note */\nfunction f(x) {\n  return `t${x}`;\n}\n";
        let full = highlight_all(syntaxes, "js", code).unwrap();
        let mut streamed = CodeHighlight::new(syntaxes, "js").unwrap();
        for end in 1..=code.len() {
            streamed.update(syntaxes, &code[..end]);
        }
        assert_eq!(streamed.line_count(), full.line_count());
        for ix in 0..full.line_count() {
            assert_eq!(streamed.line(ix), full.line(ix), "line {ix}");
        }
    }

    #[test]
    fn rewritten_code_restarts() {
        let syntaxes = syntaxes_blocking();
        let mut highlight = CodeHighlight::new(syntaxes, "py").unwrap();
        highlight.update(syntaxes, "x = 1\n``");
        highlight.update(syntaxes, "x = 1");
        assert_eq!(highlight.line_count(), 1);
        highlight.update(syntaxes, "def f():\n    pass\n");
        assert_eq!(highlight.line_count(), 2);
        assert!(
            highlight
                .line(0)
                .iter()
                .any(|t| t.kind == TokenKind::Keyword)
        );
    }

    #[test]
    fn mermaid_gets_colors() {
        let syntaxes = syntaxes_blocking();
        let code = "graph TD\n  A[Start] -->|yes| B(\"Done\") %% note";
        let highlight = highlight_all(syntaxes, "mermaid", code).unwrap();
        let first = kinds(&highlight, code, 0);
        assert!(
            first.contains(&("graph".into(), TokenKind::Keyword)),
            "{first:?}"
        );
        let second = kinds(&highlight, code, 1);
        assert!(
            second
                .iter()
                .any(|(t, k)| t == "-->" && *k == TokenKind::Keyword),
            "{second:?}"
        );
        assert!(
            second
                .iter()
                .any(|(t, k)| t.contains("Done") && *k == TokenKind::String)
        );
        assert!(
            second
                .iter()
                .any(|(t, k)| t.contains("note") && *k == TokenKind::Comment)
        );
    }
}

#[cfg(test)]
mod speed {
    use super::*;

    #[test]
    #[ignore]
    fn highlight_speed() {
        let syntaxes = syntaxes_blocking();
        for language in ["rust", "ts", "python", "bash", "js"] {
            let code: String = (0..600)
                .map(|i| format!("    let value_{i} = compute({i}, \"label\"); // step {i}\n"))
                .collect();
            let start = std::time::Instant::now();
            let h = highlight_all(syntaxes, language, &code).unwrap();
            eprintln!(
                "{language}: 600 lines in {:?} ({} lines)",
                start.elapsed(),
                h.line_count()
            );
        }
    }
}
