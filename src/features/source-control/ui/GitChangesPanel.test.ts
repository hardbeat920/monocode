// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(async () => true),
}));

const platform = vi.hoisted(() => ({ isMac: true }));

vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_MAC() {
    return platform.isMac;
  },
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
  gitDiscardFile,
  gitPrCreate,
  gitPull,
  gitPush,
  gitRangeContext,
  gitStageFile,
} from "../../../platform/tauri/fs";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  generateCommitMessage,
  generatePrContent,
} from "../../../integrations/harness";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { GitChangedFile, GitDiffIndex } from "../../../platform/tauri/fs";

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
});

function change(
  relative: string,
  overrides: Partial<GitChangedFile> = {},
): GitChangedFile {
  return {
    path: `/repo/${relative}`,
    relative,
    status: "modified",
    additions: 1,
    deletions: 0,
    staged: false,
    unstaged: true,
    ...overrides,
  };
}

describe("GitChangesPanel multi-selection", () => {
  const onOpenFile = vi.fn();
  let props: Record<string, unknown> = {};

  beforeEach(() => {
    platform.isMac = true;
    onOpenFile.mockReset();
    props = {};
    vi.mocked(ask).mockReset().mockResolvedValue(true);
    vi.mocked(gitStageFile).mockReset().mockResolvedValue(undefined);
    vi.mocked(gitDiscardFile).mockReset().mockResolvedValue(undefined);
  });

  async function render(files: GitChangedFile[], view: "list" | "tree") {
    vi.mocked(gitDiffIndex).mockResolvedValue(index({ files }));
    await rerender();
    // The view is a persisted module-level preference: force the one we want.
    const toggle = container.querySelector<HTMLButtonElement>(
      `[aria-label="View as ${view === "tree" ? "Tree" : "List"}"]`,
    );
    if (toggle) await act(async () => toggle.click());
  }

  async function rerender(next: Record<string, unknown> = {}) {
    props = { ...props, ...next };
    act(() =>
      root.render(
        createElement(GitChangesPanel, {
          cwd: "/repo",
          enabled: true,
          onOpenFile,
          onOpenAllChanges: vi.fn(),
          onOpenCommit: vi.fn(),
          ...props,
        }),
      ),
    );
    await act(async () => {});
  }

  const row = (relative: string) =>
    container.querySelector<HTMLButtonElement>(
      `li button[title="${relative}"]`,
    )!;
  const rowAction = (relative: string, label: string) =>
    row(relative)
      .closest("li")!
      .querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
  const selectedRows = () =>
    [...container.querySelectorAll("li[data-selected]")].map(
      (li) => li.querySelector("button")!.title,
    );

  async function click(relative: string, init: MouseEventInit = {}) {
    await act(async () => {
      row(relative).dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, ...init }),
      );
    });
  }

  const abc = () => [change("a.ts"), change("b.ts"), change("c.ts")];

  it("opens and selects one file on a plain click", async () => {
    await render(abc(), "list");
    await click("b.ts");
    expect(onOpenFile).toHaveBeenCalledWith("/repo/b.ts", "unstaged");
    expect(selectedRows()).toEqual(["b.ts"]);
  });

  it("toggles with Cmd on macOS without opening", async () => {
    await render(abc(), "list");
    await click("a.ts", { metaKey: true });
    await click("c.ts", { metaKey: true });
    expect(selectedRows()).toEqual(["a.ts", "c.ts"]);
    await click("a.ts", { metaKey: true });
    expect(selectedRows()).toEqual(["c.ts"]);
    // Ctrl isn't the toggle modifier on macOS.
    await click("b.ts", { ctrlKey: true });
    expect(selectedRows()).toEqual(["b.ts"]);
    expect(onOpenFile).toHaveBeenCalledTimes(1);
  });

  it("toggles with Ctrl elsewhere without opening", async () => {
    platform.isMac = false;
    await render(abc(), "list");
    await click("a.ts", { ctrlKey: true });
    await click("b.ts", { ctrlKey: true });
    expect(selectedRows()).toEqual(["a.ts", "b.ts"]);
    expect(onOpenFile).not.toHaveBeenCalled();
  });

  it("selects a Shift range in list order", async () => {
    await render([...abc(), change("d.ts")], "list");
    await click("b.ts");
    await click("d.ts", { shiftKey: true });
    expect(selectedRows()).toEqual(["b.ts", "c.ts", "d.ts"]);
    expect(onOpenFile).toHaveBeenCalledTimes(1);
  });

  it("skips a collapsed folder in a tree Shift range", async () => {
    await render(
      [
        change("ta/one.ts"),
        change("tm/two.ts"),
        change("tm/four.ts"),
        change("tz/three.ts"),
      ],
      "tree",
    );
    await click("tm/two.ts");
    await click("tm/four.ts", { metaKey: true });
    expect(selectedRows()).toEqual(["tm/four.ts", "tm/two.ts"]);
    const folder =
      container.querySelector<HTMLButtonElement>('button[title="tm"]')!;
    // Collapsing the folder drops its now hidden rows from the selection.
    await act(async () => folder.click());
    expect(selectedRows()).toEqual([]);

    await click("ta/one.ts");
    await click("tz/three.ts", { shiftKey: true });
    expect(selectedRows()).toEqual(["ta/one.ts", "tz/three.ts"]);
    await act(async () => folder.click());
  });

  it("stages every selected file one at a time", async () => {
    await render(abc(), "list");
    const pending: (() => void)[] = [];
    let inFlight = 0;
    let overlapped = false;
    vi.mocked(gitStageFile).mockImplementation(async () => {
      inFlight += 1;
      if (inFlight > 1) overlapped = true;
      await new Promise<void>((resolve) => pending.push(resolve));
      inFlight -= 1;
    });
    await click("c.ts", { metaKey: true });
    await click("a.ts", { metaKey: true });

    await act(async () => rowAction("c.ts", "Stage Changes").click());
    expect(rowAction("a.ts", "Stage Changes").disabled).toBe(true);
    expect(rowAction("c.ts", "Stage Changes").disabled).toBe(true);
    expect(rowAction("b.ts", "Stage Changes").disabled).toBe(false);
    while (pending.length) await act(async () => pending.shift()!());

    // On-screen order, not click order.
    expect(vi.mocked(gitStageFile).mock.calls).toEqual([
      ["/repo", "a.ts"],
      ["/repo", "c.ts"],
    ]);
    expect(overlapped).toBe(false);
    expect(invalidateWatchedFiles).toHaveBeenCalledWith([
      "/repo/a.ts",
      "/repo/c.ts",
    ]);
  });

  it("stages only the clicked file when it isn't selected", async () => {
    await render(abc(), "list");
    await click("a.ts", { metaKey: true });
    await click("b.ts", { metaKey: true });
    await act(async () => rowAction("c.ts", "Stage Changes").click());
    expect(vi.mocked(gitStageFile).mock.calls).toEqual([["/repo", "c.ts"]]);
  });

  it("asks once before discarding a selection, and stops if cancelled", async () => {
    await render(
      [change("a.ts"), change("b.ts", { status: "untracked" }), change("c.ts")],
      "list",
    );
    await click("a.ts");
    await click("c.ts", { shiftKey: true });
    vi.mocked(ask).mockResolvedValue(false);
    await act(async () => rowAction("b.ts", "Discard Changes").click());
    expect(ask).toHaveBeenCalledTimes(1);
    expect(vi.mocked(ask).mock.calls[0]?.[0]).toBe(
      "Discard changes in 3 files? This cannot be undone. 1 untracked file will be deleted.",
    );
    expect(gitDiscardFile).not.toHaveBeenCalled();

    vi.mocked(ask).mockResolvedValue(true);
    await act(async () => rowAction("b.ts", "Discard Changes").click());
    expect(ask).toHaveBeenCalledTimes(2);
    expect(vi.mocked(gitDiscardFile).mock.calls.map((call) => call[1])).toEqual(
      ["a.ts", "b.ts", "c.ts"],
    );
  });

  it("reports done paths and a single error on a partial failure", async () => {
    const alert = vi.fn();
    vi.stubGlobal("alert", alert);
    await render(abc(), "list");
    vi.mocked(gitStageFile).mockImplementation(async (_cwd, relative) => {
      if (relative === "b.ts") throw new Error("index.lock exists");
    });
    await click("a.ts");
    await click("c.ts", { shiftKey: true });
    await act(async () => rowAction("a.ts", "Stage Changes").click());

    expect(gitStageFile).toHaveBeenCalledTimes(3);
    expect(invalidateWatchedFiles).toHaveBeenCalledWith([
      "/repo/a.ts",
      "/repo/c.ts",
    ]);
    expect(alert).toHaveBeenCalledTimes(1);
    expect(alert).toHaveBeenCalledWith(
      "index.lock exists (1 of 3 files failed)",
    );
  });

  it("prunes the selection once its files leave the section", async () => {
    await render(abc(), "list");
    await click("a.ts");
    await click("b.ts", { shiftKey: true });
    vi.mocked(gitDiffIndex).mockResolvedValue(
      index({
        files: [
          change("a.ts", { staged: true, unstaged: false }),
          change("b.ts", { staged: true, unstaged: false }),
          change("c.ts"),
        ],
      }),
    );
    await act(async () => rowAction("a.ts", "Stage Changes").click());
    await act(async () => {});
    expect(
      container.querySelectorAll('[aria-label="Unstage Changes"]'),
    ).toHaveLength(2);
    expect(selectedRows()).toEqual([]);

    // Coming back (e.g. unstaged elsewhere) doesn't revive the old selection.
    vi.mocked(gitDiffIndex).mockResolvedValue(index({ files: abc() }));
    await act(async () => rowAction("c.ts", "Stage Changes").click());
    await act(async () => {});
    expect(rowAction("a.ts", "Stage Changes")).toBeTruthy();
    expect(selectedRows()).toEqual([]);
  });

  it("clears the selection when another file is opened", async () => {
    await render(abc(), "list");
    await click("a.ts", { metaKey: true });
    await click("b.ts", { metaKey: true });
    await rerender({ selectedPath: "a.ts", selectedKind: "unstaged" });
    expect(selectedRows()).toEqual(["a.ts", "b.ts"]);
    // Removing the open row from the selection keeps the rest.
    await click("a.ts", { metaKey: true });
    expect(selectedRows()).toEqual(["b.ts"]);
    await rerender({ selectedPath: "c.ts", selectedKind: "unstaged" });
    expect(selectedRows()).toEqual([]);
  });

  it("clears the selection on Escape", async () => {
    await render(abc(), "list");
    await click("a.ts", { metaKey: true });
    await click("b.ts", { metaKey: true });
    await act(async () => {
      row("a.ts").dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(selectedRows()).toEqual([]);
  });
});
