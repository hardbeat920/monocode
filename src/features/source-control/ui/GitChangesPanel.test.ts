// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

const { invalidateWatchedFiles } = vi.hoisted(() => ({
  invalidateWatchedFiles: vi.fn(),
}));

vi.mock("../../../platform/tauri/fs", () => ({
  gitDiffIndex: vi.fn(),
  gitHistory: vi.fn(async () => []),
  gitPrStatus: vi.fn(async () => null),
  gitPull: vi.fn(async () => {}),
  gitPush: vi.fn(async () => {}),
  gitSync: vi.fn(async () => {}),
  gitCommit: vi.fn(async () => {}),
  gitHeadMessage: vi.fn(async () => ""),
  gitStageAll: vi.fn(async () => {}),
  gitUnstageAll: vi.fn(async () => {}),
  gitDiscardAll: vi.fn(async () => {}),
  gitStageFile: vi.fn(async () => {}),
  gitUnstageFile: vi.fn(async () => {}),
  gitDiscardFile: vi.fn(async () => {}),
  gitPrCreate: vi.fn(async () => ""),
  gitRangeContext: vi.fn(),
  notifyGitChanged: vi.fn(),
  subscribeGitChanged: () => () => {},
  basename: (path: string) => path.split("/").pop() ?? path,
}));

vi.mock("../../../integrations/harness", () => ({
  generateCommitMessage: vi.fn(async () => ""),
  generatePrContent: vi.fn(async () => null),
}));

vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles,
  nudgeWatchedFiles: vi.fn(),
}));

vi.mock("../../inbox/model/inboxSelfActivity", () => ({
  recordInboxSelfActivity: vi.fn(),
}));

import { GitChangesPanel } from "./GitChangesPanel";
import {
  gitDiffIndex,
  gitPrCreate,
  gitPull,
  gitPush,
  gitRangeContext,
} from "../../../platform/tauri/fs";
import {
  generateCommitMessage,
  generatePrContent,
} from "../../../integrations/harness";
import { recordInboxSelfActivity } from "../../inbox/model/inboxSelfActivity";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { GitDiffIndex } from "../../../platform/tauri/fs";

function index(overrides: Partial<GitDiffIndex> = {}): GitDiffIndex {
  return {
    branch: "feature/pull",
    head: "abc123",
    files: [],
    additions: 0,
    deletions: 0,
    remote: null,
    upstream: null,
    defaultBranch: "main",
    ahead: 0,
    behind: 0,
    aheadOfDefault: 0,
    headPushed: true,
    ...overrides,
  };
}

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
  vi.mocked(gitDiffIndex).mockReset();
  vi.mocked(gitPull).mockReset();
  vi.mocked(generateCommitMessage).mockReset();
  invalidateWatchedFiles.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

describe("GitChangesPanel commit message generation", () => {
  it("cancels promptly and ignores a late result after a retry", async () => {
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({
        files: [
          {
            path: "/repo/change.ts",
            relative: "change.ts",
            status: "modified",
            additions: 1,
            deletions: 0,
            staged: true,
            unstaged: false,
          },
        ],
      }),
    );
    let resolveFirst!: (message: string) => void;
    vi.mocked(generateCommitMessage)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValueOnce("New message");
    await renderPanel();

    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Generate commit message"]',
        )!
        .click();
    });
    const signal = vi.mocked(generateCommitMessage).mock.calls[0]?.[2];
    expect(signal?.aborted).toBe(false);

    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Cancel commit message generation"]',
        )!
        .click();
    });
    expect(signal?.aborted).toBe(true);
    expect(
      container.querySelector<HTMLButtonElement>(
        '[aria-label="Generate commit message"]',
      )?.disabled,
    ).toBe(false);
    expect(container.querySelector("textarea")?.disabled).toBe(false);

    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Generate commit message"]',
        )!
        .click();
    });
    expect(container.querySelector("textarea")?.value).toBe("New message");

    await act(async () => resolveFirst("Old message"));
    expect(container.querySelector("textarea")?.value).toBe("New message");
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body
    .querySelectorAll("[data-popover-side]")
    .forEach((element) => element.remove());
  vi.unstubAllGlobals();
});

async function renderPanel(cwd = "/repo") {
  act(() =>
    root.render(
      createElement(GitChangesPanel, {
        cwd,
        enabled: true,
        onOpenFile: vi.fn(),
        onOpenAllChanges: vi.fn(),
        onOpenCommit: vi.fn(),
      }),
    ),
  );
  await act(async () => {});
}

async function openBranchMenu() {
  const toggle = container.querySelector<HTMLButtonElement>(
    '[aria-label="Branch actions"]',
  )!;
  await act(async () => toggle.click());
  await act(async () => {});
  return document.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
}

describe("GitChangesPanel pull action", () => {
  it("disables Pull when the branch has no upstream", async () => {
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({ remote: null, upstream: null }),
    );
    await renderPanel();

    const pull = await openBranchMenu();
    expect(pull.textContent).toContain("Pull");
    expect(pull.disabled).toBe(true);
  });

  it("disables Pull when the repository has no remote", async () => {
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({ remote: null, upstream: "origin/feature/pull" }),
    );
    await renderPanel();

    const pull = await openBranchMenu();
    expect(pull.disabled).toBe(true);
  });

  it("pulls the current branch and reloads watched files", async () => {
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({ remote: "origin", upstream: "origin/feature/pull" }),
    );
    await renderPanel();

    const pull = await openBranchMenu();
    expect(pull.disabled).toBe(false);

    invalidateWatchedFiles.mockClear();
    await act(async () => {
      pull.click();
      await Promise.resolve();
    });

    expect(gitPull).toHaveBeenCalledWith("/repo");
    expect(invalidateWatchedFiles).toHaveBeenCalled();
  });
});

describe("GitChangesPanel remote pull request", () => {
  it("creates it from the host Git range without calling a local harness", async () => {
    const cwd = "remote://machine/home/user/repo";
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({
        remote: "origin",
        upstream: "origin/feature/pull",
        ahead: 1,
        aheadOfDefault: 1,
      }),
    );
    vi.mocked(gitRangeContext).mockResolvedValue({
      base: "main",
      head: "feature/pull",
      commitSummary: "abc123 Fix remote flow\ndef456 Add coverage",
      diffSummary: "2 files changed, 4 insertions(+)\n",
      diffPatch: "",
    });
    vi.mocked(gitPrCreate).mockResolvedValue("https://example.test/pull/42");
    await renderPanel(cwd);

    const button = [
      ...container.querySelectorAll<HTMLButtonElement>("button"),
    ].find((candidate) => candidate.textContent?.trim() === "Create PR");
    expect(button?.disabled).toBe(false);
    await act(async () => {
      button!.click();
      await Promise.resolve();
    });

    expect(gitPush).toHaveBeenCalledWith(cwd);
    expect(gitRangeContext).toHaveBeenCalledWith(cwd);
    expect(generatePrContent).not.toHaveBeenCalled();
    expect(gitPrCreate).toHaveBeenCalledWith(
      cwd,
      "Fix remote flow",
      expect.stringContaining("## Changes\n\n2 files changed"),
      "main",
      "feature/pull",
    );
    expect(openUrl).toHaveBeenCalledWith("https://example.test/pull/42");
  });

  it("records Azure self-activity with the azuredevops provider", async () => {
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({ remote: "origin", upstream: "origin/feature/pull", ahead: 1, aheadOfDefault: 1 }),
    );
    vi.mocked(generatePrContent).mockResolvedValue({
      title: "Add login",
      body: "Details",
      base: "main",
      head: "feature/pull",
    });
    vi.mocked(gitPrCreate).mockResolvedValue(
      "https://dev.azure.com/acme/shop/_git/web/pullrequest/12",
    );
    await renderPanel("/repo");

    const button = [...container.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "Create PR");
    expect(button?.disabled).toBe(false);
    await act(async () => {
      button!.click();
      await Promise.resolve();
    });

    expect(openUrl).toHaveBeenCalledWith(
      "https://dev.azure.com/acme/shop/_git/web/pullrequest/12",
    );
    expect(recordInboxSelfActivity).toHaveBeenCalledWith({
      provider: "azuredevops",
      kind: "pr",
      number: 12,
      projectPath: "/repo",
    });
  });
});
