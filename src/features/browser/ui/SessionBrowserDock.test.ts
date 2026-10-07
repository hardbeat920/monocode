// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/browser", () => ({
  openBrowserView: vi.fn(async () => undefined),
  closeBrowserView: vi.fn(async () => undefined),
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  navigateBrowser: vi.fn(async () => undefined),
  browserHistory: vi.fn(async () => undefined),
  retainBrowserViews: vi.fn(async () => undefined),
  onBrowserEvent: vi.fn(async () => () => undefined),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => {
    throw new Error("no window");
  },
}));

import { BrowserDockLayout } from "./SessionBrowserDock";
import {
  closeBrowserTab,
  forgetBrowserSessions,
  hideBrowser,
  openBrowserTab,
  patchBrowserTab,
  selectBrowserTab,
  toggleBrowser,
} from "../model/browserStore";

let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  forgetBrowserSessions(["chat-a"]);
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  forgetBrowserSessions(["chat-a"]);
  vi.unstubAllGlobals();
});

describe("BrowserDockLayout", () => {
  it("shows one toolbar however many tabs links open", async () => {
    await act(async () =>
      root.render(
        createElement(BrowserDockLayout, {
          sessionId: "chat-a",
          hidden: false,
          children: null,
        }),
      ),
    );
    const ids: string[] = [];
    const expectAddress = (url: string) => {
      const bars = container.querySelectorAll('input[aria-label="Address"]');
      expect(bars).toHaveLength(1);
      expect((bars[0] as HTMLInputElement).value).toBe(url);
    };
    for (const url of [
      "https://a.test/",
      "https://b.test/",
      "https://c.test/",
    ]) {
      await act(async () => {
        ids.push(openBrowserTab(url)!);
      });
      expectAddress(url);
    }
    await act(async () => selectBrowserTab(ids[0]));
    expectAddress("https://a.test/");
    await act(async () =>
      patchBrowserTab(ids[0], { url: "https://a.test/next" }),
    );
    expectAddress("https://a.test/next");
    await act(async () => closeBrowserTab(ids[0]));
    expectAddress("https://b.test/");
    await act(async () => hideBrowser());
    expect(
      container.querySelectorAll('input[aria-label="Address"]'),
    ).toHaveLength(0);
    await act(async () => toggleBrowser());
    expectAddress("https://b.test/");
  });
});
