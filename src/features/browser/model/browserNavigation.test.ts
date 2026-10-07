import { beforeEach, describe, expect, it, vi } from "vitest";

const pending: Array<{
  resolve: () => void;
  reject: (reason: unknown) => void;
}> = [];
vi.mock("../../../platform/tauri/browser", () => ({
  navigateBrowser: vi.fn(
    () =>
      new Promise<void>((resolve, reject) => pending.push({ resolve, reject })),
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
import { navigateBrowserTab } from "./browserNavigation";

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
});
