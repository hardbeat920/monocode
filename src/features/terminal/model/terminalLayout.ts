import type { Terminal } from "@xterm/xterm";

export type TerminalFitMode = "shell" | "tui";

const DEFAULT_SCROLLBAR_WIDTH = 14;
const MIN_TUI_SCROLLBAR_WIDTH = 1;

type CellSize = { width: number; height: number };

export function terminalScrollbarWidth(
  overviewRuler?: { width?: number },
): number {
  const width = overviewRuler?.width;
  return width === undefined ? DEFAULT_SCROLLBAR_WIDTH : width;
}

function cellSize(term: Terminal): CellSize | null {
  const dims = (
    term as unknown as {
      _core?: {
        _renderService?: { dimensions?: { css?: { cell?: CellSize } } };
      };
    }
  )._core?._renderService?.dimensions?.css?.cell;
  if (!dims || dims.width < 1 || dims.height < 1) return null;
  return dims;
}

function availableSize(
  host: HTMLElement,
  mode: TerminalFitMode,
  term: Terminal,
): { width: number; height: number } | null {
  const gutter =
    mode === "tui"
      ? MIN_TUI_SCROLLBAR_WIDTH
      : terminalScrollbarWidth(term.options.overviewRuler);
  const width = host.clientWidth - gutter;
  const height = host.clientHeight;
  if (width < 8 || height < 8) return null;
  return { width, height };
}

/** Grow letter-spacing / line-height so the cell grid covers the host (TUI mode). */
export function stretchGridToHost(
  term: Terminal,
  host: HTMLElement,
  mode: TerminalFitMode,
): void {
  if (mode !== "tui") return;
  const size = availableSize(host, mode, term);
  if (!size) return;

  for (let pass = 0; pass < 4; pass++) {
    const cell = cellSize(term);
    if (!cell) break;
    const gapW = size.width - term.cols * cell.width;
    const gapH = size.height - term.rows * cell.height;
    if (gapW <= 0.5 && gapH <= 0.5) break;
    if (gapW > 0.5) {
      term.options.letterSpacing =
        (term.options.letterSpacing ?? 0) + gapW / term.cols;
    }
    if (gapH > 0.5) {
      const rowHeight = cell.height;
      const targetRow = size.height / term.rows;
      term.options.lineHeight =
        (term.options.lineHeight ?? 1) * (targetRow / rowHeight);
    }
  }
}

export function resetGridStretch(term: Terminal): void {
  term.options.letterSpacing = 0;
  term.options.lineHeight = 1;
}

export function fitTerminal(
  term: Terminal,
  host: HTMLElement,
  mode: TerminalFitMode,
): { cols: number; rows: number } | null {
  const size = availableSize(host, mode, term);
  if (!size) return null;

  let cell = cellSize(term);
  if (!cell) return null;

  const round = mode === "tui" ? Math.ceil : Math.floor;
  let cols = Math.max(2, round(size.width / cell.width));
  let rows = Math.max(1, round(size.height / cell.height));

  if (term.cols !== cols || term.rows !== rows) {
    term.resize(cols, rows);
    cell = cellSize(term);
    if (!cell) return { cols, rows };
    if (mode === "tui") {
      while (cols * cell.width < size.width - 0.5) cols++;
      while (rows * cell.height < size.height - 0.5) rows++;
      if (term.cols !== cols || term.rows !== rows) {
        term.resize(cols, rows);
      }
    }
  }

  if (mode === "tui") {
    stretchGridToHost(term, host, mode);
  } else {
    resetGridStretch(term);
  }

  return { cols: term.cols, rows: term.rows };
}

export function applyTerminalChrome(
  term: Terminal,
  outer: HTMLElement,
  tui: boolean,
): void {
  outer.classList.toggle("monocode-terminal--alt-screen", tui);
  term.options.overviewRuler = tui ? { width: MIN_TUI_SCROLLBAR_WIDTH } : {};
}
