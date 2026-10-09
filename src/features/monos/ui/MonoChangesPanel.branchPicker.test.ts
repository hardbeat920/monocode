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
import { BranchPicker } from "../../source-control/ui/BranchPicker";
import { acquireRepoLock, repoLock } from "../../source-control/model/repoLock";
import { ask } from "@tauri-apps/plugin-dialog";
import { generateCommitMessage } from "../../../integrations/harness";
import {
  gitCheckout,
  gitCommit,
  gitDiffIndex,
  gitDiscardFile,
  gitLocateFiles,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";
import { sessionCheckpointStatus } from "../../sessions/model/checkpoint";

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

describe("MonoChangesPanel reopened during a checkout", () => {
  it("keeps the repository locked until the checkout settles", async () => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    await renderPanel();
    typeMessage("Fix the app");
    await act(async () => branchTrigger().click());
    const main = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((option) => option.textContent?.includes("main"))!;
    await act(async () => main.click());

    act(() => root.render(createElement("div")));
    await renderPanel();
    typeMessage("Fix the app");
    expect(branchTrigger().disabled).toBe(true);
    await act(async () => branchTrigger().click());
    expect(document.querySelector("[data-branch-picker]")).toBeNull();
    expect(commitButton().disabled).toBe(true);
    await act(async () => commitButton().click());
    expect(gitCheckout).toHaveBeenCalledTimes(1);
    expect(gitCommit).not.toHaveBeenCalled();

    await act(async () => finish());
    expect(branchTrigger().disabled).toBe(false);
    expect(commitButton().disabled).toBe(false);
  });
});

describe("MonoChangesPanel beside the composer's branch picker", () => {
  let composer: HTMLDivElement;
  let composerRoot: Root;

  beforeEach(async () => {
    composer = document.createElement("div");
    document.body.append(composer);
    composerRoot = createRoot(composer);
  });

  afterEach(async () => {
    await act(async () => composerRoot.unmount());
    composer.remove();
  });

  async function renderComposerPicker() {
    act(() =>
      composerRoot.render(
        createElement(BranchPicker, { cwd: "/repo", branch: "feature/mono" }),
      ),
    );
    await act(async () => {});
    return composer.querySelector<HTMLButtonElement>(
      'button[aria-label="Branch feature/mono"]',
    )!;
  }

  it("locks the panel while a composer checkout runs", async () => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    await renderPanel();
    typeMessage("Fix the app");
    const composerTrigger = await renderComposerPicker();

    await act(async () => composerTrigger.click());
    const main = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((option) => option.textContent?.includes("main"))!;
    await act(async () => main.click());
    expect(gitCheckout).toHaveBeenCalledTimes(1);

    expect(branchTrigger().disabled).toBe(true);
    expect(commitButton().disabled).toBe(true);
    await act(async () => commitButton().click());
    expect(gitCommit).not.toHaveBeenCalled();

    await act(async () => finish());
    expect(branchTrigger().disabled).toBe(false);
    expect(commitButton().disabled).toBe(false);
  });

  it("disables the composer picker while a panel commit runs", async () => {
    let finish!: () => void;
    vi.mocked(gitCommit).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await renderPanel();
    const composerTrigger = await renderComposerPicker();
    expect(composerTrigger.disabled).toBe(false);

    typeMessage("Fix the app");
    await act(async () => commitButton().click());
    expect(composerTrigger.disabled).toBe(true);
    await act(async () => composerTrigger.click());
    expect(document.querySelector("[data-branch-picker]")).toBeNull();
    expect(gitCheckout).not.toHaveBeenCalled();

    await act(async () => finish());
    expect(composerTrigger.disabled).toBe(false);
  });
});

describe("MonoChangesPanel lock acquisition", () => {
  it("holds the repository lock while a discard confirmation is up", async () => {
    let confirm!: (ok: boolean) => void;
    vi.mocked(ask).mockImplementationOnce(
      () =>
        new Promise<boolean>((resolve) => {
          confirm = resolve;
        }),
    );
    vi.mocked(gitDiscardFile).mockClear();
    // A change outside the session lands in the discardable list.
    vi.mocked(gitDiffIndex).mockResolvedValue({
      ...INDEX,
      files: [
        ...INDEX.files,
        { ...INDEX.files[0]!, path: "/repo/notes.md", relative: "notes.md" },
      ],
    });
    await renderPanel();
    const discard = container.querySelector<HTMLButtonElement>(
      '[data-mono-commit] [title="Discard Changes"]',
    )!;
    await act(async () => discard.click());
    expect(ask).toHaveBeenCalled();

    // No checkout or commit can slip in while the user is deciding.
    expect(repoLock("/repo")?.kind).toBe("notes.md");
    expect(acquireRepoLock("/repo", "checkout")).toBeNull();

    await act(async () => confirm(true));
    expect(gitDiscardFile).toHaveBeenCalledWith("/repo", "notes.md");
    expect(repoLock("/repo")).toBeNull();
  });
});

describe("MonoChangesPanel generation in two panels", () => {
  it("lets only the generating panel cancel, releasing the lock", async () => {
    vi.mocked(generateCommitMessage).mockClear();
    vi.mocked(generateCommitMessage).mockImplementationOnce(
      () => new Promise<string>(() => {}),
    );
    await renderPanel();
    const other = document.createElement("div");
    document.body.append(other);
    const otherRoot = createRoot(other);
    try {
      act(() =>
        otherRoot.render(
          createElement(MonoChangesPanel, {
            sessionId: "session-2",
            cwd: "/repo",
            request: { tab: "commit" },
            color: "#888",
            onClose: vi.fn(),
          }),
        ),
      );
      await act(async () => {});
      await act(async () => {});
      const button = (host: HTMLElement, label: string) =>
        host.querySelector<HTMLButtonElement>(
          `[data-mono-commit] [aria-label="${label}"]`,
        );

      await act(async () =>
        button(container, "Generate commit message")!.click(),
      );
      expect(repoLock("/repo")?.kind).toBe("generate");
      expect(button(container, "Cancel commit message generation")).not.toBeNull();

      expect(button(other, "Cancel commit message generation")).toBeNull();
      const otherGenerate = button(other, "Generate commit message")!;
      expect(otherGenerate.disabled).toBe(true);
      await act(async () => otherGenerate.click());
      expect(generateCommitMessage).toHaveBeenCalledTimes(1);

      await act(async () =>
        button(container, "Cancel commit message generation")!.click(),
      );
      expect(repoLock("/repo")).toBeNull();
      expect(button(other, "Generate commit message")!.disabled).toBe(false);
    } finally {
      act(() => otherRoot.unmount());
      other.remove();
    }
  });
});

describe("MonoChangesPanel with several projects", () => {
  beforeEach(() => {
    vi.mocked(gitLocateFiles).mockImplementation(async (paths: string[]) =>
      paths.map((path) => {
        const repo = path.startsWith("/other/") ? "/other" : "/repo";
        return { root: repo, relative: path.replace(`${repo}/`, "") };
      }),
    );
    vi.mocked(sessionCheckpointStatus).mockResolvedValue({
      files: ["/repo/src/app.ts", "/other/src/lib.ts"].map((path) => ({
        path,
        relative: path.split("/").slice(2).join("/"),
        status: "modified",
        additions: 1,
        deletions: 0,
        exact: true,
        undoable: true,
      })),
    } as Awaited<ReturnType<typeof sessionCheckpointStatus>>);
    vi.mocked(gitDiffIndex).mockImplementation(async (repo: string) => ({
      ...INDEX,
      files: INDEX.files.map((file) =>
        repo === "/other"
          ? { ...file, path: "/other/src/lib.ts", relative: "src/lib.ts" }
          : file,
      ),
    }));
  });

  afterEach(() => {
    vi.mocked(gitLocateFiles).mockReset();
    vi.mocked(sessionCheckpointStatus).mockReset();
  });

  function visibleCommit() {
    return container.querySelector<HTMLElement>(
      '[role="tabpanel"][aria-label="Commit"]:not([hidden])',
    )!;
  }

  function visibleCommitButton() {
    return [...visibleCommit().querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent?.trim() === "Commit",
    )!;
  }

  function typeVisibleMessage(text: string) {
    const textarea = visibleCommit().querySelector("textarea")!;
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => {
      setter.call(textarea, text);
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }

  async function selectProject(name: string) {
    const switcher = container.querySelector<HTMLButtonElement>(
      'button[aria-haspopup="menu"]',
    )!;
    await act(async () => switcher.click());
    const item = [
      ...container.querySelectorAll<HTMLButtonElement>(
        '[role="menuitemradio"]',
      ),
    ].find((entry) => entry.title === name)!;
    await act(async () => item.click());
  }

  it("keeps other projects usable while one commits", async () => {
    let finish!: () => void;
    vi.mocked(gitCommit).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await renderPanel();
    await selectProject("/repo");
    typeVisibleMessage("Fix the app");
    await act(async () => visibleCommitButton().click());
    expect(vi.mocked(gitCommit).mock.calls[0]?.[0]).toBe("/repo");
    expect(branchTrigger().disabled).toBe(true);

    await selectProject("/other");
    typeVisibleMessage("Fix the lib");
    expect(visibleCommitButton().disabled).toBe(false);
    expect(branchTrigger().disabled).toBe(false);

    await selectProject("/repo");
    expect(visibleCommitButton().disabled).toBe(true);
    await act(async () => finish());
    expect(branchTrigger().disabled).toBe(false);
    expect(visibleCommit().querySelector("textarea")!.disabled).toBe(false);
  });

  it("releases a project's lock when its checkout settles after a switch", async () => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    await renderPanel();
    await selectProject("/repo");
    typeVisibleMessage("Fix the app");
    await act(async () => branchTrigger().click());
    const main = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((option) => option.textContent?.includes("main"))!;
    await act(async () => main.click());
    expect(gitCheckout).toHaveBeenCalledWith("/repo", "main", null);
    expect(visibleCommitButton().disabled).toBe(true);

    await selectProject("/other");
    typeVisibleMessage("Fix the lib");
    expect(visibleCommitButton().disabled).toBe(false);
    await act(async () => finish());

    await selectProject("/repo");
    expect(visibleCommitButton().disabled).toBe(false);
    expect(branchTrigger().disabled).toBe(false);
  });

  it("keeps a returning project locked until its checkout settles", async () => {
    let finish!: () => void;
    vi.mocked(gitCheckout).mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finish = () => resolve("");
        }),
    );
    await renderPanel();
    await selectProject("/repo");
    typeVisibleMessage("Fix the app");
    await act(async () => branchTrigger().click());
    const main = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ].find((option) => option.textContent?.includes("main"))!;
    await act(async () => main.click());
    expect(gitCheckout).toHaveBeenCalledTimes(1);

    await selectProject("/other");
    typeVisibleMessage("Fix the lib");
    expect(visibleCommitButton().disabled).toBe(false);
    expect(branchTrigger().disabled).toBe(false);

    await selectProject("/repo");
    expect(branchTrigger().disabled).toBe(true);
    await act(async () => branchTrigger().click());
    expect(document.querySelector("[data-branch-picker]")).toBeNull();
    expect(gitCheckout).toHaveBeenCalledTimes(1);
    expect(visibleCommitButton().disabled).toBe(true);
    await act(async () => visibleCommitButton().click());
    expect(gitCommit).not.toHaveBeenCalled();

    await act(async () => finish());
    expect(branchTrigger().disabled).toBe(false);
    expect(visibleCommitButton().disabled).toBe(false);
  });
});
