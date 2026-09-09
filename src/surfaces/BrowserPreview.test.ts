// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserPreview } from "./BrowserPreview";
import type { PreviewEvent } from "../hooks/useBrowserPreview";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn<
    (command: string, args?: Record<string, unknown>) => Promise<unknown>
  >(async (command) =>
    command === "browser_preview_url" ? "http://localhost:5173/" : undefined,
  ),
  events: new Set<(event: { payload: PreviewEvent }) => void>(),
  external: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (_: string, handler: (event: { payload: PreviewEvent }) => void) => {
      bridge.events.add(handler);
      return () => {
        bridge.events.delete(handler);
      };
    },
  ),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: bridge.external }));

describe("native preview lifecycle", () => {
  let root: Root;
  let container: HTMLDivElement;
  const focus = vi.fn();
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("location", new URL("http://localhost:1420"));
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
      new DOMRect(400, 100, 500, 600),
    );
    vi.spyOn(HTMLElement.prototype, "getClientRects").mockReturnValue([
      new DOMRect(400, 100, 500, 600),
    ] as unknown as DOMRectList);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    bridge.invoke.mockClear();
    bridge.external.mockClear();
    focus.mockClear();
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    bridge.events.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  async function render(url = "http://localhost:5173/") {
    await act(async () =>
      root.render(
        createElement(BrowserPreview, { initialUrl: url, onFocus: focus }),
      ),
    );
  }
  async function waitFor(assertion: () => void) {
    await act(async () => vi.waitFor(assertion));
  }
  function calls(command: string) {
    return bridge.invoke.mock.calls.filter(([name]) => name === command);
  }
  function emit(kind: PreviewEvent["kind"], url = "") {
    const id = calls("browser_preview_open")[0][1]!.id as string;
    for (const handler of bridge.events)
      handler({ payload: { id, kind, url } });
  }

  it("opens one native view and routes toolbar controls to that view", async () => {
    await render();
    expect(calls("browser_preview_open")).toHaveLength(1);
    expect(calls("browser_preview_open")[0][1]).toMatchObject({
      url: "http://localhost:5173/",
      bounds: { x: 400, y: 100, width: 500, height: 600 },
    });
    for (const [label, action] of [
      ["Back", "back"],
      ["Forward", "forward"],
      ["Reload", "reload"],
    ]) {
      await act(async () =>
        container
          .querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
          .click(),
      );
      expect(calls("browser_preview_action").at(-1)?.[1]).toMatchObject({
        action,
      });
    }
    await act(async () => emit("focus"));
    expect(focus).toHaveBeenCalledTimes(1);
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          'button[aria-label="Open in external browser"]',
        )!
        .click(),
    );
    expect(bridge.external).toHaveBeenCalledWith("http://localhost:5173/");
  });

  it("hides for menus/dialogs and inactive tabs, then restores without creating another view", async () => {
    await render();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    await act(async () => {
      document.body.append(dialog);
    });
    await waitFor(() =>
      expect(calls("browser_preview_sync").at(-1)?.[1]?.bounds).toBeNull(),
    );
    await act(async () => dialog.remove());
    await waitFor(() =>
      expect(calls("browser_preview_sync").at(-1)?.[1]?.bounds).not.toBeNull(),
    );
    await act(async () => container.setAttribute("aria-hidden", "true"));
    await waitFor(() =>
      expect(calls("browser_preview_sync").at(-1)?.[1]?.bounds).toBeNull(),
    );
    expect(calls("browser_preview_open")).toHaveLength(1);
  });

  it("closes a view even when opening finishes after the component unmounts", async () => {
    let finish!: () => void;
    bridge.invoke.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await act(async () => root.unmount());
    expect(calls("browser_preview_close")).toHaveLength(0);
    await act(async () => finish());
    await waitFor(() => expect(calls("browser_preview_close")).toHaveLength(1));
    expect(calls("browser_preview_close")[0][1]?.id).toBe(
      calls("browser_preview_open")[0][1]?.id,
    );
    expect(bridge.events.size).toBe(0);
    root = createRoot(container);
  });

  it("shows native errors and retries only after an explicit reload", async () => {
    bridge.invoke.mockRejectedValueOnce(new Error("Could not create webview"));
    await render();
    expect(container.textContent).toContain("Could not create webview");
    await act(async () => container.setAttribute("class", "changed"));
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)));
    expect(calls("browser_preview_open")).toHaveLength(1);
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[aria-label="Reload"]')!
        .click(),
    );
    expect(calls("browser_preview_open")).toHaveLength(2);
  });

  it("does not create a view for an app or file URL", async () => {
    await render("file:///etc/passwd");
    expect(calls("browser_preview_open")).toHaveLength(0);
    expect(container.textContent).toContain("Enter an HTTP or HTTPS address");
  });

  it("explains blocked page actions and clears the notice after navigation succeeds", async () => {
    await render();
    for (const [kind, message] of [
      ["blocked", "This address cannot be opened"],
      ["popup", "This page requested another window"],
      ["download", "Use the external browser to download"],
    ] as const) {
      await act(async () => emit(kind));
      expect(container.querySelector('[role="status"]')?.textContent).toContain(
        message,
      );
      await act(async () => emit("url", "http://localhost:5173/"));
      expect(container.querySelector('[role="status"]')).not.toBeNull();
      await act(async () => emit("loaded", "http://localhost:5173/next"));
      expect(container.querySelector('[role="status"]')).toBeNull();
    }
  });
});
