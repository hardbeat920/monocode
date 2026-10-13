import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  answerBrowserRequest,
  browserAccessTurnedOff,
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
  inspectPages,
  openBrowserWindow,
  resetBrowserStore,
  revokeBrowserSession,
  sessionBrowserGranted,
  sessionBrowserIndicator,
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
});

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
});

describe("browser store", () => {
  it("keeps the reply of a command and the state the event brings", async () => {
    resetBrowserStore(browserState({ toggle_on: false }));
    vi.mocked(invoke).mockResolvedValueOnce(browserState());
    const listener = vi.fn();
    const stop = subscribeBrowserState(listener);
    await setBrowserToggle(true);
    expect(invoke).toHaveBeenCalledWith("browser_toggle", { on: true });
    expect(browserStateSnapshot()?.toggle_on).toBe(true);
    expect(listener).toHaveBeenCalled();
    stop();
  });

  it("loads the state once and follows the event", async () => {
    let emit: (state: BrowserState) => void = () => {};
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      emit = (state) =>
        (handler as (event: { payload: BrowserState }) => void)({
          payload: state,
        });
      return () => {};
    });
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
    resetBrowserStore(browserState({ sessions: [granted()] }));
    vi.mocked(invoke).mockResolvedValueOnce(
      browserState({
        sessions: [granted()],
        connection: { state: "disconnected", since_ms: 1 },
      }),
    );
    await setBrowserToggle(true);
    expect(browserNoticeSnapshot()?.settings).toBe(true);
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

  it("remembers which threads the user turned off in this run", async () => {
    vi.mocked(invoke).mockResolvedValue(browserState());
    expect(browserAccessTurnedOff("s1")).toBe(false);
    await setSessionBrowserGrant("s1", false);
    expect(browserAccessTurnedOff("s1")).toBe(true);
    await answerBrowserRequest("s1", false);
    expect(browserAccessTurnedOff("s1")).toBe(true);
    await answerBrowserRequest("s1", true);
    expect(browserAccessTurnedOff("s1")).toBe(false);
    await revokeBrowserSession("s1");
    expect(browserAccessTurnedOff("s1")).toBe(true);
    await setSessionBrowserGrant("s1", true);
    expect(browserAccessTurnedOff("s1")).toBe(false);
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
    vi.mocked(invoke).mockResolvedValue({
      provider: "opencode",
      supported: false,
      registered: false,
      current: false,
      unsupportedReason: "OpenCode 2",
    });
    await ensureBrowserMcp("opencode", "/repo");
    await ensureBrowserMcp("opencode", "/repo");
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
