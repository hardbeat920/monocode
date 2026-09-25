// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { QuickWorkspaceControls } from "./QuickWorkspaceControls";
import type { QuickGitResult } from "../model/quickGitPopup";

const bridge = vi.hoisted(() => ({
  result: (_event: { payload: QuickGitResult }) => {},
  repo: true,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, callback: typeof bridge.result) => {
    bridge.result = callback;
    return () => {};
  },
}));
vi.mock("../../../platform/tauri/fs", () => ({ notifyGitChanged: vi.fn() }));
vi.mock("../../source-control/hooks/useProjectBranches", () => ({
  useProjectBranchesState: () => ({
    settled: true,
    branches: bridge.repo ? { current: "main" } : null,
  }),
}));
let root: Root;
let container: HTMLDivElement;
const onChange = vi.fn();
const onOpenChange = vi.fn();
const onClose = vi.fn();
const onError = vi.fn();
async function render() {
  await act(async () =>
    root.render(
      createElement(QuickWorkspaceControls, {
        value: { cwd: "/repo", mode: "current" },
        enabled: true,
        onChange,
        onOpenChange,
        onClose,
        onError,
      }),
    ),
  );
}
function button(label: string) {
  return container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
}
function request() {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "quick_git_open")
    .at(-1)![1] as { request: { id: string; kind: string; choice: unknown } };
}
beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(invoke).mockResolvedValue(undefined);
  bridge.repo = true;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await render();
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("opens a native picker without adding a menu or spacer to the composer", async () => {
  await act(async () => button("Choose branch").click());
  expect(request().request).toMatchObject({
    kind: "branch",
    choice: { cwd: "/repo", mode: "current" },
    branches: { current: "main" },
  });
  expect(onOpenChange).toHaveBeenLastCalledWith(true);
  expect(container.querySelector('[role="dialog"]')).toBeNull();
  expect(invoke).not.toHaveBeenCalledWith(
    "quick_composer_fit",
    expect.anything(),
  );
  const id = request().request.id;
  act(() =>
    bridge.result({
      payload: {
        id,
        choice: { cwd: "/repo", mode: "worktree", base: "main" },
        restoreFocus: true,
      },
    }),
  );
  expect(onChange).toHaveBeenCalledWith({
    cwd: "/repo",
    mode: "worktree",
    base: "main",
  });
  expect(onOpenChange).toHaveBeenLastCalledWith(false);
  expect(onClose).toHaveBeenCalledOnce();
});

it("ignores results from a picker replaced by a newer request", async () => {
  await act(async () => button("Choose branch").click());
  const oldId = request().request.id;
  await act(async () => button("Workspace Current checkout").click());
  act(() =>
    bridge.result({
      payload: {
        id: oldId,
        choice: { cwd: "/repo", mode: "worktree" },
        restoreFocus: true,
      },
    }),
  );
  expect(onChange).not.toHaveBeenCalled();
  expect(
    button("Workspace Current checkout").getAttribute("aria-expanded"),
  ).toBe("true");
});

it("does not steal focus after clicking outside the popup", async () => {
  await act(async () => button("Choose branch").click());
  act(() =>
    bridge.result({
      payload: { id: request().request.id, restoreFocus: false },
    }),
  );
  expect(onOpenChange).toHaveBeenLastCalledWith(false);
  expect(onClose).not.toHaveBeenCalled();
});

it("recovers from failure to open a popup and disables controls outside repositories", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("Window failed"));
  await act(async () => button("Choose branch").click());
  expect(onError).toHaveBeenCalledWith("Error: Window failed");
  expect(onOpenChange).toHaveBeenLastCalledWith(false);
  bridge.repo = false;
  await render();
  expect(button("Choose branch").disabled).toBe(true);
  expect(button("Workspace Current checkout").disabled).toBe(true);
});

it("keeps a trigger toggle closed when blur arrives between mousedown and click", async () => {
  await act(async () => button("Choose branch").click());
  const id = request().request.id;
  act(() =>
    button("Choose branch").dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true }),
    ),
  );
  act(() => bridge.result({ payload: { id, restoreFocus: false } }));
  await act(async () => button("Choose branch").click());
  expect(
    vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "quick_git_open"),
  ).toHaveLength(1);
  await act(async () => button("Choose branch").click());
  expect(
    vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "quick_git_open"),
  ).toHaveLength(2);
});

it("handles native blur before DOM mousedown and still allows switching pickers", async () => {
  await act(async () => button("Choose branch").click());
  act(() =>
    bridge.result({
      payload: {
        id: request().request.id,
        restoreFocus: false,
        triggerKind: "branch",
      },
    }),
  );
  act(() =>
    button("Choose branch").dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true }),
    ),
  );
  await act(async () => button("Choose branch").click());
  expect(
    vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "quick_git_open"),
  ).toHaveLength(1);
  await act(async () => button("Choose branch").click());
  act(() =>
    button("Workspace Current checkout").dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true }),
    ),
  );
  act(() =>
    bridge.result({
      payload: { id: request().request.id, restoreFocus: false },
    }),
  );
  await act(async () => button("Workspace Current checkout").click());
  expect(request().request.kind).toBe("workspace");
});
