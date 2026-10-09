// @vitest-environment happy-dom
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { QuickGitPopup } from "./QuickGitPopup";
import { gitBranches, type GitBranches } from "../../../platform/tauri/fs";
import type { QuickGitRequest } from "../model/quickGitPopup";

const bridge = vi.hoisted(() => ({
  request: null as QuickGitRequest | null,
  receive: (_event: { payload: QuickGitRequest }) => {},
}));
const nativeHost = vi.hoisted(() => ({
  activeTextJobs: new Set<string>(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: { sessionId?: string }) => {
    if (command === "harness_kill" && args?.sessionId) {
      nativeHost.activeTextJobs.delete(args.sessionId);
    }
    return command === "quick_git_state" ? bridge.request : undefined;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, callback: typeof bridge.receive) => {
    if (name === "quick_git_request") bridge.receive = callback;
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
  nativeHost.activeTextJobs.clear();
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

it("clears the popup request after native completion", async () => {
  bridge.request = {
    id: "completed-project",
    kind: "branch",
    choice: { cwd: "/repo", mode: "current" },
    branches: snapshot,
    anchor: { x: 0, y: 0, width: 100, height: 24 },
  };
  await act(async () => root.render(createElement(QuickGitPopup, { onShown })));

  const current = document.querySelector<HTMLButtonElement>(
    '[role="option"][aria-selected="true"]',
  )!;
  await act(async () => current.click());

  expect(invoke).toHaveBeenCalledWith(
    "quick_git_complete",
    expect.objectContaining({ id: "completed-project" }),
  );
  expect(container.querySelector('[role="option"]')).toBeNull();
});

it("keeps another window's text jobs alive when the branch picker closes", async () => {
  nativeHost.activeTextJobs.add("monocode-text");
  nativeHost.activeTextJobs.add("monocode-codex-text");
  bridge.request = {
    id: "unused-branch-picker",
    kind: "branch",
    choice: { cwd: "/repo", mode: "current" },
    branches: snapshot,
    anchor: { x: 0, y: 0, width: 100, height: 24 },
  };
  await act(async () => root.render(createElement(QuickGitPopup, { onShown })));

  const current = document.querySelector<HTMLButtonElement>(
    '[role="option"][aria-selected="true"]',
  )!;
  await act(async () => current.click());

  expect(container.querySelector('[role="option"]')).toBeNull();
  expect(nativeHost.activeTextJobs).toEqual(
    new Set(["monocode-text", "monocode-codex-text"]),
  );
});
