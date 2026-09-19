// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../lib/worktrees", async (original) => ({
  ...(await original<typeof import("../lib/worktrees")>()),
  listWorktrees: vi.fn(),
  createWorktree: vi.fn(),
  checkWorktreeRemoval: vi.fn(),
}));
vi.mock("../hooks/useProjectBranches", () => ({
  useProjectBranchesState: vi.fn(() => ({
    branches: {
      current: "main",
      detached: false,
      branches: [{ name: "main", remote: null, current: true }],
    },
    settled: true,
  })),
}));
vi.mock("../lib/fs", async (original) => ({
  ...(await original<typeof import("../lib/fs")>()),
  subscribeGitChanged: (listener: () => void) => {
    window.addEventListener("test-git-changed", listener);
    return () => window.removeEventListener("test-git-changed", listener);
  },
  gitCheckout: vi.fn(),
  gitCommit: vi.fn(),
  gitCreateBranch: vi.fn(),
  gitStageAll: vi.fn(),
  gitStash: vi.fn(),
  isCheckoutBlockedByChanges: () => false,
  notifyGitChanged: () => window.dispatchEvent(new Event("test-git-changed")),
  revealPath: vi.fn(),
}));

import {
  assertWorktreeFilesClosed,
  checkWorktreeRemoval,
  listWorktrees,
  namedWorktreeBranch,
  temporaryWorktreeBranchName,
  type Worktree,
  type Worktrees,
} from "../lib/worktrees";
import { newFileTab, newTerminalFile } from "../lib/layout";
import { gitCheckout, notifyGitChanged } from "../lib/fs";
import { useProjectBranchesState } from "../hooks/useProjectBranches";
import { WorktreePicker } from "./WorktreePicker";
import {
  WORKSPACE_MODE_SHORTCUT,
  WorkspaceIdentity,
  WorkspacePicker,
} from "./WorkspacePicker";
import { CreateWorktreeDialog } from "./CreateWorktreeDialog";
import { FolderTree, GitBranch } from "./icons";
import { DeleteWorktreeDialog } from "./DeleteWorktreeDialog";
import { DeleteSessionDialog } from "./DeleteSessionDialog";
import { WorktreesPage } from "../surfaces/WorktreesPage";

let container: HTMLDivElement;
let root: Root;
const tree: Worktree = {
  path: "/repo-worktrees/feature",
  branch: "feature",
  head: "abc",
  isMain: false,
  locked: false,
  prunable: false,
  missing: false,
  dirty: true,
  unpushed: 2,
  sessionIds: [],
};
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.mocked(checkWorktreeRemoval).mockResolvedValue(undefined);
  vi.mocked(useProjectBranchesState).mockReturnValue({
    branches: {
      current: "main",
      detached: false,
      branches: [{ name: "main", remote: null, current: true }],
    },
    settled: true,
  });
  vi.mocked(listWorktrees).mockResolvedValue({
    worktrees: [{ ...tree, path: "/repo", branch: "main", isMain: true }, tree],
    defaultRoot: "/repo-worktrees",
  });
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});
const button = (text: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (node) => node.textContent === text,
  )!;

const type = (input: HTMLInputElement, value: string) => {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
};

const confirmWorktree = async (name: string) => {
  const field = document.querySelector<HTMLInputElement>(
    `input[aria-label="Type ${name} to confirm"]`,
  )!;
  await act(async () => type(field, name));
};

function deferred() {
  let resolve!: (data: Worktrees) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<Worktrees>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const result = (branch = "feature"): Worktrees => ({
  worktrees: [{ ...tree, branch }],
  defaultRoot: "/repo-worktrees",
});

it("builds temporary and generated worktree branch names", () => {
  expect(
    temporaryWorktreeBranchName("12345678-90ab-cdef-1234-567890abcdef"),
  ).toBe("mc/12345678");
  expect(namedWorktreeBranch("feature/faster-worktrees")).toBe(
    "mc/feature/faster-worktrees",
  );
  expect(namedWorktreeBranch("monocode/already-prefixed")).toBe(
    "mc/already-prefixed",
  );
  expect(namedWorktreeBranch("mc/already-short")).toBe("mc/already-short");
  expect(namedWorktreeBranch("  ")).toBeNull();
});

it("selects a draft workspace without opening the creation dialog", async () => {
  const onModeChange = vi.fn();
  const onOpenSettings = vi.fn();
  await act(async () =>
    root.render(
      createElement(WorkspacePicker, {
        cwd: "/repo",
        mode: "current",
        onModeChange,
        onBaseChange: vi.fn(),
        onOpenSettings,
      }),
    ),
  );

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Workspace Current checkout"]',
      )!
      .click(),
  );
  await act(async () => button("New worktree").click());

  expect(onModeChange).toHaveBeenCalledWith("worktree", "main");
  expect(document.querySelector('[aria-label="Create worktree"]')).toBeNull();
  expect(
    container
      .querySelector('[aria-label="Workspace Current checkout"]')
      ?.getAttribute("title"),
  ).toContain(WORKSPACE_MODE_SHORTCUT);

  await act(async () =>
    container
      .querySelector<HTMLButtonElement>(
        '[aria-label="Workspace Current checkout"]',
      )!
      .click(),
  );
  const settings = document.querySelector<HTMLButtonElement>(
    '[aria-label="Open worktree settings"]',
  )!;
  expect(settings.textContent).toContain("Worktree settings");
  expect(settings.parentElement?.className).toContain("border-t");
  expect(settings.parentElement?.className).toContain("h-9");
  expect(settings.className).toContain("h-full");
  expect(settings.parentElement?.className).not.toContain("mt-1");
  expect(settings.parentElement?.className).not.toContain("pt-1");
  await act(async () => settings.click());
  expect(onOpenSettings).toHaveBeenCalledOnce();
  expect(
    document.querySelector('[aria-label="Open worktree settings"]'),
  ).toBeNull();
  expect(document.querySelector('[aria-label="Workspace"]')).toBeNull();
});

it("renders a started session's workspace as a non-interactive identity", () => {
  const markup = renderToStaticMarkup(
    createElement(WorkspaceIdentity, { worktree: true }),
  );
  expect(markup).toContain("Worktree");
  expect(markup).toContain('aria-label="Workspace Worktree"');
  expect(markup).not.toContain("<button");
});

it("keeps the selected worktree base visible beside the workspace mode", async () => {
  const onBaseChange = vi.fn();
  vi.mocked(useProjectBranchesState).mockReturnValue({
    branches: {
      current: "main",
      detached: false,
      branches: [
        { name: "main", remote: null, current: true },
        { name: "release", remote: null, current: false },
      ],
    },
    settled: true,
  });
  await act(async () =>
    root.render(
      createElement(WorkspacePicker, {
        cwd: "/repo",
        mode: "worktree",
        base: "main",
        onModeChange: vi.fn(),
        onBaseChange,
      }),
    ),
  );

  const base = container.querySelector<HTMLButtonElement>(
    '[aria-label="Create worktree from main"]',
  )!;
  expect(base.textContent).toContain("From main");
  await act(async () => base.click());
  await act(async () => button("release").click());

  expect(onBaseChange).toHaveBeenCalledWith("release");
});

it("keeps the settings rows mounted through focus refreshes and failures", async () => {
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/settings-refresh",
        onRemove: vi.fn(),
      }),
    ),
  );
  const reveal = container.querySelector('[aria-label="Reveal feature"]');
  expect(reveal).not.toBeNull();
  const request = deferred();
  vi.mocked(listWorktrees).mockReturnValue(request.promise);
  const before = vi.mocked(listWorktrees).mock.calls.length;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
    window.dispatchEvent(new Event("focus"));
  });
  expect(listWorktrees).toHaveBeenCalledTimes(before + 1);
  expect(container.querySelector('[aria-label="Reveal feature"]')).toBe(reveal);
  expect(container.textContent).not.toContain("Loading worktrees");
  expect(button("Create worktree").disabled).toBe(false);
  await act(async () => request.reject(new Error("Git unavailable")));
  expect(container.querySelector('[aria-label="Reveal feature"]')).toBe(reveal);
  expect(
    container
      .querySelector('[aria-label="Refresh worktrees"]')
      ?.getAttribute("title"),
  ).toContain("Git unavailable");
  vi.mocked(listWorktrees).mockResolvedValue(result("updated"));
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(
    container.querySelector('[aria-label="Reveal updated"]'),
  ).not.toBeNull();
});

it("reopens the picker with cached rows while revalidating", async () => {
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/picker-cache",
        executionCwd: "/picker-cache",
        onSelect: vi.fn(),
      }),
    ),
  );
  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  const request = deferred();
  vi.mocked(listWorktrees).mockReturnValue(request.promise);
  await act(async () => trigger.click());
  expect(document.querySelectorAll('[role="option"]')).toHaveLength(2);
  expect(document.body.textContent).not.toContain("Loading working copies");
  await act(async () => trigger.click());
  await act(async () => trigger.click());
  expect(document.querySelectorAll('[role="option"]')).toHaveLength(2);
  expect(document.body.textContent).not.toContain("Loading working copies");
  await act(async () => request.resolve(result("updated")));
  expect(document.querySelector('[role="option"]')?.textContent).toContain(
    "updated",
  );
});

it.each(["/repo", tree.path])(
  "highlights the current working copy %s each time the picker opens",
  async (executionCwd) => {
    const onSelect = vi.fn(async () => {});
    await act(async () =>
      root.render(
        createElement(WorktreePicker, { cwd: "/repo", executionCwd, onSelect }),
      ),
    );
    const trigger = container.querySelector<HTMLButtonElement>("button")!;
    await act(async () => trigger.click());
    const selected = document.querySelector<HTMLElement>(
      '[role="option"][aria-selected="true"]',
    )!;
    const other = document.querySelector<HTMLElement>(
      '[role="option"][aria-selected="false"]',
    )!;
    expect(selected.title).toBe(executionCwd);
    expect(selected.classList.contains("bg-selection")).toBe(true);
    expect(other.classList.contains("bg-selection")).toBe(false);
    await act(async () =>
      other.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
    );
    expect(other.classList.contains("bg-selection")).toBe(true);
    await act(async () => trigger.click());
    await act(async () => trigger.click());
    expect(
      document
        .querySelector('[role="option"][aria-selected="true"]')
        ?.classList.contains("bg-selection"),
    ).toBe(true);
    const search = document.querySelector<HTMLInputElement>(
      '[aria-label="Search working copies"]',
    )!;
    await act(async () =>
      search.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      ),
    );
    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ path: executionCwd }),
    );
  },
);

it("highlights the current worktree after a cold load and keeps navigation through refreshes", async () => {
  const request = deferred();
  vi.mocked(listWorktrees).mockReturnValue(request.promise);
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/cold-highlight",
        executionCwd: tree.path,
        onSelect: vi.fn(),
      }),
    ),
  );
  await act(async () =>
    container.querySelector<HTMLButtonElement>("button")!.click(),
  );
  expect(document.querySelector('[role="option"]')).toBeNull();
  const main = {
    ...tree,
    path: "/cold-highlight",
    branch: "main",
    isMain: true,
  };
  await act(async () =>
    request.resolve({ ...result(), worktrees: [main, tree] }),
  );
  expect(
    document
      .querySelector('[role="option"][aria-selected="true"]')
      ?.classList.contains("bg-selection"),
  ).toBe(true);
  const search = document.querySelector<HTMLInputElement>(
    '[aria-label="Search working copies"]',
  )!;
  await act(async () =>
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }),
    ),
  );
  expect(
    document
      .querySelector('.bg-selection[role="option"]')
      ?.getAttribute("title"),
  ).toBe(main.path);
  vi.mocked(listWorktrees).mockResolvedValue({
    ...result(),
    worktrees: [tree, main],
  });
  await act(async () => notifyGitChanged());
  expect(
    document
      .querySelector('.bg-selection[role="option"]')
      ?.getAttribute("title"),
  ).toBe(main.path);
});

it("highlights search results and restores the current worktree when search is cleared", async () => {
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/repo",
        executionCwd: tree.path,
        onSelect: vi.fn(),
      }),
    ),
  );
  await act(async () =>
    container.querySelector<HTMLButtonElement>("button")!.click(),
  );
  const search = document.querySelector<HTMLInputElement>(
    '[aria-label="Search working copies"]',
  )!;
  await act(async () => type(search, "main"));
  expect(
    document
      .querySelector('.bg-selection[role="option"]')
      ?.getAttribute("title"),
  ).toBe("/repo");
  await act(async () => type(search, "no matches"));
  expect(document.querySelector('.bg-selection[role="option"]')).toBeNull();
  await act(async () => type(search, ""));
  expect(
    document
      .querySelector('[role="option"][aria-selected="true"]')
      ?.classList.contains("bg-selection"),
  ).toBe(true);
});

it.each([
  { executionCwd: "/repo", current: "main", detached: false },
  { executionCwd: tree.path, current: "feature", detached: false },
  { executionCwd: tree.path, current: "abc1234", detached: true },
])(
  "keeps the $current trigger styling and content when switching to branch mode",
  async ({ executionCwd, current, detached }) => {
    vi.mocked(useProjectBranchesState).mockReturnValue({
      branches: {
        current,
        detached,
        branches: [{ name: "main", remote: null, current: current === "main" }],
      },
      settled: true,
    });
    await act(async () =>
      root.render(
        createElement(WorktreePicker, {
          cwd: "/repo",
          executionCwd,
          onSelect: vi.fn(),
        }),
      ),
    );
    const trigger = container.querySelector<HTMLButtonElement>("button")!;
    expect(trigger.classList.contains("-ml-1.5")).toBe(true);
    expect(trigger.classList.contains("px-1.5")).toBe(true);
    expect(trigger.classList.contains("hover:bg-content/8")).toBe(true);
    expect(trigger.textContent?.includes("Worktree")).toBe(
      executionCwd !== "/repo",
    );
    expect(trigger.querySelector("svg")?.outerHTML).toBe(
      renderToStaticMarkup(
        createElement(executionCwd === "/repo" ? GitBranch : FolderTree, {
          className: "size-3.5 shrink-0",
        }),
      ),
    );
    await act(async () => trigger.click());
    const options = document.querySelectorAll('[role="option"]');
    for (const [index, Icon] of [GitBranch, FolderTree].entries()) {
      expect(options[index].querySelector("svg")?.outerHTML).toBe(
        renderToStaticMarkup(
          createElement(Icon, {
            className: "size-3.5 shrink-0 text-content/50",
          }),
        ),
      );
    }
    expect(
      button("Switch branch in this working copy…").querySelector("svg")
        ?.outerHTML,
    ).toBe(
      renderToStaticMarkup(createElement(GitBranch, { className: "size-3.5" })),
    );
    await act(async () =>
      button("Switch branch in this working copy…").click(),
    );
    const branchTrigger = container.querySelector<HTMLButtonElement>("button")!;
    expect(
      document.querySelector('[aria-label="Branch picker"]'),
    ).not.toBeNull();
    expect(branchTrigger.className).toBe(trigger.className);
    expect(branchTrigger.innerHTML).toBe(trigger.innerHTML);
    expect(branchTrigger.parentElement?.className).toBe(
      trigger.parentElement?.className,
    );
    expect(branchTrigger.getAttribute("aria-expanded")).toBe("true");

    await act(async () =>
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })),
    );
    const restored = container.querySelector<HTMLButtonElement>("button")!;
    expect(restored.getAttribute("aria-label")).toBe("Choose working copy");
    expect(restored.innerHTML).toBe(trigger.innerHTML);
    expect(restored.className).toBe(trigger.className);
  },
);

it("caps the picker height without forcing short lists to fill it", async () => {
  const request = deferred();
  vi.mocked(listWorktrees).mockReturnValue(request.promise);
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/cold-picker",
        executionCwd: "/cold-picker",
        onSelect: vi.fn(),
      }),
    ),
  );
  await act(async () =>
    container.querySelector<HTMLButtonElement>("button")!.click(),
  );
  const menu = document.querySelector<HTMLElement>('[role="dialog"]')!;
  expect(menu.style.height).toBe("");
  expect(menu.style.maxHeight).toBe("398px");
  expect(menu.textContent).toContain("Loading working copies");
  expect(listWorktrees).toHaveBeenCalledTimes(1);
  await act(async () => request.resolve(result()));
  expect(menu.style.height).toBe("");
  expect(menu.style.maxHeight).toBe("398px");
  expect(menu.querySelector('[role="listbox"]')?.className).toContain(
    "overflow-y-auto",
  );
  expect(menu.textContent).not.toContain("Loading working copies");
});

it("uses the shared project picker without leaking late worktree responses", async () => {
  const request = deferred();
  vi.mocked(listWorktrees)
    .mockReturnValueOnce(request.promise)
    .mockResolvedValue(result("second"));
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/first-project",
        recents: [{ path: "/second-project", openedAt: 1 }],
        onRemove: vi.fn(),
      }),
    ),
  );
  expect(container.querySelector("select")).toBeNull();
  const picker = container.querySelector<HTMLButtonElement>(
    '[aria-label^="Switch project"]',
  )!;
  expect(picker.parentElement?.className).toContain("w-fit");
  expect(picker.parentElement?.className).not.toContain("flex-1");
  expect(picker.className.split(/\s+/)).not.toContain("w-full");
  expect(picker.className).toContain("h-7.5");
  expect(picker.querySelectorAll("svg").length).toBeGreaterThanOrEqual(2);
  const create = button("Create worktree");
  expect(create.className).toContain("h-7.5");
  const refresh = container.querySelector<HTMLButtonElement>(
    '[aria-label="Refresh worktrees"]',
  )!;
  expect(refresh.textContent).toBe("Refresh");
  expect(refresh.className).toContain("bg-content/8");
  expect(refresh.parentElement?.textContent).toContain(
    "Deleting one keeps its sessions by default",
  );
  await act(async () => picker.click());
  expect(
    document.querySelector('input[placeholder="Search projects..."]'),
  ).not.toBeNull();
  const secondProject = document.querySelector<HTMLButtonElement>(
    'button[title="/second-project"]',
  )!;
  expect(secondProject.querySelector("svg")).not.toBeNull();
  await act(async () => secondProject.click());
  expect(
    container.querySelector('[aria-label="Reveal second"]'),
  ).not.toBeNull();
  await act(async () => request.resolve(result("first")));
  expect(container.querySelector('[aria-label="Reveal first"]')).toBeNull();
  expect(
    container.querySelector('[aria-label="Reveal second"]'),
  ).not.toBeNull();
});

it("refetches after a Git mutation that arrived during an in-flight read", async () => {
  const request = deferred();
  vi.mocked(listWorktrees)
    .mockReturnValueOnce(request.promise)
    .mockResolvedValue(result("new-worktree"));
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/mutation-during-fetch",
        onRemove: vi.fn(),
      }),
    ),
  );
  await act(async () => notifyGitChanged());
  expect(listWorktrees).toHaveBeenCalledTimes(1);
  await act(async () => request.resolve(result("before-mutation")));
  expect(listWorktrees).toHaveBeenCalledTimes(2);
  expect(
    container.querySelector('[aria-label="Reveal new-worktree"]'),
  ).not.toBeNull();
});

it("selects an existing working copy without checking out a branch", async () => {
  const onSelect = vi.fn(async () => {});
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/repo",
        executionCwd: "/repo",
        onSelect,
      }),
    ),
  );
  await act(async () =>
    container.querySelector<HTMLButtonElement>("button")!.click(),
  );
  expect(document.body.textContent).not.toContain(
    "Another working copy opens a new session.",
  );
  const option =
    document.querySelectorAll<HTMLButtonElement>('[role="option"]')[1];
  await act(async () => option.click());
  expect(onSelect).toHaveBeenCalledWith(tree);
  expect(gitCheckout).not.toHaveBeenCalled();
  expect(
    document.querySelector('[aria-label="Search working copies"]'),
  ).toBeNull();
});

it("explains that a started session opens another working copy in a new session", async () => {
  const onSelect = vi.fn(async () => {});
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/repo",
        executionCwd: tree.path,
        opensNewSession: true,
        onSelect,
      }),
    ),
  );
  await act(async () =>
    container.querySelector<HTMLButtonElement>("button")!.click(),
  );
  expect(document.body.textContent).toContain(
    "Another working copy opens a new session.",
  );
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[role="option"]')!.click(),
  );
  expect(onSelect).toHaveBeenCalledWith(
    expect.objectContaining({ path: "/repo", isMain: true }),
  );
  expect(gitCheckout).not.toHaveBeenCalled();
});

it("can return to the main working copy after a worktree is deleted externally", async () => {
  vi.mocked(useProjectBranchesState).mockReturnValue({
    branches: null,
    settled: true,
  });
  const onSelect = vi.fn(async () => {});
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/repo",
        executionCwd: tree.path,
        onSelect,
      }),
    ),
  );
  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  expect(trigger.disabled).toBe(false);
  expect(trigger.textContent).toContain("Worktree unavailable");
  await act(async () => trigger.click());
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[role="option"]')!.click(),
  );
  expect(onSelect).toHaveBeenCalledWith(
    expect.objectContaining({ path: "/repo", isMain: true }),
  );
});

it("confirms session and local-change deletion before removing a worktree", async () => {
  const onRemove = vi.fn().mockResolvedValue(undefined);
  const onDeleted = vi.fn();
  await act(async () =>
    root.render(
      createElement(DeleteWorktreeDialog, {
        cwd: "/repo",
        tree,
        sessionCount: 2,
        onRemove,
        onDeleted,
        onClose: vi.fn(),
      }),
    ),
  );
  const toggle = document.querySelector<HTMLButtonElement>('[role="switch"]')!;
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  expect(document.body.textContent).toContain(
    "2 sessions using this worktree are kept.",
  );
  await act(async () => toggle.click());
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  expect(document.body.textContent).toContain(
    "2 sessions using this worktree are permanently deleted.",
  );
  expect(document.body.textContent).toContain(
    "All uncommitted and untracked changes",
  );
  const remove = button("Delete worktree and sessions");
  expect(remove.disabled).toBe(true);
  await act(async () => remove.click());
  expect(onRemove).not.toHaveBeenCalled();
  await confirmWorktree("feature");
  expect(remove.disabled).toBe(false);
  await act(async () => remove.click());
  expect(onRemove).toHaveBeenLastCalledWith("/repo", tree.path, true, true);
  expect(onDeleted).toHaveBeenCalledOnce();
});

it("offers cascade deletion for a session-linked worktree but not the main copy", async () => {
  vi.mocked(listWorktrees).mockResolvedValue({
    worktrees: [
      { ...tree, path: "/repo", branch: "main", isMain: true },
      { ...tree, sessionIds: ["session-1"] },
    ],
    defaultRoot: "/repo-worktrees",
  });
  const onDeleteSessions = vi.fn().mockResolvedValue(true);
  const onRemove = vi.fn().mockResolvedValue(undefined);
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/repo",
        onDeleteSessions,
        onRemove,
      }),
    ),
  );

  expect(document.querySelector('[aria-label="Delete main"]')).toBeNull();
  expect(document.querySelector('[aria-label="Reveal main"]')).toBeNull();
  const removeFeature = document.querySelector<HTMLButtonElement>(
    '[aria-label="Delete feature"]',
  )!;
  expect(removeFeature.disabled).toBe(false);
  await act(async () => removeFeature.click());
  await confirmWorktree("feature");
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(),
  );
  await act(async () => button("Delete worktree and session").click());
  expect(onDeleteSessions).toHaveBeenCalledWith(["session-1"]);
  expect(onRemove).toHaveBeenCalledWith("/repo", tree.path, true, false);
  expect(checkWorktreeRemoval).toHaveBeenCalledWith("/repo", tree.path, true);
  expect(
    vi.mocked(checkWorktreeRemoval).mock.invocationCallOrder[0],
  ).toBeLessThan(onDeleteSessions.mock.invocationCallOrder[0]);
  expect(onDeleteSessions.mock.invocationCallOrder[0]).toBeLessThan(
    onRemove.mock.invocationCallOrder[0],
  );
});

it.each(["file", "terminal", "native"] as const)(
  "keeps every session when a %s blocker prevents worktree deletion",
  async (blocker) => {
    vi.mocked(listWorktrees).mockResolvedValue({
      ...result(),
      worktrees: [{ ...tree, sessionIds: ["session-1", "session-2"] }],
    });
    const onDeleteSessions = vi.fn().mockResolvedValue(true);
    const onRemove = vi.fn();
    const onCheckRemove = async (cwd: string, path: string, force: boolean) => {
      const files =
        blocker === "file"
          ? [newFileTab(`${tree.path}/file.ts`, tree.path)]
          : blocker === "terminal"
            ? [newTerminalFile(tree.path)]
            : [];
      assertWorktreeFilesClosed(path, files);
      await checkWorktreeRemoval(cwd, path, force);
    };
    if (blocker === "native") {
      vi.mocked(checkWorktreeRemoval).mockRejectedValue(
        new Error("This worktree is locked"),
      );
    }
    await act(async () =>
      root.render(
        createElement(WorktreesPage, {
          cwd: `/blocked-${blocker}`,
          onCheckRemove,
          onDeleteSessions,
          onRemove,
        }),
      ),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Delete feature"]')!
        .click(),
    );
    await confirmWorktree("feature");
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(),
    );
    await act(async () => button("Delete worktree and sessions").click());
    expect(onDeleteSessions).not.toHaveBeenCalled();
    expect(onRemove).not.toHaveBeenCalled();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      blocker === "native" ? "locked" : "Close the files and terminals",
    );
  },
);

it("reports partial completion if worktree removal fails after successful preflight", async () => {
  vi.mocked(listWorktrees).mockResolvedValue({
    ...result(),
    worktrees: [{ ...tree, sessionIds: ["session-1"] }],
  });
  const onDeleteSessions = vi.fn().mockResolvedValue(true);
  const onRemove = vi.fn().mockRejectedValue(new Error("Disk unavailable"));
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/late-removal-failure",
        onDeleteSessions,
        onRemove,
      }),
    ),
  );
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Delete feature"]')!
      .click(),
  );
  await confirmWorktree("feature");
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(),
  );
  await act(async () => button("Delete worktree and session").click());
  expect(onDeleteSessions).toHaveBeenCalledOnce();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "The sessions were deleted, but the worktree was kept.",
  );
});

it.each(["partial", "rejected", "worktree", "refresh"] as const)(
  "refreshes the session IDs before retrying a %s deletion failure",
  async (failure) => {
    vi.mocked(listWorktrees).mockResolvedValue({
      ...result(),
      worktrees: [{ ...tree, sessionIds: ["deleted", "remaining"] }],
    });
    const onDeleteSessions = vi.fn().mockResolvedValue(false);
    const onRemove = vi.fn().mockResolvedValue(undefined);
    if (failure === "rejected") {
      onDeleteSessions.mockRejectedValue(new Error("Session deletion failed"));
    } else if (failure === "worktree") {
      onDeleteSessions.mockResolvedValue(true);
      onRemove.mockRejectedValue(new Error("Disk unavailable"));
    }
    await act(async () =>
      root.render(
        createElement(WorktreesPage, {
          cwd: `/retry-${failure}`,
          onDeleteSessions,
          onRemove,
        }),
      ),
    );
    const remove = container.querySelector<HTMLButtonElement>(
      '[aria-label="Delete feature"]',
    )!;
    await act(async () => remove.click());
    await confirmWorktree("feature");
    await act(async () =>
      document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(),
    );
    const refresh = deferred();
    vi.mocked(listWorktrees).mockReturnValue(refresh.promise);
    await act(async () => button("Delete worktree and sessions").click());
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(remove.disabled).toBe(true);
    expect(onDeleteSessions).toHaveBeenCalledWith(["deleted", "remaining"]);
    const sessionIds = failure === "worktree" ? [] : ["remaining"];
    if (failure === "refresh") {
      await act(async () => refresh.reject(new Error("Refresh unavailable")));
      expect(remove.disabled).toBe(true);
      vi.mocked(listWorktrees).mockResolvedValue({
        ...result(),
        worktrees: [{ ...tree, sessionIds }],
      });
      await act(async () => button("Refresh").click());
    }
    await act(async () =>
      refresh.resolve({ ...result(), worktrees: [{ ...tree, sessionIds }] }),
    );
    expect(remove.disabled).toBe(false);
    onDeleteSessions.mockResolvedValue(true);
    onRemove.mockResolvedValue(undefined);
    await act(async () => remove.click());
    await confirmWorktree("feature");
    if (sessionIds.length) {
      expect(document.body.textContent).toContain(
        "1 session using this worktree is kept.",
      );
      await act(async () =>
        document.querySelector<HTMLButtonElement>('[role="switch"]')!.click(),
      );
      await act(async () => button("Delete worktree and session").click());
      expect(onDeleteSessions).toHaveBeenLastCalledWith(["remaining"]);
    } else {
      await act(async () => button("Delete worktree").click());
      expect(onDeleteSessions).toHaveBeenCalledOnce();
    }
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  },
);

it("offers to keep or delete the last session's worktree", async () => {
  const onClose = vi.fn();
  await act(async () =>
    root.render(
      createElement(DeleteSessionDialog, {
        title: "Feature",
        unusedWorktree: tree.path,
        onClose,
      }),
    ),
  );
  const checkbox = document.querySelector<HTMLInputElement>(
    'input[type="checkbox"]',
  )!;
  expect(checkbox.checked).toBe(false);
  await act(async () => button("Delete session").click());
  expect(onClose).toHaveBeenLastCalledWith({
    confirmed: true,
    deleteWorktree: false,
  });
  await act(async () => checkbox.click());
  await act(async () => button("Delete session").click());
  expect(onClose).toHaveBeenLastCalledWith({
    confirmed: true,
    deleteWorktree: true,
  });
});

it("uses searchable custom selects in the create worktree dialog", async () => {
  vi.mocked(useProjectBranchesState).mockReturnValue({
    branches: {
      current: "main",
      detached: false,
      branches: [
        { name: "main", remote: null, current: true },
        { name: "feature/search", remote: null, current: false },
        { name: "release", remote: "origin", current: false },
      ],
    },
    settled: true,
  });
  const onCancel = vi.fn();
  await act(async () =>
    root.render(
      createElement(CreateWorktreeDialog, {
        cwd: "/repo",
        baseCwd: "/repo",
        onCreated: vi.fn(),
        onCancel,
      }),
    ),
  );

  expect(document.querySelector("select")).toBeNull();
  const branchType = document.querySelector<HTMLButtonElement>(
    '[aria-label^="Branch type:"]',
  )!;
  await act(async () => branchType.click());
  let search = document.querySelector<HTMLInputElement>(
    'input[placeholder="Search options…"]',
  )!;
  await act(async () => {
    type(search, "existing");
  });
  expect(document.querySelectorAll('[role="option"]')).toHaveLength(1);
  await act(async () =>
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );

  const existingBranch = document.querySelector<HTMLButtonElement>(
    '[aria-label^="Existing branch:"]',
  )!;
  expect(existingBranch.textContent).toContain("Choose a branch…");
  await act(async () => existingBranch.click());
  search = document.querySelector<HTMLInputElement>(
    'input[placeholder="Search local branches…"]',
  )!;
  await act(async () => {
    type(search, "feature");
  });
  expect(document.querySelectorAll('[role="option"]')).toHaveLength(1);
  expect(document.body.textContent).not.toContain("origin/release");
  await act(async () =>
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );
  expect(existingBranch.textContent).toContain("feature/search");

  await act(async () => branchType.click());
  search = document.querySelector<HTMLInputElement>(
    'input[placeholder="Search options…"]',
  )!;
  await act(async () => {
    type(search, "new");
  });
  await act(async () =>
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );
  const startFrom = document.querySelector<HTMLButtonElement>(
    '[aria-label^="Start from:"]',
  )!;
  await act(async () => startFrom.click());
  search = document.querySelector<HTMLInputElement>(
    'input[placeholder="Search branches and refs…"]',
  )!;
  expect(search).not.toBeNull();
  await act(async () => type(search, "no-matching-branch"));
  const enter = new KeyboardEvent("keydown", {
    key: "Enter",
    bubbles: true,
    cancelable: true,
  });
  await act(async () => search.dispatchEvent(enter));
  expect(enter.defaultPrevented).toBe(true);
  expect(search.isConnected).toBe(true);
  await act(async () =>
    search.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(
    document.querySelector('input[placeholder="Search branches and refs…"]'),
  ).toBeNull();
  expect(document.body.textContent).toContain("Create worktree");
  expect(onCancel).not.toHaveBeenCalled();
});

it("keeps associated sessions by default when deleting a worktree", async () => {
  vi.mocked(listWorktrees).mockResolvedValue({
    ...result(),
    worktrees: [{ ...tree, sessionIds: ["saved", "archived"] }],
  });
  const onDeleteSessions = vi.fn();
  const onRemove = vi.fn().mockResolvedValue(undefined);
  await act(async () =>
    root.render(
      createElement(WorktreesPage, {
        cwd: "/keep-sessions",
        onDeleteSessions,
        onRemove,
      }),
    ),
  );
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Delete feature"]')!
      .click(),
  );
  await confirmWorktree("feature");
  await act(async () => button("Delete worktree").click());
  expect(onDeleteSessions).not.toHaveBeenCalled();
  expect(onRemove).toHaveBeenCalledWith(
    "/keep-sessions",
    tree.path,
    true,
    true,
  );
});

it("shows no selected branch after removal even when the old path has a branch again", async () => {
  const onSelect = vi.fn(async () => {});
  await act(async () =>
    root.render(
      createElement(WorktreePicker, {
        cwd: "/repo",
        executionCwd: tree.path,
        worktreeRemoved: true,
        opensNewSession: true,
        onSelect,
      }),
    ),
  );
  const trigger = container.querySelector<HTMLButtonElement>("button")!;
  expect(trigger.textContent).toBe("No branch selected");
  expect(trigger.disabled).toBe(false);
  await act(async () => trigger.click());
  expect(document.querySelector('[aria-selected="true"]')).toBeNull();
  expect(document.body.textContent).not.toContain(
    "Another working copy opens a new session.",
  );
  expect(button("Switch branch in this working copy…").disabled).toBe(true);
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[role="option"]')!.click(),
  );
  expect(onSelect).toHaveBeenCalledWith(
    expect.objectContaining({ path: "/repo" }),
  );
});
