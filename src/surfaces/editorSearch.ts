import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  highlightSelectionMatches,
  openSearchPanel,
  replaceAll,
  replaceNext,
  search,
  SearchQuery,
  searchPanelOpen,
  setSearchQuery,
} from "@codemirror/search";
import {
  EditorSelection,
  Prec,
  type Extension,
  type SelectionRange,
} from "@codemirror/state";
import {
  EditorView,
  keymap,
  runScopeHandlers,
  type KeyBinding,
  type Panel,
  type ViewUpdate,
} from "@codemirror/view";
import { MOD, ALT, SHIFT } from "../lib/platform";

const MATCH_CAP = 999;
const panels = new WeakMap<EditorView, FindPanel>();

export function handleEditorFindKey(event: KeyboardEvent): boolean {
  if (event.isComposing) return false;

  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest("[data-file-picker], [data-model-picker], [data-branch-picker], [data-skill-picker]")) {
    return false;
  }

  const view = focusedEditorView();
  const mod = event.metaKey || event.ctrlKey;
  const key = event.key.toLowerCase();

  if (mod && event.altKey && !event.shiftKey && key === "f") {
    if (!view) return false;
    event.preventDefault();
    openReplacePanel(view);
    return true;
  }

  if (mod && !event.altKey && !event.shiftKey && key === "f") {
    event.preventDefault();
    if (view) openSearchPanel(view);
    return true;
  }

  if (event.key === "F3" || (mod && !event.altKey && key === "g")) {
    if (!view) return false;
    event.preventDefault();
    (event.shiftKey ? findPrevious : findNext)(view);
    return true;
  }

  if (event.key === "Escape") {
    if (!view || !searchPanelOpen(view.state)) return false;
    event.preventDefault();
    closeSearchPanel(view);
    return true;
  }

  return false;
}

function focusedEditorView(): EditorView | null {
  const active = document.activeElement;
  if (active instanceof Element) {
    const fromActive = active.closest(".cm-editor");
    if (fromActive instanceof HTMLElement) {
      return EditorView.findFromDOM(fromActive);
    }
  }
  const focused = document.querySelector(".cm-editor.cm-focused");
  if (focused instanceof HTMLElement) {
    return EditorView.findFromDOM(focused);
  }
  return null;
}

export function openFindInActiveEditor(): boolean {
  const view = focusedEditorView() ?? firstVisibleEditorView();
  if (!view) return false;
  openSearchPanel(view);
  return true;
}

function firstVisibleEditorView(): EditorView | null {
  for (const node of document.querySelectorAll(".cm-editor")) {
    if (!(node instanceof HTMLElement)) continue;
    if (node.closest(".invisible")) continue;
    const view = EditorView.findFromDOM(node);
    if (view) return view;
  }
  return null;
}

function openReplacePanel(view: EditorView): boolean {
  openSearchPanel(view);
  panels.get(view)?.setReplaceVisible(true, true);
  return true;
}

function findKeymap(): KeyBinding[] {
  return [
    {
      key: "Mod-f",
      run: openSearchPanel,
      scope: "editor search-panel",
      preventDefault: true,
    },
    {
      key: "Mod-Alt-f",
      run: openReplacePanel,
      scope: "editor search-panel",
      preventDefault: true,
    },
    {
      key: "F3",
      run: findNext,
      shift: findPrevious,
      scope: "editor search-panel",
      preventDefault: true,
    },
    {
      key: "Mod-g",
      run: findNext,
      shift: findPrevious,
      scope: "editor search-panel",
      preventDefault: true,
    },
    {
      key: "Escape",
      run: closeSearchPanel,
      scope: "editor search-panel",
    },
  ];
}

class FindPanel implements Panel {
  readonly view: EditorView;
  readonly dom: HTMLElement;
  readonly top = true;
  query: SearchQuery;
  private replaceVisible = false;
  private composing = false;
  private readonly searchField: HTMLInputElement;
  private readonly replaceField: HTMLInputElement;
  private readonly count: HTMLElement;
  private readonly caseButton: HTMLButtonElement;
  private readonly wordButton: HTMLButtonElement;
  private readonly regexButton: HTMLButtonElement;
  private readonly expandButton: HTMLButtonElement;
  private readonly replaceRow: HTMLElement;

  constructor(view: EditorView) {
    this.view = view;
    this.query = getSearchQuery(view.state);
    panels.set(view, this);

    this.searchField = inputField("Find", {
      name: "search",
      "main-field": "true",
      value: this.query.search,
    });
    this.replaceField = inputField("Replace", {
      name: "replace",
      value: this.query.replace,
    });
    this.count = elt("span", { class: "cm-find-count" });
    this.caseButton = toggleButton("Aa", "Match Case", `${ALT}C`);
    this.wordButton = toggleButton("ab", "Match Whole Word", `${ALT}W`);
    this.regexButton = toggleButton(".*", "Use Regular Expression", `${ALT}R`);
    this.expandButton = iconButton(
      "cm-find-expand",
      "Toggle Replace",
      `${MOD}${ALT}F`,
      svgIcon("M6 4l4 4-4 4"),
    );
    this.expandButton.setAttribute("aria-expanded", "false");

    const prevButton = iconButton(
      "cm-find-step",
      "Previous Match",
      `${MOD}${SHIFT}G`,
      svgIcon("M4 10l4-4 4 4"),
    );
    const nextButton = iconButton(
      "cm-find-step",
      "Next Match",
      `${MOD}G`,
      svgIcon("M4 6l4 4 4-4"),
    );
    const closeButton = iconButton(
      "cm-find-close",
      "Close",
      "Escape",
      svgIcon("M4.5 4.5l7 7M11.5 4.5l-7 7"),
    );

    this.replaceRow = elt("div", { class: "cm-find-row cm-find-replace-row" },
      elt("div", { class: "cm-find-query" }, this.replaceField),
      elt("div", { class: "cm-find-replace-actions" },
        textButton("Replace", () => replaceNext(this.view)),
        textButton("All", () => replaceAll(this.view)),
      ),
    );

    this.dom = elt("div", { class: "cm-find", onkeydown: (event) => this.onKeyDown(event) },
      this.expandButton,
      elt("div", { class: "cm-find-fields" },
        elt("div", { class: "cm-find-row" },
          elt("div", { class: "cm-find-query cm-find-search" },
            this.searchField,
            this.count,
          ),
          elt("div", { class: "cm-find-toggles" },
            this.caseButton,
            this.wordButton,
            this.regexButton,
          ),
        ),
        this.replaceRow,
      ),
      elt("div", { class: "cm-find-nav" }, prevButton, nextButton, closeButton),
    );

    this.searchField.addEventListener("compositionstart", () => {
      this.composing = true;
    });
    this.searchField.addEventListener("compositionend", () => {
      this.composing = false;
      this.commit(true);
    });
    this.searchField.addEventListener("input", () => {
      if (!this.composing) this.commit(true);
    });
    this.replaceField.addEventListener("input", () => this.commit(false));
    this.caseButton.addEventListener("click", () => this.toggle("caseSensitive"));
    this.wordButton.addEventListener("click", () => this.toggle("wholeWord"));
    this.regexButton.addEventListener("click", () => this.toggle("regexp"));
    this.expandButton.addEventListener("click", () =>
      this.setReplaceVisible(!this.replaceVisible, this.replaceVisible),
    );
    prevButton.addEventListener("click", () => findPrevious(this.view));
    nextButton.addEventListener("click", () => findNext(this.view));
    closeButton.addEventListener("click", () => closeSearchPanel(this.view));

    this.syncControls();
    this.syncCount();
  }

  mount() {
    this.searchField.select();
    this.reveal();
    this.syncCount();
  }

  destroy() {
    panels.delete(this.view);
  }

  update(update: ViewUpdate) {
    for (const transaction of update.transactions) {
      for (const effect of transaction.effects) {
        if (effect.is(setSearchQuery) && !effect.value.eq(this.query)) {
          this.query = effect.value;
          this.searchField.value = this.query.search;
          this.replaceField.value = this.query.replace;
          this.syncControls();
        }
      }
    }
    if (
      update.docChanged ||
      update.selectionSet ||
      update.transactions.some((transaction) =>
        transaction.effects.some((effect) => effect.is(setSearchQuery)),
      )
    ) {
      this.syncCount();
    }
  }

  setReplaceVisible(visible: boolean, focusReplace = false) {
    this.replaceVisible = visible;
    this.dom.classList.toggle("is-replace", visible);
    this.expandButton.setAttribute("aria-expanded", String(visible));
    if (visible && focusReplace) {
      this.replaceField.focus();
      this.replaceField.select();
    }
  }

  private toggle(flag: "caseSensitive" | "wholeWord" | "regexp") {
    this.query = new SearchQuery({
      search: this.searchField.value,
      replace: this.replaceField.value,
      caseSensitive: flag === "caseSensitive" ? !this.query.caseSensitive : this.query.caseSensitive,
      wholeWord: flag === "wholeWord" ? !this.query.wholeWord : this.query.wholeWord,
      regexp: flag === "regexp" ? !this.query.regexp : this.query.regexp,
      literal: true,
    });
    this.syncControls();
    this.view.dispatch({ effects: setSearchQuery.of(this.query) });
    this.reveal();
    this.syncCount();
  }

  private commit(reveal: boolean) {
    const query = new SearchQuery({
      search: this.searchField.value,
      replace: this.replaceField.value,
      caseSensitive: this.query.caseSensitive,
      wholeWord: this.query.wholeWord,
      regexp: this.query.regexp,
      literal: true,
    });
    if (!query.eq(this.query)) {
      this.query = query;
      this.view.dispatch({ effects: setSearchQuery.of(query) });
    }
    if (reveal) this.reveal();
    this.syncCount();
  }

  private reveal() {
    const query = getSearchQuery(this.view.state);
    if (!query.valid) return;
    const selection = this.view.state.selection.main;
    const forward = query.getCursor(this.view.state, selection.from);
    let match = nextMatch(forward);
    if (!match) {
      match = nextMatch(query.getCursor(this.view.state, 0, selection.from));
    }
    if (!match) return;
    if (selection.from === match.from && selection.to === match.to) return;
    const range = EditorSelection.single(match.from, match.to);
    this.view.dispatch({
      selection: range,
      effects: scrollToSearchMatch(range.main, this.view),
      userEvent: "select.search",
    });
  }

  private syncControls() {
    setPressed(this.caseButton, this.query.caseSensitive);
    setPressed(this.wordButton, this.query.wholeWord);
    setPressed(this.regexButton, this.query.regexp);
  }

  private syncCount() {
    const value = this.searchField.value;
    if (!value) {
      this.count.textContent = "";
      this.count.dataset.state = "idle";
      this.dom.classList.remove("is-empty", "is-invalid");
      return;
    }
    if (!this.query.valid) {
      this.count.textContent = "Invalid regex";
      this.count.dataset.state = "invalid";
      this.dom.classList.add("is-invalid");
      this.dom.classList.remove("is-empty");
      return;
    }

    const { current, total, capped } = countMatches(this.view);
    this.dom.classList.toggle("is-empty", total === 0);
    this.dom.classList.remove("is-invalid");
    if (total === 0) {
      this.count.textContent = "No results";
      this.count.dataset.state = "empty";
      return;
    }
    const suffix = capped ? "+" : "";
    this.count.textContent =
      current > 0 ? `${current} of ${total}${suffix}` : `${total}${suffix}`;
    this.count.dataset.state = "ok";
  }

  private onKeyDown(event: Event) {
    const keyEvent = event as KeyboardEvent;
    if (runScopeHandlers(this.view, keyEvent, "search-panel")) {
      keyEvent.preventDefault();
      return;
    }
    if (keyEvent.altKey && !keyEvent.metaKey && !keyEvent.ctrlKey && !keyEvent.shiftKey) {
      if (keyEvent.code === "KeyC") {
        keyEvent.preventDefault();
        this.toggle("caseSensitive");
        return;
      }
      if (keyEvent.code === "KeyW") {
        keyEvent.preventDefault();
        this.toggle("wholeWord");
        return;
      }
      if (keyEvent.code === "KeyR") {
        keyEvent.preventDefault();
        this.toggle("regexp");
        return;
      }
    }
    if (keyEvent.key !== "Enter") return;
    keyEvent.preventDefault();
    if (keyEvent.target === this.replaceField) {
      replaceNext(this.view);
      return;
    }
    (keyEvent.shiftKey ? findPrevious : findNext)(this.view);
  }
}

function countMatches(view: EditorView): {
  current: number;
  total: number;
  capped: boolean;
} {
  const query = getSearchQuery(view.state);
  if (!query.valid) return { current: 0, total: 0, capped: false };
  const selection = view.state.selection.main;
  const cursor = query.getCursor(view.state);
  let total = 0;
  let current = 0;
  for (let match = nextMatch(cursor); match; match = nextMatch(cursor)) {
    total += 1;
    if (match.from === selection.from && match.to === selection.to) {
      current = total;
    }
    if (total >= MATCH_CAP) return { current, total, capped: true };
  }
  return { current, total, capped: false };
}

function nextMatch(
  cursor: Iterator<{ from: number; to: number }>,
): { from: number; to: number } | null {
  const result = cursor.next();
  return result.done ? null : result.value;
}

function inputField(
  label: string,
  attrs: Record<string, string>,
): HTMLInputElement {
  return elt("input", {
    type: "text",
    placeholder: label,
    "aria-label": label,
    autocomplete: "off",
    autocorrect: "off",
    autocapitalize: "off",
    spellcheck: "false",
    ...attrs,
  });
}

function toggleButton(label: string, title: string, shortcut: string) {
  const button = elt("button", {
    type: "button",
    class: "cm-find-toggle",
    title: `${title} (${shortcut})`,
    "aria-label": title,
    "aria-pressed": "false",
    tabindex: "-1",
  }, label);
  return button;
}

function iconButton(
  className: string,
  title: string,
  shortcut: string,
  icon: Node,
) {
  return elt("button", {
    type: "button",
    class: className,
    title: `${title} (${shortcut})`,
    "aria-label": title,
    tabindex: "-1",
  }, icon);
}

function textButton(label: string, onClick: () => void) {
  return elt("button", {
    type: "button",
    class: "cm-find-text",
    tabindex: "-1",
    onclick: onClick,
  }, label);
}

function setPressed(button: HTMLButtonElement, pressed: boolean) {
  button.setAttribute("aria-pressed", String(pressed));
  button.classList.toggle("is-active", pressed);
}

function svgIcon(path: string) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", "14");
  svg.setAttribute("height", "14");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.75");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of path.split("M").slice(1)) {
    const node = document.createElementNS("http://www.w3.org/2000/svg", "path");
    node.setAttribute("d", `M${d}`);
    svg.append(node);
  }
  return svg;
}

type Attrs = Record<
  string,
  string | boolean | ((event: Event) => void) | null | undefined
>;

function elt<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Attrs,
  ...children: Array<Node | string | null | false>
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value == null || value === false) continue;
      if (key.startsWith("on") && typeof value === "function") {
        node.addEventListener(
          key.slice(2).toLowerCase(),
          value as EventListener,
        );
        continue;
      }
      if (value === true) {
        node.setAttribute(key, "");
        continue;
      }
      node.setAttribute(key, String(value));
    }
  }
  for (const child of children) {
    if (!child) continue;
    node.append(child);
  }
  return node;
}

const findTheme = EditorView.theme(
  {
    "&": {
      position: "relative",
    },
    ".cm-panels.cm-panels-top": {
      position: "absolute",
      top: "8px",
      right: "16px",
      left: "auto",
      width: "auto",
      maxWidth: "calc(100% - 24px)",
      zIndex: "20",
      backgroundColor: "transparent",
      border: "none",
    },
    ".cm-panel.cm-find": {
      display: "flex",
      alignItems: "stretch",
      gap: "4px",
      padding: "6px",
      border:
        "1px solid color-mix(in srgb, var(--color-content) 12%, transparent)",
      borderRadius: "10px",
      backgroundColor: "var(--color-background-base)",
      boxShadow: "0 10px 28px hsl(0 0% 0% / 0.28)",
      color: "var(--color-content)",
    },
    ".cm-find-expand, .cm-find-step, .cm-find-close, .cm-find-toggle, .cm-find-text":
      {
        display: "grid",
        placeItems: "center",
        margin: "0",
        padding: "0",
        border: "0",
        borderRadius: "6px",
        background: "transparent",
        color: "color-mix(in srgb, var(--color-content) 62%, transparent)",
        cursor: "pointer",
      },
    ".cm-find-expand:hover, .cm-find-step:hover, .cm-find-close:hover, .cm-find-toggle:hover, .cm-find-text:hover":
      {
        backgroundColor:
          "color-mix(in srgb, var(--color-content) 10%, transparent)",
        color: "var(--color-content)",
      },
    ".cm-find-expand": {
      width: "20px",
      alignSelf: "stretch",
    },
    ".cm-find-expand svg": {
      transition: "transform 0.12s ease",
    },
    ".cm-find.is-replace .cm-find-expand svg": {
      transform: "rotate(90deg)",
    },
    ".cm-find-fields": {
      display: "flex",
      flexDirection: "column",
      gap: "4px",
      minWidth: "0",
    },
    ".cm-find-row": {
      display: "flex",
      alignItems: "center",
      gap: "4px",
      minWidth: "0",
    },
    ".cm-find-replace-row": {
      display: "none",
    },
    ".cm-find.is-replace .cm-find-replace-row": {
      display: "flex",
    },
    ".cm-find-query": {
      display: "flex",
      alignItems: "center",
      boxSizing: "border-box",
      width: "240px",
      flex: "none",
      height: "26px",
      padding: "0 8px",
      border:
        "1px solid color-mix(in srgb, var(--color-content) 12%, transparent)",
      borderRadius: "6px",
      backgroundColor:
        "color-mix(in srgb, var(--color-content) 6%, transparent)",
    },
    ".cm-find.is-empty .cm-find-search, .cm-find.is-invalid .cm-find-search": {
      borderColor: "color-mix(in srgb, #f87171 55%, transparent)",
    },
    ".cm-find input": {
      minWidth: "0",
      flex: "1",
      height: "24px",
      margin: "0",
      padding: "0",
      border: "0",
      outline: "none",
      background: "transparent",
      color: "var(--color-content)",
      fontFamily: "var(--font-mono)",
      fontSize: "12px",
      userSelect: "text",
    },
    ".cm-find-count": {
      boxSizing: "border-box",
      width: "13ch",
      flex: "none",
      paddingLeft: "8px",
      overflow: "hidden",
      color: "color-mix(in srgb, var(--color-content) 45%, transparent)",
      fontFamily: "var(--font-mono)",
      fontSize: "11px",
      fontVariantNumeric: "tabular-nums",
      textAlign: "right",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    },
    ".cm-find-count[data-state=empty], .cm-find-count[data-state=invalid]": {
      color: "#f87171",
    },
    ".cm-find-toggles, .cm-find-nav, .cm-find-replace-actions": {
      display: "flex",
      alignItems: "center",
      gap: "2px",
    },
    ".cm-find-toggle": {
      width: "24px",
      height: "24px",
      fontFamily: "var(--font-mono)",
      fontSize: "11px",
      fontWeight: "600",
    },
    ".cm-find-toggle.is-active": {
      backgroundColor:
        "color-mix(in srgb, var(--color-accent) 28%, transparent)",
      color: "var(--color-content)",
    },
    ".cm-find-step, .cm-find-close": {
      width: "24px",
      height: "24px",
    },
    ".cm-find-text": {
      height: "24px",
      padding: "0 8px",
      fontSize: "11px",
    },
    ".cm-searchMatch": {
      backgroundColor: "color-mix(in srgb, #e2c08d 46%, transparent)",
    },
    ".cm-searchMatch-selected": {
      backgroundColor:
        "color-mix(in srgb, var(--color-accent) 52%, transparent)",
    },
    ".cm-selectionMatch": {
      backgroundColor:
        "color-mix(in srgb, var(--color-content) 14%, transparent)",
    },
  },
  { dark: true },
);

function scrollToSearchMatch(range: SelectionRange, view: EditorView) {
  const coords = view.coordsAtPos(range.from);
  const scroller = view.scrollDOM;
  const margin = Math.max(64, Math.floor(scroller.clientHeight * 0.28));
  if (coords) {
    const rect = scroller.getBoundingClientRect();
    if (
      coords.top >= rect.top + margin &&
      coords.bottom <= rect.bottom - margin
    ) {
      return EditorView.scrollIntoView(range, { y: "nearest", yMargin: margin });
    }
  }
  return EditorView.scrollIntoView(range, { y: "center" });
}

export const editorSearch: Extension = [
  search({
    top: true,
    literal: true,
    createPanel: (view) => new FindPanel(view),
    scrollToMatch: scrollToSearchMatch,
  }),
  highlightSelectionMatches(),
  Prec.high(keymap.of(findKeymap())),
  findTheme,
];
