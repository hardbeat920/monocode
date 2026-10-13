import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

/**
 * Frontend side of the browser module. Every shape here mirrors
 * `src-tauri/src/browser/protocol.rs` (serde field names, snake_case), and
 * all state comes from one event carrying the whole `BrowserState`, so the
 * UI keeps no rules of its own about grants, leases or the connection.
 */
export const BROWSER_STATE_EVENT = "monocode-browser-state";

export type BrowserConnectionState =
  | "off"
  | "discovering"
  | "needs_toggle"
  | "stale_port"
  | "awaiting_consent"
  | "consent_denied"
  | "consent_timeout"
  | "unsupported_version"
  | "policy_blocked"
  | "connected"
  | "disconnected";

export type BrowserKind = "chrome" | "edge" | "chromium";

export type BrowserInfo = {
  kind: BrowserKind;
  channel: string;
  product: string;
  major: number;
  user_data_dir: string;
  tabs_total: number;
};

export type BrowserConnection = {
  state: BrowserConnectionState;
  browser?: BrowserInfo;
  detail?: string;
  since_ms: number;
  last_error?: string;
};

export type BrowserChoice =
  | { kind: "auto" }
  | { kind: "channel"; channel: string }
  | { kind: "user_data_dir"; path: string };

export type BrowserLimits = {
  tabs_per_session: number;
  tabs_total: number;
  concurrent_actions: number;
};

export type BrowserSettings = {
  browser: BrowserChoice;
  separate_window: boolean;
  limits: BrowserLimits;
  upload_roots: string[];
  allow_dotfiles: boolean;
  protocol_trace: boolean;
};

export type BrowserTabState =
  "attaching" | "open" | "closed" | "crashed" | "revoked";

export type BrowserTab = {
  tab: string;
  url: string;
  title: string;
  state: BrowserTabState;
  opened_by?: string;
  dialog_pending: boolean;
  busy: boolean;
  created_at_ms: number;
  last_action_at_ms: number;
  session_id: string;
};

export type BrowserSessionAccess = {
  session_id: string;
  window: string;
  granted: boolean;
  turn_active: boolean;
  granted_since_ms?: number;
  tabs: number;
};

export type BrowserState = {
  toggle_on: boolean;
  connection: BrowserConnection;
  sessions: BrowserSessionAccess[];
  tabs: BrowserTab[];
  settings: BrowserSettings;
  global_epoch: { run: number; seq: number };
};

export type BrowserOutcome = "done" | "blocked" | "failed" | "unknown";

export type BrowserAuditEntry = {
  id: number;
  ts_ms: number;
  session_id: string;
  request_id: string;
  global_epoch: string;
  session_epoch: string;
  action: string;
  tab?: string;
  url?: string;
  params: unknown;
  outcome: BrowserOutcome;
  reason?: string;
  duration_ms: number;
  commands_sent: number;
};

export const DEFAULT_BROWSER_SETTINGS: BrowserSettings = {
  browser: { kind: "auto" },
  separate_window: false,
  limits: { tabs_per_session: 8, tabs_total: 24, concurrent_actions: 4 },
  upload_roots: [],
  allow_dotfiles: false,
  protocol_trace: false,
};

/** Channel ids discovery knows, in its own search order (spec 4). */
export const BROWSER_CHANNELS: { channel: string; label: string }[] = [
  { channel: "chrome", label: "Google Chrome" },
  { channel: "msedge", label: "Microsoft Edge" },
  { channel: "chrome-beta", label: "Chrome Beta" },
  { channel: "chrome-dev", label: "Chrome Dev" },
  { channel: "chrome-canary", label: "Chrome Canary" },
  { channel: "msedge-beta", label: "Edge Beta" },
  { channel: "msedge-dev", label: "Edge Dev" },
  { channel: "msedge-canary", label: "Edge Canary" },
];

export const BROWSER_AUDIT_LIMIT = 50;

// ---------------------------------------------------------------------------
// Store: the latest BrowserState, fed by the event and by command replies
// ---------------------------------------------------------------------------

let current: BrowserState | null = null;
let started = false;
/** "Disconnect" in Settings closes the link on purpose; that is no drop. */
let disconnecting = false;
const listeners = new Set<() => void>();

function publish(next: BrowserState) {
  const previous = current;
  current = next;
  const drop = connectionDropNotice(previous, next);
  if (drop && !disconnecting) showBrowserNotice(drop);
  if (next.connection.state !== "connected") disconnecting = false;
  for (const listener of listeners) listener();
}

function start() {
  if (started || typeof isTauri !== "function" || !isTauri()) return;
  started = true;
  void listen<BrowserState>(BROWSER_STATE_EVENT, (event) =>
    publish(event.payload),
  ).catch(() => {});
  void invoke<BrowserState>("browser_status")
    .then((state) => {
      // An event that already arrived is at least as new as this reply.
      if (!current) publish(state);
    })
    .catch(() => {});
}

export function subscribeBrowserState(listener: () => void): () => void {
  start();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function browserStateSnapshot(): BrowserState | null {
  return current;
}

export function useBrowserState(): BrowserState | null {
  return useSyncExternalStore(
    subscribeBrowserState,
    browserStateSnapshot,
    () => null,
  );
}

/** Test seam. */
export function resetBrowserStore(state: BrowserState | null = null): void {
  current = state;
  started = state != null;
  disconnecting = false;
  notice = null;
  for (const listener of listeners) listener();
  for (const listener of noticeListeners) listener();
}

async function command(
  name: string,
  args?: Record<string, unknown>,
): Promise<BrowserState> {
  const state = await invoke<BrowserState>(name, args);
  publish(state);
  return state;
}

export const setBrowserToggle = (on: boolean) =>
  command("browser_toggle", { on });
export const connectBrowser = () => command("browser_connect");
export const disconnectBrowser = () => {
  disconnecting = true;
  return command("browser_disconnect").catch((error: unknown) => {
    disconnecting = false;
    throw error;
  });
};
export const revokeBrowserSession = (sessionId: string) =>
  command("browser_revoke", { sessionId });
export const setSessionBrowserGrant = (sessionId: string, enabled: boolean) =>
  command("browser_set_session_grant", { sessionId, enabled });
export const transferBrowserTab = (
  tab: string,
  fromSession: string,
  toSession: string,
) => command("browser_transfer", { tab, fromSession, toSession });
/** "Take back": the agent loses the tab, its marker goes, the tab stays open. */
export const releaseBrowserTab = (tab: string) =>
  command("browser_release_tab", { tab });
export const closeBrowserTab = (tab: string) =>
  command("browser_close_tab", { tab });
export const closeAllBrowserTabs = () => command("browser_close_all_tabs");
export const saveBrowserSettings = (settings: BrowserSettings) =>
  command("browser_set_settings", { settings });

export function listBrowserAudit(
  sessionId?: string,
  limit = BROWSER_AUDIT_LIMIT,
): Promise<BrowserAuditEntry[]> {
  return invoke<BrowserAuditEntry[]>("browser_audit_list", {
    sessionId: sessionId ?? null,
    limit,
  });
}

export function clearBrowserAudit(): Promise<void> {
  return invoke<void>("browser_audit_clear");
}

// ---------------------------------------------------------------------------
// Derived views
// ---------------------------------------------------------------------------

export function sessionBrowserAccess(
  state: BrowserState | null,
  sessionId: string | undefined,
): BrowserSessionAccess | undefined {
  if (!state || !sessionId) return undefined;
  return state.sessions.find((entry) => entry.session_id === sessionId);
}

/** Whether `/browser` is on for the session; false while the toggle is off. */
export function sessionBrowserGranted(
  state: BrowserState | null,
  sessionId: string | undefined,
): boolean {
  return (
    !!state?.toggle_on && !!sessionBrowserAccess(state, sessionId)?.granted
  );
}

export function sessionBrowserTabs(
  state: BrowserState | null,
  sessionId: string | undefined,
): BrowserTab[] {
  if (!state || !sessionId) return [];
  return state.tabs.filter((tab) => tab.session_id === sessionId);
}

/** The three indicator states of spec 13.2; null hides the indicator. */
export type SessionBrowserIndicator = "granted" | "busy" | "dialog";

export function sessionBrowserIndicator(
  state: BrowserState | null,
  sessionId: string | undefined,
): SessionBrowserIndicator | null {
  if (!sessionBrowserGranted(state, sessionId)) return null;
  const tabs = sessionBrowserTabs(state, sessionId);
  if (tabs.some((tab) => tab.dialog_pending)) return "dialog";
  if (tabs.some((tab) => tab.busy)) return "busy";
  return "granted";
}

/** A tab's status the way Settings lists it (spec 13.1). */
export function browserTabStatus(tab: BrowserTab): string {
  if (tab.state === "attaching") return "Opening";
  if (tab.state === "crashed") return "Crashed";
  if (tab.state === "revoked") return "Released";
  if (tab.dialog_pending) return "Waiting on a dialog";
  if (tab.busy) return "Action running";
  return "Idle";
}

export function browserTabHost(url: string): string {
  if (!url || url === "about:blank") return "about:blank";
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

export function browserChannelLabel(channel: string): string {
  return (
    BROWSER_CHANNELS.find((entry) => entry.channel === channel)?.label ??
    channel
  );
}

/** Where the user turns remote debugging on for the chosen or found browser. */
export function inspectPages(
  state: Pick<BrowserState, "connection" | "settings">,
): { label: string; url: string }[] {
  const chrome = {
    label: "Chrome",
    url: "chrome://inspect/#remote-debugging",
  };
  const edge = { label: "Edge", url: "edge://inspect" };
  const found = state.connection.browser?.kind;
  if (found === "edge") return [edge];
  if (found) return [chrome];
  const choice = state.settings.browser;
  if (choice.kind === "channel")
    return choice.channel.startsWith("msedge") ? [edge] : [chrome];
  return [chrome, edge];
}

export type ConnectionAction = "connect" | "retry" | "disconnect" | null;

export type ConnectionView = {
  /** One sentence for the state (spec 4 "Poruka u UI-u"). */
  message: string;
  tone: "neutral" | "pending" | "good" | "warning" | "error";
  action: ConnectionAction;
  /** Show the guided consent steps (spec 13.3). */
  guide: boolean;
};

export function connectionView(state: BrowserState): ConnectionView {
  const { connection } = state;
  switch (connection.state) {
    case "off":
      return {
        message: "Browser access is off.",
        tone: "neutral",
        action: null,
        guide: false,
      };
    case "discovering":
      return {
        message: "Looking for Chrome or Edge…",
        tone: "pending",
        action: null,
        guide: false,
      };
    case "needs_toggle":
      return {
        message: "Remote debugging is not turned on in the browser yet.",
        tone: "warning",
        action: "retry",
        guide: true,
      };
    case "stale_port":
      return {
        message:
          "The browser is not running, or it left an old debugging record behind.",
        tone: "warning",
        action: "retry",
        guide: false,
      };
    case "awaiting_consent":
      return {
        message: "Confirm the connection in your browser.",
        tone: "pending",
        action: null,
        guide: true,
      };
    case "consent_denied":
      return {
        message:
          "The browser denied the connection. Connect again when you are ready.",
        tone: "warning",
        action: "connect",
        guide: true,
      };
    case "consent_timeout":
      return {
        message:
          "Nobody answered the browser's prompt in time. Connect again when you are ready.",
        tone: "warning",
        action: "connect",
        guide: true,
      };
    case "unsupported_version":
      return {
        message: "Chrome or Edge 144 or newer is required.",
        tone: "error",
        action: "retry",
        guide: false,
      };
    case "policy_blocked":
      return {
        message:
          "An administrator policy blocks remote debugging in this browser. Contact your administrator.",
        tone: "error",
        action: null,
        guide: false,
      };
    case "connected":
      return {
        message: connection.browser
          ? `Connected to ${browserProductLabel(connection.browser)}.`
          : "Connected.",
        tone: "good",
        action: "disconnect",
        guide: false,
      };
    case "disconnected":
      return {
        message:
          "The connection ended. The next connection asks for permission in the browser again.",
        tone: "warning",
        action: "connect",
        guide: false,
      };
  }
}

/** `Chrome/144.0.7559.60` → `Chrome 144.0.7559.60`, Edge spelled out. */
export function browserProductLabel(browser: BrowserInfo): string {
  const [name, version] = browser.product.split("/");
  const label =
    browser.kind === "edge"
      ? "Microsoft Edge"
      : browser.kind === "chrome"
        ? "Google Chrome"
        : name || "Chromium";
  return version ? `${label} ${version}` : label;
}

// ---------------------------------------------------------------------------
// In-app notices (spec 13.4: no OS notifications, one toast on a dropped link)
// ---------------------------------------------------------------------------

export type BrowserNotice = {
  id: number;
  message: string;
  /** Offer the Settings page as the next step. */
  settings: boolean;
};

let notice: BrowserNotice | null = null;
let noticeSeq = 0;
const noticeListeners = new Set<() => void>();

export function showBrowserNotice(
  input: Omit<BrowserNotice, "id">,
): BrowserNotice {
  notice = { ...input, id: ++noticeSeq };
  for (const listener of noticeListeners) listener();
  return notice;
}

export function dismissBrowserNotice(id?: number): void {
  if (!notice || (id != null && notice.id !== id)) return;
  notice = null;
  for (const listener of noticeListeners) listener();
}

function subscribeBrowserNotice(listener: () => void): () => void {
  noticeListeners.add(listener);
  return () => noticeListeners.delete(listener);
}

export function browserNoticeSnapshot(): BrowserNotice | null {
  return notice;
}

export function useBrowserNotice(): BrowserNotice | null {
  return useSyncExternalStore(
    subscribeBrowserNotice,
    browserNoticeSnapshot,
    () => null,
  );
}

/**
 * The one toast of spec 13.4: the link to the browser dropped while it was
 * on and some session had `/browser`. Turning it off yourself is not a drop.
 */
export function connectionDropNotice(
  previous: BrowserState | null,
  next: BrowserState,
): Omit<BrowserNotice, "id"> | null {
  if (!previous || previous.connection.state !== "connected") return null;
  if (!next.toggle_on || next.connection.state === "connected") return null;
  if (!previous.sessions.some((session) => session.granted)) return null;
  return {
    message:
      "MonoCode lost its connection to the browser. Agents can't use it until you connect again from Settings.",
    settings: true,
  };
}

export const BROWSER_OFF_NOTICE: Omit<BrowserNotice, "id"> = {
  message:
    "Browser access is off. Turn it on in Settings → Browser, then use /browser again.",
  settings: true,
};
