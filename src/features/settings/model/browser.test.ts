import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  answerBrowserRequest,
  answerPendingBrowserRequest,
  BROWSER_CHANNELS,
  browserRequestPending,
  browserAccessTurnedOff,
  browserRequestsToDecline,
  cancelBrowserPick,
  channelKind,
  browserNoticeSnapshot,
  browserProductLabel,
  browserStateSnapshot,
  browserTabHost,
  browserTabStatus,
  connectionDropNotice,
  connectionView,
  DEFAULT_BROWSER_SETTINGS,
  disconnectBrowser,
  ensureBrowserMcp,
  giveBrowserTab,
  inspectPages,
  listUserBrowserTabs,
  MANAGED_BROWSER_CHANNELS,
  managedChoiceChannel,
  openBrowserWindow,
  pickBrowserElement,
  resetBrowserStore,
  revokeBrowserSession,
  sessionBrowserGranted,
  sessionBrowserIndicator,
  closeFreeBrowserTab,
  setBrowserToggle,
  setSessionBrowserGrant,
  subscribeBrowserState,
  type BrowserConnectionState,
  type BrowserMode,
  type BrowserState,
} from "./browser";
import { browserState, browserTab } from "./browser.fixtures";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const granted = (sessionId = "s1") => ({
  session_id: sessionId,
  window: "main",
  granted: true,
  turn_active: false,
  tabs: 0,
  requested: false,
  turned_off: false,
});

/** Lets a test push `BrowserState` events through the store's listener. */
function captureStateEvents(): (state: BrowserState) => void {
  let handler: ((event: { payload: BrowserState }) => void) | undefined;
  vi.mocked(listen).mockImplementationOnce(async (_event, next) => {
    handler = next as (event: { payload: BrowserState }) => void;
    return () => {};
  });
  return (state) => handler?.({ payload: state });
}

const inMode = (mode: BrowserMode, patch: Partial<BrowserState> = {}) =>
  browserState({ settings: { ...DEFAULT_BROWSER_SETTINGS, mode }, ...patch });

beforeEach(() => resetBrowserStore());
afterEach(() => vi.mocked(invoke).mockReset());

describe("browser state views", () => {
  const states: BrowserConnectionState[] = [
    "off",
    "discovering",
    "needs_toggle",
    "stale_port",
    "awaiting_consent",
    "consent_denied",
    "consent_timeout",
    "unsupported_version",
    "policy_blocked",
    "connected",
    "disconnected",
    "idle",
  ];
  const views = (mode: BrowserMode) =>
    states.map((state) => ({
      state,
      view: connectionView(
        inMode(mode, { connection: { state, since_ms: 0 } }),
      ),
    }));

  it("gives every connection state a message, and a button only where one helps", () => {
    const actions = Object.fromEntries(
      views("attach").map(({ state, view }) => {
        expect(view.message, state).not.toBe("");
        return [state, view.action];
      }),
    );
    expect(actions).toEqual({
      off: null,
      discovering: null,
      needs_toggle: "retry",
      stale_port: "retry",
      awaiting_consent: null,
      consent_denied: "connect",
      consent_timeout: "connect",
      unsupported_version: "retry",
      policy_blocked: null,
      connected: "disconnect",
      disconnected: "connect",
      idle: "connect",
    });
    expect(
      connectionView(
        inMode("attach", { connection: { state: "idle", since_ms: 0 } }),
      ).message,
    ).toBe("Ready. MonoCode connects when an agent needs the browser.");
  });

  it("starts the MonoCode browser on demand and never shows the consent guide", () => {
    for (const { state, view } of views("managed")) {
      expect(view.message, state).not.toBe("");
      expect(view.guide, state).toBe(false);
    }
    const idle = connectionView(
      inMode("managed", { connection: { state: "idle", since_ms: 0 } }),
    );
    expect(idle).toMatchObject({
      message: "Ready. The MonoCode browser starts when an agent needs it.",
      action: null,
    });
    const ended = connectionView(
      inMode("managed", { connection: { state: "disconnected", since_ms: 0 } }),
    );
    expect(ended.action).toBeNull();
    expect(ended.message).not.toMatch(/permission/i);
    expect(connectionView(inMode("managed")).message).toBe(
      "The MonoCode browser is running on Google Chrome 144.0.7559.60.",
    );
  });

  it("names the connected browser and version", () => {
    const view = connectionView(inMode("attach"));
    expect(view.message).toBe("Connected to Google Chrome 144.0.7559.60.");
    expect(
      browserProductLabel({
        kind: "edge",
        channel: "msedge",
        product: "Edg/145.0.1",
        major: 145,
        user_data_dir: "",
        tabs_total: 0,
        managed: false,
      }),
    ).toBe("Microsoft Edge 145.0.1");
    // Brave reports the Chromium it is built on as "Chrome/…".
    expect(
      browserProductLabel({
        kind: "brave",
        channel: "brave",
        product: "Chrome/144.0.7559.60",
        major: 144,
        user_data_dir: "",
        tabs_total: 0,
        managed: true,
      }),
    ).toBe("Brave (Chromium 144.0.7559.60)");
  });

  it("shows the browser a channel kept from attach mode starts", () => {
    expect(managedChoiceChannel({ kind: "auto" })).toBe("auto");
    expect(managedChoiceChannel({ kind: "user_data_dir", path: "/p" })).toBe(
      "auto",
    );
    expect(managedChoiceChannel({ kind: "channel", channel: "brave" })).toBe(
      "brave",
    );
    expect(
      managedChoiceChannel({ kind: "channel", channel: "chrome-beta" }),
    ).toBe("chrome");
    expect(
      managedChoiceChannel({ kind: "channel", channel: "msedge-canary" }),
    ).toBe("msedge");
  });

  it("points the consent guide at the chosen browser", () => {
    const base = inMode("attach", {
      connection: { state: "needs_toggle", since_ms: 0 },
    });
    expect(inspectPages(base).map((page) => page.url)).toEqual([
      "chrome://inspect/#remote-debugging",
      "edge://inspect",
    ]);
    expect(
      inspectPages({
        ...base,
        settings: {
          ...base.settings,
          browser: { kind: "channel", channel: "msedge-beta" },
        },
      }).map((page) => page.url),
    ).toEqual(["edge://inspect"]);
    expect(
      inspectPages({
        ...base,
        settings: {
          ...base.settings,
          browser: { kind: "channel", channel: "brave" },
        },
      }).map((page) => page.url),
    ).toEqual(["brave://inspect/#remote-debugging"]);
    expect(
      inspectPages({
        ...base,
        connection: {
          state: "consent_denied",
          since_ms: 0,
          browser: {
            kind: "vivaldi",
            channel: "vivaldi",
            product: "Chrome/146.0.0.0",
            major: 146,
            user_data_dir: "/home/o/.config/vivaldi",
            tabs_total: 0,
            managed: false,
          },
        },
      }).map((page) => page.url),
    ).toEqual(["chrome://inspect/#remote-debugging"]);
  });

  it("knows the other Chromium browsers and starts only stable builds", () => {
    expect(channelKind("msedge-dev")).toBe("edge");
    expect(channelKind("chrome-canary")).toBe("chrome");
    expect(channelKind("brave")).toBe("brave");
    expect(channelKind("opera")).toBe("opera");
    expect(channelKind("arc")).toBe("arc");
    expect(channelKind("custom")).toBe("chromium");
    expect(MANAGED_BROWSER_CHANNELS.map((entry) => entry.channel)).toEqual([
      "chrome",
      "msedge",
      "brave",
      "vivaldi",
      "opera",
      "arc",
    ]);
    expect(BROWSER_CHANNELS).toHaveLength(12);
  });

  it("asks the backend for an element pick and can cancel it", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(pickBrowserElement("s1", "t_1")).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("browser_pick_element", {
      sessionId: "s1",
      tab: "t_1",
      inPane: false,
    });
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await pickBrowserElement("s1", "t_1", true);
    expect(invoke).toHaveBeenLastCalledWith("browser_pick_element", {
      sessionId: "s1",
      tab: "t_1",
      inPane: true,
    });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await cancelBrowserPick();
    expect(invoke).toHaveBeenCalledWith("browser_cancel_pick");
  });

  it("shows the thread indicator only with a grant, dialog over busy", () => {
    expect(sessionBrowserIndicator(browserState(), "s1")).toBeNull();
    const state = browserState({ sessions: [granted()] });
    expect(sessionBrowserIndicator(state, "s1")).toBe("granted");
    expect(
      sessionBrowserIndicator(
        { ...state, tabs: [browserTab({ busy: true })] },
        "s1",
      ),
    ).toBe("busy");
    expect(
      sessionBrowserIndicator(
        {
          ...state,
          tabs: [
            browserTab({ busy: true }),
            browserTab({ tab: "t_2", dialog_pending: true }),
          ],
        },
        "s1",
      ),
    ).toBe("dialog");
    expect(sessionBrowserIndicator(state, "s2")).toBeNull();
    // The MonoCode browser closed (its last tab went): no globe.
    expect(
      sessionBrowserIndicator(
        { ...state, connection: { ...state.connection, state: "idle" } },
        "s1",
      ),
    ).toBeNull();
    // A grant means nothing while the main switch is off.
    expect(sessionBrowserGranted({ ...state, toggle_on: false }, "s1")).toBe(
      false,
    );
  });

  it("lists a tab by host and status", () => {
    expect(browserTabHost("https://example.com/a?b=1")).toBe("example.com");
    expect(browserTabHost("about:blank")).toBe("about:blank");
    expect(browserTabStatus(browserTab())).toBe("Idle");
    expect(browserTabStatus(browserTab({ busy: true }))).toBe("Action running");
    expect(browserTabStatus(browserTab({ dialog_pending: true }))).toBe(
      "Waiting on a dialog",
    );
    expect(browserTabStatus(browserTab({ state: "revoked" }))).toBe("Released");
  });
});

describe("dropped connection toast", () => {
  const connected = browserState({ sessions: [granted()] });
  const dropped: BrowserState = {
    ...connected,
    connection: { state: "disconnected", since_ms: 1 },
  };

  it("fires when a connected link drops while a thread has access", () => {
    expect(connectionDropNotice(connected, dropped)?.settings).toBe(true);
  });

  it("stays quiet without a grant, when switched off, or when nothing changed", () => {
    expect(
      connectionDropNotice(browserState(), {
        ...browserState(),
        connection: dropped.connection,
      }),
    ).toBeNull();
    expect(
      connectionDropNotice(connected, {
        ...dropped,
        toggle_on: false,
        connection: { state: "off", since_ms: 1 },
      }),
    ).toBeNull();
    expect(connectionDropNotice(connected, connected)).toBeNull();
    expect(connectionDropNotice(null, dropped)).toBeNull();
  });

  it("stays quiet when the MonoCode browser closes, and not in attach mode", () => {
    for (const mode of ["managed", "private"] as const) {
      const running = inMode(mode, { sessions: [granted()] });
      expect(
        connectionDropNotice(running, {
          ...running,
          connection: { state: "idle", since_ms: 1 },
        }),
      ).toBeNull();
    }
    const attached = inMode("attach", { sessions: [granted()] });
    expect(
      connectionDropNotice(attached, {
        ...attached,
        connection: { state: "disconnected", since_ms: 1 },
      }),
    ).not.toBeNull();
  });
});

describe("browser store", () => {
  it("keeps the reply of a command and the state the event brings", async () => {
    const emit = captureStateEvents();
    vi.mocked(invoke).mockResolvedValueOnce(browserState({ toggle_on: false }));
    const listener = vi.fn();
    const stop = subscribeBrowserState(listener);
    await vi.waitFor(() =>
      expect(browserStateSnapshot()?.toggle_on).toBe(false),
    );
    vi.mocked(invoke).mockResolvedValueOnce(browserState());
    await setBrowserToggle(true);
    expect(invoke).toHaveBeenCalledWith("browser_toggle", { on: true });
    expect(browserStateSnapshot()?.toggle_on).toBe(true);
    emit(browserState({ sessions: [granted()] }));
    expect(browserStateSnapshot()?.sessions).toHaveLength(1);
    expect(listener).toHaveBeenCalledTimes(3);
    stop();
  });

  it("loads the state once and follows the event", async () => {
    const emit = captureStateEvents();
    vi.mocked(invoke).mockResolvedValueOnce(browserState({ toggle_on: false }));
    const stop = subscribeBrowserState(() => {});
    await vi.waitFor(() =>
      expect(browserStateSnapshot()?.toggle_on).toBe(false),
    );
    emit(browserState({ sessions: [granted()] }));
    expect(browserStateSnapshot()?.sessions).toHaveLength(1);
    stop();
  });

  it("toasts when the event reports a drop", async () => {
    const emit = captureStateEvents();
    vi.mocked(invoke).mockResolvedValueOnce(
      inMode("attach", { sessions: [granted()] }),
    );
    const stop = subscribeBrowserState(() => {});
    await vi.waitFor(() =>
      expect(browserStateSnapshot()?.sessions).toHaveLength(1),
    );
    expect(browserNoticeSnapshot()).toBeNull();
    emit(
      inMode("attach", {
        sessions: [granted()],
        connection: { state: "disconnected", since_ms: 1 },
      }),
    );
    expect(browserNoticeSnapshot()?.settings).toBe(true);
    stop();
  });

  it("answers a thread's request and opens the MonoCode browser", async () => {
    vi.mocked(invoke).mockResolvedValue(browserState());
    await answerBrowserRequest("s1", false);
    expect(invoke).toHaveBeenCalledWith("browser_answer_request", {
      sessionId: "s1",
      allow: false,
    });
    await openBrowserWindow();
    expect(invoke).toHaveBeenCalledWith("browser_open_window", undefined);
  });

  it("answers from the phone only while the thread still asks", async () => {
    const asking = browserState({
      sessions: [{ ...granted(), granted: false, requested: true }],
    });
    expect(browserRequestPending(asking, "s1")).toBe(true);
    expect(browserRequestPending(asking, "s2")).toBe(false);
    expect(browserRequestPending({ ...asking, toggle_on: false }, "s1")).toBe(
      false,
    );
    await expect(answerPendingBrowserRequest("s1", true)).rejects.toThrow(
      "no longer waiting",
    );
    expect(invoke).not.toHaveBeenCalled();
    resetBrowserStore(asking);
    vi.mocked(invoke).mockResolvedValue(
      browserState({ sessions: [granted()] }),
    );
    await answerPendingBrowserRequest("s1", true);
    expect(invoke).toHaveBeenCalledWith("browser_answer_request", {
      sessionId: "s1",
      allow: true,
    });
    await expect(answerPendingBrowserRequest("s1", false)).rejects.toThrow(
      "no longer waiting",
    );
  });

  it("reads which threads the user turned off from the backend", async () => {
    const off = browserState({
      sessions: [{ ...granted(), granted: false, turned_off: true }],
    });
    expect(browserAccessTurnedOff(browserState(), "s1")).toBe(false);
    expect(browserAccessTurnedOff(off, "s1")).toBe(true);
    expect(browserAccessTurnedOff(off, "s2")).toBe(false);
    expect(browserAccessTurnedOff(null, "s1")).toBe(false);
    vi.mocked(invoke).mockResolvedValue(off);
    await revokeBrowserSession("s1");
    expect(invoke).toHaveBeenCalledWith("browser_revoke", { sessionId: "s1" });
    expect(browserAccessTurnedOff(browserStateSnapshot(), "s1")).toBe(true);
    vi.mocked(invoke).mockResolvedValue(
      browserState({ sessions: [granted()] }),
    );
    await setSessionBrowserGrant("s1", true);
    expect(browserAccessTurnedOff(browserStateSnapshot(), "s1")).toBe(false);
  });

  it("declines only the requests of sessions this window runs", () => {
    const asking = (session_id: string, window: string) => ({
      ...granted(session_id),
      granted: false,
      window,
      requested: true,
    });
    const state = browserState({
      sessions: [
        asking("mine", "main"),
        asking("eligible", "main"),
        asking("theirs", "window-2"),
        { ...granted("quiet"), window: "main" },
      ],
    });
    const leave = (id: string) => id === "eligible";
    expect(browserRequestsToDecline(state, "main", leave)).toEqual(["mine"]);
    expect(browserRequestsToDecline(state, "window-2", leave)).toEqual([
      "theirs",
    ]);
    expect(browserRequestsToDecline(null, "main", leave)).toEqual([]);
  });

  it("lists the free tabs and gives one to a thread", async () => {
    const free = [
      { id: "f_0000000a", url: "https://example.com/", title: "Example" },
    ];
    vi.mocked(invoke).mockResolvedValueOnce(free);
    await expect(listUserBrowserTabs()).resolves.toEqual(free);
    expect(invoke).toHaveBeenCalledWith("browser_user_tabs");

    vi.mocked(invoke).mockResolvedValue(
      browserState({ sessions: [granted("s2")], tabs: [browserTab()] }),
    );
    await giveBrowserTab("f_0000000a", "s2");
    expect(invoke).toHaveBeenCalledWith("browser_give_tab", {
      free: "f_0000000a",
      sessionId: "s2",
    });
    expect(browserStateSnapshot()?.tabs).toHaveLength(1);

    await closeFreeBrowserTab("f_0000000b");
    expect(invoke).toHaveBeenCalledWith("browser_close_free_tab", {
      free: "f_0000000b",
    });
  });

  it("does not toast after the user disconnects", async () => {
    resetBrowserStore(browserState({ sessions: [granted()] }));
    vi.mocked(invoke).mockResolvedValueOnce(
      browserState({
        sessions: [granted()],
        connection: { state: "disconnected", since_ms: 1 },
      }),
    );
    await disconnectBrowser();
    expect(browserNoticeSnapshot()).toBeNull();
  });
});

describe("browser MCP registration", () => {
  it("registers each MCP provider once per run and skips the others", async () => {
    vi.mocked(invoke).mockResolvedValue({
      provider: "codex",
      supported: true,
      registered: true,
      current: true,
    });
    await ensureBrowserMcp("codex", "/repo");
    await ensureBrowserMcp("codex", "/repo");
    await ensureBrowserMcp("pi", "/repo");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("browser_mcp_register", {
      provider: "codex",
      cwd: "/repo",
    });
  });

  it("does not ask an unsupported provider again in the same run", async () => {
    // `browser_mcp_register`'s reply for OpenCode 2 (`register.rs`).
    vi.mocked(invoke).mockResolvedValue({
      provider: "opencode",
      supported: false,
      registered: false,
      current: false,
      configPath: "/home/me/.config/opencode/opencode.json",
      unsupportedReason:
        "OpenCode 2 runs MCP servers and shell commands from its shared background service, which does not carry this session's MonoCode credential, so browser tools cannot work there yet. OpenCode 1 is supported.",
    });
    await ensureBrowserMcp("opencode", "/repo");
    await ensureBrowserMcp("opencode", "/repo");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("shares one registration between threads starting together", async () => {
    let finish: (value: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementationOnce(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const first = ensureBrowserMcp("cursor", "/repo");
    const second = ensureBrowserMcp("cursor", "/other");
    finish({
      provider: "cursor",
      supported: true,
      registered: true,
      current: true,
    });
    await Promise.all([first, second]);
    await ensureBrowserMcp("cursor", "/repo");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("tries again after a failed registration", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("locked"));
    await expect(ensureBrowserMcp("claude", "/repo")).rejects.toThrow("locked");
    vi.mocked(invoke).mockResolvedValueOnce({
      provider: "claude",
      supported: true,
      registered: true,
      current: true,
    });
    await ensureBrowserMcp("claude", "/repo");
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
