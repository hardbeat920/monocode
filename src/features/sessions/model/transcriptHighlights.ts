const MATCH_HIGHLIGHT = "monocode-transcript-search-match";
const CURRENT_HIGHLIGHT = "monocode-transcript-search-current";
const MATCH_CAP = 1000;
const BLOCK_ELEMENTS = new Set([
  "blockquote",
  "dd",
  "div",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "li",
  "p",
  "pre",
  "td",
]);

let highlightOwner: symbol | null = null;

function transcriptSearchPattern(
  query: string,
  flags: string,
): RegExp | null {
  const needle = query.trim();
  if (!needle) return null;
  return new RegExp(needle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), flags);
}

export function transcriptMutationNeedsRepaint(
  records: MutationRecord[],
  query: string,
): boolean {
  const pattern = transcriptSearchPattern(query, "iu");
  if (!pattern) return false;
  const seen = new Set<HTMLElement>();
  for (const record of records) {
    if (record.removedNodes.length) return true;
    // A match can span multiple text nodes, so a character edit may remove part
    // of a match even when neither the old node nor the new item contains it.
    if (record.type === "characterData") return true;
    const target =
      record.target.nodeType === Node.TEXT_NODE
        ? record.target.parentElement
        : (record.target as Element);
    const item = target?.closest<HTMLElement>("[data-transcript-search-item]");
    if (!item) return true;
    if (seen.has(item)) continue;
    seen.add(item);
    if (pattern.test(item.textContent ?? "")) return true;
  }
  return false;
}

export function transcriptWordRanges(
  root: HTMLElement,
  query: string,
): { matches: Range[]; current: Range | null } {
  const pattern = transcriptSearchPattern(query, "giu");
  if (!pattern) return { matches: [], current: null };
  const matches: Range[] = [];
  let current: Range | null = null;

  for (const item of root.querySelectorAll<HTMLElement>(
    "[data-transcript-search-item]",
  )) {
    const content = item.querySelectorAll<HTMLElement>(
      "[data-selectable-agent-response]",
    );
    const roots = content.length ? [...content] : [item];
    for (const contentRoot of roots) {
      for (const run of textRuns(contentRoot)) {
        for (const hit of run.text.matchAll(pattern)) {
          if (hit.index === undefined || !hit[0]) continue;
          const range = toRange(
            run,
            hit.index,
            hit.index + hit[0].length,
            root.ownerDocument,
          );
          if (!range) continue;
          matches.push(range);
          if (!current && item.dataset.transcriptSearchCurrent === "true")
            current = range;
          if (matches.length >= MATCH_CAP) return { matches, current };
        }
      }
    }
  }
  return { matches, current };
}

export function paintTranscriptHighlights(
  owner: symbol,
  ranges: Range[],
  current: Range | null,
): void {
  if (typeof CSS === "undefined" || !("highlights" in CSS)) return;
  if (highlightOwner !== owner) {
    CSS.highlights.delete(MATCH_HIGHLIGHT);
    CSS.highlights.delete(CURRENT_HIGHLIGHT);
  }
  highlightOwner = owner;
  CSS.highlights.set(MATCH_HIGHLIGHT, new Highlight(...ranges));
  if (current) CSS.highlights.set(CURRENT_HIGHLIGHT, new Highlight(current));
  else CSS.highlights.delete(CURRENT_HIGHLIGHT);
}

export function clearTranscriptHighlights(owner: symbol): void {
  if (highlightOwner !== owner) return;
  if (typeof CSS !== "undefined" && "highlights" in CSS) {
    CSS.highlights.delete(MATCH_HIGHLIGHT);
    CSS.highlights.delete(CURRENT_HIGHLIGHT);
  }
  highlightOwner = null;
}

type Segment = { node: Text; from: number; to: number };
type TextRun = { text: string; segments: Segment[]; key: Element };

function textRuns(root: HTMLElement): TextRun[] {
  const walker = root.ownerDocument.createTreeWalker(
    root,
    NodeFilter.SHOW_TEXT,
  );
  const runs: TextRun[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!(node instanceof Text) || !node.data) continue;
    const parent = node.parentElement;
    if (
      !parent ||
      parent.closest("script, style, noscript, [hidden], [aria-hidden='true']")
    )
      continue;
    const key = blockContainer(parent, root);
    let run = runs[runs.length - 1];
    if (!run || run.key !== key) {
      run = { text: "", segments: [], key };
      runs.push(run);
    }
    const from = run.text.length;
    run.text += node.data;
    run.segments.push({ node, from, to: run.text.length });
  }
  return runs;
}

function blockContainer(element: Element, root: HTMLElement): Element {
  for (
    let current: Element | null = element;
    current;
    current = current.parentElement
  ) {
    if (current === root || BLOCK_ELEMENTS.has(current.localName))
      return current;
  }
  return root;
}

function toRange(
  run: TextRun,
  from: number,
  to: number,
  document: Document,
): Range | null {
  const start = run.segments.find(
    (segment) => from >= segment.from && from < segment.to,
  );
  const endOffset = to - 1;
  const end = run.segments.find(
    (segment) => endOffset >= segment.from && endOffset < segment.to,
  );
  if (!start || !end) return null;
  const range = document.createRange();
  range.setStart(start.node, from - start.from);
  range.setEnd(end.node, to - end.from);
  return range;
}
