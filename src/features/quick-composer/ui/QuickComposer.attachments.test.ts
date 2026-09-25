// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { QuickComposer } from "./QuickComposer";

const native = vi.hoisted(() => ({
  hide: vi.fn(),
  shown: () => {},
  submitFails: false,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: native.hide }),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn().mockResolvedValue(undefined),
  listen: vi.fn(async (name, callback) => {
    if (name === "quick_composer_shown") native.shown = callback;
    return () => {};
  }),
}));
vi.mock("../../../platform/tauri/fs", () => ({
  pickFiles: async () => ["/tmp/image.png"],
  basename: (path: string) => path.split("/").pop(),
  subscribeGitChanged: () => () => {},
  gitBranches: async () => null,
}));
vi.mock("./useQuickPickerMotion", () => ({ useQuickPickerMotion: () => {} }));
vi.mock("./QuickProjectIcon", () => ({
  QuickProjectIcon: () => null,
  loadQuickProjectAppearance: () => ({}),
}));
vi.mock("./QuickModelSelector", () => ({ QuickModelSelector: () => null }));
vi.mock("../model/quickComposer", async (actual) => ({
  ...(await actual<object>()),
  loadQuickProjects: () => ["/tmp/project"],
  initialQuickChoice: () => ({ harness: "codex", model: "test" }),
  resolveQuickModel: () => ({
    harness: "codex",
    id: "test",
    name: "Test model",
  }),
}));

let root: Root;
let container: HTMLDivElement;
function button(label: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) =>
      button.textContent?.trim() === label ||
      button.getAttribute("aria-label") === label,
  )!;
}
async function attach() {
  act(() => button("Add attachment").click());
  await act(async () => {
    button("Choose files…").click();
  });
}
beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: () => void) => {
    callback();
    return 0;
  });
  native.submitFails = false;
  vi.mocked(invoke).mockImplementation(async (cmd) => {
    if (cmd === "inspect_paths")
      return [
        { path: "/tmp/image.png", name: "image.png", size: 4, isDir: false },
      ];
    if (cmd === "read_file_base64") return "dGVzdA==";
    if (cmd === "quick_composer_submit" && native.submitFails)
      throw new Error("Could not start session");
    return undefined;
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () =>
    root.render(createElement(QuickComposer, { onShown: () => {} })),
  );
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("uploads from the plus menu, preserves attachments on dismissal, and submits them", async () => {
  await attach();
  expect(
    container.querySelector('button[aria-label="Open image.png full screen"]'),
  ).not.toBeNull();
  act(() => button("Close composer").click());
  expect(invoke).toHaveBeenCalledWith("quick_composer_dismiss");
  await act(async () => native.shown());
  expect(
    container.querySelector('button[aria-label="Open image.png full screen"]'),
  ).not.toBeNull();
  expect(button("Start").disabled).toBe(false);
  await act(async () => button("Start").click());
  expect(invoke).toHaveBeenCalledWith("quick_composer_submit", {
    request: expect.objectContaining({
      prompt: "",
      attachments: [
        expect.objectContaining({ path: "/tmp/image.png", kind: "image" }),
      ],
    }),
  });
  expect(
    container.querySelector('button[aria-label="Open image.png full screen"]'),
  ).toBeNull();
});

it("retains attachments for retry if starting the session fails", async () => {
  await attach();
  native.submitFails = true;
  await act(async () => button("Start").click());
  expect(container.textContent).toContain("Could not start session");
  expect(
    container.querySelector('button[aria-label="Open image.png full screen"]'),
  ).not.toBeNull();
  expect(button("Start").disabled).toBe(false);
});
