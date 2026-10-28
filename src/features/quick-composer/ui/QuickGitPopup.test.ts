// @vitest-environment happy-dom
import { act, createElement, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { gitCheckout, gitCreateBranch } from "../../../platform/tauri/fs";
import { QuickGitPopupPicker } from "./QuickGitPopup";
import { NativePopupHost } from "../../../shared/ui/NativePopupHost";
import type { QuickGitRequest } from "../model/quickGitPopup";
import type { QuickWorkspace } from "../model/quickWorkspace";

const git = vi.hoisted(() => ({ available: true, settled: true }));
vi.mock("../../source-control/hooks/useProjectBranches", () => ({
  useProjectBranchesState: () => ({
    settled: git.settled,
    branches: git.available
      ? {
          current: "main",
          detached: false,
          branches: [
            { name: "main", current: true, remote: null },
            { name: "develop", current: false, remote: "origin" },
          ],
        }
      : null,
  }),
}));
vi.mock("../../source-control/hooks/useProjectWorktrees", () => ({
  useProjectWorktrees: () => ({
    data: {
      worktrees: [
        {
          path: "/repo-feature",
          branch: "feature",
          head: "abc",
          isMain: false,
          missing: false,
        },
      ],
    },
  }),
}));
vi.mock("../../../platform/tauri/fs", async (actual) => ({
  ...(await actual<object>()),
  gitCheckout: vi.fn().mockResolvedValue(undefined),
  gitCreateBranch: vi.fn(),
  notifyGitChanged: vi.fn(),
}));

let root: Root;
let container: HTMLDivElement;
let selection: QuickWorkspace;
let request: QuickGitRequest;
const onFinish = vi.fn(async (_id: string, choice?: QuickWorkspace) => {
  if (choice) selection = choice;
});
function Harness() {
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  return createElement(
    "div",
    { ref: setHost },
    host
      ? createElement(
          NativePopupHost,
          { value: host },
          createElement(QuickGitPopupPicker, {
            key: request.id,
            request,
            onFinish,
          }),
        )
      : null,
  );
}
async function render(
  kind: QuickGitRequest["kind"],
  choice: QuickWorkspace = { cwd: "/repo", mode: "current" },
) {
  request = {
    id: crypto.randomUUID(),
    kind,
    choice,
    anchor: { x: 0, y: 0, width: 24, height: 24 },
  };
  await act(async () => root.render(createElement(Harness)));
}
function button(label: string) {
  return [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (el) =>
      el.textContent?.trim() === label ||
      el.getAttribute("aria-label") === label,
  )!;
}
beforeEach(async () => {
  git.available = true;
  git.settled = true;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await render("workspace");
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("selects a new worktree and base branch without checking out or creating anything yet", async () => {
  await act(async () => button("New worktree").click());
  expect(selection).toMatchObject({ mode: "worktree", base: "main" });
  await render("base", selection);
  await act(async () => button("origin/develop").click());
  expect(selection.base).toBe("origin/develop");
  expect(gitCheckout).not.toHaveBeenCalled();
  expect(gitCreateBranch).not.toHaveBeenCalled();
});

it.each(["workspace", "base"] as const)(
  "keeps the first %s request open while branches load",
  async (kind) => {
    git.available = false;
    git.settled = false;
    await render(kind, { cwd: "/cold-repo", mode: "worktree" });
    expect(container.textContent).toContain("Loading branches…");
    expect(onFinish).not.toHaveBeenCalled();

    git.available = true;
    git.settled = true;
    // Same request: no close/reopen to recover the initially empty cache.
    await act(async () => root.render(createElement(Harness)));
    await act(async () =>
      button(kind === "workspace" ? "New worktree" : "origin/develop").click(),
    );
    expect(onFinish).toHaveBeenCalledTimes(1);
  },
);

it("shows a dismissible message when the first branch lookup fails", async () => {
  git.available = false;
  await render("workspace");
  expect(container.textContent).toContain("Couldn’t load branches");
  await act(async () => button("Close").click());
  expect(onFinish).toHaveBeenCalledWith(request.id, undefined);
});

it("uses the main composer's branch checkout flow in the current working copy", async () => {
  await render("branch");
  const option = document.querySelector<HTMLButtonElement>(
    '[role="listbox"][aria-label="Branches"] [role="option"]:last-child',
  )!;
  await act(async () => option.click());
  expect(gitCheckout).toHaveBeenCalledWith("/repo", "develop", "origin");
});

it("selects an existing working copy for the upcoming session", async () => {
  await act(async () => button("Existing worktree…").click());
  const tree = document.querySelector<HTMLButtonElement>(
    '[role="menuitem"][title="/repo-feature"]',
  )!;
  await act(async () => tree.click());
  expect(selection).toMatchObject({
    mode: "current",
    cwd: "/repo",
    tree: { path: "/repo-feature" },
  });
  expect(gitCheckout).not.toHaveBeenCalled();
});

it("dismisses with Escape without changing the selection", async () => {
  await render("branch");
  await act(async () =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(onFinish).toHaveBeenCalledWith(request.id, undefined);
});

it("keeps branch creation inside the popup and finishes only after creating the branch", async () => {
  await render("branch");
  await act(async () => button("New branch").click());
  expect(onFinish).not.toHaveBeenCalled();
  const input = document.querySelector<HTMLInputElement>(
    '[aria-label="Branch name"]',
  )!;
  expect(input).not.toBeNull();
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, "feature/popup");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Create branch").click());
  expect(gitCreateBranch).toHaveBeenCalledWith("/repo", "feature/popup");
  expect(onFinish).toHaveBeenCalledTimes(1);
});
