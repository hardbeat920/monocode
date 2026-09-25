// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { QuickGitPopup } from "./QuickGitPopup";
import { gitBranches, type GitBranches } from "../../../platform/tauri/fs";
import type { QuickGitRequest } from "../model/quickGitPopup";

const bridge = vi.hoisted(() => ({
  request: null as QuickGitRequest | null,
  receive: (_event: { payload: QuickGitRequest }) => {},
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) =>
    command === "quick_git_state" ? bridge.request : undefined,
  ),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, callback: typeof bridge.receive) => {
    bridge.receive = callback;
    return () => {};
  },
}));
vi.mock("../../../platform/tauri/fs", async (actual) => ({
  ...(await actual<object>()),
  gitBranches: vi.fn(),
  subscribeGitChanged: () => () => {},
}));

let root: Root;
let container: HTMLDivElement;
let resolveBranches: (branches: GitBranches) => void;
const onShown = vi.fn();
const snapshot: GitBranches = {
  current: "main",
  detached: false,
  branches: [{ name: "main", current: true, remote: null }],
};

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.mocked(gitBranches).mockImplementation(
    () =>
      new Promise((resolve) => {
        resolveBranches = resolve;
      }),
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it.each(["workspace", "base", "branch"] as const)(
  "shows the full %s menu on a cold webview before Git revalidation finishes",
  async (kind) => {
    bridge.request = {
      id: crypto.randomUUID(),
      kind,
      choice: { cwd: `/cold-${kind}`, mode: "worktree" },
      branches: snapshot,
      anchor: { x: 0, y: 0, width: 100, height: 24 },
    };
    await act(async () =>
      root.render(
        createElement(
          StrictMode,
          null,
          createElement(QuickGitPopup, { onShown }),
        ),
      ),
    );
    expect(container.textContent).not.toContain("Loading branches");
    expect(container.textContent).toContain(
      kind === "workspace" ? "New worktree" : "main",
    );
    expect(gitBranches).toHaveBeenCalledWith(`/cold-${kind}`);
    // The carried snapshot does not disable normal Git refreshes.
    await act(async () =>
      resolveBranches({
        ...snapshot,
        branches: [
          ...snapshot.branches,
          { name: "fresh-branch", current: false, remote: null },
        ],
      }),
    );
    if (kind !== "workspace")
      expect(container.textContent).toContain("fresh-branch");
  },
);

it("uses the new project's snapshot when reusing the popup", async () => {
  bridge.request = {
    id: "first-project",
    kind: "branch",
    choice: { cwd: "/first-project", mode: "current" },
    branches: snapshot,
    anchor: { x: 0, y: 0, width: 100, height: 24 },
  };
  await act(async () => root.render(createElement(QuickGitPopup, { onShown })));
  await act(async () =>
    bridge.receive({
      payload: {
        ...bridge.request!,
        id: "second-project",
        choice: { cwd: "/second-project", mode: "current" },
        branches: {
          current: "develop",
          detached: false,
          branches: [{ name: "develop", current: true, remote: null }],
        },
      },
    }),
  );
  expect(container.textContent).toContain("develop");
  expect(container.textContent).not.toContain("main");
  expect(container.textContent).not.toContain("Loading branches");
});
