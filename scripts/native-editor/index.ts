// The React editor's parsers, completions, folding and search run without a DOM
// in a bounded QuickJS worker. All offsets at this boundary are UTF-16.
import { EditorState, Text } from "@codemirror/state";
import { ensureSyntaxTree, foldable } from "@codemirror/language";
import { CompletionContext, completeFromList } from "@codemirror/autocomplete";
import { SearchQuery } from "@codemirror/search";
import { highlightTree } from "@lezer/highlight";
import { syntaxTree } from "@codemirror/language";
import { languageForPath, syntaxTagHighlighter } from "../../src/features/files/editor/editorLanguage";
import { Chunk } from "@codemirror/merge";
import { isLintable, syntaxDiagnostics } from "../../src/features/files/editor/editorLint";
import { stageChunkText, revertChunkText } from "../../src/features/files/editor/editorGit";
import { editorTyping, tryExpandEmmet } from "../../src/features/files/editor/editorEditing";

async function stateFor(path: string, source: string, pos = 0, fullParse = false) {
  const language = await languageForPath(path) ?? [];
  const state = EditorState.create({ doc: source, selection: { anchor: pos }, extensions: [language, editorTyping(path)] });
  return parsedState(state, fullParse);
}

function parsedState(state: EditorState, fullParse: boolean) {
  // Analysis has no EditorView/idle worker to finish a partial viewport tree.
  // Finish it here, on the native background worker, whose interpreter enforces
  // the overall time and memory limits. Refresh the state's tree afterwards.
  ensureSyntaxTree(state, state.doc.length, fullParse ? Infinity : 100);
  return state.update({}).state;
}

function originalState(state: EditorState, source: string, original: string) {
  // Reuse the parsed current document for the diff baseline. Rebuilding both
  // trees from scratch doubles large-file work even for a one-line change.
  let from = 0, to = source.length, end = original.length;
  while (from < to && from < end && source.charCodeAt(from) === original.charCodeAt(from)) from++;
  while (to > from && end > from && source.charCodeAt(to - 1) === original.charCodeAt(end - 1)) { to--; end--; }
  return parsedState(state.update({ changes: { from, to, insert: original.slice(from, end) } }).state, true);
}

globalThis.monocodeAnalyze = async (path: string, source: string, original: string | null) => {
  const state = await stateFor(path, source, 0, true);
  const tokensFor = state => {
    const tokens = [];
    highlightTree(syntaxTree(state), syntaxTagHighlighter, (from, to, kind) => tokens.push({ from, to, kind }));
    return tokens;
  };
  const hasDiff = original != null && original !== source;
  const originalTokens = hasDiff ? tokensFor(originalState(state, source, original)) : [];
  const changes = !hasDiff ? [] : Chunk.build(Text.of(original.split("\n")), state.doc, { scanLimit: 5_000, timeout: 100 }).map(chunk => ({
    kind: chunk.fromA !== chunk.toA ? chunk.fromB !== chunk.toB ? "mod" : "del" : "add",
    from: Math.min(chunk.fromB, source.length),
    to: Math.min(chunk.endB, source.length),
    original: original.slice(chunk.fromA, chunk.toA),
    old_line: original.slice(0, chunk.fromA).split("\n").length,
    tokens: originalTokens.filter(t => t.to > chunk.fromA && t.from < chunk.toA).map(t => ({ from: Math.max(t.from, chunk.fromA) - chunk.fromA, to: Math.min(t.to, chunk.toA) - chunk.fromA, kind: t.kind })),
    deleted: original.slice(chunk.fromA, chunk.toA).replace(/\n$/, "").split("\n").length,
  }));
  const folds = [];
  for (let n = 1; n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    const range = foldable(state, line.from, line.to);
    if (range && state.doc.lineAt(range.to).number > n) folds.push(range);
  }
  const tokens = tokensFor(state);
  return JSON.stringify({ diagnostics: isLintable(path) ? syntaxDiagnostics(state) : [], changes, folds, tokens });
};

globalThis.monocodeComplete = async (path: string, source: string, pos: number, explicit: boolean) => {
  const state = await stateFor(path, source, pos);
  const context = new CompletionContext(state, pos, explicit);
  const results = await Promise.all(state.languageDataAt<any>("autocomplete", pos).map(data => (Array.isArray(data) ? completeFromList(data) : data)(context)));
  const options = [];
  for (const result of results) {
    if (!result) continue;
    const prefix = source.slice(result.from, pos).toLowerCase();
    for (const option of result.options) {
      if (!option.label.toLowerCase().startsWith(prefix)) continue;
      let insert = typeof option.apply === "string" ? option.apply : option.label;
      let cursor = insert.length;
      // CodeMirror snippet applications only need state and dispatch to expand.
      if (typeof option.apply === "function") {
        try {
          option.apply({ state, dispatch: spec => {
            const tr = state.update(spec);
            insert = tr.newDoc.sliceString(result.from, tr.newDoc.length - (source.length - (result.to ?? pos)));
            cursor = tr.newSelection.main.head - result.from;
          } }, option, result.from, result.to ?? pos);
        } catch { /* ordinary labels remain usable for view-dependent sources */ }
      }
      options.push({ label: option.label, detail: option.detail ?? "", kind: option.type ?? "", from: result.from, to: result.to ?? pos, insert, cursor });
    }
  }
  return JSON.stringify(options.sort((a, b) => a.label.localeCompare(b.label)).slice(0, 100));
};

globalThis.monocodeEmmet = async (path: string, source: string, pos: number) => {
  const state = await stateFor(path, source, pos);
  let result = null;
  tryExpandEmmet({ state, dispatch: spec => {
    const tr = state.update(spec);
    result = { formatted: tr.newDoc.toString(), cursorOffset: tr.newSelection.main.head };
  } } as any);
  return JSON.stringify(result);
};

globalThis.monocodeSearch = (source: string, query: string, replacement: string, caseSensitive: boolean, wholeWord: boolean, regexp: boolean) => {
  const state = EditorState.create({ doc: source });
  const search = new SearchQuery({ search: query, replace: replacement, caseSensitive, wholeWord, regexp, literal: true });
  if (!search.valid) return JSON.stringify({ matches: [], invalid: !!query });
  const cursor = search.getCursor(state);
  const matches = [];
  while (!cursor.next().done) {
    const { from, to } = cursor.value;
    if (to > from) matches.push({ from, to, insert: (search as any).create().getReplacement(cursor.value) });
  }
  return JSON.stringify({ matches, invalid: false });
};

globalThis.monocodeChunkAction = (original: string, current: string, pos: number, start: number, end: number, stage: boolean) => JSON.stringify(
  (stage ? stageChunkText : revertChunkText)(original, current, pos, start === end ? null : { from: start, to: end })
);
