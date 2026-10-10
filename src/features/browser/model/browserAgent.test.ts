import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const evals: string[] = [];
vi.mock("../../../platform/tauri/browser", () => ({
  browserHistory: vi.fn(),
  closeBrowserView: vi.fn(async () => undefined),
  evalInBrowser: vi.fn(async (id: string, script: string) => {
    // The readyState probe after a load timeout is not a tool call.
    if (script === "document.readyState") return "loading";
    evals.push(id);
    return "ok";
  }),
  navigateBrowser: vi.fn(),
  screenshotBrowser: vi.fn(),
}));
vi.mock("../../settings/model/displayPrefs", () => ({
  loadAgentBrowser: () => true,
}));

import {
  closeBrowserTab,
  dockOfTab,
  noteLoadStarted,
  getBrowserState,
  openBrowserTab,
  patchBrowserTab,
} from "./browserStore";
import { createBrowserAgent } from "./browserAgent";
import * as browserStore from "./browserStore";
import { setNativeTabReady } from "./nativeTabs";
import {
  browserHistory,
  evalInBrowser,
  navigateBrowser,
} from "../../../platform/tauri/browser";

const tab = (id: string) =>
  dockOfTab(getBrowserState(), id)?.pane.files.find((f) => f.id === id)
    ?.browser;

const opened: string[] = [];
const openTab = () => {
  const id = openBrowserTab("https://example.com", {
    sessionId: "s1",
    background: true,
  })!;
  opened.push(id);
  return id;
};

/** Settle a call's outcome without leaving a rejection unhandled. */
function track(promise: Promise<unknown>) {
  const state = {
    done: false,
    value: undefined as unknown,
    error: undefined as unknown,
  };
  void promise.then(
    (value) => {
      state.done = true;
      state.value = value;
    },
    (error: unknown) => {
      state.done = true;
      state.error = error;
    },
  );
  return state;
}

describe("browser tools waiting on a tab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    evals.length = 0;
    vi.useFakeTimers();
  });

  afterEach(() => {
    for (const id of opened.splice(0)) {
      setNativeTabReady(id, false);
      if (tab(id)) closeBrowserTab(id);
    }
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  const evalCall = (id: string) =>
    createBrowserAgent("s1")("browser_eval", { tabId: id, script: "1" });

  it("waits for the page to finish loading after resuming", async () => {
    const id = openTab();
    const call = track(evalCall(id));

    await vi.advanceTimersByTimeAsync(100);
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    await vi.advanceTimersByTimeAsync(2_000);
    expect(evals).toHaveLength(0);
    expect(call.done).toBe(false);

    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(evals).toEqual([id]);
  });

  it("holds every concurrent call until a ready but loading page finishes", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    const calls = [
      track(evalCall(id)),
      track(evalCall(id)),
      track(evalCall(id)),
    ];

    await vi.advanceTimersByTimeAsync(2_000);
    expect(evals).toHaveLength(0);
    expect(calls.some((c) => c.done)).toBe(false);

    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(10);
    expect(calls.every((c) => c.done && !c.error)).toBe(true);
    expect(evals).toEqual([id, id, id]);
  });

  it("holds concurrent calls on a suspended tab until it has loaded", async () => {
    const id = openTab();
    const first = track(evalCall(id));
    const second = track(evalCall(id));

    await vi.advanceTimersByTimeAsync(100);
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    await vi.advanceTimersByTimeAsync(500);
    // A third call arrives mid-load, with the view already ready.
    const third = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(500);
    expect([first, second, third].some((c) => c.done)).toBe(false);
    expect(evals).toHaveLength(0);

    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(10);
    expect([first, second, third].every((c) => c.done && !c.error)).toBe(true);
    expect(evals).toHaveLength(3);
  });

  it("fails clearly when the page never finishes loading", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    const call = track(evalCall(id));

    await vi.advanceTimersByTimeAsync(16_000);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/still loading/);
    expect(evals).toHaveLength(0);

    // A later call is not stuck behind the failed wait.
    patchBrowserTab(id, { loading: false });
    const retry = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(10);
    expect(retry.done && !retry.error).toBe(true);
    expect(evals).toEqual([id]);
  });

  it.each(["complete", "interactive"])(
    "recovers a tab stuck loading when the page reports %s",
    async (readyState) => {
      const id = openTab();
      setNativeTabReady(id, true);
      patchBrowserTab(id, { loading: true });
      vi.mocked(evalInBrowser).mockImplementation(async (_id, script) => {
        if (script === "document.readyState") return readyState;
        evals.push(id);
        return "ok";
      });
      try {
        const call = track(evalCall(id));
        await vi.advanceTimersByTimeAsync(16_000);
        expect(call.done && !call.error).toBe(true);
        expect(tab(id)?.loading).toBeFalsy();

        // The next call is not held up by the stale flag.
        const next = track(evalCall(id));
        await vi.advanceTimersByTimeAsync(10);
        expect(next.done && !next.error).toBe(true);
      } finally {
        vi.mocked(evalInBrowser).mockImplementation(
          async (id: string, script: string) => {
            if (script === "document.readyState") return "loading";
            evals.push(id);
            return "ok";
          },
        );
      }
    },
  );

  it("keeps reporting a load that the page says is still loading", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(16_000);
    expect(String(call.error)).toMatch(/still loading/);
    expect(tab(id)?.loading).toBe(true);
  });

  /** Time out a load, leaving the readyState probe pending. */
  async function pendingProbe(reply: () => Promise<unknown>) {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    vi.mocked(evalInBrowser).mockImplementation(async (_id, script) => {
      if (script === "document.readyState") return reply();
      evals.push(id);
      return "ok";
    });
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(16_000);
    return { id, call };
  }

  const restoreEval = () =>
    vi
      .mocked(evalInBrowser)
      .mockImplementation(async (id: string, script: string) => {
        if (script === "document.readyState") return "loading";
        evals.push(id);
        return "ok";
      });

  it("keeps a newer load that starts while the probe is in flight", async () => {
    let answer!: (value: unknown) => void;
    const { id, call } = await pendingProbe(
      () => new Promise((resolve) => (answer = resolve)),
    );
    try {
      expect(call.done).toBe(false);
      noteLoadStarted(id);
      patchBrowserTab(id, { loading: true });
      answer("complete");
      await vi.advanceTimersByTimeAsync(10);
      expect(String(call.error)).toMatch(/still loading/);
      expect(tab(id)?.loading).toBe(true);
    } finally {
      restoreEval();
    }
  });

  it("reports a closed tab when it closes during a successful probe", async () => {
    let answer!: (value: unknown) => void;
    const { id, call } = await pendingProbe(
      () => new Promise((resolve) => (answer = resolve)),
    );
    try {
      closeBrowserTab(id);
      answer("complete");
      await vi.advanceTimersByTimeAsync(10);
      expect(String(call.error)).toMatch(/closed/);
    } finally {
      restoreEval();
    }
  });

  it("reports a closed tab when it closes during a failing probe", async () => {
    let fail!: (reason: unknown) => void;
    const { id, call } = await pendingProbe(
      () => new Promise((_resolve, reject) => (fail = reject)),
    );
    try {
      closeBrowserTab(id);
      fail(new Error("gone"));
      await vi.advanceTimersByTimeAsync(10);
      expect(String(call.error)).toMatch(/closed/);
    } finally {
      restoreEval();
    }
  });

  it("still reports loading when the probe rejects on a live tab", async () => {
    const { id, call } = await pendingProbe(() =>
      Promise.reject(new Error("unresponsive")),
    );
    try {
      expect(String(call.error)).toMatch(/still loading/);
      expect(tab(id)?.loading).toBe(true);
    } finally {
      restoreEval();
    }
  });

  it("fails when the tab is closed mid-wait", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(100);

    closeBrowserTab(id);
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/closed/);
    expect(evals).toHaveLength(0);
  });

  it("fails when the tab is closed before its view appears", async () => {
    const id = openTab();
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(100);

    closeBrowserTab(id);
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/closed/);
    expect(evals).toHaveLength(0);
  });

  it("fails fast when the view cannot be created", async () => {
    const id = openTab();
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(100);

    patchBrowserTab(id, { error: "webview failed" });
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/webview failed/);
    expect(evals).toHaveLength(0);
  });

  it("does not leave loading stuck when the view is reused without a load", async () => {
    const id = openTab();
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(100);

    // A reused native view: ready again, but no load event ever follows.
    setNativeTabReady(id, true);
    await vi.advanceTimersByTimeAsync(3_100);
    expect(call.done).toBe(true);
    expect(call.error).toBeUndefined();
    expect(evals).toEqual([id]);
    expect(tab(id)?.loading).toBeFalsy();
  });

  it("runs straight away on a ready page that already finished loading", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    const call = track(evalCall(id));
    await vi.advanceTimersByTimeAsync(0);
    expect(call.done && !call.error).toBe(true);
    expect(evals).toEqual([id]);
  });

  it("browser_open fails instead of timing out when creation fails", async () => {
    const agent = createBrowserAgent("s1");
    const call = track(agent("browser_open", { url: "https://example.com" }));
    await vi.advanceTimersByTimeAsync(10);
    const id = getBrowserState().docks.flatMap((d) =>
      d.pane.files.map((f) => f.id),
    )[0];
    opened.push(id);

    patchBrowserTab(id, { error: "webview failed" });
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/webview failed/);
  });

  it("shares one load wait between concurrent callers", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: true });
    const subscribe = vi.spyOn(browserStore, "subscribeBrowser");
    const first = track(evalCall(id));
    const second = track(evalCall(id));
    expect(subscribe).toHaveBeenCalledTimes(1);
    subscribe.mockRestore();
    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(first.error).toBeUndefined();
    expect(second.error).toBeUndefined();
    expect(evals).toEqual([id, id]);
  });

  it("fails fast when resume repeats the same creation error", async () => {
    const id = openTab();
    patchBrowserTab(id, { error: "webview failed" });
    const call = track(evalCall(id));
    expect(tab(id)?.error).toBeUndefined();
    patchBrowserTab(id, { error: "webview failed" });
    await vi.advanceTimersByTimeAsync(10);
    expect(call.done).toBe(true);
    expect(String(call.error)).toMatch(/webview failed/);
  });

  it("does not wait out the full timeout when an unknown outcome never loads", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    vi.mocked(navigateBrowser).mockResolvedValueOnce({ outcome: "unknown" });
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/#hash",
      }),
    );
    await vi.advanceTimersByTimeAsync(1_600);
    expect(call.done).toBe(true);
    expect(call.value).not.toHaveProperty("timedOut");
  });

  it("does not count a load that began before the navigation was dispatched", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    vi.mocked(navigateBrowser).mockImplementationOnce(
      async (_id, _url, onDispatch) => {
        // An earlier navigation's load started while this one was queued.
        noteLoadStarted(id);
        onDispatch?.();
        return { outcome: "document" };
      },
    );
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/next",
      }),
    );
    await vi.advanceTimersByTimeAsync(5_000);
    // The earlier load does not complete this navigation's wait.
    expect(call.done).toBe(false);
  });

  it("navigates without load events and leaves the tab usable", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    // The page applied the fragment itself and said so.
    vi.mocked(navigateBrowser).mockResolvedValueOnce({ outcome: "applied" });
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/#hash",
      }),
    );
    await vi.advanceTimersByTimeAsync(1_600);
    expect(call.done).toBe(true);
    expect(call.error).toBeUndefined();
    expect(tab(id)?.loading).toBeFalsy();
    expect(navigateBrowser).toHaveBeenCalledWith(
      id,
      "https://example.com/#hash",
      expect.any(Function),
    );
    await evalCall(id);
    expect(evals).toEqual([id]);
  });

  it("waits for a load when the page leaves a fragment target to the webview", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    patchBrowserTab(id, { url: "https://example.com/a" });
    // The page was already somewhere else (pushState the store has not seen),
    // so the fragment target is a document navigation.
    vi.mocked(navigateBrowser).mockResolvedValueOnce({ outcome: "document" });
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/a#section",
      }),
    );
    await vi.advanceTimersByTimeAsync(1_600);
    expect(call.done).toBe(false);
    patchBrowserTab(id, { loading: true });
    await vi.advanceTimersByTimeAsync(0);
    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(call.done).toBe(true);
  });

  it("waits for normal navigation load events", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/next",
      }),
    );
    await vi.advanceTimersByTimeAsync(100);
    patchBrowserTab(id, { loading: true });
    await vi.advanceTimersByTimeAsync(2_000);
    expect(call.done).toBe(false);
    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(call.done).toBe(true);
    expect(call.value).not.toHaveProperty("timedOut");
  });

  it("does not report a reload as loaded until the load runs", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    const call = track(
      createBrowserAgent("s1")("browser_history", {
        tabId: id,
        action: "reload",
      }),
    );
    // No load event within the old start window: not success yet.
    await vi.advanceTimersByTimeAsync(5_000);
    expect(call.done).toBe(false);
    expect(browserHistory).toHaveBeenCalledWith(id, "reload");
    noteLoadStarted(id);
    patchBrowserTab(id, { loading: true });
    await vi.advanceTimersByTimeAsync(2_000);
    expect(call.done).toBe(false);
    patchBrowserTab(id, { loading: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(call.error).toBeUndefined();
    expect(call.value).not.toHaveProperty("timedOut");
  });

  it("reports timedOut when a reload never starts loading", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    const call = track(
      createBrowserAgent("s1")("browser_history", {
        tabId: id,
        action: "reload",
      }),
    );
    await vi.advanceTimersByTimeAsync(15_100);
    expect(call.error).toBeUndefined();
    expect(call.value).toMatchObject({ tabId: id, timedOut: true });
    expect(tab(id)?.loading).toBeFalsy();
  });

  it("does not miss a navigation load that finished before the wait began", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    vi.mocked(navigateBrowser).mockImplementationOnce(async () => {
      noteLoadStarted(id);
      patchBrowserTab(id, { loading: true });
      patchBrowserTab(id, { loading: false });
    });
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/fast",
      }),
    );
    await vi.advanceTimersByTimeAsync(0);
    expect(call.done).toBe(true);
    expect(call.value).not.toHaveProperty("timedOut");
  });

  it("holds a cross-document navigation open when no load starts", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    const call = track(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/other",
      }),
    );
    await vi.advanceTimersByTimeAsync(5_000);
    expect(call.done).toBe(false);
    await vi.advanceTimersByTimeAsync(10_200);
    expect(call.value).toMatchObject({ timedOut: true });
  });

  it.each(["browser_navigate", "browser_history"])(
    "%s summarizes the page after its load finishes",
    async (action) => {
      const id = openTab();
      setNativeTabReady(id, true);
      const call = track(
        createBrowserAgent("s1")(action, {
          tabId: id,
          url: "https://example.com/final",
          action: "reload",
        }),
      );
      await vi.advanceTimersByTimeAsync(100);
      noteLoadStarted(id);
      patchBrowserTab(id, { loading: true });
      await vi.advanceTimersByTimeAsync(100);
      patchBrowserTab(id, {
        url: "https://example.com/redirected",
        title: "Final",
        loading: false,
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(call.value).toMatchObject({
        tabId: id,
        url: "https://example.com/redirected",
        title: "Final",
      });
    },
  );

  it("summarizes a browser_open after its load finishes", async () => {
    const call = track(
      createBrowserAgent("s1")("browser_open", { url: "https://example.com" }),
    );
    await vi.advanceTimersByTimeAsync(10);
    const id = getBrowserState().docks.flatMap((d) =>
      d.pane.files.map((f) => f.id),
    )[0];
    opened.push(id);
    setNativeTabReady(id, true);
    await vi.advanceTimersByTimeAsync(10);
    noteLoadStarted(id);
    patchBrowserTab(id, { loading: true });
    patchBrowserTab(id, {
      url: "https://example.com/landed",
      title: "Landed",
      loading: false,
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(call.value).toMatchObject({
      url: "https://example.com/landed",
      title: "Landed",
    });
  });

  it("clears loading when native navigation rejects", async () => {
    const id = openTab();
    setNativeTabReady(id, true);
    vi.mocked(navigateBrowser).mockImplementationOnce(async () => {
      patchBrowserTab(id, { loading: true });
      throw new Error("navigation failed");
    });
    await expect(
      createBrowserAgent("s1")("browser_navigate", {
        tabId: id,
        url: "https://example.com/next",
      }),
    ).rejects.toThrow("navigation failed");
    expect(tab(id)?.loading).toBeFalsy();
    expect(tab(id)?.url).toBe("https://example.com");
    await evalCall(id);
    expect(evals).toEqual([id]);
  });

  it.each(["browser_navigate", "browser_history"])(
    "%s surfaces a load timeout",
    async (action) => {
      const id = openTab();
      setNativeTabReady(id, true);
      const call = track(
        createBrowserAgent("s1")(action, {
          tabId: id,
          url: "https://example.com/next",
          action: "reload",
        }),
      );
      await vi.advanceTimersByTimeAsync(100);
      patchBrowserTab(id, { loading: true });
      await vi.advanceTimersByTimeAsync(15_000);
      expect(call.error).toBeUndefined();
      expect(call.value).toMatchObject({ tabId: id, timedOut: true });
      patchBrowserTab(id, { loading: false });
      await evalCall(id);
      expect(evals).toEqual([id]);
    },
  );

  it.each([
    "browser_navigate",
    "browser_history",
    "browser_click",
    "browser_open",
  ])("%s throws when the tab closes during its load wait", async (action) => {
    let id = openTab();
    setNativeTabReady(id, true);
    const call = track(
      createBrowserAgent("s1")(action, {
        tabId: id,
        url: "https://example.com/next",
        action: "reload",
        text: "Next",
      }),
    );
    await vi.advanceTimersByTimeAsync(10);
    if (action === "browser_open") {
      id = getBrowserState()
        .docks.flatMap((d) => d.pane.files.map((f) => f.id))
        .find((candidate) => !opened.includes(candidate))!;
      opened.push(id);
      setNativeTabReady(id, true);
      await vi.advanceTimersByTimeAsync(10);
    }
    closeBrowserTab(id);
    await vi.advanceTimersByTimeAsync(0);
    expect(call.done).toBe(true);
    expect(String(call.error)).toContain("The browser tab was closed");
  });
});
