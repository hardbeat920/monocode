import { useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  addTerminalToDock,
  clampDockSize,
  closeTerminalInDock,
  isDockSide,
  reorderDockTerminals,
  selectDockTerminal,
  withDockOpen,
  withDockSide,
  withDockSize,
  type DockSide,
  type DockState,
} from "../../projects/model/projectTerminal";
import { newEditorPane } from "../../workspace/model/layout";
import type {
  BrowserTabSource,
  FilePaneTab,
} from "../../workspace/model/layout";
import { BLANK_URL } from "./browserUrl";

/** One session's browser: its tabs and where its panel sits. */
export type BrowserDock = DockState & { sessionId: string };

export type BrowserState = {
  docks: BrowserDock[];
  /** Session the window is showing; "" when none. Its dock is the visible one. */
  sessionId: string;
  lastSide: DockSide;
  /**
   * Tabs an agent used recently, by last use. They stay live while out of
   * view so the agent can keep working. Not persisted.
   */
  agentTabs: Record<string, number>;
  /**
   * When each tab was last on screen or used by an agent. Out-of-view pages
   * stay live, most recent first, up to MAX_LIVE_TABS. Not persisted.
   */
  recent: Record<string, number>;
};

export const BROWSER_DEFAULT_SIDE: DockSide = "right";
/** How long an agent's tab stays live out of view after its last use. */
export const AGENT_TAB_TTL_MS = 3 * 60_000;
/**
 * Pages kept running at once. Each is a browser process, so past this the
 * least recently viewed tabs of other sessions unload and reload from their
 * URL when shown again. The focused session's tabs and agent tabs always stay.
 */
export const MAX_LIVE_TABS = 12;
const DEFAULT_SIZE = { horizontal: 520, vertical: 320 };
const STORAGE_PREFIX = "monocode.browser.v2:";

export function browserDefaultSize(side: DockSide): number {
  return side === "left" || side === "right"
    ? DEFAULT_SIZE.horizontal
    : DEFAULT_SIZE.vertical;
}

export function newBrowserTab(url: string): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: url,
    cwd: "",
    browser: { url },
  };
}

// ---- pure transitions --------------------------------------------------

function viewport() {
  return typeof window === "undefined"
    ? undefined
    : { width: window.innerWidth, height: window.innerHeight };
}

export function findBrowserDock(
  docks: BrowserDock[],
  sessionId: string,
): BrowserDock | undefined {
  return sessionId
    ? docks.find((dock) => dock.sessionId === sessionId)
    : undefined;
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
  sessionId: string,
  update: (dock: BrowserDock) => BrowserDock | null,
): BrowserState {
  let changed = false;
  const docks: BrowserDock[] = [];
  for (const dock of state.docks) {
    if (dock.sessionId !== sessionId) {
      docks.push(dock);
      continue;
    }
    const next = update(dock);
    if (next !== dock) changed = true;
    if (next) docks.push(next);
  }
  return changed ? { ...state, docks } : state;
}

/** Map the dock that holds `fileId`. */
function mapDockOfTab(
  state: BrowserState,
  fileId: string,
  update: (dock: BrowserDock) => BrowserDock | null,
): BrowserState {
  const dock = dockOfTab(state, fileId);
  return dock ? mapDock(state, dock.sessionId, update) : state;
}

export function addBrowserTab(
  state: BrowserState,
  sessionId: string,
  file: FilePaneTab,
  options: { afterId?: string; open?: boolean } = {},
): BrowserState {
  const existing = findBrowserDock(state.docks, sessionId);
  if (!existing) {
    const side = state.lastSide;
    const dock: BrowserDock = {
      sessionId,
      pane: newEditorPane(file),
      side,
      size: clampDockSize(side, browserDefaultSize(side), viewport()),
      open: options.open ?? true,
    };
    return { ...state, docks: [...state.docks, dock] };
  }
  return mapDock(state, sessionId, (dock) => {
    let next = addTerminalToDock(dock, file);
    if (options.open === false) next = { ...next, open: dock.open };
    const at = options.afterId
      ? dock.pane.files.findIndex((entry) => entry.id === options.afterId)
      : -1;
    if (at < 0) return next;
    const files = [...dock.pane.files];
    files.splice(at + 1, 0, file);
    return { ...next, pane: { ...next.pane, files } };
  });
}

/** The status bar toggle: show or hide the current session's browser. */
export function toggleBrowserDock(state: BrowserState): BrowserState {
  if (!state.sessionId) return state;
  if (!findBrowserDock(state.docks, state.sessionId)) {
    return addBrowserTab(state, state.sessionId, newBrowserTab(BLANK_URL));
  }
  return mapDock(state, state.sessionId, (dock) =>
    withDockOpen(dock, !dock.open),
  );
}

export function patchBrowserTabIn(
  state: BrowserState,
  fileId: string,
  patch: Partial<BrowserTabSource>,
): BrowserState {
  return mapDockOfTab(state, fileId, (dock) => {
    let changed = false;
    const files = dock.pane.files.map((file) => {
      if (file.id !== fileId || !file.browser) return file;
      const browser = { ...file.browser, ...patch };
      if (
        browser.url === file.browser.url &&
        browser.title === file.browser.title &&
        browser.loading === file.browser.loading &&
        browser.error === file.browser.error
      ) {
        return file;
      }
      changed = true;
      return { ...file, path: browser.url, browser };
    });
    return changed ? { ...dock, pane: { ...dock.pane, files } } : dock;
  });
}

/** Drop the browsers of sessions that no longer exist. */
export function forgetBrowserSessionsIn(
  state: BrowserState,
  sessionIds: Iterable<string>,
): BrowserState {
  const gone = new Set(sessionIds);
  if (!state.docks.some((dock) => gone.has(dock.sessionId))) return state;
  return {
    ...state,
    docks: state.docks.filter((dock) => !gone.has(dock.sessionId)),
  };
}

export function touchAgentTabIn(
  state: BrowserState,
  fileId: string,
  now: number,
): BrowserState {
  return {
    ...state,
    agentTabs: { ...state.agentTabs, [fileId]: now },
    recent: { ...state.recent, [fileId]: now },
  };
}

/** Focus another session; the one leaving keeps its pages, newest first. */
export function switchBrowserSessionIn(
  state: BrowserState,
  sessionId: string,
  now: number,
): BrowserState {
  if (state.sessionId === sessionId) return state;
  const leaving = findBrowserDock(state.docks, state.sessionId);
  const recent = { ...state.recent };
  for (const file of leaving?.pane.files ?? []) recent[file.id] = now;
  return { ...state, sessionId, recent };
}

export function pruneAgentTabsIn(
  state: BrowserState,
  now: number,
  ttl = AGENT_TAB_TTL_MS,
): BrowserState {
  const open = new Set(browserTabIds(state));
  const entries = Object.entries(state.agentTabs);
  const kept = entries.filter(
    ([id, used]) => open.has(id) && now - used < ttl,
  );
  if (kept.length === entries.length) return state;
  return { ...state, agentTabs: Object.fromEntries(kept) };
}

/**
 * Tabs whose native page should exist: every tab of the session on screen
 * and tabs an agent is using, then other sessions' tabs by how recently they
 * were seen, up to `limit`. The rest are suspended to their URL.
 */
export function liveBrowserTabIds(
  state: BrowserState,
  limit = MAX_LIVE_TABS,
): Set<string> {
  const open = new Set(browserTabIds(state));
  const live = new Set(
    Object.keys(state.agentTabs).filter((id) => open.has(id)),
  );
  const current = findBrowserDock(state.docks, state.sessionId);
  for (const file of current?.pane.files ?? []) live.add(file.id);
  const others = Object.entries(state.recent)
    .filter(([id]) => open.has(id) && !live.has(id))
    .sort((a, b) => b[1] - a[1]);
  for (const [id] of others) {
    if (live.size >= limit) break;
    live.add(id);
  }
  return live;
}

/** Every tab this window knows about. */
export function browserTabIds(state: BrowserState): string[] {
  return state.docks.flatMap((dock) => dock.pane.files.map((file) => file.id));
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
        if (!dock || typeof dock.sessionId !== "string" || !dock.sessionId) {
          return [];
        }
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
            sessionId: dock.sessionId,
            side: dock.side,
            size: clampDockSize(dock.side, Number(dock.size)),
            open: !!dock.open,
            pane: {
              id: dock.pane.id || crypto.randomUUID(),
              files,
              activeFileId,
            },
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

// Per window, like terminals: each window keeps its own sessions' browsers.
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
    sessionId: "",
    lastSide: stored.lastSide ?? BROWSER_DEFAULT_SIDE,
    agentTabs: {},
    recent: {},
  };
}

let state: BrowserState | null = null;
const listeners = new Set<() => void>();
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let pruneTimer: ReturnType<typeof setTimeout> | undefined;

export function getBrowserState(): BrowserState {
  state ??= initialState();
  return state;
}

function setState(next: BrowserState) {
  const previous = getBrowserState();
  if (next === previous) return;
  state = next;
  for (const listener of listeners) listener();
  if (next.docks === previous.docks && next.lastSide === previous.lastSide) {
    return;
  }
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

export function setBrowserSession(sessionId: string) {
  update((current) => switchBrowserSessionIn(current, sessionId, Date.now()));
}

export function toggleBrowser() {
  update(toggleBrowserDock);
}

export function hideBrowser() {
  update((current) =>
    mapDock(current, current.sessionId, (dock) => withDockOpen(dock, false)),
  );
}

/**
 * Open `url` in a new tab of a session's browser (the one on screen unless
 * given). A user's tab shows the panel; an agent's (`background`) does not.
 * Returns the tab id, or null when there is no session to hold it.
 */
export function openBrowserTab(
  url: string = BLANK_URL,
  options: { sessionId?: string; afterId?: string; background?: boolean } = {},
): string | null {
  const sessionId = options.sessionId ?? getBrowserState().sessionId;
  if (!sessionId) return null;
  const file = newBrowserTab(url);
  const open = options.background ? false : true;
  update((current) => {
    const next = addBrowserTab(current, sessionId, file, {
      afterId: options.afterId,
      open,
    });
    return options.background
      ? next
      : mapDock(next, sessionId, (dock) => withDockOpen(dock, true));
  });
  return file.id;
}

export function selectBrowserTab(fileId: string) {
  update((current) =>
    mapDockOfTab(current, fileId, (dock) => selectDockTerminal(dock, fileId)),
  );
}

export function closeBrowserTab(fileId: string) {
  update((current) =>
    mapDockOfTab(current, fileId, (dock) => closeTerminalInDock(dock, fileId)),
  );
}

export function closeOtherBrowserTabs(fileId: string) {
  update((current) =>
    mapDockOfTab(current, fileId, (dock) => {
      const keep = dock.pane.files.filter((file) => file.id === fileId);
      return {
        ...dock,
        pane: { ...dock.pane, files: keep, activeFileId: fileId },
      };
    }),
  );
}

export function reorderBrowserTabs(ids: string[]) {
  update((current) =>
    mapDock(current, current.sessionId, (dock) => {
      const byId = new Map(dock.pane.files.map((file) => [file.id, file]));
      const files = ids.flatMap((id) => byId.get(id) ?? []);
      if (files.length !== dock.pane.files.length) return dock;
      return reorderDockTerminals(dock, files);
    }),
  );
}

export function setBrowserSide(side: DockSide) {
  update((current) => ({
    ...mapDock(current, current.sessionId, (dock) =>
      withDockSide(dock, side, viewport()),
    ),
    lastSide: side,
  }));
}

export function setBrowserSize(size: number) {
  update((current) =>
    mapDock(current, current.sessionId, (dock) =>
      withDockSize(dock, size, viewport()),
    ),
  );
}

export function patchBrowserTab(
  fileId: string,
  patch: Partial<BrowserTabSource>,
) {
  update((current) => patchBrowserTabIn(current, fileId, patch));
}

export function forgetBrowserSessions(sessionIds: Iterable<string>) {
  update((current) => forgetBrowserSessionsIn(current, sessionIds));
}

/** Keep an agent's tab live out of view for a while. */
export function touchAgentTab(fileId: string) {
  update((current) => touchAgentTabIn(current, fileId, Date.now()));
  clearTimeout(pruneTimer);
  pruneTimer = setTimeout(function prune() {
    update((current) => pruneAgentTabsIn(current, Date.now()));
    if (Object.keys(getBrowserState().agentTabs).length > 0) {
      pruneTimer = setTimeout(prune, 30_000);
    }
  }, 30_000);
}
