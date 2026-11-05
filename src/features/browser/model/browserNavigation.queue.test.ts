import { beforeEach, describe, expect, it, vi } from "vitest";

// Only the Tauri bridge is faked: the per-tab dispatch queue is the real one.
const sent: Array<{
  url: string;
  resolve: (reply: { outcome: string; url?: string }) => void;
}> = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(
    (_command: string, args: { url: string }) =>
      new Promise((resolve) => sent.push({ url: args.url, resolve })),
  ),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import {
  loadGeneration,
  noteLoadStarted,
  openBrowserTab,
  patchBrowserTab,
} from "./browserStore";
import {
  DISPATCH_GATE_MS,
  isNavigating,
  navigateBrowserTab,
} from "./browserNavigation";

/** A native load start, reported the way the browser event handler does. */
const loadStarts = (id: string) => {
  noteLoadStarted(id);
  patchBrowserTab(id, { loading: true });
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("navigateBrowserTab through the platform queue", () => {
  beforeEach(() => {
    sent.length = 0;
  });

  it("takes each baseline when the request is dispatched, not when it is queued", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "queue-s" })!;
    const a = navigateBrowserTab(id, "https://a.test/");
    const b = navigateBrowserTab(id, "https://b.test/");
    await flush();
    // Only A is with the native side; B waits its turn.
    expect(sent.map((call) => call.url)).toEqual(["https://a.test/"]);
    const queuedAt = loadGeneration(id);
    // A's load starts while B still waits, then A's request is answered.
    noteLoadStarted(id);
    sent[0].resolve({ outcome: "document" });
    await a;
    await flush();
    expect(sent).toHaveLength(2);
    sent[1].resolve({ outcome: "document" });
    const reply = await b;
    // B's baseline already includes A's load, so that load is not B's...
    expect(reply.since).toBe(queuedAt + 1);
    // ...and B, whose own load has not started, stays protected from polling.
    expect(isNavigating(id)).toBe(true);
    noteLoadStarted(id);
    expect(isNavigating(id)).toBe(false);
  });

  it("holds the next navigation until the previous one's load starts", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "queue-s2" })!;
    patchBrowserTab(id, { loading: false });
    const a = navigateBrowserTab(id, "https://a.test/");
    const b = navigateBrowserTab(id, "https://b.test/");
    await flush();
    // The webview accepts A before A's load starts: the usual order.
    sent[0].resolve({ outcome: "document" });
    await a;
    await flush();
    expect(sent).toHaveLength(1);
    const beforeA = loadGeneration(id);
    loadStarts(id);
    await flush();
    expect(sent.map((call) => call.url)).toEqual([
      "https://a.test/",
      "https://b.test/",
    ]);
    sent[1].resolve({ outcome: "document" });
    const reply = await b;
    expect(reply).toMatchObject({ since: beforeA + 1, overlapped: false });
    expect(isNavigating(id)).toBe(true);
  });

  it("dispatches anyway, flagged as overlapped, when the previous load never starts", async () => {
    vi.useFakeTimers();
    try {
      const id = openBrowserTab("https://old.test/", {
        sessionId: "queue-s3",
      })!;
      patchBrowserTab(id, { loading: false });
      const a = navigateBrowserTab(id, "https://a.test/");
      const b = navigateBrowserTab(id, "https://b.test/");
      await vi.advanceTimersByTimeAsync(0);
      sent[0].resolve({ outcome: "document" });
      await a;
      await vi.advanceTimersByTimeAsync(DISPATCH_GATE_MS - 1);
      expect(sent).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(sent).toHaveLength(2);
      sent[1].resolve({ outcome: "document" });
      await expect(b).resolves.toMatchObject({ overlapped: true });
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not hold a navigation behind an applied one", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "queue-s4" })!;
    const a = navigateBrowserTab(id, "https://old.test/#a");
    const b = navigateBrowserTab(id, "https://b.test/");
    await flush();
    sent[0].resolve({ outcome: "applied", url: "https://old.test/#a" });
    await a;
    await flush();
    expect(sent).toHaveLength(2);
    sent[1].resolve({ outcome: "document" });
    await expect(b).resolves.toMatchObject({ overlapped: false });
  });
});
