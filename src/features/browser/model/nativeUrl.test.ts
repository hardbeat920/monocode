import { afterEach, describe, expect, it } from "vitest";
import {
  closeBrowserTab,
  dockOfTab,
  getBrowserState,
  noteLoadStarted,
  nativeUrlEpoch,
  openBrowserTab,
  parseBrowserState,
  patchBrowserTab,
  serializeBrowserState,
  setBrowserSession,
} from "./browserStore";
import { setNativeTabReady } from "./nativeTabs";
import { syncNativeUrl, trackedBrowserTabIds } from "./nativeUrl";

const tab = (id: string) =>
  dockOfTab(getBrowserState(), id)?.pane.files.find((f) => f.id === id)
    ?.browser;

const opened: string[] = [];
function openLive(url = "https://example.com/app") {
  const id = openBrowserTab(url, { sessionId: "s1", background: true })!;
  opened.push(id);
  setNativeTabReady(id, true);
  patchBrowserTab(id, { loading: false });
  return id;
}

afterEach(() => {
  for (const id of opened.splice(0)) {
    setNativeTabReady(id, false);
    if (tab(id)) closeBrowserTab(id);
  }
});

const at = (url: string) => async () => url;

describe("same-document URL tracking", () => {
  it.each([
    ["pushState", "https://example.com/app/items/7"],
    ["replaceState", "https://example.com/app?tab=2"],
    ["hash change", "https://example.com/app#section"],
  ])("applies a %s URL without touching loading", async (_, url) => {
    const id = openLive();
    patchBrowserTab(id, { title: "App" });
    const epoch = nativeUrlEpoch(id);
    expect(await syncNativeUrl(id, at(url))).toBe(true);
    expect(tab(id)).toMatchObject({ url, title: "App", loading: false });
    expect(nativeUrlEpoch(id)).toBe(epoch + 1);
  });

  it("changes nothing when the URL is the same", async () => {
    const id = openLive();
    const epoch = nativeUrlEpoch(id);
    expect(await syncNativeUrl(id, at(tab(id)!.url))).toBe(false);
    expect(nativeUrlEpoch(id)).toBe(epoch);
  });

  it("drops a reply when a load started while it was in flight", async () => {
    const id = openLive();
    const result = await syncNativeUrl(id, async () => {
      noteLoadStarted(id);
      patchBrowserTab(id, { loading: true });
      return "https://example.com/stale";
    });
    expect(result).toBe(false);
    expect(tab(id)?.url).not.toBe("https://example.com/stale");
  });

  it("drops a reply when the URL was written while it was in flight", async () => {
    const id = openLive();
    const result = await syncNativeUrl(id, async () => {
      patchBrowserTab(id, { url: "https://example.com/typed" });
      return "https://example.com/stale";
    });
    expect(result).toBe(false);
    expect(tab(id)?.url).toBe("https://example.com/typed");
  });

  it("drops a reply for a tab that was closed or suspended", async () => {
    const closing = openLive();
    expect(
      await syncNativeUrl(closing, async () => {
        closeBrowserTab(closing);
        return "https://example.com/x";
      }),
    ).toBe(false);

    const suspending = openLive();
    expect(
      await syncNativeUrl(suspending, async () => {
        setNativeTabReady(suspending, false);
        return "https://example.com/y";
      }),
    ).toBe(false);
    expect(tab(suspending)?.url).toBe("https://example.com/app");
  });

  it("ignores read failures and non-web URLs", async () => {
    const id = openLive();
    expect(
      await syncNativeUrl(id, async () => {
        throw new Error("Browser tab is not open");
      }),
    ).toBe(false);
    expect(await syncNativeUrl(id, at("file:///etc/passwd"))).toBe(false);
  });

  it("tracks only live tabs with a native page", () => {
    setBrowserSession("s1");
    const live = openLive();
    const suspended = openBrowserTab("https://example.com/s", {
      sessionId: "s1",
      background: true,
    })!;
    opened.push(suspended);
    const ids = trackedBrowserTabIds();
    expect(ids).toContain(live);
    expect(ids).not.toContain(suspended);
    setNativeTabReady(live, false);
    expect(trackedBrowserTabIds()).not.toContain(live);
  });

  it("restores a suspended tab at its pushState URL", async () => {
    const id = openLive();
    await syncNativeUrl(id, at("https://example.com/app/items/7"));
    setNativeTabReady(id, false); // suspended: the view is gone
    const restored = parseBrowserState(
      JSON.parse(JSON.stringify(serializeBrowserState(getBrowserState()))),
    );
    const file = restored.docks
      .flatMap((dock) => dock.pane.files)
      .find((f) => f.id === id);
    expect(file?.browser?.url).toBe("https://example.com/app/items/7");
  });
});
