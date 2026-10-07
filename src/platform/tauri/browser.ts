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

export function navigateBrowser(id: string, url: string): Promise<void> {
  return invoke("browser_navigate", { id, url });
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
