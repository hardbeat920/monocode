// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("../../../platform/tauri/fs", () => ({
  basename: (path: string) => path.split("/").pop() ?? path,
  gitLocateFiles: vi.fn(async (paths: string[]) =>
    paths.map((path) => ({
      root: "/repo",
      relative: path.replace("/repo/", ""),
    })),
  ),
  gitDiffIndex: vi.fn(),
  gitPrStatus: vi.fn(async () => null),
  gitCommit: vi.fn(async () => {}),
  gitDiscardFile: vi.fn(async () => {}),
  gitPrCreate: vi.fn(async () => ""),
  gitPush: vi.fn(async () => {}),
  gitStageFile: vi.fn(async () => {}),
  gitSync: vi.fn(async () => {}),
  gitBranches: vi.fn(async () => ({
    current: "feature/mono",
    detached: false,
    branches: [
      { name: "feature/mono", current: true, remote: null },
      { name: "main", current: false, remote: null },
    ],
  })),
  gitCheckout: vi.fn(async () => ""),
  gitCreateBranch: vi.fn(async () => ""),
  gitStageAll: vi.fn(async () => {}),
  gitStash: vi.fn(async () => ""),
  isCheckoutBlockedByChanges: () => false,
  notifyGitChanged: vi.fn(),
  subscribeGitChanged: () => () => {},
}));

vi.mock("../../sessions/model/checkpoint", () => ({
  sessionCheckpointStatus: vi.fn(async () => ({
    files: [
      {
        path: "/repo/src/app.ts",
        relative: "src/app.ts",
        status: "modified",
        additions: 1,
        deletions: 0,
        exact: true,
        undoable: true,
      },
    ],
  })),
  subscribeReviewChanged: () => () => {},
  keepSessionChanges: vi.fn(async () => {}),
  notifyReviewChanged: vi.fn(),
}));

vi.mock("../../source-control/ui/SessionChangesDiff", () => ({
  SessionChangesDiff: () => null,
}));

vi.mock("../../../integrations/harness", () => ({
  generateCommitMessage: vi.fn(async () => ""),
  generatePrContent: vi.fn(async () => null),
}));

vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles: vi.fn(),
  nudgeWatchedFiles: vi.fn(),
}));

vi.mock("../../inbox/model/inboxSelfActivity", () => ({
  recordInboxSelfActivity: vi.fn(),
}));

import { MonoChangesPanel } from "./MonoChangesPanel";
import {
  gitCheckout,
  gitCommit,
  gitDiffIndex,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";

const INDEX: GitDiffIndex = {
  branch: "feature/mono",
  head: "abc123",
  files: [
    {
      path: "/repo/src/app.ts",
      relative: "src/app.ts",
      status: "modified",
      additions: 1,
      deletions: 0,
      staged: false,
      unstaged: true,
    },
  ],
  additions: 1,
  deletions: 0,
  remote: "origin",
  upstream: "origin/feature/mono",
  defaultBranch: "main",
  ahead: 0,
  behind: 0,
  aheadOfDefault: 0,
  headPushed: true,
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(gitDiffIndex).mockResolvedValue(INDEX);
  vi.mocked(gitCommit).mockReset().mockResolvedValue(undefined);
  vi.mocked(gitCheckout).mockReset().mockResolvedValue("");
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  document.body
    .querySelectorAll("[data-popover-side]")
    .forEach((element) => element.remove());
  vi.unstubAllGlobals();
});

async function renderPanel() {
  act(() =>
    root.render(
      createElement(MonoChangesPanel, {
        sessionId: "session-1",
        cwd: "/repo",
        request: { tab: "commit" },
        color: "#888",
        onClose: vi.fn(),
      }),
    ),
  );
  await act(async () => {});
  await act(async () => {});
}

function branchTrigger() {
  return container.querySelector<HTMLButtonElement>(
    'button[aria-label="Branch feature/mono"]',
  )!;
}

function commitButton() {
  const form = container.querySelector("[data-mono-commit]")!;
  return [...form.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent?.trim() === "Commit",
  )!;
}

function typeMessage(text: string) {
  const textarea = container.querySelector("textarea")!;
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(textarea, text);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("MonoChangesPanel branch picker", () => {
  it("disables the branch picker while a commit runs", async () => {
    let finish!: () => void;
    vi.mocked(gitCommit).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await renderPanel();
    expect(branchTrigger().disabled).toBe(false);

    typeMessage("Fix the app");
    await act(async () => commitButton().click());
    expect(gitCommit).toHaveBeenCalled();
    expect(branchTrigger().disabled).toBe(true);
    await act(async () => branchTrigger().click());
    expect(document.querySelector("[data-branch-picker]")).toBeNull();

    await act(async () => finish());
    expect(branchTrigger().disabled).toBe(false);
  });

  it("locks the Commit tab while a checkout runs", async () => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    await renderPanel();
    typeMessage("Fix the app");
    expect(commitButton().disabled).toBe(false);

    await act(async () => branchTrigger().click());
    const main = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((option) => option.textContent?.includes("main"))!;
    await act(async () => main.click());

    expect(gitCheckout).toHaveBeenCalledWith("/repo", "main", null);
    expect(commitButton().disabled).toBe(true);
    expect(container.querySelector("textarea")!.disabled).toBe(true);
    await act(async () => commitButton().click());
    expect(gitCommit).not.toHaveBeenCalled();

    await act(async () => finish());
    expect(commitButton().disabled).toBe(false);
  });
});

type Dismissal = "Escape" | "outside click" | "trigger click";

async function dismissPicker(how: Dismissal, trigger: HTMLButtonElement) {
  await act(async () => {
    if (how === "Escape") {
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    } else if (how === "outside click") {
      document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    } else {
      trigger.click();
    }
  });
  expect(document.querySelector("[data-branch-picker]")).toBeNull();
}

const DISMISSALS: Dismissal[] = ["Escape", "outside click", "trigger click"];

describe("MonoChangesPanel dismissed checkout", () => {
  it.each(DISMISSALS)(
    "keeps the Commit tab locked after %s until the checkout settles",
    async (how) => {
      let finish!: () => void;
      vi.mocked(gitCheckout).mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            finish = () => resolve("");
          }),
      );
      await renderPanel();
      typeMessage("Fix the app");
      const trigger = branchTrigger();

      await act(async () => trigger.click());
      const main = [
        ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
      ].find((option) => option.textContent?.includes("main"))!;
      await act(async () => main.click());
      await dismissPicker(how, trigger);

      expect(commitButton().disabled).toBe(true);
      await act(async () => commitButton().click());
      expect(gitCommit).not.toHaveBeenCalled();
      await act(async () => trigger.click());
      expect(document.querySelector("[data-branch-picker]")).toBeNull();
      expect(gitCheckout).toHaveBeenCalledTimes(1);

      await act(async () => finish());
      expect(commitButton().disabled).toBe(false);
    },
  );
});
