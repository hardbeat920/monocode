import { useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  addTerminalToDock,
  clampDockSize,
  closeTerminalInDock,
  createProjectTerminal,
  findProjectTerminal,
  isDockSide,
  mapProjectTerminal,
  reorderDockTerminals,
  selectDockTerminal,
  withDockOpen,
  withDockSide,
  withDockSize,
  type DockSide,
  type ProjectTerminalDock,
} from "../../projects/model/projectTerminal";
import { normalizeProjectPath } from "../../projects/model/recents";
import type {
  BrowserTabSource,
  FilePaneTab,
} from "../../workspace/model/layout";
import { BLANK_URL } from "./browserUrl";

/** Browser docks reuse the terminal dock model: per project, one pane of tabs. */
export type BrowserDock = ProjectTerminalDock;

export type BrowserState = {
  docks: BrowserDock[];
  /** Project the window is showing; new tabs land in its dock. */
  projectPath: string;
  lastSide: DockSide;
};

export const BROWSER_DEFAULT_SIDE: DockSide = "right";
const DEFAULT_SIZE = { horizontal: 520, vertical: 320 };
const STORAGE_PREFIX = "monocode.browser.v1:";

export function browserDefaultSize(side: DockSide): number {
  return side === "left" || side === "right"
    ? DEFAULT_SIZE.horizontal
    : DEFAULT_SIZE.vertical;
}

export function newBrowserTab(url: string, cwd: string): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: url,
    cwd,
    browser: { url },
  };
}

// ---- pure transitions --------------------------------------------------

function viewport() {
  return typeof window === "undefined"
    ? undefined
    : { width: window.innerWidth, height: window.innerHeight };
}

export function addBrowserTab(
  state: BrowserState,
  file: FilePaneTab,
  projectPath = state.projectPath,
  options: { afterId?: string } = {},
): BrowserState {
  const existing = findProjectTerminal(state.docks, projectPath);
  if (!existing) {
    const dock = createProjectTerminal(projectPath, file, state.lastSide);
    return {
      ...state,
      docks: [
        ...state.docks,
        {
          ...dock,
          size: clampDockSize(
            dock.side,
            browserDefaultSize(dock.side),
            viewport(),
          ),
        },
      ],
    };
  }
  return {
    ...state,
    docks: mapProjectTerminal(state.docks, projectPath, (dock) => {
      const next = addTerminalToDock(dock, file);
      const at = options.afterId
        ? dock.pane.files.findIndex((entry) => entry.id === options.afterId)
        : -1;
      if (at < 0) return next;
      const files = [...dock.pane.files];
      files.splice(at + 1, 0, file);
      return { ...next, pane: { ...next.pane, files } };
    }),
  };
}

export function toggleBrowserDock(state: BrowserState): BrowserState {
  const dock = findProjectTerminal(state.docks, state.projectPath);
  if (!dock) {
    return addBrowserTab(state, newBrowserTab(BLANK_URL, state.projectPath));
  }
  return mapDock(state, (entry) => withDockOpen(entry, !entry.open));
}

export function patchBrowserTabIn(
  state: BrowserState,
  fileId: string,
  patch: Partial<BrowserTabSource>,
): BrowserState {
  let changed = false;
  const docks = state.docks.map((dock) => {
    if (!dock.pane.files.some((file) => file.id === fileId)) return dock;
    const files = dock.pane.files.map((file) => {
      if (file.id !== fileId || !file.browser) return file;
      const browser = { ...file.browser, ...patch };
      if (
        browser.url === file.browser.url &&
        browser.title === file.browser.title &&
        browser.loading === file.browser.loading
      ) {
        return file;
      }
      changed = true;
      return { ...file, path: browser.url, browser };
    });
    return { ...dock, pane: { ...dock.pane, files } };
  });
  return changed ? { ...state, docks } : state;
}

export function dockOfTab(
  state: BrowserState,
  fileId: string,
): BrowserDock | undefined {
  return state.docks.find((dock) =>
    dock.pane.files.some((file) => file.id === fileId),
  );
}

function mapDock(
  state: BrowserState,
  update: (dock: BrowserDock) => BrowserDock | null,
  projectPath = state.projectPath,
): BrowserState {
  const docks = mapProjectTerminal(state.docks, projectPath, update);
  return docks === state.docks ? state : { ...state, docks };
}

// ---- persistence -------------------------------------------------------

type StoredState = {
  docks: BrowserDock[];
  lastSide?: DockSide;
};

export function serializeBrowserState(state: BrowserState): string {
  const stored: StoredState = {
    lastSide: state.lastSide,
    docks: state.docks.map((dock) => ({
      ...dock,
      pane: {
        ...dock.pane,
        files: dock.pane.files.map((file) => ({
          id: file.id,
          path: file.path,
          cwd: file.cwd,
          browser: file.browser
            ? { url: file.browser.url, title: file.browser.title }
            : undefined,
        })),
      },
    })),
  };
  return JSON.stringify(stored);
}

export function parseBrowserState(raw: string | null): StoredState {
  if (!raw) return { docks: [] };
  try {
    const value = JSON.parse(raw) as Partial<StoredState>;
    const docks = (Array.isArray(value.docks) ? value.docks : []).flatMap(
      (dock): BrowserDock[] => {
        if (!dock || typeof dock.projectPath !== "string") return [];
        if (!isDockSide(dock.side) || !dock.pane) return [];
        const files = (dock.pane.files ?? []).filter(
          (file) =>
            typeof file?.id === "string" &&
            /^[\w-]{1,64}$/.test(file.id) &&
            typeof file.browser?.url === "string",
        );
        if (files.length === 0) return [];
        const activeFileId = files.some(
          (file) => file.id === dock.pane.activeFileId,
        )
          ? dock.pane.activeFileId
          : files[0].id;
        return [
          {
            projectPath: dock.projectPath,
            side: dock.side,
            size: clampDockSize(dock.side, Number(dock.size)),
            open: !!dock.open,
            pane: { id: dock.pane.id || crypto.randomUUID(), files, activeFileId },
          },
        ];
      },
    );
    return {
      docks,
      lastSide: isDockSide(value.lastSide) ? value.lastSide : undefined,
    };
  } catch {
    return { docks: [] };
  }
}

// ---- store -------------------------------------------------------------

// Per window: each window keeps its own docks, like its terminals.
function storageKey(): string | null {
  try {
    return `${STORAGE_PREFIX}${getCurrentWindow().label}`;
  } catch {
    return null;
  }
}

function initialState(): BrowserState {
  const key = typeof window === "undefined" ? null : storageKey();
  const stored = parseBrowserState(key ? localStorage.getItem(key) : null);
  return {
    docks: stored.docks,
    projectPath: "",
    lastSide: stored.lastSide ?? BROWSER_DEFAULT_SIDE,
  };
}

let state: BrowserState | null = null;
const listeners = new Set<() => void>();
let saveTimer: ReturnType<typeof setTimeout> | undefined;

export function getBrowserState(): BrowserState {
  state ??= initialState();
  return state;
}

function setState(next: BrowserState) {
  if (next === getBrowserState()) return;
  state = next;
  for (const listener of listeners) listener();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    const key = storageKey();
    if (key && state) localStorage.setItem(key, serializeBrowserState(state));
  }, 250);
}

function update(fn: (current: BrowserState) => BrowserState) {
  setState(fn(getBrowserState()));
}

export function subscribeBrowser(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useBrowserState(): BrowserState {
  return useSyncExternalStore(subscribeBrowser, getBrowserState);
}

export function setBrowserProject(projectPath: string) {
  const path = projectPath ? normalizeProjectPath(projectPath) : "";
  update((current) =>
    current.projectPath === path ? current : { ...current, projectPath: path },
  );
}

export function toggleBrowser() {
  update(toggleBrowserDock);
}

export function showBrowser() {
  update((current) =>
    findProjectTerminal(current.docks, current.projectPath)
      ? mapDock(current, (dock) => withDockOpen(dock, true))
      : toggleBrowserDock(current),
  );
}

export function hideBrowser() {
  update((current) => mapDock(current, (dock) => withDockOpen(dock, false)));
}

/** Open `url` in a new tab of the current project's browser and show it. */
export function openBrowserTab(
  url: string = BLANK_URL,
  options: { projectPath?: string; afterId?: string } = {},
): string {
  const projectPath = options.projectPath ?? getBrowserState().projectPath;
  const file = newBrowserTab(url, projectPath);
  update((current) =>
    mapDock(
      addBrowserTab(current, file, projectPath, options),
      (dock) => withDockOpen(dock, true),
      projectPath,
    ),
  );
  return file.id;
}

export function selectBrowserTab(fileId: string) {
  update((current) => mapDock(current, (dock) => selectDockTerminal(dock, fileId)));
}

export function closeBrowserTab(fileId: string) {
  update((current) => {
    const dock = dockOfTab(current, fileId);
    if (!dock) return current;
    return mapDock(
      current,
      (entry) => closeTerminalInDock(entry, fileId),
      dock.projectPath,
    );
  });
}

export function closeOtherBrowserTabs(fileId: string) {
  update((current) =>
    mapDock(current, (dock) => {
      const keep = dock.pane.files.filter((file) => file.id === fileId);
      if (keep.length === 0) return dock;
      return { ...dock, pane: { ...dock.pane, files: keep, activeFileId: fileId } };
    }),
  );
}

export function reorderBrowserTabs(ids: string[]) {
  update((current) =>
    mapDock(current, (dock) => {
      const byId = new Map(dock.pane.files.map((file) => [file.id, file]));
      const files = ids.flatMap((id) => byId.get(id) ?? []);
      if (files.length !== dock.pane.files.length) return dock;
      return reorderDockTerminals(dock, files);
    }),
  );
}

export function setBrowserSide(side: DockSide) {
  update((current) => ({
    ...mapDock(current, (dock) => withDockSide(dock, side, viewport())),
    lastSide: side,
  }));
}

export function setBrowserSize(size: number) {
  update((current) =>
    mapDock(current, (dock) => withDockSize(dock, size, viewport())),
  );
}

export function patchBrowserTab(
  fileId: string,
  patch: Partial<BrowserTabSource>,
) {
  update((current) => patchBrowserTabIn(current, fileId, patch));
}

/** Every native tab id this window should keep alive. */
export function browserTabIds(current = getBrowserState()): string[] {
  return current.docks.flatMap((dock) => dock.pane.files.map((file) => file.id));
}
