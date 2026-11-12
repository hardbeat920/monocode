import { beforeEach, describe, expect, it, vi } from "vitest";

type Reply =
  { outcome: "applied" | "document" | "unknown"; url?: string } | undefined;
const pending: Array<{
  /** Answer like the native side; no outcome means a document navigation. */
  resolve: (outcome?: Reply) => void;
  reject: (reason: unknown) => void;
}> = [];
let chains = 0;
vi.mock("../../../platform/tauri/browser", () => ({
  // Dispatches at once; the queue itself is covered in browserNavigation.queue.test.ts.
  navigateBrowser: vi.fn(
    (_id: string, _url: string, onDispatch?: () => void) =>
      new Promise<Reply>((resolve, reject) => {
        onDispatch?.();
        pending.push({
          resolve: (outcome) => resolve(outcome ?? { outcome: "document" }),
          reject,
        });
      }),
  ),
}));

import {
  dockOfTab,
  getBrowserState,
  closeBrowserTab,
  noteLoadFinished,
  noteLoadStarted,
  openBrowserTab,
  patchBrowserTab,
} from "./browserStore";
import {
  DOCUMENT_SETTLE_MS,
  UNKNOWN_SETTLE_MS,
  isNavigating,
  navigateBrowserTab,
} from "./browserNavigation";
import { setNativeTabReady } from "./nativeTabs";
import { syncNativeUrl } from "./nativeUrl";

/** A native URL read that finds the page at `url`. */
const at = (url: string) => async () => url;

const SAME = { outcome: "applied" } as const;

const tab = (id: string) =>
  dockOfTab(getBrowserState(), id)?.pane.files.find((f) => f.id === id)
    ?.browser;

describe("navigateBrowserTab rollback", () => {
  beforeEach(() => {
    pending.length = 0;
  });

  it("rolls back a rejected navigation that is still current", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-1" })!;
    const call = navigateBrowserTab(id, "https://new.test");
    pending[0].reject("blocked");
    await expect(call).rejects.toBe("blocked");
    expect(tab(id)).toMatchObject({
      url: "https://old.test",
      loading: false,
      error: "blocked",
    });
  });

  it("leaves a newer request alone when an older one rejects", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-2" })!;
    const first = navigateBrowserTab(id, "https://a.test");
    const second = navigateBrowserTab(id, "https://b.test");
    pending[0].reject("late");
    await expect(first).rejects.toBe("late");
    expect(tab(id)).toMatchObject({ url: "https://b.test", error: undefined });
    pending[1].resolve();
    await second;
  });

  it("leaves a newer load start alone when the request rejects", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-3" })!;
    const call = navigateBrowserTab(id, "https://a.test");
    noteLoadStarted(id);
    patchBrowserTab(id, { url: "https://c.test", loading: true });
    pending[0].reject("late");
    await expect(call).rejects.toBe("late");
    expect(tab(id)).toMatchObject({
      url: "https://c.test",
      loading: true,
      error: undefined,
    });
  });

  it("restores the pre-navigation url when overlapping requests both reject", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-4" })!;
    const first = navigateBrowserTab(id, "https://a.test");
    const second = navigateBrowserTab(id, "https://b.test");
    pending[0].reject("a");
    pending[1].reject("b");
    await expect(first).rejects.toBe("a");
    await expect(second).rejects.toBe("b");
    expect(tab(id)).toMatchObject({ url: "https://old.test", error: "b" });
  });

  it("preserves an active load when navigation is rejected", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-5" })!;
    noteLoadStarted(id);
    patchBrowserTab(id, { loading: true });
    const call = navigateBrowserTab(id, "https://a.test");
    pending[0].reject("blocked");
    await expect(call).rejects.toBe("blocked");
    expect(tab(id)).toMatchObject({ url: "https://old.test", loading: true });
  });

  it("does not keep state for settled requests after the tab closes", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-6" })!;
    const call = navigateBrowserTab(id, "https://a.test");
    pending[0].resolve();
    await call;
    const again = navigateBrowserTab(id, "https://b.test");
    pending[1].reject("x");
    await expect(again).rejects.toBe("x");
    // a fresh request after settlement captures the current url, not a stale one
    expect(tab(id)?.url).toBe("https://a.test");
  });

  it("rolls back to the native url when it changed before a later request rejects", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-7" })!;
    const first = navigateBrowserTab(id, "https://a.test");
    noteLoadStarted(id);
    patchBrowserTab(id, { url: "https://a.test", loading: true });
    const second = navigateBrowserTab(id, "https://b.test");
    pending[1].reject("b");
    await expect(second).rejects.toBe("b");
    expect(tab(id)).toMatchObject({ url: "https://a.test", error: "b" });
    pending[0].resolve();
    await first;
  });

  it("rolls back to an accepted navigation when a later overlapping one rejects", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-8" })!;
    const first = navigateBrowserTab(id, "https://a.test");
    const second = navigateBrowserTab(id, "https://b.test");
    pending[0].resolve();
    await first;
    pending[1].reject("b");
    await expect(second).rejects.toBe("b");
    expect(tab(id)).toMatchObject({ url: "https://a.test", error: "b" });
  });

  it("does not restore loading after the native load finished", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-9" })!;
    noteLoadStarted(id);
    patchBrowserTab(id, { loading: true });
    const call = navigateBrowserTab(id, "https://a.test");
    patchBrowserTab(id, { loading: false });
    pending[0].reject("blocked");
    await expect(call).rejects.toBe("blocked");
    expect(tab(id)).toMatchObject({ loading: false, error: "blocked" });
  });

  it("keeps a redirected native url when a request rejects after the load finished", async () => {
    const id = openBrowserTab("https://a.test", { sessionId: "nav-11" })!;
    noteLoadStarted(id);
    const call = navigateBrowserTab(id, "https://b.test");
    noteLoadFinished(id);
    patchBrowserTab(id, { url: "https://c.test", loading: false });
    pending[0].reject("blocked");
    await expect(call).rejects.toBe("blocked");
    expect(tab(id)?.url).toBe("https://c.test");
  });

  it("shows an older acceptance that arrives after a newer rejection", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-12" })!;
    const first = navigateBrowserTab(id, "https://old.test#a");
    const second = navigateBrowserTab(id, "https://b.test");
    pending[1].reject("b");
    await expect(second).rejects.toBe("b");
    expect(tab(id)?.url).toBe("https://old.test");
    pending[0].resolve();
    await first;
    expect(tab(id)?.url).toBe("https://old.test#a");
  });

  it("does not let a late acceptance override a newer request or native event", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-13" })!;
    const first = navigateBrowserTab(id, "https://a.test");
    const second = navigateBrowserTab(id, "https://b.test");
    pending[1].reject("b");
    await expect(second).rejects.toBe("b");
    const third = navigateBrowserTab(id, "https://c.test");
    pending[0].resolve();
    await first;
    expect(tab(id)?.url).toBe("https://c.test");
    pending[2].resolve();
    await third;

    const id2 = openBrowserTab("https://old.test", { sessionId: "nav-13b" })!;
    const a = navigateBrowserTab(id2, "https://a.test");
    const b = navigateBrowserTab(id2, "https://b.test");
    pending[4].reject("b");
    await expect(b).rejects.toBe("b");
    noteLoadFinished(id2);
    patchBrowserTab(id2, { url: "https://z.test" });
    pending[3].resolve();
    await a;
    expect(tab(id2)?.url).toBe("https://z.test");
  });

  it("shows the highest acceptance after a newer rejection, in either order", async () => {
    const run = async (order: [number, number], sessionId: string) => {
      const id = openBrowserTab("https://old.test", { sessionId })!;
      const base = pending.length;
      const a = navigateBrowserTab(id, "https://old.test#a");
      const b = navigateBrowserTab(id, "https://old.test#b");
      const c = navigateBrowserTab(id, "https://c.test");
      pending[base + 2].reject("c");
      await expect(c).rejects.toBe("c");
      expect(tab(id)?.url).toBe("https://old.test");
      const calls = [a, b];
      for (const i of order) {
        pending[base + i].resolve();
        await calls[i];
      }
      return id;
    };
    expect(tab(await run([0, 1], "nav-14a"))?.url).toBe("https://old.test#b");
    expect(tab(await run([1, 0], "nav-14b"))?.url).toBe("https://old.test#b");
  });

  it("does not show an older acceptance after a native event or newer request", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-15" })!;
    const base = pending.length;
    const a = navigateBrowserTab(id, "https://a.test");
    const b = navigateBrowserTab(id, "https://b.test");
    const c = navigateBrowserTab(id, "https://c.test");
    pending[base + 2].reject("c");
    await expect(c).rejects.toBe("c");
    pending[base].resolve();
    await a;
    expect(tab(id)?.url).toBe("https://a.test");
    noteLoadFinished(id);
    patchBrowserTab(id, { url: "https://z.test" });
    pending[base + 1].resolve();
    await b;
    expect(tab(id)?.url).toBe("https://z.test");
  });

  it("tolerates the tab closing mid-request", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-10" })!;
    const call = navigateBrowserTab(id, "https://a.test");
    closeBrowserTab(id);
    pending[0].reject("gone");
    await expect(call).rejects.toBe("gone");
    expect(tab(id)).toBeUndefined();
    const reopened = navigateBrowserTab(id, "https://b.test");
    pending[1].resolve();
    await reopened;
  });

  it("reports whether the page applied the target itself", async () => {
    const id = openBrowserTab("https://old.test", { sessionId: "nav-16" })!;
    const hash = navigateBrowserTab(id, "https://old.test#a");
    pending[0].resolve(SAME);
    await expect(hash).resolves.toMatchObject({ outcome: "applied" });
    const document = navigateBrowserTab(id, "https://new.test");
    pending[1].resolve();
    await expect(document).resolves.toMatchObject({ outcome: "document" });
  });
});

describe("document navigation settling", () => {
  beforeEach(() => {
    pending.length = 0;
  });

  const ready = (url = "https://old.test/") => {
    const id = openBrowserTab(url, { sessionId: `nav-s-${++chains}` })!;
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: false });
    return id;
  };

  it("keeps the optimistic url from being polled back before load start", async () => {
    const id = ready();
    const call = navigateBrowserTab(id, "https://new.test/");
    pending[0].resolve();
    await call;
    expect(isNavigating(id)).toBe(true);
    // Native still reports the old document: the poll must not revert.
    expect(await syncNativeUrl(id, at("https://old.test/"))).toBe(false);
    expect(tab(id)?.url).toBe("https://new.test/");
    // Once a load starts, protection hands off to the loading flag.
    noteLoadStarted(id);
    expect(isNavigating(id)).toBe(false);
  });

  it("keeps protecting an accepted navigation when load start is delayed", async () => {
    vi.useFakeTimers();
    try {
      const id = ready();
      const call = navigateBrowserTab(id, "https://new.test/");
      pending[0].resolve();
      await call;
      vi.advanceTimersByTime(DOCUMENT_SETTLE_MS - 1_000);
      expect(isNavigating(id)).toBe(true);
      expect(await syncNativeUrl(id, at("https://old.test/"))).toBe(false);
      expect(tab(id)?.url).toBe("https://new.test/");
    } finally {
      vi.useRealTimers();
    }
  });

  it("recovers polling when a document load never reports a start", async () => {
    vi.useFakeTimers();
    try {
      const id = ready();
      const call = navigateBrowserTab(id, "https://denied.test/");
      pending[0].resolve({ outcome: "document" });
      await call;
      vi.advanceTimersByTime(DOCUMENT_SETTLE_MS);
      expect(isNavigating(id)).toBe(false);
      expect(await syncNativeUrl(id, at("https://old.test/"))).toBe(true);
      expect(tab(id)?.url).toBe("https://old.test/");
    } finally {
      vi.useRealTimers();
    }
  });

  it("gives an unknown outcome only a short protection, then lets polling decide", async () => {
    vi.useFakeTimers();
    try {
      const id = ready();
      const call = navigateBrowserTab(id, "https://old.test/#x");
      pending[0].resolve({ outcome: "unknown" });
      await expect(call).resolves.toMatchObject({ outcome: "unknown" });
      expect(isNavigating(id)).toBe(true);
      vi.advanceTimersByTime(UNKNOWN_SETTLE_MS);
      expect(isNavigating(id)).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows where the page's handlers left an applied navigation", async () => {
    const id = ready();
    const call = navigateBrowserTab(id, "https://old.test/#b");
    pending[0].resolve({ outcome: "applied", url: "https://old.test/#c" });
    await call;
    expect(isNavigating(id)).toBe(false);
    expect(tab(id)?.url).toBe("https://old.test/#c");
  });

  it("rolls a later rejection back to the rewritten url, not the applied target", async () => {
    const id = ready();
    const applied = navigateBrowserTab(id, "https://old.test/#b");
    const rejected = navigateBrowserTab(id, "https://bad.test/");
    pending[0].resolve({ outcome: "applied", url: "https://old.test/#c" });
    await applied;
    pending[1].reject("blocked");
    await expect(rejected).rejects.toBe("blocked");
    expect(tab(id)?.url).toBe("https://old.test/#c");
  });

  it("treats a reply without an outcome as a document navigation", async () => {
    const id = ready();
    const call = navigateBrowserTab(id, "https://old.test/#x");
    pending[0].resolve(undefined);
    await call;
    // Nothing says the page applied it, so only a load start can end this.
    expect(isNavigating(id)).toBe(true);
    expect(await syncNativeUrl(id, at("https://old.test/#x"))).toBe(false);
    expect(isNavigating(id)).toBe(true);
    noteLoadStarted(id);
    expect(isNavigating(id)).toBe(false);
  });

  it("recovers polling after a lone rejected navigation", async () => {
    const id = ready();
    const call = navigateBrowserTab(id, "https://new.test/");
    pending[0].reject("blocked");
    await expect(call).rejects.toBe("blocked");
    expect(isNavigating(id)).toBe(false);
    expect(await syncNativeUrl(id, at("https://old.test/#z"))).toBe(true);
  });

  it("keeps an accepted navigation protected when an overlapping one rejects", async () => {
    const id = ready();
    const a = navigateBrowserTab(id, "https://a.test/");
    const b = navigateBrowserTab(id, "https://b.test/");
    pending[0].resolve();
    await a;
    pending[1].reject("blocked");
    await expect(b).rejects.toBe("blocked");
    expect(isNavigating(id)).toBe(true);
    expect(await syncNativeUrl(id, at("https://old.test/"))).toBe(false);
    expect(tab(id)?.url).toBe("https://a.test/");
  });

  it("drops settling state for tabs that close, including late replies", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "nav-c" })!;
    const done = navigateBrowserTab(id, "https://a.test/");
    pending[0].resolve();
    await done;
    expect(isNavigating(id)).toBe(true);
    closeBrowserTab(id);
    expect(isNavigating(id)).toBe(false);

    const id2 = openBrowserTab("https://old.test/", { sessionId: "nav-c2" })!;
    const late = navigateBrowserTab(id2, "https://a.test/");
    closeBrowserTab(id2);
    pending[1].resolve();
    await late;
    expect(isNavigating(id2)).toBe(false);
  });

  it("does not protect a navigation whose load started before the reply", async () => {
    const id = ready();
    const call = navigateBrowserTab(id, "https://a.test/");
    noteLoadStarted(id);
    noteLoadFinished(id);
    patchBrowserTab(id, { url: "https://b.test/", loading: false });
    pending[0].resolve();
    await call;
    expect(isNavigating(id)).toBe(false);
    expect(await syncNativeUrl(id, at("https://b.test/#next"))).toBe(true);
    expect(tab(id)?.url).toBe("https://b.test/#next");
  });
});

describe("same-document navigation completion", () => {
  beforeEach(() => {
    pending.length = 0;
  });

  const A = "https://old.test/#a";
  const B = "https://old.test/#b";
  const C = "https://old.test/#c";

  const chain = (seed: string, ...targets: string[]) => {
    const id = openBrowserTab(seed, { sessionId: `nav-chain-${++chains}` })!;
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: false });
    return {
      id,
      calls: targets.map((url) => navigateBrowserTab(id, url)),
      base: pending.length - targets.length,
    };
  };

  /**
   * The native side applies each fragment in the page before it replies and
   * takes requests one at a time, so the page ends at the last target whatever
   * order the replies arrive in. Every read taken after the last reply, fresh
   * and carrying nothing but the page's URL, sees that outcome.
   */
  const permutations = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
  ];

  it.each(permutations)(
    "settles A,B,A on the page's URL whatever the reply order (%i/%i/%i)",
    async (...order) => {
      const { id, calls, base } = chain("https://old.test/", A, B, A);
      // Polls while a reply is outstanding are held off, whatever they read.
      for (const [step, i] of order.entries()) {
        for (const url of ["https://old.test/", A, B]) {
          expect(await syncNativeUrl(id, at(url))).toBe(false);
        }
        expect(tab(id)?.url).toBe(A);
        expect(isNavigating(id)).toBe(true);
        pending[base + i].resolve(SAME);
        await calls[i];
        expect(isNavigating(id)).toBe(step < order.length - 1);
      }
      // Every later read is fresh. The page is at A, and later moves show up.
      for (let i = 0; i < 3; i++) {
        expect(await syncNativeUrl(id, at(A))).toBe(false);
        expect(isNavigating(id)).toBe(false);
      }
      expect(tab(id)?.url).toBe(A);
      expect(await syncNativeUrl(id, at(C))).toBe(true);
      expect(tab(id)?.url).toBe(C);
    },
  );

  const longOrders = [
    [0, 1, 2, 3, 4],
    [4, 3, 2, 1, 0],
    [2, 0, 4, 1, 3],
    [1, 3, 0, 4, 2],
    [4, 0, 3, 1, 2],
  ];

  it.each(longOrders)(
    "settles A,B,A,C,A on the page's URL whatever the reply order (%i/%i/%i/%i/%i)",
    async (...order) => {
      const { id, calls, base } = chain("https://old.test/", A, B, A, C, A);
      for (const i of order) {
        expect(await syncNativeUrl(id, at(C))).toBe(false);
        expect(tab(id)?.url).toBe(A);
        pending[base + i].resolve(SAME);
        await calls[i];
      }
      expect(isNavigating(id)).toBe(false);
      expect(await syncNativeUrl(id, at(A))).toBe(false);
      expect(tab(id)?.url).toBe(A);
      expect(await syncNativeUrl(id, at(B))).toBe(true);
      expect(tab(id)?.url).toBe(B);
    },
  );

  it("follows a page that rewrites the hash it was sent to", async () => {
    const { id, calls, base } = chain("https://old.test/", B);
    pending[base].resolve(SAME);
    await calls[0];
    // The page's hashchange handler replaced the URL before the first poll.
    expect(isNavigating(id)).toBe(false);
    expect(await syncNativeUrl(id, at(C))).toBe(true);
    expect(tab(id)?.url).toBe(C);
  });

  it("follows a page that rewrites an earlier hash in a longer chain", async () => {
    const { id, calls, base } = chain("https://old.test/", A, B);
    pending[base + 1].resolve(SAME);
    pending[base].resolve(SAME);
    await Promise.all(calls);
    expect(await syncNativeUrl(id, at(C))).toBe(true);
    expect(tab(id)?.url).toBe(C);
  });

  it("settles a no-op navigation to the URL the page is already at", async () => {
    const { id, calls, base } = chain(A, A);
    expect(isNavigating(id)).toBe(true);
    pending[base].resolve(SAME);
    await calls[0];
    expect(isNavigating(id)).toBe(false);
    expect(await syncNativeUrl(id, at(A))).toBe(false);
    expect(await syncNativeUrl(id, at(C))).toBe(true);
  });

  it("settles a navigation to the URL a tab was seeded with", async () => {
    const { id, calls, base } = chain(A, B, A);
    pending[base].resolve(SAME);
    pending[base + 1].resolve(SAME);
    await Promise.all(calls);
    expect(isNavigating(id)).toBe(false);
    for (let i = 0; i < 3; i++) {
      expect(await syncNativeUrl(id, at(A))).toBe(false);
    }
    expect(tab(id)?.url).toBe(A);
    expect(await syncNativeUrl(id, at(C))).toBe(true);
  });

  it("drops a read that began before the navigation", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "nav-v" })!;
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: false });
    let release!: (url: string) => void;
    const stale = syncNativeUrl(
      id,
      () => new Promise<string>((resolve) => (release = resolve)),
    );
    const call = navigateBrowserTab(id, A);
    pending[0].resolve(SAME);
    await call;
    // The read sampled the page before the fragment was applied.
    release("https://old.test/");
    expect(await stale).toBe(false);
    expect(tab(id)?.url).toBe(A);
  });

  it("lets a completed fragment supersede an older document request", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "nav-x" })!;
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: false });
    const doc = navigateBrowserTab(id, "https://elsewhere.test/");
    pending[0].resolve();
    await doc;
    expect(isNavigating(id)).toBe(true);
    const hash = navigateBrowserTab(id, A);
    pending[1].resolve(SAME);
    await hash;
    // The page navigated away from what the document request was waiting for.
    expect(isNavigating(id)).toBe(false);
    expect(await syncNativeUrl(id, at(A))).toBe(false);
    expect(await syncNativeUrl(id, at(C))).toBe(true);
  });

  it("keeps a newer document request protected when an older fragment completes", async () => {
    const id = openBrowserTab("https://old.test/", { sessionId: "nav-y" })!;
    setNativeTabReady(id, true);
    patchBrowserTab(id, { loading: false });
    const hash = navigateBrowserTab(id, A);
    const doc = navigateBrowserTab(id, "https://elsewhere.test/");
    pending[1].resolve();
    await doc;
    pending[0].resolve(SAME);
    await hash;
    expect(isNavigating(id)).toBe(true);
    expect(await syncNativeUrl(id, at(A))).toBe(false);
    expect(tab(id)?.url).toBe("https://elsewhere.test/");
    noteLoadStarted(id);
    expect(isNavigating(id)).toBe(false);
  });

  it("recovers polling when a fragment request is rejected", async () => {
    const { id, calls, base } = chain("https://old.test/", A);
    pending[base].reject("no webview");
    await expect(calls[0]).rejects.toBe("no webview");
    expect(isNavigating(id)).toBe(false);
    expect(tab(id)?.url).toBe("https://old.test/");
    expect(await syncNativeUrl(id, at(C))).toBe(true);
  });
});
