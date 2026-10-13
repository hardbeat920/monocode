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
  | "disconnected"
  /** Switch on, nothing running yet: the first agent action starts or connects. */
  | "idle";

export type BrowserKind = "chrome" | "edge" | "chromium";

export type BrowserInfo = {
  kind: BrowserKind;
  channel: string;
  product: string;
  major: number;
  user_data_dir: string;
  tabs_total: number;
  /** MonoCode launched this browser with its own profile. */
  managed: boolean;
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

/**
 * `managed`: MonoCode starts Chrome or Edge with its own profile.
 * `attach`: MonoCode connects to the browser the user runs.
 */
export type BrowserMode = "managed" | "private" | "attach";

/** MonoCode starts the browser itself (the MonoCode browser, kept or private). */
export function browserLaunches(mode: BrowserMode): boolean {
  return mode !== "attach";
}

/**
 * `ask`: a thread's first browser action asks the user in that thread.
 * `always`: every regular thread has access while the switch is on.
 */
export type BrowserAccessMode = "ask" | "always";

export type BrowserSettings = {
  /** Attach: which browser or profile. Managed: which executable to start. */
  browser: BrowserChoice;
  separate_window: boolean;
  limits: BrowserLimits;
  upload_roots: string[];
  allow_dotfiles: boolean;
  protocol_trace: boolean;
  mode: BrowserMode;
  access: BrowserAccessMode;
  /** The remembered main switch; `toggle_on` mirrors it. */
  enabled: boolean;
  /** Managed only; false runs the browser without a window. */
  show_window: boolean;
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
  /** An agent in this session waits for the user to allow the browser. */
  requested: boolean;
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
  mode: "managed",
  access: "ask",
  enabled: false,
  show_window: true,
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

/** What the MonoCode browser can start; Automatic tries Chrome, then Edge. */
export const MANAGED_BROWSER_CHANNELS = BROWSER_CHANNELS.slice(0, 2);

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
  registered.clear();
  turnedOff.clear();
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
/**
 * Threads the user took access away from in this app run. "Always on" does
 * not grant them again on their next turn; they ask like in "Ask" mode.
 */
const turnedOff = new Set<string>();

function markTurnedOff(sessionId: string, off: boolean) {
  if (off) turnedOff.add(sessionId);
  else turnedOff.delete(sessionId);
}

export function browserAccessTurnedOff(sessionId: string): boolean {
  return turnedOff.has(sessionId);
}

export const revokeBrowserSession = (sessionId: string) =>
  command("browser_revoke", { sessionId }).then((state) => {
    markTurnedOff(sessionId, true);
    return state;
  });
export const setSessionBrowserGrant = (sessionId: string, enabled: boolean) =>
  command("browser_set_session_grant", { sessionId, enabled }).then((state) => {
    markTurnedOff(sessionId, !enabled);
    return state;
  });
/**
 * The user's answer to a thread's browser request. Allow grants the thread;
 * Not now declines until the user's next message.
 */
export const answerBrowserRequest = (sessionId: string, allow: boolean) =>
  command("browser_answer_request", { sessionId, allow }).then((state) => {
    if (allow) markTurnedOff(sessionId, false);
    return state;
  });
/** Managed mode: starts the MonoCode browser with a window, for signing in. */
export const openBrowserWindow = () => command("browser_open_window");
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
// Agent side: MCP registration and the per-turn instructions (spec 12.2, 12.5)
// ---------------------------------------------------------------------------

/** Providers whose MCP configuration MonoCode writes the browser server into. */
export const BROWSER_MCP_PROVIDERS = [
  "claude",
  "codex",
  "cursor",
  "opencode",
] as const;

export type BrowserMcpRegistration = {
  provider: string;
  supported: boolean;
  registered: boolean;
  /** The entry starts this app; false means a moved or updated app left it. */
  current: boolean;
  configPath?: string;
  /** Why a provider that takes MCP still cannot use the tools (OpenCode 2). */
  unsupportedReason?: string;
};

export function listBrowserMcpStatus(): Promise<BrowserMcpRegistration[]> {
  return invoke<BrowserMcpRegistration[]>("browser_mcp_status");
}

export function registerBrowserMcp(
  provider: string,
  cwd: string,
): Promise<BrowserMcpRegistration> {
  return invoke<BrowserMcpRegistration>("browser_mcp_register", {
    provider,
    cwd,
  });
}

const registered = new Set<string>();

/**
 * Registers the browser server once per app run for a provider that takes
 * MCP, when one of its threads can use the browser. Registration is
 * idempotent; this only saves rewriting the provider's config on every turn.
 * A failure leaves the provider unmarked, so the next turn tries again.
 */
export async function ensureBrowserMcp(
  provider: string,
  cwd: string,
): Promise<void> {
  if (
    registered.has(provider) ||
    !(BROWSER_MCP_PROVIDERS as readonly string[]).includes(provider)
  )
    return;
  const result = await registerBrowserMcp(provider, cwd);
  // An unsupported provider (OpenCode 2) stays so for this run; asking again
  // on every turn would only rerun its version check.
  if (!result.supported || (result.registered && result.current))
    registered.add(provider);
}

/** The `<monocode_browser>` block a turn carries while it has the browser tools. */
export function browserAgentInstructions(provider: string): Promise<string> {
  return invoke<string>("browser_agent_instructions", { provider });
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

/**
 * Whether the session has access (`/browser`, Allow or Always on); false
 * while the toggle is off.
 */
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

/**
 * Where the user turns remote debugging on for the chosen or found browser.
 * Attach mode only; the MonoCode browser needs no setup.
 */
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
  /** Show the guided consent steps (spec 13.3). Attach mode only. */
  guide: boolean;
};

export function connectionView(state: BrowserState): ConnectionView {
  if (!browserLaunches(state.settings.mode)) return attachView(state);
  // The MonoCode browser has no consent step, so it never shows the guide.
  return { ...attachView(state), ...managedView(state), guide: false };
}

/** The states where the MonoCode browser reads differently from Your Chrome. */
function managedView(state: BrowserState): Partial<ConnectionView> {
  const { connection } = state;
  const isPrivate = state.settings.mode === "private";
  const name = isPrivate ? "private MonoCode browser" : "MonoCode browser";
  switch (connection.state) {
    case "discovering":
      return { message: `Starting the ${name}…` };
    case "idle":
      return {
        message: isPrivate
          ? `Ready. The ${name} starts when an agent needs it and forgets everything when it closes.`
          : `Ready. The ${name} starts when an agent needs it.`,
        action: null,
      };
    case "connected":
      return {
        message: connection.browser
          ? `The ${name} is running on ${browserProductLabel(connection.browser)}.`
          : `The ${name} is running.`,
      };
    case "disconnected":
      return {
        message: `The ${name} closed. It starts again when an agent needs it.`,
        tone: "neutral",
        action: null,
      };
    default:
      return {};
  }
}

function attachView(state: BrowserState): ConnectionView {
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
    case "idle":
      return {
        message: "Ready. MonoCode connects when an agent needs the browser.",
        tone: "neutral",
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
 * on and some session had access. Turning it off yourself is not a drop.
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
    "Browser access is off for all agents. Turn it on in Settings → Browser, then try again.",
  settings: true,
};
