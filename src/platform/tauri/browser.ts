import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type BrowserBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type BrowserEvent =
  | { kind: "load"; id: string; url: string; loading: boolean }
  | { kind: "title"; id: string; title: string }
  | { kind: "openTab"; id: string; url: string };

export function openBrowserView(
  id: string,
  url: string,
  bounds: BrowserBounds,
  visible: boolean,
): Promise<void> {
  return invoke("browser_open", { id, url, bounds, visible });
}

/** The page's current URL, including same-document (pushState, hash) changes. */
export function readBrowserUrl(id: string): Promise<string> {
  return invoke("browser_url", { id });
}

export function setBrowserBounds(
  id: string,
  bounds: BrowserBounds,
): Promise<void> {
  return invoke("browser_set_bounds", { id, bounds });
}

export function setBrowserVisible(id: string, visible: boolean): Promise<void> {
  return invoke("browser_set_visible", { id, visible });
}

export function focusBrowserView(id: string): Promise<void> {
  return invoke("browser_focus", { id });
}

export type BrowserNavigation = {
  /**
   * - applied: the page stayed in its document; no load follows, and `url` is
   *   where its synchronous handlers left it.
   * - document: the webview was told to load a document; load events report
   *   what happens next, though one that fails before committing may report none.
   * - unknown: not known whether a load follows (the page refused the target,
   *   replaced it with a load not yet committed, or did not answer).
   */
  outcome: "applied" | "document" | "unknown";
  url?: string;
};

/** Per tab, the last navigation handed to the native side, settled or not. */
const dispatching = new Map<string, Promise<unknown>>();

/**
 * Navigations for one tab reach the native side one at a time, so the page
 * applies them in the order they were made whatever order replies come in.
 * `onDispatch` runs when this navigation's turn comes, before it is sent: the
 * moment to read state (such as load counters) that earlier navigations of the
 * tab can still change while this one waits. A promise it returns holds the
 * navigation, and the tab's later ones, until it settles.
 */
export function navigateBrowser(
  id: string,
  url: string,
  onDispatch?: () => void | Promise<void>,
): Promise<BrowserNavigation> {
  const turn = (dispatching.get(id) ?? Promise.resolve()).then(async () => {
    await onDispatch?.();
    return invoke<BrowserNavigation>("browser_navigate", { id, url });
  });
  const tail = turn.then(
    () => undefined,
    () => undefined,
  );
  dispatching.set(id, tail);
  void tail.then(() => {
    if (dispatching.get(id) === tail) dispatching.delete(id);
  });
  return turn;
}

export function browserHistory(
  id: string,
  action: "back" | "forward" | "reload" | "stop",
): Promise<void> {
  return invoke("browser_history", { id, action });
}

export function closeBrowserView(id: string): Promise<void> {
  return invoke("browser_close", { id });
}

/** Close this window's native tabs that are not in `ids`. */
export function retainBrowserViews(ids: string[]): Promise<void> {
  return invoke("browser_retain", { ids });
}

export function onBrowserEvent(
  handler: (event: BrowserEvent) => void,
): Promise<UnlistenFn> {
  return listen<BrowserEvent>("browser-event", (event) =>
    handler(event.payload),
  );
}

/** Run an agent script in a tab; resolves with its JSON result. */
export function evalInBrowser(
  id: string,
  script: string,
  timeoutMs?: number,
): Promise<unknown> {
  return invoke("browser_eval", { id, script, timeoutMs });
}

/** The visible part of a tab as base64 PNG. */
export function screenshotBrowser(id: string): Promise<string> {
  return invoke("browser_screenshot", { id });
}

/** How a provider launches the browser MCP server for a session. */
export type BrowserMcpLaunch = {
  command: string;
  args: string[];
  env: Record<string, string>;
};

export function browserMcpLaunch(
  sessionId: string,
): Promise<BrowserMcpLaunch | null> {
  return invoke("control_browser_mcp", { sessionId });
}
