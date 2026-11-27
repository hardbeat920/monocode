//! The Tauri editor's CodeMirror/Lezer analysis, without a web view. The bundled
//! parsers and editorLint.ts run in a bounded interpreter on a background worker.
//! Languages with a Tree-sitter grammar are analysed natively by editor_syntax.
use gpui::SharedString;
use rquickjs::{Context, Function, Promise, Runtime};
use serde::Deserialize;
use std::{
    ops::Range,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub changes: Vec<Change>,
    pub folds: Vec<Fold>,
    pub tokens: Vec<Token>,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Token {
    pub from: usize,
    pub to: usize,
    pub kind: SharedString,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Diagnostic {
    pub from: usize,
    pub to: usize,
    pub severity: String,
    pub message: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Change {
    pub kind: String,
    pub from: usize,
    pub to: usize,
    pub deleted: usize,
    pub original: String,
    pub old_line: usize,
    pub tokens: Vec<Token>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Fold {
    pub from: usize,
    pub to: usize,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Completion {
    pub label: String,
    pub detail: String,
    pub kind: String,
    pub from: usize,
    pub to: usize,
    pub insert: String,
    pub cursor: usize,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SearchMatch {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub invalid: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Formatted {
    pub formatted: String,
    #[serde(rename = "cursorOffset")]
    pub cursor: usize,
}
fn byte(source: &str, offset: usize) -> usize {
    let mut units = 0;
    for (at, ch) in source.char_indices() {
        if units >= offset {
            return at;
        }
        units += ch.len_utf16();
    }
    source.len()
}
fn utf16(source: &str, at: usize) -> usize {
    source[..at].encode_utf16().count()
}
fn runtime() -> Result<(Runtime, Context), String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(256 * 1024 * 1024);
    runtime.set_max_stack_size(4 * 1024 * 1024);
    let deadline = Instant::now() + Duration::from_secs(3);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    Ok((runtime, context))
}
pub(crate) fn complete(
    path: &str,
    source: &str,
    at: usize,
    explicit: bool,
) -> Result<Vec<Completion>, String> {
    let (_runtime, context) = runtime()?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(include_str!("../assets/native-editor.js"))?;
            let function: Function = ctx.globals().get("monocodeComplete")?;
            function
                .call::<_, Promise>((path, source, utf16(source, at), explicit))?
                .finish()
        })
        .map_err(|e| e.to_string())?;
    let mut options: Vec<Completion> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    for option in &mut options {
        option.from = byte(source, option.from);
        option.to = byte(source, option.to);
        option.cursor = byte(&option.insert, option.cursor);
    }
    Ok(options)
}
pub(crate) fn search(
    source: &str,
    query: &str,
    replacement: &str,
    case: bool,
    word: bool,
    regex: bool,
) -> Result<SearchResult, String> {
    let (_runtime, context) = runtime()?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(include_str!("../assets/native-editor.js"))?;
            let function: Function = ctx.globals().get("monocodeSearch")?;
            function.call((source, query, replacement, case, word, regex))
        })
        .map_err(|e| e.to_string())?;
    let mut result: SearchResult = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    // Convert all positions in one pass, including large replace-all results.
    let offsets = byte_offsets(source);
    for m in &mut result.matches {
        m.from = offsets[m.from.min(offsets.len() - 1)];
        m.to = offsets[m.to.min(offsets.len() - 1)];
    }
    Ok(result)
}
pub(crate) fn chunk_action(
    original: &str,
    current: &str,
    at: usize,
    selection: Range<usize>,
    stage: bool,
) -> Result<Option<String>, String> {
    let (_runtime, context) = runtime()?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(include_str!("../assets/native-editor.js"))?;
            let function: Function = ctx.globals().get("monocodeChunkAction")?;
            function.call((
                original,
                current,
                utf16(current, at),
                utf16(current, selection.start),
                utf16(current, selection.end),
                stage,
            ))
        })
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}
pub(crate) fn format(path: &str, source: &str, at: usize) -> Result<Option<Formatted>, String> {
    transform(
        "monocodeFormat",
        path,
        source,
        at,
        include_str!("../assets/native-format.js"),
    )
}
pub(crate) fn emmet(path: &str, source: &str, at: usize) -> Result<Option<Formatted>, String> {
    transform(
        "monocodeEmmet",
        path,
        source,
        at,
        include_str!("../assets/native-editor.js"),
    )
}
fn transform(
    name: &str,
    path: &str,
    source: &str,
    at: usize,
    bundle: &str,
) -> Result<Option<Formatted>, String> {
    let (_runtime, context) = runtime()?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(bundle)?;
            let function: Function = ctx.globals().get(name)?;
            if name == "monocodeFormat" || name == "monocodeEmmet" {
                function
                    .call::<_, Promise>((path, source, utf16(source, at)))?
                    .finish()
            } else {
                function.call((path, source, utf16(source, at)))
            }
        })
        .map_err(|e| e.to_string())?;
    let mut result: Option<Formatted> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    if let Some(result) = &mut result {
        result.cursor = byte(&result.formatted, result.cursor);
    }
    Ok(result)
}
fn byte_offsets(source: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(source.len() + 1);
    for (at, ch) in source.char_indices() {
        offsets.extend(std::iter::repeat_n(at, ch.len_utf16()));
    }
    offsets.push(source.len());
    offsets
}
/// The one contiguous replacement between two texts, found by trimming their
/// common prefix and suffix. Typing, undo, reloads and IME all reduce to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}
impl Edit {
    pub(crate) fn between(old: &str, new: &str) -> Option<Self> {
        if old == new {
            return None;
        }
        let (a, b) = (old.as_bytes(), new.as_bytes());
        let mut start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        let room = a.len().min(b.len()) - start;
        let suffix = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(room)
            .take_while(|(x, y)| x == y)
            .count();
        // The shared bytes are identical, so both texts agree on boundaries.
        while !old.is_char_boundary(start) {
            start -= 1;
        }
        let (mut old_end, mut new_end) = (a.len() - suffix, b.len() - suffix);
        while !old.is_char_boundary(old_end) {
            old_end += 1;
            new_end += 1;
        }
        Some(Self {
            start,
            old_end,
            new_end,
        })
    }
    /// Where `from..to` lands after the edit. Ranges around the edit stretch
    /// over the new text, ranges cut by it are trimmed, and ranges inside the
    /// replaced text are gone.
    pub(crate) fn map(&self, from: usize, to: usize) -> Option<(usize, usize)> {
        let shift = |at: usize| at - self.old_end + self.new_end;
        if to <= self.start {
            return Some((from, to));
        }
        if from >= self.old_end {
            return Some((shift(from), shift(to)));
        }
        let from = if from <= self.start {
            from
        } else {
            self.new_end
        };
        let to = if to >= self.old_end {
            shift(to)
        } else {
            self.start
        };
        (from < to).then_some((from, to))
    }
}
impl Analysis {
    /// Last results moved through an edit, so the buffer keeps its colours,
    /// markers and gutter until fresh analysis arrives instead of flashing.
    pub(crate) fn remapped(&self, edit: Edit) -> Self {
        let tokens = self
            .tokens
            .iter()
            .filter_map(|t| {
                let (from, to) = edit.map(t.from, t.to)?;
                Some(Token {
                    from,
                    to,
                    kind: t.kind.clone(),
                })
            })
            .collect();
        let diagnostics = self
            .diagnostics
            .iter()
            .filter_map(|d| {
                let (from, to) = edit.map(d.from, d.to)?;
                Some(Diagnostic {
                    from,
                    to,
                    ..d.clone()
                })
            })
            .collect();
        let changes = self
            .changes
            .iter()
            .filter_map(|c| {
                let (from, to) = edit.map(c.from, c.to)?;
                Some(Change {
                    from,
                    to,
                    ..c.clone()
                })
            })
            .collect();
        let folds = self
            .folds
            .iter()
            .filter_map(|f| {
                let (from, to) = edit.map(f.from, f.to)?;
                Some(Fold { from, to })
            })
            .collect();
        Self {
            diagnostics,
            changes,
            folds,
            tokens,
        }
    }
}
impl Diagnostic {
    pub fn range(&self) -> Range<usize> {
        self.from..self.to
    }
    pub fn priority(&self) -> u8 {
        match self.severity.as_str() {
            "error" => 3,
            "warning" => 2,
            "info" => 1,
            _ => 0,
        }
    }
}

pub(crate) fn analyze(
    path: &str,
    source: &str,
    original: Option<&str>,
) -> Result<Analysis, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(2 * 1024 * 1024);
    // A headless worker must finish parsing the whole document. A fixed two
    // seconds interrupts large TSX files (and their diff baseline) before the
    // complete tree can be published. Keep small requests short and all work
    // bounded, while allowing about one second per 64 KiB of actual input.
    let bytes = source.len().saturating_add(
        original
            .filter(|original| *original != source)
            .map_or(0, str::len),
    );
    let budget = Duration::from_secs(bytes.div_ceil(64 * 1024).clamp(2, 30) as u64);
    let deadline = Instant::now() + budget;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(include_str!("../assets/native-editor.js"))?;
            let function: Function = ctx.globals().get("monocodeAnalyze")?;
            function
                .call::<_, Promise>((path, source, original))?
                .finish()
        })
        .map_err(|e| e.to_string())?;
    let mut analysis: Analysis = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    // CodeMirror offsets are UTF-16, while native selection/IME uses UTF-8.
    let offsets = byte_offsets(source);
    let byte = |at: usize| offsets[at.min(offsets.len() - 1)];
    for diagnostic in &mut analysis.diagnostics {
        diagnostic.from = byte(diagnostic.from);
        diagnostic.to = byte(diagnostic.to);
    }
    for change in &mut analysis.changes {
        change.from = byte(change.from);
        change.to = byte(change.to);
        let offsets = byte_offsets(&change.original);
        for token in &mut change.tokens {
            token.from = offsets[token.from.min(offsets.len() - 1)];
            token.to = offsets[token.to.min(offsets.len() - 1)];
        }
    }
    for token in &mut analysis.tokens {
        token.from = byte(token.from);
        token.to = byte(token.to);
    }
    for fold in &mut analysis.folds {
        fold.from = byte(fold.from);
        fold.to = byte(fold.to);
    }
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frontend_parsers_report_real_syntax_errors_and_keep_grammar_gap_filters() {
        for (path, source) in [
            ("a.tsx", "export const App = () => <div>hello</div>;"),
            (
                "a.ts",
                "const isKey = (x: unknown): x is [string, number] => true;",
            ),
            ("a.css", "@source '../src';"),
            ("a.rs", "fn main() { let Some(a) = None else { return }; }"),
        ] {
            assert!(
                analyze(path, source, None).unwrap().diagnostics.is_empty(),
                "{path}"
            );
        }
        for (path, source) in [
            ("a.json", "{\"x\": }"),
            ("a.js", "const x = ;"),
            ("a.py", "def broken(:\n pass"),
        ] {
            let errors = analyze(path, source, None).unwrap().diagnostics;
            assert!(!errors.is_empty(), "{path}");
            assert!(errors.iter().all(|d| d.from < d.to
                && source.is_char_boundary(d.from)
                && source.is_char_boundary(d.to)));
        }
    }
    #[test]
    fn frontend_chunks_and_diagnostics_keep_unicode_byte_positions() {
        let source = "const emoji = '🙂';\nconst x = ;\n";
        let result = analyze("a.js", source, Some("const emoji = '🙂';\nconst x = 1;\n")).unwrap();
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].kind, "mod");
        assert_eq!(result.changes[0].from, source.find("const x").unwrap());
        assert!(result.diagnostics[0].from >= source.find("const x").unwrap());
        assert!(source.is_char_boundary(result.diagnostics[0].to));
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    #[test]
    fn search_and_replace_share_codemirror_unicode_captures_and_word_boundaries() {
        let source = "🙂 café CAFE caféine\nCAFÉ";
        let found = search(source, "café", "tea", false, true, false).unwrap();
        assert_eq!(found.matches.len(), 2);
        assert_eq!(&source[found.matches[0].from..found.matches[0].to], "café");
        assert_eq!(&source[found.matches[1].from..found.matches[1].to], "CAFÉ");
        let found = search(source, "(caf)(é)", "$2$1", false, true, true).unwrap();
        assert_eq!(found.matches[0].insert, "écaf");
        assert!(search(source, "[", "", false, false, true).unwrap().invalid);
        assert!(
            search(source, "^", "x", false, false, true)
                .unwrap()
                .matches
                .is_empty()
        );
        assert_eq!(
            search("a\\nb", "a\\nb", "$1", true, false, false)
                .unwrap()
                .matches[0]
                .insert,
            "$1"
        );
    }
    #[test]
    fn prettier_and_emmet_keep_cursor_bytes_and_the_frontend_defaults() {
        let source = "const emoji='🙂';const value={x:1}";
        let result = format("a.ts", source, source.find("value").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            result.formatted,
            "const emoji = \"🙂\";\nconst value = { x: 1 };\n"
        );
        assert!(result.formatted[result.cursor..].starts_with("value"));
        for (path, source) in [
            ("a.json", "{\"x\":1}"),
            ("a.css", "a{color:red}"),
            ("a.html", "<div><p>hello</p></div>"),
            ("a.md", "# heading\n\nhello   world"),
        ] {
            assert!(format(path, source, 0).unwrap().is_some(), "{path}");
        }
        assert!(format("a.json", "{bad", 0).unwrap().is_none());
        assert!(format("a.rs", "fn main(){}", 0).unwrap().is_none());
        let result = emmet("a.tsx", ".container>span", 15).unwrap().unwrap();
        assert!(result.formatted.contains("className=\"container\""));
        assert!(result.formatted[result.cursor..].starts_with("</span>"));
    }
    #[test]
    fn completions_snippets_and_folds_use_the_frontend_languages() {
        let options = complete("a.ts", "// 🙂\ncons", "// 🙂\ncons".len(), true).unwrap();
        let keyword = options.iter().find(|o| o.label == "const").unwrap();
        assert_eq!(keyword.from, "// 🙂\n".len());
        assert!(options.iter().all(|o| o.insert.is_char_boundary(o.cursor)));
        let source = "fn main() {\n    let emoji = \"🙂\";\n}\n";
        let analysis = analyze("a.rs", source, None).unwrap();
        assert!(!analysis.folds.is_empty());
        assert!(
            analysis
                .folds
                .iter()
                .all(|f| source.is_char_boundary(f.from) && source.is_char_boundary(f.to))
        );
        assert!(
            !analyze("a.md", "# Heading\n\nparagraph\n\n## Next\ntext", None)
                .unwrap()
                .folds
                .is_empty()
        );
    }
    #[test]
    fn tokens_and_folds_cover_files_past_the_initial_parse_viewport() {
        let source = "function f() {\n  return 1;\n}\n".repeat(200);
        let analysis = analyze("a.ts", &source, None).unwrap();
        let last = analysis.tokens.iter().map(|t| t.to).max().unwrap();
        assert!(
            last > source.len() - 20,
            "tokens stop at {last} of {}",
            source.len()
        );
        assert_eq!(analysis.folds.len(), 200);
    }
    #[test]
    fn large_tsx_analysis_highlights_the_end_of_the_current_and_original_file() {
        let source = include_str!("../../../src/app/App.tsx");
        let original = format!("{source}\nexport const previousHighlightTail = \"tail\";\n");
        let analysis = analyze("App.tsx", source, Some(&original)).unwrap();
        let last = analysis
            .tokens
            .iter()
            .map(|token| token.to)
            .max()
            .unwrap_or(0);
        assert!(
            last > source.len() - 1_024,
            "highlighting stops at {last} of {} bytes",
            source.len()
        );
        let tail = analysis
            .changes
            .iter()
            .find(|change| change.original.contains("previousHighlightTail"))
            .unwrap();
        assert!(
            tail.tokens
                .iter()
                .any(|token| token.kind == "var(--syntax-keyword)")
        );
        assert!(
            tail.tokens
                .iter()
                .any(|token| token.kind == "var(--syntax-string)")
        );
        assert!(
            analysis
                .tokens
                .iter()
                .all(|token| source.is_char_boundary(token.from)
                    && source.is_char_boundary(token.to))
        );
        assert!(
            analysis
                .folds
                .iter()
                .any(|fold| fold.to > source.len() - 1_024)
        );
    }
    #[test]
    fn hunk_actions_preserve_eof_newlines_and_selected_line_scope() {
        for (original, current) in [
            ("a\nb\n", "a\nx\nb\n"),
            ("a", "a\nx"),
            ("🙂\nb\n", "🙂\nc\n"),
            ("old\n", ""),
            ("", "new"),
        ] {
            let changes = analyze("a.txt", current, Some(original)).unwrap().changes;
            let at = changes[0].from;
            assert_eq!(
                chunk_action(original, current, at, at..at, false)
                    .unwrap()
                    .as_deref(),
                Some(original)
            );
            assert_eq!(
                chunk_action(original, current, at, at..at, true)
                    .unwrap()
                    .as_deref(),
                Some(current)
            );
        }
        let original = "one\ntwo\nthree\n";
        let current = "ONE\nTWO\nthree\n";
        assert_eq!(
            chunk_action(original, current, 0, 0..3, true)
                .unwrap()
                .as_deref(),
            Some("ONE\ntwo\nthree\n")
        );
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn edits_trim_shared_text_on_character_boundaries() {
        assert_eq!(Edit::between("abc", "abc"), None);
        assert_eq!(
            Edit::between("let a;", "let ab;"),
            Some(Edit {
                start: 5,
                old_end: 5,
                new_end: 6
            })
        );
        // é and è share their first UTF-8 byte; the edit covers whole chars.
        assert_eq!(
            Edit::between("aéb", "aèb"),
            Some(Edit {
                start: 1,
                old_end: 3,
                new_end: 3
            })
        );
    }

    #[test]
    fn remapping_keeps_colours_through_an_edit() {
        let token = |from, to| Token {
            from,
            to,
            kind: "var(--syntax-string)".into(),
        };
        let analysis = Analysis {
            tokens: vec![token(0, 3), token(4, 9), token(10, 12)],
            ..Default::default()
        };
        // Typing inside the middle token stretches it; later ones shift.
        let typed = Edit::between("let \"abc\" x;", "let \"abXc\" x;").unwrap();
        let spans: Vec<_> = analysis
            .remapped(typed)
            .tokens
            .iter()
            .map(|t| (t.from, t.to))
            .collect();
        assert_eq!(spans, [(0, 3), (4, 10), (11, 13)]);
        // Deleting a token's whole text drops it.
        let gone = Edit {
            start: 4,
            old_end: 9,
            new_end: 4,
        };
        assert_eq!(analysis.remapped(gone).tokens.len(), 2);
    }
}
