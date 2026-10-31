import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

async function load() {
  vi.resetModules();
  const browser = await import("../../browser/model/browserStore");
  const store = await import("./sessionStore");
  return { browser, store };
}

const docked = (browser: {
  getBrowserState: () => { docks: { sessionId: string }[] };
}) =>
  browser
    .getBrowserState()
    .docks.map((dock) => dock.sessionId)
    .sort();

afterEach(() => {
  mocks.invoke.mockReset();
});

describe("deleting a session", () => {
  it("forgets that session's browser tabs, and only after the delete succeeds", async () => {
    const { browser, store } = await load();
    browser.openBrowserTab("https://a.test/", { sessionId: "gone" });
    browser.openBrowserTab("https://b.test/", { sessionId: "kept" });
    expect(docked(browser)).toEqual(["gone", "kept"]);

    mocks.invoke.mockRejectedValueOnce(new Error("disk full"));
    await expect(store.deleteSession("gone")).rejects.toThrow("disk full");
    expect(docked(browser)).toEqual(["gone", "kept"]);

    mocks.invoke.mockResolvedValue(undefined);
    await store.deleteSession("gone");
    expect(docked(browser)).toEqual(["kept"]);
  });
});
