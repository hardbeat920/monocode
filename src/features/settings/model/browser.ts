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

export type BrowserKind =
  "chrome" | "edge" | "chromium" | "brave" | "vivaldi" | "opera" | "arc";

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
 * `managed`: MonoCode starts an installed Chromium browser with its own profile.
 * `private`: like `managed`, with a fresh profile deleted when it closes.
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
  /** A thread's new tab opens in a pane beside the thread. */
  tabs_in_monocode: boolean;
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
  /**
   * The user turned this session's access off. "Always on" and "Full
   * access" do not grant it again; it asks until the user allows it.
   */
  turned_off: boolean;
};

export type BrowserState = {
  toggle_on: boolean;
  connection: BrowserConnection;
  sessions: BrowserSessionAccess[];
  tabs: BrowserTab[];
  settings: BrowserSettings;
  global_epoch: { run: number; seq: number };
};

/** A web page in the browser that no thread holds (`targets::FreeTab`). */
export type FreeBrowserTab = {
  /** `f_` + 8 hex; valid while the page stays free. */
  id: string;
  url: string;
  title: string;
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
  tabs_in_monocode: false,
};

/** Channel ids discovery knows, in its own search order. */
export const BROWSER_CHANNELS: { channel: string; label: string }[] = [
  { channel: "chrome", label: "Google Chrome" },
  { channel: "msedge", label: "Microsoft Edge" },
  { channel: "chrome-beta", label: "Chrome Beta" },
  { channel: "chrome-dev", label: "Chrome Dev" },
  { channel: "chrome-canary", label: "Chrome Canary" },
  { channel: "msedge-beta", label: "Edge Beta" },
  { channel: "msedge-dev", label: "Edge Dev" },
  { channel: "msedge-canary", label: "Edge Canary" },
  { channel: "brave", label: "Brave" },
  { channel: "vivaldi", label: "Vivaldi" },
  { channel: "opera", label: "Opera" },
  { channel: "arc", label: "Arc (macOS)" },
];

/**
 * What the MonoCode browser can start; Automatic tries Chrome, then Edge,
 * Brave, Vivaldi, Opera and Arc.
 */
export const MANAGED_BROWSER_CHANNELS = BROWSER_CHANNELS.filter(
  (entry) => !/-(beta|dev|canary)$/.test(entry.channel),
);

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
  registering.clear();
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
 * Whether the user turned the thread's access off (Settings "Revoke",
 * `/browser` or the indicator's "Turn off"). The backend keeps the mark until
 * the user grants the thread again, so it holds in every window.
 */
export function browserAccessTurnedOff(
  state: BrowserState | null,
  sessionId: string,
): boolean {
  return !!sessionBrowserAccess(state, sessionId)?.turned_off;
}

/** Settings "Revoke": access off, marked as the user's choice. */
export const revokeBrowserSession = (sessionId: string) =>
  command("browser_revoke", { sessionId });
/** On clears the "turned off" mark, off sets it. */
export const setSessionBrowserGrant = (sessionId: string, enabled: boolean) =>
  command("browser_set_session_grant", { sessionId, enabled });
/**
 * The user's answer to a thread's browser request. Allow grants the thread;
 * Not now declines until the user's next message.
 */
export const answerBrowserRequest = (sessionId: string, allow: boolean) =>
  command("browser_answer_request", { sessionId, allow });
/**
 * An answer from away from the thread (the phone): only while the request
 * still waits, so a late tap never grants a thread that is no longer asking.
 */
export async function answerPendingBrowserRequest(
  sessionId: string,
  allow: boolean,
): Promise<BrowserState> {
  if (!browserRequestPending(current, sessionId))
    throw new Error("The agent is no longer waiting for the browser");
  return answerBrowserRequest(sessionId, allow);
}

/**
 * The access requests this window declines without asking: requests of the
 * sessions whose turns this window runs (`access.window`), except those
 * `leave` keeps (a turn that may use the browser, a decline in flight).
 * Another window answers for its own sessions.
 */
export function browserRequestsToDecline(
  state: BrowserState | null,
  window: string,
  leave: (sessionId: string) => boolean,
): string[] {
  return (state?.sessions ?? [])
    .filter(
      (access) =>
        access.requested &&
        access.window === window &&
        !leave(access.session_id),
    )
    .map((access) => access.session_id);
}
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

/**
 * The http(s) pages in the connected browser that no thread holds. Fails
 * with the backend's message while MonoCode is not connected.
 */
export function listUserBrowserTabs(): Promise<FreeBrowserTab[]> {
  return invoke<FreeBrowserTab[]>("browser_user_tabs");
}

/**
 * The user hands a free page to a thread. The thread gets browser access
 * with it, so a thread the user turned off counts as allowed again.
 */
export const giveBrowserTab = (free: string, sessionId: string) =>
  command("browser_give_tab", { free, sessionId });
/** A local dev server of a thread's checkout (`devservers::DevServer`). */
export type BranchApp = {
  port: number;
  /** `http://localhost:<port>`. */
  url: string;
  pid: number;
  process: string;
  checkout: string;
  /** `/` answers with HTML: an app to open, not only an API. */
  page: boolean;
};

/** Dev servers the checkout holding `cwd` runs (its worktree's app). */
export function listBranchApps(cwd: string): Promise<BranchApp[]> {
  return invoke<BranchApp[]>("dev_servers", { cwd });
}

/**
 * "Open this branch's app": a new tab of the thread at the app's address.
 * Like handing over a tab, it gives the thread browser access.
 */
export const openBranchApp = (sessionId: string, url: string) =>
  command("browser_open_app", { sessionId, url });
/** An element the user picked in a thread's tab (`pick::PickedElement`). */
export type PickedBrowserElement = {
  tab: string;
  url: string;
  role: string;
  name: string;
  tag: string;
  /** `e12` when a snapshot already gave the element a ref. */
  ref?: string;
  /** PNG of the element, base64. */
  image?: string;
  /** The line for the agent. */
  text: string;
};

/**
 * "Point at an element": the browser highlights what the pointer is over in
 * `tab` until the user clicks. `null` when the pick was cancelled.
 */
/** `inPane`: the user clicks the tab's picture in a MonoCode pane, so the
 * browser window is neither needed nor brought forward. */
export function pickBrowserElement(
  sessionId: string,
  tab: string,
  inPane = false,
): Promise<PickedBrowserElement | null> {
  return invoke<PickedBrowserElement | null>("browser_pick_element", {
    sessionId,
    tab,
    inPane,
  });
}

export function cancelBrowserPick(): Promise<void> {
  return invoke<void>("browser_cancel_pick");
}

/** One picture of the live view (mirrors `live::LiveFrame`). */
export type BrowserLiveFrame = {
  /** JPEG while the page moves, a lossless PNG once it is still. */
  format: "jpeg" | "png";
  /** The page's viewport in CSS pixels; input coordinates use this space. */
  width: number;
  height: number;
  seq: number;
  /** The picture's own bytes, in `format`. */
  picture: Uint8Array;
};

/** Bytes in front of the picture (mirrors `live::PACKED_HEADER`). */
const LIVE_HEADER = 24;

/**
 * A picture as `live::LiveFrame::packed` sends it: format byte, three zero
 * bytes, `seq` as u32, `width` and `height` as f64 (little endian), then
 * the picture. `null` for anything shorter than the header.
 */
export function parseBrowserLiveFrame(
  buffer: ArrayBuffer,
): BrowserLiveFrame | null {
  if (buffer.byteLength <= LIVE_HEADER) return null;
  const head = new DataView(buffer, 0, LIVE_HEADER);
  return {
    format: head.getUint8(0) === 1 ? "png" : "jpeg",
    seq: head.getUint32(4, true),
    width: head.getFloat64(8, true),
    height: head.getFloat64(16, true),
    picture: new Uint8Array(buffer, LIVE_HEADER),
  };
}

/** What the user does in the live view (mirrors `live::LiveInput`). */
export type BrowserLiveInput =
  | {
      kind: "mouse";
      event: "mousePressed" | "mouseReleased" | "mouseMoved";
      x: number;
      y: number;
      button?: "none" | "left" | "middle" | "right";
      buttons?: number;
      clickCount?: number;
      modifiers?: number;
    }
  | {
      kind: "wheel";
      x: number;
      y: number;
      deltaX: number;
      deltaY: number;
      modifiers?: number;
    }
  | {
      kind: "key";
      event: "keyDown" | "rawKeyDown" | "keyUp";
      key: string;
      code: string;
      text?: string;
      keyCode?: number;
      modifiers?: number;
    }
  | { kind: "text"; text: string };

/**
 * "Show in MonoCode": `tab` is shown live as `view` until
 * {@link stopBrowserLive}, another view, or the tab is gone; the promise
 * settles then. Meanwhile the tab's viewport is `width` × `height` CSS
 * pixels at `scale`, the panel's size. Pictures are not pushed: the panel
 * asks for each with {@link pullBrowserLiveFrame}.
 */
export function startBrowserLive(
  sessionId: string,
  tab: string,
  view: string,
  width: number,
  height: number,
  scale: number,
): Promise<"stopped" | "tab_gone"> {
  return invoke("browser_live_start", {
    sessionId,
    tab,
    view,
    width,
    height,
    scale,
  });
}

/**
 * The next picture of `view`: the newest one after `after` (a `seq`),
 * waited for up to about a second. `null` when none came in that time or
 * the view is over; the pane asks again while it shows. Asking when ready
 * to draw is what keeps the picture current: a pane that falls behind skips
 * what it missed instead of catching up through it.
 */
export async function pullBrowserLiveFrame(
  view: string,
  after: number,
): Promise<BrowserLiveFrame | null> {
  const bytes = await invoke<ArrayBuffer | Uint8Array | null>(
    "browser_live_frame",
    { view, after },
  );
  if (!bytes) return null;
  const buffer =
    bytes instanceof ArrayBuffer
      ? bytes
      : bytes.buffer.slice(
          bytes.byteOffset,
          bytes.byteOffset + bytes.byteLength,
        );
  return parseBrowserLiveFrame(buffer as ArrayBuffer);
}

/** Stop `view`, the one a pane started (a newer pane's view keeps running). */
export function stopBrowserLive(view?: string): Promise<void> {
  return invoke<void>("browser_live_stop", { view });
}

export function sendBrowserLiveInput(
  sessionId: string,
  tab: string,
  input: BrowserLiveInput,
): Promise<void> {
  return invoke<void>("browser_live_input", { sessionId, tab, input });
}

/** The pane of `view` changed size; the tab's viewport follows it. */
export function resizeBrowserLive(
  view: string,
  width: number,
  height: number,
  scale: number,
): Promise<void> {
  return invoke<void>("browser_live_resize", { view, width, height, scale });
}

/** The panel's Back, Forward, Reload and address field (mirrors `live::LiveNav`). */
export type BrowserLiveNav =
  | { kind: "back" }
  | { kind: "forward" }
  | { kind: "reload" }
  | { kind: "go"; url: string };

export function navigateBrowserLive(
  sessionId: string,
  tab: string,
  nav: BrowserLiveNav,
): Promise<void> {
  return invoke<void>("browser_live_navigate", { sessionId, tab, nav });
}

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
// Agent side: MCP registration and the per-turn instructions
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
  accountId?: string,
): Promise<BrowserMcpRegistration> {
  return invoke<BrowserMcpRegistration>("browser_mcp_register", {
    provider,
    cwd,
    ...(accountId ? { accountId } : {}),
  });
}

/** Provider and account pairs registered in this run. */
const registered = new Set<string>();
/** Registrations in flight, so threads starting together share one. */
const registering = new Map<string, Promise<void>>();

/**
 * Registers the browser server once per app run for a provider that takes
 * MCP and the account a thread uses, when one of its threads can use the
 * browser. Registration is idempotent; this only saves rewriting the
 * provider's config on every turn. A failure leaves the pair unmarked, so
 * the next turn tries again.
 */
export function ensureBrowserMcp(
  provider: string,
  cwd: string,
  accountId?: string,
): Promise<void> {
  const key = `${provider}\u0000${accountId ?? ""}`;
  if (
    registered.has(key) ||
    !(BROWSER_MCP_PROVIDERS as readonly string[]).includes(provider)
  )
    return Promise.resolve();
  const pending = registering.get(key);
  if (pending) return pending;
  const run = registerBrowserMcp(provider, cwd, accountId)
    .then((result) => {
      // An unsupported provider (OpenCode 2) stays so for this run; asking
      // again on every turn would only rerun its version check.
      if (!result.supported || (result.registered && result.current))
        registered.add(key);
    })
    .finally(() => registering.delete(key));
  registering.set(key, run);
  return run;
}

/** The `<monocode_browser>` block a turn carries while it has the browser tools. */
export function browserAgentInstructions(
  provider: string,
  cwd?: string,
): Promise<string> {
  return invoke<string>("browser_agent_instructions", { provider, cwd });
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

/** Whether an agent in the session waits for the user to allow the browser. */
export function browserRequestPending(
  state: BrowserState | null,
  sessionId: string | undefined,
): boolean {
  return (
    !!state?.toggle_on && !!sessionBrowserAccess(state, sessionId)?.requested
  );
}

export function sessionBrowserTabs(
  state: BrowserState | null,
  sessionId: string | undefined,
): BrowserTab[] {
  if (!state || !sessionId) return [];
  return state.tabs.filter((tab) => tab.session_id === sessionId);
}

/** The thread indicator's states; null hides the indicator. */
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

/** A tab's status the way Settings lists it. */
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
  const brave = { label: "Brave", url: "brave://inspect/#remote-debugging" };
  const pageOf = (kind: BrowserKind) =>
    kind === "edge" ? edge : kind === "brave" ? brave : chrome;
  const found = state.connection.browser?.kind;
  if (found) return [pageOf(found)];
  const choice = state.settings.browser;
  if (choice.kind === "channel") return [pageOf(channelKind(choice.channel))];
  return [chrome, edge];
}

/** The browser behind a channel id (`discovery::channel_kind`). */
export function channelKind(channel: string): BrowserKind {
  if (channel.startsWith("msedge")) return "edge";
  if (channel.startsWith("chrome")) return "chrome";
  for (const kind of ["brave", "vivaldi", "opera", "arc"] as const) {
    if (channel.startsWith(kind)) return kind;
  }
  return "chromium";
}

export type ConnectionAction = "connect" | "retry" | "disconnect" | null;

export type ConnectionView = {
  /** One sentence for the state. */
  message: string;
  tone: "neutral" | "pending" | "good" | "warning" | "error";
  action: ConnectionAction;
  /** Show the guided consent steps. Attach mode only. */
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
        message: "Looking for your browser…",
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
        message: "A browser built on Chromium 144 or newer is required.",
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

const BROWSER_KIND_LABELS: Record<BrowserKind, string> = {
  chrome: "Google Chrome",
  edge: "Microsoft Edge",
  chromium: "Chromium",
  brave: "Brave",
  vivaldi: "Vivaldi",
  opera: "Opera",
  arc: "Arc",
};

/**
 * The browser by its kind, with the version its product string reports:
 * `Google Chrome 144.0.7559.60`. Brave, Vivaldi, Opera and Arc report the
 * Chromium version they are built on, so it is named as such.
 */
export function browserProductLabel(browser: BrowserInfo): string {
  const version = browser.product.split("/")[1];
  const label = BROWSER_KIND_LABELS[browser.kind] ?? "Chromium";
  if (!version) return label;
  return browser.kind === "chrome" ||
    browser.kind === "edge" ||
    browser.kind === "chromium"
    ? `${label} ${version}`
    : `${label} (Chromium ${version})`;
}

/**
 * The managed-mode picker's value for `choice`. A channel kept from "Your
 * own browser" starts that browser's stable build (`launch::wanted` looks
 * for its kind), so a Beta, Dev or Canary channel shows as that browser.
 */
export function managedChoiceChannel(choice: BrowserChoice): string {
  if (choice.kind !== "channel") return "auto";
  const kind = channelKind(choice.channel);
  return (
    MANAGED_BROWSER_CHANNELS.find(
      (entry) => channelKind(entry.channel) === kind,
    )?.channel ?? "auto"
  );
}

// ---------------------------------------------------------------------------
// In-app notices: no OS notifications, a toast for a dropped link or a
// browser action while the main switch is off
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
 * The toast for a dropped link: the browser went away while the switch was
 * on and some session had access. Turning it off yourself is not a drop,
 * and neither is the MonoCode browser closing, which starts again on demand.
 */
export function connectionDropNotice(
  previous: BrowserState | null,
  next: BrowserState,
): Omit<BrowserNotice, "id"> | null {
  if (!previous || previous.connection.state !== "connected") return null;
  if (!next.toggle_on || next.connection.state === "connected") return null;
  if (browserLaunches(next.settings.mode) && next.connection.state === "idle")
    return null;
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
