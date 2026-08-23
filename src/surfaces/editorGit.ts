import { Chunk } from "@codemirror/merge";
import {
  EditorState,
  Facet,
  RangeSet,
  RangeSetBuilder,
  StateEffect,
  StateField,
  Text,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  GutterMarker,
  ViewPlugin,
  WidgetType,
  gutter,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

const DIFF_CONFIG = { scanLimit: 5_000, timeout: 100 };

const setOriginalEffect = StateEffect.define<string | null>();

export type GitTextRange = { from: number; to: number };

type ChunkRange = {
  fromA: number;
  toA: number;
  fromB: number;
  toB: number;
};

type GitStageHandler = (contents: string) => Promise<void> | void;

const gitStageFacet = Facet.define<GitStageHandler, GitStageHandler | null>({
  combine: (values) => values[0] ?? null,
});

const insertedLine = Decoration.line({ class: "cm-gitInsertedLine" });
const LINE_HEIGHT = Math.round(13 * 1.6);

const addMarker = new (class extends GutterMarker {
  eq() {
    return true;
  }
  toDOM() {
    const el = document.createElement("div");
    el.className = "cm-gitMarker cm-gitAdd";
    return el;
  }
})();

const originalField = StateField.define<Text | null>({
  create() {
    return null;
  },
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setOriginalEffect)) {
        return effect.value == null ? null : textFromString(effect.value);
      }
    }
    return value;
  },
});

const chunksField = StateField.define<readonly Chunk[]>({
  create(state) {
    return chunksFor(state.field(originalField), state.doc);
  },
  update(chunks, tr) {
    const original = tr.state.field(originalField);
    if (tr.effects.some((effect) => effect.is(setOriginalEffect))) {
      return chunksFor(original, tr.state.doc);
    }
    if (!original) return [];
    if (tr.docChanged) {
      return Chunk.updateB(
        chunks,
        original,
        tr.state.doc,
        tr.changes,
        DIFF_CONFIG,
      );
    }
    return chunks;
  },
});

type GitDecorations = {
  lines: DecorationSet;
  gutter: RangeSet<GutterMarker>;
};

const gitDecorations = StateField.define<GitDecorations>({
  create(state) {
    return buildDecorations(state);
  },
  update(value, tr) {
    if (
      tr.docChanged ||
      tr.state.field(chunksField) !== tr.startState.field(chunksField)
    ) {
      return buildDecorations(tr.state);
    }
    return value;
  },
  provide: (field) =>
    EditorView.decorations.from(field, (value) => value.lines),
});

class DeletedLinesWidget extends WidgetType {
  constructor(
    readonly lines: readonly string[],
    readonly pos: number,
  ) {
    super();
  }

  eq(other: DeletedLinesWidget) {
    return (
      this.pos === other.pos &&
      this.lines.length === other.lines.length &&
      this.lines.every((line, i) => line === other.lines[i])
    );
  }

  toDOM() {
    const wrap = document.createElement("div");
    wrap.className = "cm-gitDeletedChunk";
    wrap.setAttribute("aria-hidden", "true");
    for (const text of this.lines) {
      const line = wrap.appendChild(document.createElement("div"));
      line.className = "cm-gitDeletedLine";
      line.textContent = text || "\u00a0";
    }
    return wrap;
  }

  get estimatedHeight() {
    return this.lines.length * LINE_HEIGHT;
  }

  ignoreEvent() {
    return true;
  }
}

export function editorGit(options?: { onStage?: GitStageHandler }): Extension {
  return [
    options?.onStage ? gitStageFacet.of(options.onStage) : [],
    originalField,
    chunksField,
    gitDecorations,
    gitGutter,
    gitHunkActions,
    gitOverview,
    gitTheme,
  ];
}

export function diffNavigablePositions(view: EditorView): number[] {
  const original = view.state.field(originalField);
  if (!original) return [];
  return navigableChunkPositions(
    view.state.doc,
    view.state.field(chunksField),
    original,
  );
}

export function diffActiveChunkIndex(
  view: EditorView,
  positions: number[],
): number {
  return activeChunkIndex(view, positions);
}

export function diffScrollToChunk(view: EditorView, pos: number): void {
  view.dispatch({
    effects: EditorView.scrollIntoView(pos, { y: "start", yMargin: 48 }),
    selection: { anchor: pos },
  });
  view.focus();
}

export function diffNavUpdateRelevant(update: ViewUpdate): boolean {
  return (
    update.docChanged ||
    update.geometryChanged ||
    update.state.field(chunksField) !== update.startState.field(chunksField)
  );
}

export function diffLineStats(
  doc: Text,
  chunks: readonly Chunk[],
  original: Text | null,
): { additions: number; deletions: number } {
  if (!original || chunks.length === 0) {
    return { additions: 0, deletions: 0 };
  }
  let additions = 0;
  let deletions = 0;
  for (const chunk of chunks) {
    const insertion = chunk.fromB !== chunk.toB;
    const deletion = chunk.fromA !== chunk.toA;
    if (deletion) {
      deletions += deletedLineTexts(original, chunk).length;
    }
    if (insertion) {
      const pos = Math.min(Math.max(0, chunk.fromB), doc.length);
      const startLine = doc.lineAt(pos).number;
      const endPos = Math.max(pos, Math.min(chunk.endB, doc.length) - 1);
      additions += doc.lineAt(endPos).number - startLine + 1;
    }
  }
  return { additions, deletions };
}

export function diffLineStatsForView(view: EditorView): {
  additions: number;
  deletions: number;
} {
  return diffLineStats(
    view.state.doc,
    view.state.field(chunksField),
    view.state.field(originalField),
  );
}

export function setGitOriginal(view: EditorView, original: string | null) {
  const current = view.state.field(originalField);
  const next = original == null ? null : textFromString(original);
  if (sameText(current, next)) return;
  view.dispatch({ effects: setOriginalEffect.of(original) });
}

/** Apply git original (index) text to an editor state. Used by the view and by tests. */
export function stateWithGitOriginal(
  doc: string,
  original: string | null,
): EditorState {
  return EditorState.create({ doc, extensions: editorGit() }).update({
    effects: setOriginalEffect.of(original),
  }).state;
}

export function revertChunkAt(view: EditorView, pos: number): boolean {
  const original = view.state.field(originalField);
  if (!original) return false;
  const changes = revertChunkChanges(
    original,
    view.state.doc,
    pos,
    actionRange(view),
    view.state.lineBreak,
  );
  if (!changes) return false;
  view.dispatch({ changes, userEvent: "revert" });
  return true;
}

export function revertChunkText(
  original: string,
  current: string,
  pos: number,
  selection?: GitTextRange | null,
): string | null {
  const orig = textFromString(original);
  const doc = textFromString(current);
  const changes = revertChunkChanges(orig, doc, pos, selection, "\n");
  if (!changes) return null;
  return doc.replace(changes.from, changes.to, changes.insert).toString();
}

export function stageChunkText(
  original: string,
  current: string,
  pos: number,
  selection?: GitTextRange | null,
): string | null {
  const orig = textFromString(original);
  const doc = textFromString(current);
  const changes = stageChunkChanges(orig, doc, pos, selection, "\n");
  if (!changes) return null;
  return orig.replace(changes.from, changes.to, changes.insert).toString();
}

export async function stageChunkAt(
  view: EditorView,
  pos: number,
): Promise<boolean> {
  const original = view.state.field(originalField);
  const onStage = view.state.facet(gitStageFacet);
  if (!original || !onStage) return false;
  const contents = stageChunkText(
    original.toString(),
    view.state.doc.toString(),
    pos,
    actionRange(view),
  );
  if (contents == null) return false;
  await onStage(contents);
  if (view.state.field(originalField)?.toString() !== contents) {
    setGitOriginal(view, contents);
  }
  return true;
}

export function findChunk(
  doc: Text,
  chunks: readonly Chunk[],
  pos: number,
): Chunk | undefined {
  const at = Math.max(0, Math.min(pos, doc.length));
  const covering = chunks.find(
    (chunk) => chunk.fromB <= at && chunk.endB >= at,
  );
  if (covering) return covering;
  if (doc.length === 0) return chunks[0];
  const line = doc.lineAt(at);
  return chunks.find((chunk) => {
    if (chunk.fromB !== chunk.toB) return false;
    return chunk.fromB >= line.from && chunk.fromB <= line.to + 1;
  });
}

function chunksFor(original: Text | null, current: Text): readonly Chunk[] {
  if (!original) return [];
  return Chunk.build(original, current, DIFF_CONFIG);
}

function revertChunkChanges(
  original: Text,
  doc: Text,
  pos: number,
  selection: GitTextRange | null | undefined,
  lineBreak: string,
): { from: number; to: number; insert: Text } | null {
  const range = actionChunkRange(original, doc, pos, selection);
  if (!range) return null;
  return applySide(
    original,
    doc,
    range.fromA,
    range.toA,
    range.fromB,
    range.toB,
    lineBreak,
  );
}

function stageChunkChanges(
  original: Text,
  doc: Text,
  pos: number,
  selection: GitTextRange | null | undefined,
  lineBreak: string,
): { from: number; to: number; insert: Text } | null {
  const range = actionChunkRange(original, doc, pos, selection);
  if (!range) return null;
  return applySide(
    doc,
    original,
    range.fromB,
    range.toB,
    range.fromA,
    range.toA,
    lineBreak,
  );
}

function applySide(
  source: Text,
  target: Text,
  fromS: number,
  toS: number,
  fromT: number,
  toT: number,
  lineBreak: string,
): { from: number; to: number; insert: Text } {
  let insert = source.sliceString(fromS, Math.max(fromS, toS - 1));
  if (fromS !== toS && toT <= target.length) {
    insert += lineBreak;
  }
  return {
    from: fromT,
    to: Math.min(target.length, toT),
    insert: textFromString(insert),
  };
}

function actionRange(view: EditorView): GitTextRange | null {
  const selection = view.state.selection.main;
  if (selection.empty) return null;
  return { from: selection.from, to: selection.to };
}

function actionChunkRange(
  original: Text,
  doc: Text,
  pos: number,
  selection: GitTextRange | null | undefined,
): ChunkRange | null {
  const chunk = findChunk(doc, chunksFor(original, doc), pos);
  if (!chunk) return null;
  return narrowChunk(original, doc, chunk, selection);
}

function narrowChunk(
  original: Text,
  doc: Text,
  chunk: Chunk,
  selection: GitTextRange | null | undefined,
): ChunkRange {
  const whole = {
    fromA: chunk.fromA,
    toA: chunk.toA,
    fromB: chunk.fromB,
    toB: chunk.toB,
  };
  const selected = selectedLines(doc, selection);
  const hunkB = hunkLines(doc, chunk.fromB, chunk.toB, chunk.endB);
  if (!selected || !hunkB) return whole;

  const fromLine = Math.max(selected.fromLine, hunkB.fromLine);
  const toLine = Math.min(selected.toLine, hunkB.toLine);
  if (fromLine > toLine) return whole;
  if (fromLine === hunkB.fromLine && toLine === hunkB.toLine) return whole;

  const nextB = offsetsForLines(doc, fromLine, toLine);
  const hunkA = hunkLines(original, chunk.fromA, chunk.toA, chunk.endA);
  if (!hunkA) {
    return {
      fromA: chunk.fromA,
      toA: chunk.toA,
      fromB: nextB.from,
      toB: nextB.to,
    };
  }

  const aCount = hunkA.toLine - hunkA.fromLine + 1;
  const bCount = hunkB.toLine - hunkB.fromLine + 1;
  if (aCount === bCount) {
    const delta = fromLine - hunkB.fromLine;
    const length = toLine - fromLine;
    const aFromLine = hunkA.fromLine + delta;
    const aToLine = aFromLine + length;
    const nextA = offsetsForLines(original, aFromLine, aToLine);
    return {
      fromA: nextA.from,
      toA: nextA.to,
      fromB: nextB.from,
      toB: nextB.to,
    };
  }

  return {
    fromA: chunk.fromA,
    toA: chunk.toA,
    fromB: nextB.from,
    toB: nextB.to,
  };
}

function selectedLines(
  doc: Text,
  range: GitTextRange | null | undefined,
): { fromLine: number; toLine: number } | null {
  if (!range || range.from === range.to) return null;
  const from = Math.min(range.from, range.to);
  const to = Math.max(range.from, range.to);
  const start = doc.lineAt(Math.min(from, doc.length)).number;
  let end = doc.lineAt(Math.min(to, doc.length)).number;
  if (to > from && doc.lineAt(Math.min(to, doc.length)).from === to) {
    end = Math.max(start, end - 1);
  }
  return { fromLine: start, toLine: end };
}

function hunkLines(
  doc: Text,
  from: number,
  to: number,
  end: number,
): { fromLine: number; toLine: number } | null {
  if (from === to) return null;
  if (doc.length === 0) return { fromLine: 1, toLine: 1 };
  const start = Math.min(from, doc.length);
  const last = Math.max(start, Math.min(end, doc.length) - 1);
  return {
    fromLine: doc.lineAt(start).number,
    toLine: doc.lineAt(last).number,
  };
}

function offsetsForLines(
  doc: Text,
  fromLine: number,
  toLine: number,
): { from: number; to: number } {
  const from = doc.line(fromLine).from;
  const last = doc.line(toLine);
  return { from, to: last.to + 1 };
}

export function deletedLineTexts(original: Text, chunk: Chunk): string[] {
  if (chunk.fromA === chunk.toA) return [];
  return original
    .sliceString(chunk.fromA, Math.max(chunk.fromA, chunk.toA - 1))
    .split("\n");
}

export type OverviewTick = {
  kind: "add" | "del" | "mod";
  top: number;
  size: number;
  pos: number;
};

export function navigableChunkPositions(
  doc: Text,
  chunks: readonly Chunk[],
  original: Text | null,
): number[] {
  return overviewTicks(doc, chunks, original).map((tick) => tick.pos);
}

export function overviewTicks(
  doc: Text,
  chunks: readonly Chunk[],
  original: Text | null,
): OverviewTick[] {
  const total = Math.max(1, doc.lines);
  const ticks: OverviewTick[] = [];
  for (const chunk of chunks) {
    const insertion = chunk.fromB !== chunk.toB;
    const deletion = chunk.fromA !== chunk.toA;
    if (!insertion && !deletion) continue;
    const pos = Math.min(Math.max(0, chunk.fromB), doc.length);
    const startLine = doc.lineAt(pos).number;
    let lineCount = 1;
    if (insertion) {
      const endPos = Math.max(pos, Math.min(chunk.endB, doc.length) - 1);
      lineCount = Math.max(1, doc.lineAt(endPos).number - startLine + 1);
    } else if (original) {
      lineCount = Math.max(1, deletedLineTexts(original, chunk).length);
    }
    ticks.push({
      kind: insertion && deletion ? "mod" : deletion ? "del" : "add",
      top: (startLine - 1) / total,
      size: lineCount / total,
      pos,
    });
  }
  return ticks;
}

function buildDecorations(state: EditorState): GitDecorations {
  const original = state.field(originalField);
  const chunks = state.field(chunksField);
  if (!original || chunks.length === 0) {
    return { lines: Decoration.none, gutter: RangeSet.empty };
  }

  const lineItems: { from: number; deco: Decoration }[] = [];
  const markItems: { from: number }[] = [];

  for (const chunk of chunks) {
    const insertion = chunk.fromB !== chunk.toB;
    const widgetAt = widgetPos(state.doc, chunk);
    const deleted =
      chunk.fromA !== chunk.toA ? deletedLineTexts(original, chunk) : [];
    if (deleted.length > 0) {
      lineItems.push({
        from: widgetAt,
        deco: Decoration.widget({
          widget: new DeletedLinesWidget(deleted, widgetAt),
          block: true,
          side: -1,
        }),
      });
    }

    if (!insertion) continue;

    const start = Math.min(chunk.fromB, state.doc.length);
    const last = Math.max(start, Math.min(chunk.endB, state.doc.length) - 1);
    let line = state.doc.lineAt(start);
    while (line.from <= last) {
      lineItems.push({ from: line.from, deco: insertedLine });
      markItems.push({ from: line.from });
      if (line.number >= state.doc.lines) break;
      line = state.doc.line(line.number + 1);
    }
  }

  lineItems.sort(
    (a, b) => a.from - b.from || a.deco.startSide - b.deco.startSide,
  );
  markItems.sort((a, b) => a.from - b.from);

  const lines = new RangeSetBuilder<Decoration>();
  const marks = new RangeSetBuilder<GutterMarker>();
  for (const item of lineItems) lines.add(item.from, item.from, item.deco);
  for (const item of markItems) marks.add(item.from, item.from, addMarker);

  return { lines: lines.finish(), gutter: marks.finish() };
}

function widgetPos(doc: Text, chunk: Chunk): number {
  if (doc.length === 0) return 0;
  return Math.min(chunk.fromB, doc.length);
}

function textFromString(value: string): Text {
  return Text.of(value.split("\n"));
}

function sameText(a: Text | null, b: Text | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.eq(b);
}

const gitGutter = gutter({
  class: "cm-gitGutter",
  markers: (view) => view.state.field(gitDecorations).gutter,
});

function activeChunkIndex(view: EditorView, positions: number[]): number {
  if (positions.length === 0) return -1;
  const scrollTop = view.scrollDOM.scrollTop;
  const centerY = scrollTop + view.scrollDOM.clientHeight * 0.35;
  const block = view.lineBlockAtHeight(centerY);
  const pos = block.from;
  let index = 0;
  for (let i = 0; i < positions.length; i++) {
    if (positions[i] <= pos) index = i;
    else break;
  }
  return index;
}

const gitOverview = ViewPlugin.fromClass(
  class {
    readonly dom: HTMLDivElement;

    constructor(readonly view: EditorView) {
      this.dom = document.createElement("div");
      this.dom.className = "cm-gitOverview";
      this.dom.title = "Changes";
      this.dom.addEventListener("mousedown", (event) => {
        this.onMouseDown(event);
      });
      view.dom.appendChild(this.dom);
      this.draw();
    }

    update(update: ViewUpdate) {
      if (
        update.docChanged ||
        update.geometryChanged ||
        update.state.field(chunksField) !== update.startState.field(chunksField)
      ) {
        this.draw();
      }
    }

    destroy() {
      this.dom.remove();
    }

    onMouseDown(event: MouseEvent) {
      if (event.button !== 0) return;
      event.preventDefault();
      this.jump(event);
      const move = (next: MouseEvent) => this.jump(next);
      const stop = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", stop);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", stop);
    }

    jump(event: MouseEvent) {
      const rect = this.dom.getBoundingClientRect();
      if (rect.height <= 0) return;
      const ratio = Math.min(
        1,
        Math.max(0, (event.clientY - rect.top) / rect.height),
      );
      const doc = this.view.state.doc;
      const lineNumber = Math.min(
        doc.lines,
        Math.max(1, Math.floor(ratio * doc.lines) + 1),
      );
      const pos = doc.line(lineNumber).from;
      this.view.dispatch({
        effects: EditorView.scrollIntoView(pos, { y: "center" }),
      });
    }

    draw() {
      const { state } = this.view;
      const chunks = state.field(chunksField);
      const original = state.field(originalField);
      this.dom.replaceChildren();
      if (chunks.length === 0) {
        this.dom.hidden = true;
        return;
      }
      this.dom.hidden = false;
      const height = Math.max(
        1,
        this.dom.clientHeight || this.view.dom.clientHeight,
      );
      for (const tick of overviewTicks(state.doc, chunks, original)) {
        const el = document.createElement("div");
        el.className = `cm-gitOverviewTick cm-gitOverview-${tick.kind}`;
        el.style.top = `${tick.top * 100}%`;
        el.style.height = `${Math.max(3, tick.size * height)}px`;
        this.dom.appendChild(el);
      }
    }
  },
);

const gitHunkActions = ViewPlugin.fromClass(
  class {
    readonly bar: HTMLDivElement;
    readonly revertButton: HTMLButtonElement;
    readonly stageButton: HTMLButtonElement;
    hover = -1;
    busy = false;

    constructor(readonly view: EditorView) {
      this.bar = document.createElement("div");
      this.bar.className = "cm-gitHunkBar";
      this.bar.hidden = true;
      this.revertButton = hunkButton("Revert change", UNDO_SVG);
      this.stageButton = hunkButton("Stage change", PLUS_SVG);
      this.bar.append(this.revertButton, this.stageButton);
      this.revertButton.addEventListener("mousedown", (event) => {
        this.onAction(event, "revert");
      });
      this.stageButton.addEventListener("mousedown", (event) => {
        this.onAction(event, "stage");
      });
      this.stageButton.hidden = view.state.facet(gitStageFacet) == null;
      view.dom.appendChild(this.bar);
      view.dom.addEventListener("mousemove", this.onMove);
      view.dom.addEventListener("mouseleave", this.onLeave);
      view.scrollDOM.addEventListener("scroll", this.onScroll, {
        passive: true,
      });
    }

    update(update: ViewUpdate) {
      if (
        update.state.field(chunksField) !== update.startState.field(chunksField)
      ) {
        this.hover = -1;
      }
      if (
        update.docChanged ||
        update.geometryChanged ||
        update.viewportChanged ||
        update.state.field(chunksField) !== update.startState.field(chunksField)
      ) {
        this.sync();
      }
      this.stageButton.hidden = update.state.facet(gitStageFacet) == null;
    }

    destroy() {
      this.view.dom.removeEventListener("mousemove", this.onMove);
      this.view.dom.removeEventListener("mouseleave", this.onLeave);
      this.view.scrollDOM.removeEventListener("scroll", this.onScroll);
      this.bar.remove();
    }

    onMove = (event: MouseEvent) => {
      if (this.busy) return;
      if (event.target instanceof Node && this.bar.contains(event.target)) {
        this.sync();
        return;
      }
      const index = hunkIndexAt(this.view, event.clientY);
      if (index !== this.hover) this.hover = index;
      this.sync();
    };

    onLeave = (event: MouseEvent) => {
      if (
        event.relatedTarget instanceof Node &&
        this.bar.contains(event.relatedTarget)
      ) {
        return;
      }
      this.hover = -1;
      this.sync();
    };

    onScroll = () => {
      this.sync();
    };

    onAction(event: MouseEvent, action: "revert" | "stage") {
      event.preventDefault();
      event.stopPropagation();
      const chunk = this.view.state.field(chunksField)[this.hover];
      if (!chunk || this.busy) return;
      const pos = widgetPos(this.view.state.doc, chunk);
      if (action === "revert") {
        revertChunkAt(this.view, pos);
        return;
      }
      this.busy = true;
      void stageChunkAt(this.view, pos).finally(() => {
        this.busy = false;
        this.sync();
      });
    }

    sync() {
      const chunk = this.view.state.field(chunksField)[this.hover];
      if (!chunk) {
        this.bar.hidden = true;
        return;
      }
      const bounds = hunkScreenBounds(this.view, chunk);
      const gutter = this.view.dom.querySelector(".cm-gitGutter");
      if (!bounds || !gutter) {
        this.bar.hidden = true;
        return;
      }
      const scroller = this.view.scrollDOM.getBoundingClientRect();
      if (bounds.bottom < scroller.top || bounds.top > scroller.bottom) {
        this.bar.hidden = true;
        return;
      }
      const editor = this.view.dom.getBoundingClientRect();
      const gutterRect = gutter.getBoundingClientRect();
      this.bar.hidden = false;
      this.stageButton.hidden = this.view.state.facet(gitStageFacet) == null;
      const height = this.bar.offsetHeight;
      const width = this.bar.offsetWidth;
      const mid = (bounds.top + bounds.bottom) / 2;
      this.bar.style.top = `${mid - editor.top - height / 2}px`;
      this.bar.style.left = `${gutterRect.left - editor.left + (gutterRect.width - width) / 2}px`;
    }
  },
);

function hunkButton(label: string, svg: string) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "cm-gitHunkBtn";
  button.title = label;
  button.setAttribute("aria-label", label);
  button.innerHTML = svg;
  return button;
}

function hunkIndexAt(view: EditorView, clientY: number): number {
  const chunks = view.state.field(chunksField);
  for (let i = 0; i < chunks.length; i++) {
    const bounds = hunkScreenBounds(view, chunks[i]!);
    if (!bounds) continue;
    if (clientY >= bounds.top && clientY <= bounds.bottom) return i;
  }
  return -1;
}

function hunkScreenBounds(
  view: EditorView,
  chunk: Chunk,
): { top: number; bottom: number } | null {
  const doc = view.state.doc;
  const from = widgetPos(doc, chunk);
  const start = view.lineBlockAt(from);
  let top = start.top;
  let bottom = start.bottom;
  if (chunk.fromB !== chunk.toB) {
    const last = Math.max(from, Math.min(chunk.endB, doc.length) - 1);
    bottom = view.lineBlockAt(last).bottom;
  }
  const scroller = view.scrollDOM.getBoundingClientRect();
  return {
    top: scroller.top + top - view.scrollDOM.scrollTop,
    bottom: scroller.top + bottom - view.scrollDOM.scrollTop,
  };
}

const gitTheme = EditorView.theme({
  "&": {
    position: "relative",
  },
  ".cm-gitGutter": {
    width: "22px",
    padding: "0",
    minWidth: "22px",
    overflow: "visible",
  },
  ".cm-gitGutter .cm-gutterElement": {
    display: "flex",
    justifyContent: "flex-end",
    padding: "0",
  },
  ".cm-gitMarker": {
    width: "3px",
    height: "100%",
  },
  ".cm-gitAdd": {
    backgroundColor: "#34d399",
  },
  ".cm-gitInsertedLine": {
    backgroundColor: "color-mix(in srgb, #34d399 18%, transparent)",
    boxShadow: "inset 3px 0 0 #34d399",
  },
  ".cm-gitDeletedChunk": {
    position: "relative",
    width: "100%",
  },
  ".cm-gitDeletedLine": {
    padding: "0 12px 0 6px",
    backgroundColor: "color-mix(in srgb, #f87171 16%, transparent)",
    boxShadow: "inset 3px 0 0 #f87171",
    whiteSpace: "pre-wrap",
    overflowWrap: "anywhere",
  },
  ".cm-gitOverview": {
    position: "absolute",
    top: "0",
    right: "0",
    bottom: "0",
    zIndex: "8",
    width: "10px",
    cursor: "pointer",
  },
  ".cm-gitOverviewTick": {
    position: "absolute",
    left: "1px",
    right: "1px",
    boxSizing: "border-box",
    borderRadius: "1px",
    pointerEvents: "none",
  },
  ".cm-gitOverview-add": {
    backgroundColor: "#34d399",
  },
  ".cm-gitOverview-del": {
    backgroundColor: "#f87171",
  },
  ".cm-gitOverview-mod": {
    display: "flex",
    flexDirection: "row",
    background: "linear-gradient(to right, #f87171 0 50%, #34d399 50% 100%)",
  },
  ".cm-gitHunkBar": {
    position: "absolute",
    zIndex: "24",
    display: "flex",
    flexDirection: "column",
    overflow: "hidden",
    userSelect: "none",
    borderRadius: "4px",
    color: "var(--color-content)",
    background:
      "color-mix(in srgb, var(--color-background-base) 92%, transparent)",
    boxShadow:
      "0 0 0 1px color-mix(in srgb, var(--color-content) 14%, transparent), 0 6px 16px color-mix(in srgb, #000 22%, transparent)",
  },
  ".cm-gitHunkBtn": {
    display: "grid",
    placeItems: "center",
    width: "18px",
    height: "18px",
    padding: "0",
    border: "none",
    color: "inherit",
    background: "transparent",
    cursor: "pointer",
  },
  ".cm-gitHunkBtn:hover": {
    backgroundColor:
      "color-mix(in srgb, var(--color-content) 12%, transparent)",
  },
  ".cm-gitHunkBtn:focus-visible": {
    outline: "1px solid var(--color-accent)",
    outlineOffset: "-1px",
  },
});

const UNDO_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"/></svg>';

const PLUS_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>';
