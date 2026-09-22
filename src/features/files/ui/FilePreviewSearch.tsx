import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { ALT, MOD, SHIFT } from "../../../platform/tauri/platform";
import { ChevronDown, ChevronUp, X } from "../../../shared/ui/icons";

const MATCH_CAP = 999;
const MATCH_HIGHLIGHT = "monocode-file-preview-search-match";
const CURRENT_HIGHLIGHT = "monocode-file-preview-search-current";
const HIGHLIGHT_STYLE_ID = "monocode-file-preview-search-styles";
const BLOCK_ELEMENTS = new Set([
  "address",
  "article",
  "aside",
  "blockquote",
  "dd",
  "details",
  "div",
  "dl",
  "dt",
  "figcaption",
  "figure",
  "footer",
  "form",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "header",
  "li",
  "main",
  "nav",
  "ol",
  "p",
  "pre",
  "section",
  "summary",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "tr",
  "ul",
]);

type SearchOptions = {
  caseSensitive: boolean;
  wholeWord: boolean;
  regexp: boolean;
};

type SearchResult<T> = {
  matches: T[];
  capped: boolean;
  invalid: boolean;
};

type TextMatch = { from: number; to: number };
type TextSegment = { node: Text; from: number; to: number };
type TextRun = { text: string; segments: TextSegment[]; key: Element };

type PreviewSearchController = {
  root: () => HTMLElement | null;
  active: () => boolean;
  open: () => void;
  close: () => void;
  isOpen: () => boolean;
  next: () => void;
  previous: () => void;
};

const controllers = new Set<PreviewSearchController>();
let highlightOwner: symbol | null = null;

export function handleFilePreviewFindKey(event: KeyboardEvent): boolean {
  if (event.isComposing) return false;
  const controller = activeController();
  if (!controller) return false;

  const mod = event.metaKey || event.ctrlKey;
  const key = event.key.toLowerCase();
  if (mod && !event.altKey && !event.shiftKey && key === "f") {
    event.preventDefault();
    controller.open();
    return true;
  }
  if (
    controller.isOpen() &&
    (event.key === "F3" || (mod && !event.altKey && key === "g"))
  ) {
    event.preventDefault();
    (event.shiftKey ? controller.previous : controller.next)();
    return true;
  }
  if (controller.isOpen() && event.key === "Escape") {
    event.preventDefault();
    controller.close();
    return true;
  }
  return false;
}

export function openFindInActiveFilePreview(): boolean {
  const controller = activeController();
  if (!controller) return false;
  controller.open();
  return true;
}

function activeController(): PreviewSearchController | null {
  const ordered = [...controllers];
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const controller = ordered[index];
    const root = controller.root();
    if (
      controller.active() &&
      root &&
      !root.closest(".hidden, .invisible, [hidden], [aria-hidden='true']")
    ) {
      return controller;
    }
  }
  return null;
}

export function FilePreviewSearch({
  active,
  contentVersion,
  children,
}: {
  active: boolean;
  contentVersion: string;
  children: ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const activeRef = useRef(active);
  const openRef = useRef(false);
  const matchesRef = useRef<Range[]>([]);
  const currentRef = useRef(-1);
  const signatureRef = useRef("");
  const revealRef = useRef(false);
  const ownerRef = useRef(Symbol("file-preview-search"));
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [regexp, setRegexp] = useState(false);
  const [current, setCurrent] = useState(-1);
  const [result, setResult] = useState<Omit<SearchResult<Range>, "matches">>({
    capped: false,
    invalid: false,
  });
  const [total, setTotal] = useState(0);
  const [mutationVersion, setMutationVersion] = useState(0);
  activeRef.current = active;
  openRef.current = open;

  const focusInput = useCallback(() => {
    requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
  }, []);

  const openSearch = useCallback(() => {
    openRef.current = true;
    revealRef.current = true;
    setOpen(true);
    focusInput();
  }, [focusInput]);

  const closeSearch = useCallback(() => {
    openRef.current = false;
    setOpen(false);
    clearHighlights(ownerRef.current);
    requestAnimationFrame(() => {
      rootRef.current
        ?.querySelector<HTMLElement>('[aria-label="Markdown preview"]')
        ?.focus();
    });
  }, []);

  const step = useCallback((delta: number) => {
    const matches = matchesRef.current;
    if (matches.length === 0) return;
    const next =
      (Math.max(0, currentRef.current) + delta + matches.length) %
      matches.length;
    currentRef.current = next;
    setCurrent(next);
    paintHighlights(ownerRef.current, matches, next);
    revealRange(matches[next], contentRef.current);
  }, []);

  useLayoutEffect(() => {
    const controller: PreviewSearchController = {
      root: () => rootRef.current,
      active: () => activeRef.current,
      open: openSearch,
      close: closeSearch,
      isOpen: () => openRef.current,
      next: () => step(1),
      previous: () => step(-1),
    };
    controllers.add(controller);
    return () => {
      controllers.delete(controller);
    };
  }, [closeSearch, openSearch, step]);

  useLayoutEffect(() => {
    if (active) return;
    openRef.current = false;
    setOpen(false);
    clearHighlights(ownerRef.current);
  }, [active]);

  useEffect(() => {
    if (!open || !contentRef.current) return;
    const observer = new MutationObserver(() => {
      setMutationVersion((version) => version + 1);
    });
    observer.observe(contentRef.current, {
      childList: true,
      characterData: true,
      subtree: true,
    });
    return () => observer.disconnect();
  }, [open]);

  useLayoutEffect(() => {
    const root = contentRef.current;
    if (!open || !active || !root || !query) {
      matchesRef.current = [];
      currentRef.current = -1;
      setCurrent(-1);
      setTotal(0);
      setResult({ capped: false, invalid: false });
      clearHighlights(ownerRef.current);
      return;
    }

    const signature = `${query}\0${caseSensitive}\0${wholeWord}\0${regexp}`;
    const search = findPreviewRanges(root, query, {
      caseSensitive,
      wholeWord,
      regexp,
    });
    const nextCurrent =
      search.matches.length === 0
        ? -1
        : signatureRef.current === signature
          ? Math.min(Math.max(0, currentRef.current), search.matches.length - 1)
          : 0;
    signatureRef.current = signature;
    matchesRef.current = search.matches;
    currentRef.current = nextCurrent;
    setCurrent(nextCurrent);
    setTotal(search.matches.length);
    setResult({ capped: search.capped, invalid: search.invalid });
    paintHighlights(ownerRef.current, search.matches, nextCurrent);
    if (revealRef.current && nextCurrent >= 0) {
      revealRange(search.matches[nextCurrent], root);
    }
    revealRef.current = false;
  }, [
    active,
    caseSensitive,
    contentVersion,
    mutationVersion,
    open,
    query,
    regexp,
    wholeWord,
  ]);

  useEffect(
    () => () => {
      clearHighlights(ownerRef.current);
    },
    [],
  );

  const updateQuery = (value: string) => {
    revealRef.current = true;
    setQuery(value);
  };
  const toggle = (setter: (update: (value: boolean) => boolean) => void) => {
    revealRef.current = true;
    setter((value) => !value);
  };
  const onKeyDown = (event: ReactKeyboardEvent) => {
    if (event.key === "Enter") {
      event.preventDefault();
      step(event.shiftKey ? -1 : 1);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeSearch();
      return;
    }
    if (event.altKey && !event.metaKey && !event.ctrlKey && !event.shiftKey) {
      if (event.code === "KeyC") {
        event.preventDefault();
        toggle(setCaseSensitive);
      } else if (event.code === "KeyW") {
        event.preventDefault();
        toggle(setWholeWord);
      } else if (event.code === "KeyR") {
        event.preventDefault();
        toggle(setRegexp);
      }
    }
  };

  const count = result.invalid
    ? "Invalid regex"
    : query && total === 0
      ? "No results"
      : total > 0
        ? `${current + 1} of ${total}${result.capped ? "+" : ""}`
        : "";

  return (
    <div
      ref={rootRef}
      data-file-preview-search-open={open && active ? "" : undefined}
      className="flex h-full min-h-0 flex-col"
    >
      {open ? (
        <div
          role="search"
          aria-label="Find in preview"
          className="relative z-30 flex h-[35px] shrink-0 items-center gap-1 border-b border-stroke px-2 py-1 text-content"
          onKeyDown={onKeyDown}
        >
          <div
            className={`flex h-[26px] min-w-0 flex-1 items-center rounded-md border bg-content/[0.06] px-2 ${
              (query && total === 0) || result.invalid
                ? "border-red-400/55"
                : "border-content/10"
            }`}
          >
            <input
              ref={inputRef}
              type="text"
              value={query}
              aria-label="Find"
              placeholder="Find"
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
              onChange={(event) => updateQuery(event.currentTarget.value)}
              className="h-6 min-w-0 flex-1 border-0 bg-transparent p-0 font-mono text-[12px] text-content outline-none"
            />
            <span
              aria-live="polite"
              className={`w-[13ch] shrink-0 overflow-hidden pl-2 text-right font-mono text-[11px] tabular-nums text-ellipsis whitespace-nowrap ${
                (query && total === 0) || result.invalid
                  ? "text-red-400"
                  : "text-content/45"
              }`}
            >
              {count}
            </span>
          </div>
          <FindToggle
            label="Aa"
            title={`Match Case (${ALT}C)`}
            pressed={caseSensitive}
            onClick={() => toggle(setCaseSensitive)}
          />
          <FindToggle
            label="ab"
            title={`Match Whole Word (${ALT}W)`}
            pressed={wholeWord}
            onClick={() => toggle(setWholeWord)}
          />
          <FindToggle
            label=".*"
            title={`Use Regular Expression (${ALT}R)`}
            pressed={regexp}
            onClick={() => toggle(setRegexp)}
          />
          <FindButton
            label="Previous Match"
            title={`Previous Match (${MOD}${SHIFT}G)`}
            disabled={total === 0}
            onClick={() => step(-1)}
          >
            <ChevronUp className="size-3.5" strokeWidth={1.75} />
          </FindButton>
          <FindButton
            label="Next Match"
            title={`Next Match (${MOD}G)`}
            disabled={total === 0}
            onClick={() => step(1)}
          >
            <ChevronDown className="size-3.5" strokeWidth={1.75} />
          </FindButton>
          <FindButton
            label="Close"
            title="Close (Escape)"
            onClick={closeSearch}
          >
            <X className="size-3.5" strokeWidth={1.75} />
          </FindButton>
        </div>
      ) : null}
      <div ref={contentRef} className="min-h-0 flex-1">
        {children}
      </div>
    </div>
  );
}

function FindToggle({
  label,
  title,
  pressed,
  onClick,
}: {
  label: string;
  title: string;
  pressed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title.slice(0, title.lastIndexOf(" ("))}
      aria-pressed={pressed}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      className={`grid size-6 shrink-0 place-items-center rounded-md font-mono text-[11px] font-semibold hover:bg-content/10 hover:text-content ${
        pressed ? "bg-accent/25 text-content" : "text-content/60"
      }`}
    >
      {label}
    </button>
  );
}

function FindButton({
  label,
  title,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  title: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={label}
      disabled={disabled}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      className="grid size-6 shrink-0 place-items-center rounded-md text-content/60 hover:bg-content/10 hover:text-content disabled:opacity-35"
    >
      {children}
    </button>
  );
}

export function findPreviewTextMatches(
  text: string,
  query: string,
  options: SearchOptions,
): SearchResult<TextMatch> {
  const pattern = searchPattern(query, options);
  if (!pattern) {
    return { matches: [], capped: false, invalid: Boolean(query) };
  }
  const matches: TextMatch[] = [];
  for (const match of text.matchAll(pattern)) {
    const value = match[0];
    const from = match.index;
    if (!value || from === undefined) continue;
    const to = from + value.length;
    if (options.wholeWord && !isWholeWord(text, from, to)) continue;
    if (matches.length >= MATCH_CAP) {
      return { matches, capped: true, invalid: false };
    }
    matches.push({ from, to });
  }
  return { matches, capped: false, invalid: false };
}

function findPreviewRanges(
  root: HTMLElement,
  query: string,
  options: SearchOptions,
): SearchResult<Range> {
  const pattern = searchPattern(query, options);
  if (!pattern) {
    return { matches: [], capped: false, invalid: Boolean(query) };
  }

  const ranges: Range[] = [];
  for (const run of previewTextRuns(root)) {
    for (const match of run.text.matchAll(pattern)) {
      const value = match[0];
      const from = match.index;
      if (!value || from === undefined) continue;
      const to = from + value.length;
      if (options.wholeWord && !isWholeWord(run.text, from, to)) continue;
      if (ranges.length >= MATCH_CAP) {
        return { matches: ranges, capped: true, invalid: false };
      }
      const range = domRange(run, from, to, root.ownerDocument);
      if (range) ranges.push(range);
    }
  }
  return { matches: ranges, capped: false, invalid: false };
}

function searchPattern(query: string, options: SearchOptions): RegExp | null {
  if (!query) return null;
  const source = options.regexp
    ? query
    : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  try {
    return new RegExp(source, options.caseSensitive ? "gu" : "giu");
  } catch {
    return null;
  }
}

function isWholeWord(text: string, from: number, to: number): boolean {
  return !isWordCharacter(text[from - 1]) && !isWordCharacter(text[to]);
}

function isWordCharacter(character: string | undefined): boolean {
  return character ? /[\p{L}\p{N}_]/u.test(character) : false;
}

function previewTextRuns(root: HTMLElement): TextRun[] {
  const showText = root.ownerDocument.defaultView?.NodeFilter.SHOW_TEXT ?? 4;
  const walker = root.ownerDocument.createTreeWalker(root, showText);
  const runs: TextRun[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!(node instanceof Text) || !node.data) continue;
    const parent = node.parentElement;
    if (
      !parent ||
      parent.closest("script, style, noscript, [hidden], [aria-hidden='true']")
    ) {
      continue;
    }
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

function domRange(
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

function paintHighlights(owner: symbol, ranges: Range[], current: number) {
  if (typeof CSS === "undefined" || !("highlights" in CSS)) return;
  ensureHighlightStyles(ranges[0]?.startContainer.ownerDocument);
  const registry = CSS.highlights;
  if (highlightOwner !== owner) {
    registry.delete(MATCH_HIGHLIGHT);
    registry.delete(CURRENT_HIGHLIGHT);
  }
  highlightOwner = owner;
  registry.set(MATCH_HIGHLIGHT, new Highlight(...ranges));
  registry.delete(CURRENT_HIGHLIGHT);
  if (current >= 0 && ranges[current]) {
    registry.set(CURRENT_HIGHLIGHT, new Highlight(ranges[current]));
  }
}

function clearHighlights(owner: symbol) {
  if (highlightOwner !== owner) return;
  if (typeof CSS !== "undefined" && "highlights" in CSS) {
    CSS.highlights.delete(MATCH_HIGHLIGHT);
    CSS.highlights.delete(CURRENT_HIGHLIGHT);
  }
  highlightOwner = null;
}

function ensureHighlightStyles(document: Document | null | undefined) {
  if (!document || document.getElementById(HIGHLIGHT_STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = HIGHLIGHT_STYLE_ID;
  style.textContent = `
    ::highlight(${MATCH_HIGHLIGHT}) {
      background-color: rgba(226, 192, 141, 0.46);
      background-color: color-mix(in srgb, #e2c08d 46%, transparent);
    }
    ::highlight(${CURRENT_HIGHLIGHT}) {
      background-color: rgba(69, 155, 247, 0.62);
      background-color: color-mix(in srgb, var(--color-accent) 62%, transparent);
    }
  `;
  document.head.append(style);
}

function revealRange(range: Range, root: HTMLElement | null) {
  if (!root) return;
  const rangeWithRect = range as Range & {
    getBoundingClientRect?: () => DOMRect;
  };
  const scroller = root.querySelector<HTMLElement>(".markdown-preview") ?? root;
  const rangeRect = rangeWithRect.getBoundingClientRect?.();
  if (rangeRect) {
    const scrollerRect = scroller.getBoundingClientRect();
    if (
      rangeRect.top >= scrollerRect.top &&
      rangeRect.bottom <= scrollerRect.bottom
    ) {
      return;
    }
    scroller.scrollTo({
      top:
        scroller.scrollTop +
        rangeRect.top -
        scrollerRect.top -
        scroller.clientHeight / 2,
    });
    return;
  }
  range.startContainer.parentElement?.scrollIntoView({ block: "center" });
}
