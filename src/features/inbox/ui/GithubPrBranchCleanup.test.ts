// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../model/githubTasks", async (original) => ({
  ...(await original<typeof import("../model/githubTasks")>()),
  githubPrBranch: vi.fn(),
  githubPrDeleteBranch: vi.fn(),
}));

import { githubPrBranch, githubPrDeleteBranch } from "../model/githubTasks";
import { GithubPrBranchCleanup } from "./GithubPrBranchCleanup";

const branch = {
  name: "feature/inbox",
  repo: "contributor/web",
  exists: true,
  canDelete: true,
  reason: "",
};
let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.mocked(githubPrBranch).mockReset().mockResolvedValue(branch);
  vi.mocked(githubPrDeleteBranch).mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(revision = 0) {
  await act(async () => {
    root.render(
      createElement(GithubPrBranchCleanup, {
        cwd: "/tmp/web",
        repo: "acme/web",
        number: 42,
        revision,
      }),
    );
  });
}

function openConfirmation() {
  act(() => container.querySelector<HTMLButtonElement>("button")!.click());
  return document.querySelector<HTMLElement>(
    '[role="dialog"][aria-label="Delete this branch?"]',
  )!;
}

async function confirm(dialog: HTMLElement) {
  await act(async () => {
    [...dialog.querySelectorAll("button")]
      .find((button) => button.textContent?.trim() === "Delete branch")!
      .click();
  });
}

describe("merged PR branch cleanup", () => {
  it("confirms the actual fork and branch, then shows deletion", async () => {
    vi.mocked(githubPrDeleteBranch).mockResolvedValue({
      ...branch,
      exists: false,
      canDelete: false,
    });
    await render();
    const dialog = openConfirmation();
    expect(dialog.textContent).toContain("feature/inbox");
    expect(dialog.textContent).toContain("contributor/web");
    expect(dialog.textContent).toContain(
      "local branch and worktrees will be kept",
    );
    expect(githubPrDeleteBranch).not.toHaveBeenCalled();
    await confirm(dialog);
    expect(githubPrDeleteBranch).toHaveBeenCalledWith(
      "/tmp/web",
      "acme/web",
      42,
    );
    expect(container.textContent).toContain("Branch deleted");
    expect(container.querySelector("button")).toBeNull();
  });

  it("can cancel without deleting", async () => {
    await render();
    const dialog = openConfirmation();
    act(() =>
      [...dialog.querySelectorAll("button")]
        .find((button) => button.textContent === "Cancel")!
        .click(),
    );
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(githubPrDeleteBranch).not.toHaveBeenCalled();
  });

  it("recognizes GitHub's automatic branch deletion on load and refresh", async () => {
    await render();
    vi.mocked(githubPrBranch).mockResolvedValue({
      ...branch,
      exists: false,
      canDelete: false,
    });
    await render(1);
    expect(container.textContent).toContain("Branch deleted");
    expect(container.querySelector("button")).toBeNull();
    expect(githubPrDeleteBranch).not.toHaveBeenCalled();
  });

  it("disables deletion when the backend identifies new commits", async () => {
    vi.mocked(githubPrBranch).mockResolvedValue({
      ...branch,
      canDelete: false,
      reason: "This branch has new commits since the pull request was merged.",
    });
    await render();
    expect(container.querySelector<HTMLButtonElement>("button")!.disabled).toBe(
      true,
    );
    expect(container.textContent).toContain("new commits");
  });

  it("keeps a failed deletion open with GitHub's error and allows retry", async () => {
    vi.mocked(githubPrDeleteBranch)
      .mockRejectedValueOnce(new Error("Branch protection prevents deletion"))
      .mockResolvedValueOnce({ ...branch, exists: false, canDelete: false });
    await render();
    const dialog = openConfirmation();
    await confirm(dialog);
    expect(dialog.querySelector('[role="alert"]')?.textContent).toContain(
      "Branch protection prevents deletion",
    );
    await confirm(dialog);
    expect(container.textContent).toContain("Branch deleted");
  });

  it("can retry a failed branch lookup", async () => {
    vi.mocked(githubPrBranch).mockRejectedValueOnce(
      new Error("Network unavailable"),
    );
    await render();
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "Network unavailable",
    );
    await act(async () =>
      [...container.querySelectorAll("button")]
        .find((button) => button.textContent === "Retry")!
        .click(),
    );
    expect(container.querySelector<HTMLButtonElement>("button")!.disabled).toBe(
      false,
    );
  });

  it("ignores lookup responses after the component unmounts", async () => {
    let resolve!: (value: typeof branch) => void;
    vi.mocked(githubPrBranch).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    await render();
    await act(async () => {
      root.render(null);
      resolve(branch);
    });
    expect(container.textContent).toBe("");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("prevents duplicate deletion and ignores a stale refresh after success", async () => {
    let finishDeletion!: (value: typeof branch) => void;
    let finishLookup!: (value: typeof branch) => void;
    vi.mocked(githubPrDeleteBranch).mockReturnValue(
      new Promise((done) => {
        finishDeletion = done;
      }),
    );
    await render();
    const dialog = openConfirmation();
    await confirm(dialog);
    expect(
      [...dialog.querySelectorAll("button")].every((button) => button.disabled),
    ).toBe(true);
    vi.mocked(githubPrBranch).mockReturnValue(
      new Promise((done) => {
        finishLookup = done;
      }),
    );
    await render(1);
    await act(async () =>
      finishDeletion({ ...branch, exists: false, canDelete: false }),
    );
    await act(async () => finishLookup(branch));
    expect(container.textContent).toContain("Branch deleted");
    expect(container.querySelector("button")).toBeNull();
    expect(githubPrDeleteBranch).toHaveBeenCalledTimes(1);
  });
});
