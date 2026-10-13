import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  browserNoticeSnapshot,
  browserProductLabel,
  browserStateSnapshot,
  browserTabHost,
  browserTabStatus,
  connectionDropNotice,
  connectionView,
  disconnectBrowser,
  inspectPages,
  resetBrowserStore,
  sessionBrowserGranted,
  sessionBrowserIndicator,
  setBrowserToggle,
  subscribeBrowserState,
  type BrowserConnectionState,
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
});

beforeEach(() => resetBrowserStore());
afterEach(() => vi.mocked(invoke).mockReset());

describe("browser state views", () => {
  it("gives every connection state a message, and a button only where one helps", () => {
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
    ];
    const actions = Object.fromEntries(
      states.map((state) => {
        const view = connectionView(
          browserState({ connection: { state, since_ms: 0 } }),
        );
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
    });
  });

  it("names the connected browser and version", () => {
    const view = connectionView(browserState());
    expect(view.message).toBe("Connected to Google Chrome 144.0.7559.60.");
    expect(
      browserProductLabel({
        kind: "edge",
        channel: "msedge",
        product: "Edg/145.0.1",
        major: 145,
        user_data_dir: "",
        tabs_total: 0,
      }),
    ).toBe("Microsoft Edge 145.0.1");
  });

  it("points the consent guide at the chosen browser", () => {
    const base = browserState({
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
