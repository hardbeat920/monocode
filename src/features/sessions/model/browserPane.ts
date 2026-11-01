import type { BrowserPanePlacement } from "../../workspace/model/layout";
import {
  browserStateSnapshot,
  subscribeBrowserState,
  type BrowserState,
} from "../../settings/model/browser";

/** "Show in MonoCode": a thread asks the workspace to open one of its browser tabs in a pane. */
export type BrowserPaneRequest = {
  sessionId: string;
  tab: string;
  title: string;
  /**
   * Opened on its own ("Show agent tabs: In MonoCode"), not by the user: only
   * beside the thread where it is open, and without taking focus.
   */
  auto?: boolean;
  /** Opened by the user in a pane ("New tab"); see `BrowserPaneSource.own`. */
  own?: boolean;
};

let pending: BrowserPaneRequest[] = [];
const listeners = new Set<() => void>();

export function requestBrowserPane(request: BrowserPaneRequest): void {
  pending = [...pending, request];
  for (const listener of listeners) listener();
}

/** The requests waiting for the workspace, which takes them once. */
export function takeBrowserPaneRequests(): BrowserPaneRequest[] {
  const requests = pending;
  pending = [];
  return requests;
}

export function subscribeBrowserPaneRequests(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * The thread tabs that just opened, by the browser state before and after.
 * Only tabs that are open now and were not open before count.
 */
export function newlyOpenBrowserTabs(
  before: BrowserState,
  after: BrowserState,
): BrowserState["tabs"] {
  const open = new Set(
    before.tabs.filter((tab) => tab.state === "open").map((tab) => tab.tab),
  );
  return after.tabs.filter((tab) => tab.state === "open" && !open.has(tab.tab));
}

/**
 * "Show agent tabs: In MonoCode": each tab a thread gets opens beside it.
 * Tabs open when this starts are left alone. Returns the unsubscribe.
 */
export function openNewBrowserTabsInPanes(): () => void {
  let last = browserStateSnapshot();
  return subscribeBrowserState(() => {
    const next = browserStateSnapshot();
    if (!next) return;
    // The first state after start-up names tabs that were already open.
    const opened = last ? newlyOpenBrowserTabs(last, next) : [];
    last = next;
    if (!next.settings.tabs_in_monocode) return;
    for (const tab of opened) {
      requestBrowserPane({
        sessionId: tab.session_id,
        tab: tab.tab,
        title: tab.title,
        auto: true,
      });
    }
  });
}

const PLACEMENT_KEY = "monocode.browserPanePlacement";

/** Where the user last left a browser pane; `null` until they have. */
export function loadBrowserPanePlacement(): BrowserPanePlacement | null {
  try {
    const raw = JSON.parse(localStorage.getItem(PLACEMENT_KEY) ?? "null");
    if (
      raw &&
      ["left", "right", "top", "bottom"].includes(raw.edge) &&
      typeof raw.size === "number" &&
      raw.size > 0 &&
      raw.size < 1
    ) {
      return { edge: raw.edge, size: raw.size };
    }
  } catch {
    // Storage can be missing or blocked; the pane opens on the right.
  }
  return null;
}

export function saveBrowserPanePlacement(
  placement: BrowserPanePlacement,
): void {
  try {
    localStorage.setItem(PLACEMENT_KEY, JSON.stringify(placement));
  } catch {
    // Not remembered this time.
  }
}

const shown = new Map<string, number>();
let picking: string | null = null;
const pickListeners = new Set<() => void>();

/** A pane shows `tab` live now; call the result when it no longer does. */
export function markBrowserTabShown(tab: string): () => void {
  shown.set(tab, (shown.get(tab) ?? 0) + 1);
  return () => {
    const left = (shown.get(tab) ?? 1) - 1;
    if (left > 0) shown.set(tab, left);
    else shown.delete(tab);
  };
}

export function browserTabShown(tab: string): boolean {
  return shown.has(tab);
}

/** The tab where "Point at an element" waits for the user's click. */
export function setBrowserPickTab(tab: string | null): void {
  if (picking === tab) return;
  picking = tab;
  for (const listener of pickListeners) listener();
}

export function browserPickTab(): string | null {
  return picking;
}

export function subscribeBrowserPick(listener: () => void): () => void {
  pickListeners.add(listener);
  return () => pickListeners.delete(listener);
}
