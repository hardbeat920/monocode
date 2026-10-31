//! The file editor's syntax on Tree-sitter: highlight tokens, folds, syntax
//! errors and the git gutter, in the `Analysis` shape the editor renders. A
//! session keeps its last tree, so an edit reparses and re-highlights only the
//! lines it touched. Languages without a grammar here use the QuickJS path.
use crate::editor_analysis::{Analysis, Change, Diagnostic, Edit, Fold, Token};
use gpui::SharedString;
use std::{
    collections::HashMap,
    ops::Range,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Language, Node, Parser, Point, Query, QueryCursor, Tree};
use tree_sitter_language::LanguageFn;

/// Mirrors editorLint.ts: past this a file is a viewer, not linted.
const MAX_LINT_BYTES: usize = 512 * 1024;
/// One bad character can strand the rest of the file behind follow-on errors.
const MAX_DIAGNOSTICS: usize = 50;
const MAX_TOKEN_CHARS: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Lang {
    JavaScript,
    TypeScript,
    Tsx,
    Json,
    Css,
    Html,
    Python,
    Rust,
    Go,
    Bash,
    C,
    Cpp,
    Java,
    Yaml,
    Toml,
    Ruby,
}

/// The same file → language decisions as `languageForPath` in editorLanguage.ts,
/// for the languages that have a Tree-sitter grammar here.
fn language_for_path(path: &str) -> Option<Lang> {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    let extension = name.rfind('.').map_or("", |at| &name[at..]);
    Some(match extension {
        ".js" | ".jsx" | ".mjs" | ".cjs" => Lang::JavaScript,
        ".ts" | ".mts" | ".cts" => Lang::TypeScript,
        ".tsx" => Lang::Tsx,
        ".json" | ".jsonc" => Lang::Json,
        ".css" => Lang::Css,
        ".html" | ".htm" => Lang::Html,
        ".py" => Lang::Python,
        ".rs" => Lang::Rust,
        ".go" => Lang::Go,
        ".sh" | ".bash" | ".zsh" => Lang::Bash,
        ".c" => Lang::C,
        ".h" | ".cc" | ".cpp" | ".cxx" | ".hh" | ".hpp" | ".hxx" => Lang::Cpp,
        ".java" => Lang::Java,
        ".yaml" | ".yml" => Lang::Yaml,
        ".toml" => Lang::Toml,
        ".rb" | ".rake" => Lang::Ruby,
        _ => match name.as_str() {
            ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile" => Lang::Bash,
            "gemfile" | "rakefile" => Lang::Ruby,
            _ => return None,
        },
    })
}

/// Whether `path` is analysed natively; everything else uses QuickJS.
pub(crate) fn supports(path: &str) -> bool {
    language_for_path(path).is_some()
}

/// Same list as LINTABLE_EXTENSIONS in editorLint.ts.
fn lintable(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    let extension = name.rfind('.').map_or("", |at| &name[at..]);
    matches!(
        extension,
        ".js"
            | ".jsx"
            | ".mjs"
            | ".cjs"
            | ".ts"
            | ".tsx"
            | ".json"
            | ".css"
            | ".html"
            | ".htm"
            | ".py"
    )
}

#[derive(Clone, Copy)]
enum Capture {
    /// Captures that keep the default colour (variables, operators, …) do not
    /// claim their node, so a later, more specific pattern can still colour it.
    Ignore,
    /// Template substitutions and the like: the default colour inside an
    /// otherwise coloured node.
    Reset,
    Color(&'static str),
}

/// Tree-sitter capture names → the editor's `--syntax-*` groups, matching the
/// Lezer tag groups in editorLanguage.ts.
fn capture(name: &str, attribute_is_property: bool) -> Capture {
    let group = |name: &str| name.split('.').next().unwrap_or(name).to_owned();
    Capture::Color(match name {
        "embedded" => return Capture::Reset,
        "boolean" | "constant.builtin" | "variable.builtin" => "var(--syntax-keyword)",
        "label" => "var(--syntax-callable)",
        "escape" | "character" => "var(--syntax-string)",
        "constructor" | "namespace" | "module" => "var(--syntax-type)",
        "tag.error" => "var(--syntax-invalid)",
        "number" | "float" | "constant.numeric" => "var(--syntax-number)",
        "text.title" => "var(--color-markdown-heading)",
        "string.special.key" => "var(--syntax-property)",
        _ => match group(name).as_str() {
            "keyword" => "var(--syntax-keyword)",
            "function" => "var(--syntax-callable)",
            "string" => "var(--syntax-string)",
            "type" => "var(--syntax-type)",
            "tag" => "var(--syntax-tag)",
            "comment" => "var(--syntax-comment)",
            "property" => "var(--syntax-property)",
            "attribute" if attribute_is_property => "var(--syntax-property)",
            "attribute" => "var(--syntax-meta)",
            _ if name.starts_with("markup.heading") => "var(--color-markdown-heading)",
            _ => return Capture::Ignore,
        },
    })
}

struct Grammar {
    language: Language,
    query: Query,
    captures: Vec<Capture>,
    /// Patterns gated on `#is?`/`#is-not?` local-scope predicates. Without a
    /// locals pass they would match everywhere, so they are skipped.
    skipped: Vec<bool>,
}

impl Grammar {
    fn new(lang: Lang) -> Result<Self, String> {
        use tree_sitter_javascript as js;
        let (language, sources, attribute_is_property): (LanguageFn, Vec<&str>, bool) = match lang {
            Lang::JavaScript => (
                js::LANGUAGE,
                vec![js::HIGHLIGHT_QUERY, js::JSX_HIGHLIGHT_QUERY],
                true,
            ),
            Lang::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                vec![
                    js::HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                ],
                false,
            ),
            Lang::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX,
                vec![
                    js::HIGHLIGHT_QUERY,
                    js::JSX_HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                ],
                true,
            ),
            Lang::Json => (
                tree_sitter_json::LANGUAGE,
                vec![tree_sitter_json::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Css => (
                tree_sitter_css::LANGUAGE,
                vec![tree_sitter_css::HIGHLIGHTS_QUERY],
                true,
            ),
            Lang::Html => (
                tree_sitter_html::LANGUAGE,
                vec![tree_sitter_html::HIGHLIGHTS_QUERY],
                true,
            ),
            Lang::Python => (
                tree_sitter_python::LANGUAGE,
                vec![tree_sitter_python::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Rust => (
                tree_sitter_rust::LANGUAGE,
                vec![tree_sitter_rust::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Go => (
                tree_sitter_go::LANGUAGE,
                vec![tree_sitter_go::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Bash => (
                tree_sitter_bash::LANGUAGE,
                vec![tree_sitter_bash::HIGHLIGHT_QUERY],
                false,
            ),
            Lang::C => (
                tree_sitter_c::LANGUAGE,
                vec![tree_sitter_c::HIGHLIGHT_QUERY],
                false,
            ),
            // The C++ query only adds to C's, as in the grammar's own config.
            Lang::Cpp => (
                tree_sitter_cpp::LANGUAGE,
                vec![
                    tree_sitter_c::HIGHLIGHT_QUERY,
                    tree_sitter_cpp::HIGHLIGHT_QUERY,
                ],
                false,
            ),
            Lang::Java => (
                tree_sitter_java::LANGUAGE,
                vec![tree_sitter_java::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Yaml => (
                tree_sitter_yaml::LANGUAGE,
                vec![tree_sitter_yaml::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Toml => (
                tree_sitter_toml_ng::LANGUAGE,
                vec![tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
                false,
            ),
            Lang::Ruby => (
                tree_sitter_ruby::LANGUAGE,
                vec![tree_sitter_ruby::HIGHLIGHTS_QUERY],
                false,
            ),
        };
        let language = Language::new(language);
        let query =
            Query::new(&language, &sources.join("\n")).map_err(|e| format!("{lang:?}: {e}"))?;
        let captures = query
            .capture_names()
            .iter()
            .map(|name| capture(name, attribute_is_property))
            .collect();
        let skipped = (0..query.pattern_count())
            .map(|ix| !query.property_predicates(ix).is_empty())
            .collect();
        Ok(Self {
            language,
            query,
            captures,
            skipped,
        })
    }
}

/// Queries are compiled once per language and shared by every session.
fn grammar(lang: Lang) -> Option<&'static Grammar> {
    static GRAMMARS: OnceLock<Mutex<HashMap<Lang, Option<&'static Grammar>>>> = OnceLock::new();
    let mut grammars = GRAMMARS.get_or_init(Default::default).lock().ok()?;
    *grammars
        .entry(lang)
        .or_insert_with(|| match Grammar::new(lang) {
            Ok(grammar) => Some(Box::leak(Box::new(grammar))),
            Err(error) => {
                eprintln!("monocode: syntax grammar failed: {error}");
                None
            }
        })
}

/// Highlights `range` of the tree (clipped to it) as flat, sorted,
/// non-overlapping tokens. Nested captures win over their ancestors, and of
/// several patterns on one node the earliest wins, as in tree-sitter-highlight.
fn highlight(grammar: &Grammar, tree: &Tree, text: &str, range: Range<usize>) -> Vec<Token> {
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let mut spans: Vec<(usize, usize, usize, Capture)> = Vec::new();
    let mut captures = cursor.captures(&grammar.query, tree.root_node(), text.as_bytes());
    while let Some((found, ix)) = captures.next() {
        if grammar.skipped[found.pattern_index] {
            continue;
        }
        let capture = found.captures[*ix];
        let kind = grammar.captures[capture.index as usize];
        if matches!(kind, Capture::Ignore) {
            continue;
        }
        let node = capture.node;
        if node.start_byte() < node.end_byte() {
            spans.push((
                node.start_byte(),
                node.end_byte(),
                found.pattern_index,
                kind,
            ));
        }
    }
    // Outer nodes first; on identical nodes the earliest pattern first.
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    spans.dedup_by(|later, first| later.0 == first.0 && later.1 == first.1);

    let mut tokens: Vec<Token> = Vec::new();
    let mut emit = |from: usize, to: usize, kind: Capture| {
        let (from, to) = (from.max(range.start), to.min(range.end));
        let Capture::Color(kind) = kind else { return };
        if from >= to {
            return;
        }
        if let Some(last) = tokens.last_mut()
            && last.to == from
            && last.kind == kind
        {
            last.to = to;
        } else {
            tokens.push(Token {
                from,
                to,
                kind: SharedString::new_static(kind),
            });
        }
    };
    let mut stack: Vec<(usize, Capture)> = Vec::new();
    let mut at = 0;
    for (from, to, _, kind) in spans {
        while let Some(&(end, outer)) = stack.last()
            && end <= from
        {
            emit(at, end, outer);
            at = at.max(end);
            stack.pop();
        }
        if let Some(&(end, outer)) = stack.last() {
            emit(at, from, outer);
            stack.push((to.min(end), kind));
        } else {
            stack.push((to, kind));
        }
        at = at.max(from);
    }
    while let Some((end, outer)) = stack.pop() {
        emit(at, end, outer);
        at = at.max(end);
    }
    tokens
}

/// `<script>` and `<style>` bodies, highlighted with their own grammars.
fn injections(tree: &Tree, text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut parser = Parser::new();
    walk(tree, |node| {
        let lang = match node.kind() {
            "script_element" => Lang::JavaScript,
            "style_element" => Lang::Css,
            _ => return true,
        };
        let mut cursor = node.walk();
        let body = node
            .children(&mut cursor)
            .find(|child| child.kind() == "raw_text");
        if let (Some(body), Some(grammar)) = (body, grammar(lang))
            && parser.set_language(&grammar.language).is_ok()
        {
            let source = &text[body.byte_range()];
            if let Some(inner) = parser.parse(source, None) {
                tokens.extend(
                    highlight(grammar, &inner, source, 0..source.len())
                        .into_iter()
                        .map(|t| Token {
                            from: t.from + body.start_byte(),
                            to: t.to + body.start_byte(),
                            kind: t.kind,
                        }),
                );
            }
        }
        false
    });
    tokens
}

/// Depth-first over the tree; `visit` returns whether to enter the node.
fn walk(tree: &Tree, mut visit: impl FnMut(Node) -> bool) {
    let mut cursor = tree.walk();
    loop {
        if visit(cursor.node()) && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

/// One fold per starting line, from the end of its opening line or bracket to
/// the start of its closer: CodeMirror's `foldable` ranges.
fn folds(tree: &Tree, text: &str) -> Vec<Fold> {
    let mut by_row: HashMap<usize, Fold> = HashMap::new();
    let mut add = |row: usize, from: usize, to: usize| {
        if from < to {
            let fold = by_row.entry(row).or_insert(Fold { from, to });
            if to > fold.to {
                *fold = Fold { from, to };
            }
        }
    };
    let line_end = |at: usize| text[at..].find('\n').map_or(text.len(), |end| at + end);
    walk(tree, |node| {
        let (start, end) = (node.start_position().row, node.end_position().row);
        if start == end {
            return false;
        }
        let mut cursor = node.walk();
        let mut children = node.children(&mut cursor);
        let first = children.next();
        let last = children.last();
        match (first, last) {
            (Some(open), Some(close))
                if !open.is_named()
                    && matches!(
                        (open.kind(), close.kind()),
                        ("{", "}") | ("[", "]") | ("(", ")")
                    ) =>
            {
                if open.end_position().row < close.start_position().row {
                    add(
                        open.start_position().row,
                        open.end_byte(),
                        close.start_byte(),
                    );
                }
            }
            (Some(open), Some(close))
                if open.kind().ends_with("start_tag") && close.kind() == "end_tag" =>
            {
                if open.end_position().row < close.start_position().row {
                    add(
                        open.start_position().row,
                        open.end_byte(),
                        close.start_byte(),
                    );
                }
            }
            // `do … end`, `def … end`, `class … end` and friends.
            (Some(_), Some(close)) if !close.is_named() && close.kind() == "end" => {
                add(start, line_end(node.start_byte()), close.start_byte());
            }
            _ => {}
        }
        // Indented bodies: Python blocks, YAML mappings and sequences.
        match node.kind() {
            "block" if node.prev_sibling().is_some_and(|colon| colon.kind() == ":") => {
                let header = node.prev_sibling().unwrap();
                add(
                    header.start_position().row,
                    header.end_byte(),
                    node.end_byte(),
                );
            }
            "block_mapping_pair" | "block_sequence_item" => {
                add(start, line_end(node.start_byte()), node.end_byte());
            }
            kind if kind.contains("comment") && text[node.byte_range()].starts_with("/*") => {
                add(
                    start,
                    node.start_byte() + 2,
                    node.end_byte().saturating_sub(2),
                );
            }
            _ => {}
        }
        true
    });
    let mut folds: Vec<_> = by_row
        .into_values()
        .filter(|fold| text[fold.from..fold.to].contains('\n'))
        .collect();
    folds.sort_by_key(|fold| fold.from);
    folds
}

/// Error and missing nodes left by the parser's recovery, worded as
/// editorLint.ts words Lezer's.
fn diagnostics(tree: &Tree, text: &str) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    if text.len() > MAX_LINT_BYTES || !tree.root_node().has_error() {
        return found;
    }
    walk(tree, |node| {
        if found.len() >= MAX_DIAGNOSTICS || !node.has_error() {
            return false;
        }
        if !node.is_error() && !node.is_missing() {
            return true;
        }
        let (mut from, mut to) = (node.start_byte(), node.end_byte());
        let message = if node.is_missing() {
            if node.is_named() {
                format!("Expected {}", node.kind())
            } else {
                format!("Expected {}", quote(node.kind()))
            }
        } else {
            let skipped = text[from..to].trim();
            if !skipped.is_empty() {
                format!("Unexpected {}", quote(skipped))
            } else if let Some(token) = token_at(text, from) {
                format!("Unexpected {}", quote(token))
            } else if from >= text.len() {
                "Unexpected end of file".into()
            } else {
                "Unexpected end of line".into()
            }
        };
        // Zero-width points widen to one character, never across a line break.
        if from == to {
            let line_start = text[..from].rfind('\n').map_or(0, |at| at + 1);
            let next = text[from..].chars().next().filter(|ch| *ch != '\n');
            if let Some(ch) = next {
                to += ch.len_utf8();
            } else if from > line_start {
                from -= text[..from].chars().next_back().map_or(0, char::len_utf8);
            }
        }
        found.push(Diagnostic {
            from,
            to,
            severity: "error".into(),
            message,
        });
        // Error nodes nest; their children would repaint the same typo.
        false
    });
    found
}

fn token_at(text: &str, at: usize) -> Option<&str> {
    let line = &text[at..text[at..].find('\n').map_or(text.len(), |end| at + end)];
    let rest = line.trim_start();
    let first = rest.chars().next()?;
    let len = if first.is_alphanumeric() || first == '_' {
        rest.find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .unwrap_or(rest.len())
    } else {
        first.len_utf8()
    };
    Some(&rest[..len])
}

fn quote(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(MAX_TOKEN_CHARS) {
        Some((at, _)) => format!("\"{}…\"", &flat[..at]),
        None => format!("\"{flat}\""),
    }
}

/// Line starts, and where a run of lines ending before line `ix` stops: one
/// past its last line break, as @codemirror/merge's chunk ends are.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}
impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        if text.ends_with('\n') || text.is_empty() {
            starts.pop();
        }
        Self { text, starts }
    }
    fn start(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.text.len())
    }
    fn end(&self, line: usize) -> usize {
        self.starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len() + usize::from(!self.text.ends_with('\n')))
    }
}

/// The git gutter: line chunks between the baseline and the buffer, in the
/// shape the CodeMirror `Chunk.build` path produced.
fn changes(original: &str, current: &str, original_tokens: &[Token]) -> Vec<Change> {
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Myers)
        .deadline(Instant::now() + Duration::from_millis(100))
        .diff_lines(original, current);
    let (old, new) = (Lines::new(original), Lines::new(current));
    // Merge runs of edits, and edits split only by a single blank line, as
    // `toChunks` does when the next change starts on the following line.
    let mut groups: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    let mut gap: Option<(usize, usize, usize)> = None;
    for op in diff.ops() {
        let (old_range, new_range) = (op.old_range(), op.new_range());
        if matches!(op.tag(), similar::DiffTag::Equal) {
            gap = Some((old_range.start, old_range.len(), new_range.start));
            continue;
        }
        let joins = match (groups.last(), gap.take()) {
            (Some(_), None) => true,
            (Some(_), Some((line, 1, _))) => original
                [old.start(line)..old.end(line + 1).min(original.len())]
                .trim_end_matches(['\n', '\r'])
                .is_empty(),
            _ => false,
        };
        match groups.last_mut() {
            Some((a, b)) if joins => {
                a.end = old_range.end;
                b.end = new_range.end;
            }
            _ => groups.push((old_range, new_range)),
        }
    }
    groups
        .into_iter()
        .map(|(a, b)| {
            let from_a = old.start(a.start);
            let to_a = if a.is_empty() { from_a } else { old.end(a.end) };
            let from_b = new.start(b.start);
            let to_b = if b.is_empty() { from_b } else { new.end(b.end) };
            let end_b = from_b.max(to_b.saturating_sub(1));
            let slice = &original[from_a.min(original.len())..to_a.min(original.len())];
            Change {
                kind: match (a.is_empty(), b.is_empty()) {
                    (false, false) => "mod",
                    (false, true) => "del",
                    _ => "add",
                }
                .into(),
                from: from_b.min(current.len()),
                to: end_b.min(current.len()),
                deleted: a.len().max(1),
                original: slice.to_owned(),
                old_line: a.start + 1,
                tokens: original_tokens
                    .iter()
                    .filter(|t| t.to > from_a && t.from < to_a)
                    .map(|t| Token {
                        from: t.from.max(from_a) - from_a,
                        to: t.to.min(to_a) - from_a,
                        kind: t.kind.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn point(text: &str, at: usize) -> Point {
    let line_start = text[..at].rfind('\n').map_or(0, |nl| nl + 1);
    Point::new(text[..at].matches('\n').count(), at - line_start)
}

/// Widens byte ranges to whole lines and merges the overlapping ones.
fn line_ranges(text: &str, mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    for range in &mut ranges {
        range.start = text[..range.start.min(text.len())]
            .rfind('\n')
            .map_or(0, |nl| nl + 1);
        let end = range.end.min(text.len());
        range.end = text[end..].find('\n').map_or(text.len(), |nl| end + nl + 1);
    }
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// Replaces the `dirty` ranges of sorted `tokens` with `fresh` ones.
fn splice(tokens: Vec<Token>, dirty: &[Range<usize>], fresh: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len() + fresh.len());
    let mut first = 0;
    for token in tokens {
        while first < dirty.len() && dirty[first].end <= token.from {
            first += 1;
        }
        let mut from = token.from;
        for range in dirty[first..]
            .iter()
            .take_while(|range| range.start < token.to)
        {
            if from < range.start {
                out.push(Token {
                    from,
                    to: range.start,
                    kind: token.kind.clone(),
                });
            }
            from = from.max(range.end);
        }
        if from < token.to {
            out.push(Token { from, ..token });
        }
    }
    out.extend(fresh);
    out.sort_by_key(|token| token.from);
    out
}

/// One open file's parser, last tree and tokens.
pub(crate) struct Session {
    path: String,
    lang: Lang,
    grammar: &'static Grammar,
    parser: Parser,
    tree: Option<Tree>,
    text: String,
    tokens: Vec<Token>,
    original: Option<(String, Vec<Token>)>,
}

impl Session {
    pub(crate) fn new(path: &str) -> Option<Self> {
        let lang = language_for_path(path)?;
        let grammar = grammar(lang)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        Some(Self {
            path: path.to_owned(),
            lang,
            grammar,
            parser,
            tree: None,
            text: String::new(),
            tokens: Vec::new(),
            original: None,
        })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    fn full_tokens(&self, tree: &Tree, text: &str) -> Vec<Token> {
        let mut tokens = highlight(self.grammar, tree, text, 0..text.len());
        if self.lang == Lang::Html {
            tokens.extend(injections(tree, text));
            tokens.sort_by_key(|token| token.from);
        }
        tokens
    }

    /// Reparses `text` against the previous tree and re-highlights only the
    /// lines whose text or structure changed.
    fn update(&mut self, text: &str) -> Option<&Tree> {
        if self.tree.is_some() && self.text == text {
            return self.tree.as_ref();
        }
        let edit = self
            .tree
            .is_some()
            .then(|| Edit::between(&self.text, text))
            .flatten();
        let previous = self.tree.take();
        let (tree, dirty) = match (previous, edit) {
            (Some(mut old), Some(edit)) => {
                old.edit(&InputEdit {
                    start_byte: edit.start,
                    old_end_byte: edit.old_end,
                    new_end_byte: edit.new_end,
                    start_position: point(text, edit.start),
                    old_end_position: point(&self.text, edit.old_end),
                    new_end_position: point(text, edit.new_end),
                });
                let tree = self.parser.parse(text, Some(&old))?;
                let mut dirty: Vec<_> = old
                    .changed_ranges(&tree)
                    .map(|range| range.start_byte..range.end_byte)
                    .collect();
                dirty.push(edit.start..edit.new_end);
                let tokens = std::mem::take(&mut self.tokens)
                    .into_iter()
                    .filter_map(|token| {
                        let (from, to) = edit.map(token.from, token.to)?;
                        Some(Token { from, to, ..token })
                    })
                    .collect();
                self.tokens = tokens;
                (tree, Some(line_ranges(text, dirty)))
            }
            _ => (self.parser.parse(text, None)?, None),
        };
        self.tokens = match dirty {
            Some(dirty) if self.lang != Lang::Html => {
                let fresh = dirty
                    .iter()
                    .flat_map(|range| highlight(self.grammar, &tree, text, range.clone()))
                    .collect();
                splice(std::mem::take(&mut self.tokens), &dirty, fresh)
            }
            _ => self.full_tokens(&tree, text),
        };
        self.text = text.to_owned();
        self.tree = Some(tree);
        self.tree.as_ref()
    }

    fn original_tokens(&mut self, original: &str) -> &[Token] {
        if self
            .original
            .as_ref()
            .is_none_or(|(text, _)| text != original)
        {
            let tokens = self
                .parser
                .parse(original, None)
                .map(|tree| self.full_tokens(&tree, original))
                .unwrap_or_default();
            self.original = Some((original.to_owned(), tokens));
        }
        &self.original.as_ref().unwrap().1
    }

    pub(crate) fn analyze(&mut self, text: &str, original: Option<&str>) -> Analysis {
        let Some(tree) = self.update(text).cloned() else {
            return Analysis::default();
        };
        let diagnostics = if lintable(&self.path) {
            diagnostics(&tree, text)
        } else {
            Vec::new()
        };
        let changes = match original.filter(|original| *original != text) {
            Some(original) => {
                let tokens = self.original_tokens(original).to_vec();
                changes(original, text, &tokens)
            }
            None => Vec::new(),
        };
        Analysis {
            diagnostics,
            changes,
            folds: folds(&tree, text),
            tokens: self.tokens.clone(),
        }
    }
}

/// Analyses `text` in the session held for a file, starting one for `path`
/// (or replacing a renamed file's) as needed.
pub(crate) fn analyze(
    session: &Mutex<Option<Session>>,
    path: &str,
    text: &str,
    original: Option<&str>,
) -> Result<Analysis, String> {
    let mut session = session.lock().map_err(|e| e.to_string())?;
    if session.as_ref().is_none_or(|s| s.path() != path) {
        *session = Session::new(path);
    }
    let session = session
        .as_mut()
        .ok_or_else(|| format!("no syntax grammar for {path}"))?;
    Ok(session.analyze(text, original))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [(Lang, &str); 16] = [
        (Lang::JavaScript, "a.jsx"),
        (Lang::TypeScript, "a.ts"),
        (Lang::Tsx, "a.tsx"),
        (Lang::Json, "a.json"),
        (Lang::Css, "a.css"),
        (Lang::Html, "a.html"),
        (Lang::Python, "a.py"),
        (Lang::Rust, "a.rs"),
        (Lang::Go, "a.go"),
        (Lang::Bash, "a.sh"),
        (Lang::C, "a.c"),
        (Lang::Cpp, "a.cpp"),
        (Lang::Java, "a.java"),
        (Lang::Yaml, "a.yaml"),
        (Lang::Toml, "a.toml"),
        (Lang::Ruby, "a.rb"),
    ];

    const TSX: &str = r#"import { useState } from "react";
// Counts clicks.
export function Counter({ start }: { start: number }) {
  const [count, setCount] = useState(start);
  const isEven = (n: unknown): n is number => typeof n === "number";
  try {
    load();
  } catch (error: unknown) {
    console.log(`failed ${error}`);
  }
  return <button onClick={() => setCount(count + 1)}>{count}</button>;
}
"#;

    fn kind_at<'a>(tokens: &'a [Token], text: &str, needle: &str) -> Option<&'a str> {
        let at = text.find(needle).unwrap();
        tokens
            .iter()
            .find(|t| t.from <= at && at < t.to)
            .map(|t| t.kind.as_ref())
    }

    /// Adjacent same-kind tokens are equivalent to one longer token.
    fn normalized(tokens: &[Token]) -> Vec<(usize, usize, String)> {
        let mut out: Vec<(usize, usize, String)> = Vec::new();
        for t in tokens {
            match out.last_mut() {
                Some(last) if last.1 == t.from && last.2 == t.kind.as_ref() => last.1 = t.to,
                _ => out.push((t.from, t.to, t.kind.to_string())),
            }
        }
        out
    }

    #[test]
    fn every_grammar_and_highlight_query_loads() {
        for (lang, path) in LANGS {
            assert_eq!(language_for_path(path), Some(lang));
            assert!(grammar(lang).is_some(), "{lang:?} query must compile");
        }
        assert!(
            !supports("a.swift"),
            "other languages keep the QuickJS path"
        );
    }

    #[test]
    fn tsx_tokens_use_the_editor_palette_groups() {
        let mut session = Session::new("a.tsx").unwrap();
        let analysis = session.analyze(TSX, None);
        let tokens = &analysis.tokens;
        assert_eq!(
            kind_at(tokens, TSX, "import"),
            Some("var(--syntax-keyword)")
        );
        assert_eq!(
            kind_at(tokens, TSX, "\"react\""),
            Some("var(--syntax-string)")
        );
        assert_eq!(
            kind_at(tokens, TSX, "// Counts"),
            Some("var(--syntax-comment)")
        );
        assert_eq!(
            kind_at(tokens, TSX, "useState(start)"),
            Some("var(--syntax-callable)")
        );
        assert_eq!(kind_at(tokens, TSX, "number }"), Some("var(--syntax-type)"));
        assert_eq!(kind_at(tokens, TSX, "button"), Some("var(--syntax-tag)"));
        assert_eq!(kind_at(tokens, TSX, "1)"), Some("var(--syntax-number)"));
        // Template substitutions are not string-coloured.
        assert_eq!(kind_at(tokens, TSX, "error}`"), None);
        assert!(
            tokens.windows(2).all(|w| w[0].to <= w[1].from),
            "sorted, disjoint"
        );
        // Type predicates and typed catch were Lezer grammar gaps; not errors here.
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn incremental_highlighting_matches_a_full_pass() {
        for (path, source) in [
            ("a.tsx", TSX),
            (
                "a.rs",
                "/// Doc.\nfn main() {\n    let s = \"hi\";\n    println!(\"{s}\");\n}\n",
            ),
            (
                "a.py",
                "def f(x):\n    return \"y\" + str(x)  # done\n\nclass A:\n    pass\n",
            ),
        ] {
            let mut session = Session::new(path).unwrap();
            let mut text = source.to_owned();
            session.analyze(&text, None);
            // A fixed pseudo-random walk of inserts and deletes, including
            // ones that open strings and comments over the rest of the file.
            let inserts = ["\"", "/*", "x", "{", "\n", "}", "*/", "'", "#", "( ", "// "];
            let mut seed = 0x2545_f491_u64;
            for step in 0..200 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let mut at = (seed as usize) % (text.len() + 1);
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                if step % 3 == 2 && at < text.len() {
                    let mut end = (at + 1 + (seed as usize >> 8) % 4).min(text.len());
                    while !text.is_char_boundary(end) {
                        end += 1;
                    }
                    text.replace_range(at..end, "");
                } else {
                    text.insert_str(at, inserts[(seed as usize >> 16) % inserts.len()]);
                }
                let incremental = session.analyze(&text, None).tokens;
                let full = Session::new(path).unwrap().analyze(&text, None).tokens;
                assert_eq!(
                    normalized(&incremental),
                    normalized(&full),
                    "{path} step {step}:\n{text}"
                );
            }
        }
    }

    #[test]
    fn html_highlights_its_script_and_style_bodies() {
        let html = "<div class=\"a\">\n<script>const x = 1;</script>\n<style>a { color: red; }</style>\n</div>\n";
        let tokens = Session::new("a.html").unwrap().analyze(html, None).tokens;
        assert_eq!(kind_at(&tokens, html, "div"), Some("var(--syntax-tag)"));
        assert_eq!(
            kind_at(&tokens, html, "class"),
            Some("var(--syntax-property)")
        );
        assert_eq!(
            kind_at(&tokens, html, "const"),
            Some("var(--syntax-keyword)")
        );
        assert_eq!(
            kind_at(&tokens, html, "color"),
            Some("var(--syntax-property)")
        );
    }

    #[test]
    fn folds_start_on_their_opening_line() {
        let js = "function f() {\n  return [\n    1,\n  ];\n}\n";
        let analysis = Session::new("a.js").unwrap().analyze(js, None);
        let ranges: Vec<_> = analysis.folds.iter().map(|f| &js[f.from..f.to]).collect();
        assert_eq!(ranges, ["\n  return [\n    1,\n  ];\n", "\n    1,\n  "]);

        let py = "def f():\n    a = 1\n    return a\nb = 2\n";
        let analysis = Session::new("a.py").unwrap().analyze(py, None);
        assert_eq!(analysis.folds.len(), 1);
        assert_eq!(
            &py[analysis.folds[0].from..analysis.folds[0].to],
            "\n    a = 1\n    return a"
        );
    }

    #[test]
    fn syntax_errors_are_reported_where_the_parse_fails() {
        let mut session = Session::new("a.ts").unwrap();
        let broken = "const a = {\n  b: 1,\n\nfunction g() {}\n";
        let diagnostics = session.analyze(broken, None).diagnostics;
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity == "error" && d.from < d.to)
        );
        // Fixing it clears them on the incremental pass.
        let fixed = "const a = {\n  b: 1,\n};\nfunction g() {}\n";
        assert!(session.analyze(fixed, None).diagnostics.is_empty());
        // Rust is highlighted but, as in editorLint.ts, not linted.
        assert!(
            Session::new("a.rs")
                .unwrap()
                .analyze("fn (", None)
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn git_changes_match_codemirror_line_chunks() {
        let original = "one\ntwo\nthree\nfour\n";
        let current = "one\nTWO\nthree\nfour\nfive\n";
        let changes = changes(original, current, &[]);
        let summary: Vec<_> = changes
            .iter()
            .map(|c| {
                (
                    c.kind.as_str(),
                    &current[c.from..c.to],
                    c.original.as_str(),
                    c.old_line,
                    c.deleted,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [("mod", "TWO", "two\n", 2, 1), ("add", "five", "", 5, 1)]
        );

        let deleted = changes_for("a\nb\nc\n", "a\nc\n");
        assert_eq!(
            (deleted.kind.as_str(), deleted.from, deleted.to),
            ("del", 2, 2)
        );
        assert_eq!((deleted.original.as_str(), deleted.deleted), ("b\n", 1));

        // A last line without a newline is covered to the end of the file.
        let tail = changes_for("a\nb", "a\nc");
        assert_eq!((tail.from, tail.to), (2, 3));
    }

    fn changes_for(original: &str, current: &str) -> Change {
        let mut all = changes(original, current, &[]);
        assert_eq!(all.len(), 1);
        all.remove(0)
    }

    #[test]
    fn changes_carry_the_baselines_tokens() {
        let original = "let a = \"x\";\n";
        let current = "let a = 1;\n";
        let mut session = Session::new("a.js").unwrap();
        let analysis = session.analyze(current, Some(original));
        let change = &analysis.changes[0];
        let string = change
            .tokens
            .iter()
            .find(|t| t.kind == "var(--syntax-string)")
            .unwrap();
        assert_eq!(&change.original[string.from..string.to], "\"x\"");
    }
}
