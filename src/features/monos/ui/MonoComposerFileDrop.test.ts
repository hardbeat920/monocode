// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { MonoComposer } from "./MonoComposer";

const { invoke, listen } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: listen }),
}));

let container: HTMLDivElement;
let root: Root;
let handlers: Set<(event: { payload: DragDropEvent }) => void>;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    disconnect() {}
  });
  handlers = new Set();
  listen.mockReset();
  listen.mockImplementation(async (handler) => {
    handlers.add(handler);
    return () => handlers.delete(handler);
  });
  invoke.mockReset();
  invoke.mockImplementation(async (command: string, args: { paths?: string[] }) =>
    command === "inspect_paths"
      ? (args.paths ?? []).map((path) => ({
          path,
          name: path.split("/").pop() ?? path,
          size: 12,
          isDir: false,
        }))
      : [],
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render() {
  await act(async () => {
    root.render(
      createElement(
        "div",
        { "data-session-drop": "mono" },
        createElement("div", { "data-transcript": true }),
        createElement(MonoComposer, {
          sessionId: "mono",
          name: "Captain Awesome",
          onSubmit: vi.fn(),
        }),
      ),
    );
  });
  const pane = container.querySelector<HTMLElement>("[data-session-drop]")!;
  pane.getBoundingClientRect = () =>
    ({ left: 0, top: 0, right: 500, bottom: 500 }) as DOMRect;
  return pane;
}

async function native(payload: DragDropEvent) {
  await act(async () => {
    for (const handler of handlers) handler({ payload });
  });
}

it("attaches files dropped anywhere on the Mono chat", async () => {
  await render();
  const layout = () =>
    container.querySelector<HTMLElement>("[data-layout]")!.dataset.layout;
  expect(layout()).toBe("inline");
  await native({ type: "over", position: { x: 50, y: 20 } } as DragDropEvent);
  expect(container.textContent).toContain("Drop files to attach");

  await native({
    type: "drop",
    paths: ["/Users/me/photo.png"],
    position: { x: 50, y: 20 },
  } as DragDropEvent);
  expect(container.textContent).not.toContain("Drop files to attach");
  expect(container.querySelector('[title="/Users/me/photo.png"]')).not.toBeNull();
  // An attachment moves the field above the buttons, like a second line.
  expect(layout()).toBe("multiline");
});

it("attaches DOM file drops on the transcript area", async () => {
  await render();
  const transcript = container.querySelector<HTMLElement>("[data-transcript]")!;
  const file = Object.assign(
    new File(["image"], "image.png", { type: "image/png" }),
    { path: "/Users/me/image.png" },
  );
  const drop = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(drop, "dataTransfer", {
    value: { types: ["Files"], files: [file], items: [], dropEffect: "none" },
  });
  await act(async () => {
    transcript.dispatchEvent(drop);
  });
  expect(drop.defaultPrevented).toBe(true);
  expect(container.querySelector('[title="/Users/me/image.png"]')).not.toBeNull();
});

it("ignores native drops outside the chat", async () => {
  await render();
  await native({
    type: "drop",
    paths: ["/Users/me/photo.png"],
    position: { x: 900, y: 900 },
  } as DragDropEvent);
  expect(invoke).not.toHaveBeenCalledWith("inspect_paths", expect.anything());
});
