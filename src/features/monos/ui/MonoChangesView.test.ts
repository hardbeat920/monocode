// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));
vi.mock("../../../platform/tauri/fs", () => ({
  gitLocateFiles: vi.fn(),
  gitDiffIndex: vi.fn(),
  gitPrStatus: vi.fn(async () => null),
  gitHistory: vi.fn(async () => []),
  gitCommit: vi.fn(async () => {}),
  gitPush: vi.fn(async () => {}),
  gitSync: vi.fn(async () => {}),
  gitPrCreate: vi.fn(),
  gitStageFile: vi.fn(async () => {}),
  gitUnstageFile: vi.fn(async () => {}),
  gitStageAll: vi.fn(async () => {}),
  gitUnstageAll: vi.fn(async () => {}),
  gitDiscardFile: vi.fn(),
  notifyGitChanged: vi.fn(),
  subscribeGitChanged: () => () => {},
  basename: (path: string) => path.split("/").pop() ?? path,
}));
vi.mock("../../../integrations/harness", () => ({
  generateCommitMessage: vi.fn(async () => "Generated message"),
  generatePrContent: vi.fn(),
}));
vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles: vi.fn(),
}));
vi.mock("../../inbox/model/inboxSelfActivity", () => ({
  recordInboxSelfActivity: vi.fn(),
}));
vi.mock("../../sessions/model/checkpoint", () => ({
  sessionCheckpointStatus: vi.fn(),
  keepSessionChanges: vi.fn(async () => {}),
  notifyReviewChanged: vi.fn(),
  subscribeReviewChanged: () => () => {},
}));

vi.mock("../../source-control/model/worktrees", () => ({
  listWorktrees: vi.fn(async (cwd: string) => ({
    defaultRoot: "",
    worktrees: [{ path: cwd.replace(/-wt$/, ""), isMain: true }],
  })),
}));

import { MonoChangesView } from "./MonoChangesView";
import {
  gitCommit,
  gitDiffIndex,
  gitLocateFiles,
  gitPush,
  gitStageFile,
  type GitDiffIndex,
  type GitFileLocation,
} from "../../../platform/tauri/fs";
import { generateCommitMessage } from "../../../integrations/harness";
import {
  keepSessionChanges,
  sessionCheckpointStatus,
} from "../../sessions/model/checkpoint";

/** Locate the attached projects' folders, then the session's files. */
function locateAs(
  files: (GitFileLocation | null)[],
  attached: (GitFileLocation | null)[] = [],
) {
  vi.mocked(gitLocateFiles).mockResolvedValue([...attached, ...files]);
}

let container: HTMLDivElement;
let root: Root;
// The view caches each session's projects, so every test is its own session.
let sessionCount = 0;
let session = "mono";

beforeEach(() => {
  vi.clearAllMocks();
  session = `mono-${++sessionCount}`;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(window, "alert").mockImplementation(() => {});
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  const paths = [
    "a/same.txt",
    "a/extra.txt",
    "b/same.txt",
    "b/reverted.txt",
    "outside.txt",
  ];
  vi.mocked(sessionCheckpointStatus).mockResolvedValue({
    files: paths.map((relative) => ({
      path: `/home/${relative}`,
      relative,
      status: "modified",
      additions: 1,
      deletions: 0,
      exact: true,
      undoable: true,
    })),
  });
  locateAs(
    paths.map((path) =>
      path.includes("/")
        ? {
            root: `/home/${path.split("/")[0]}`,
            relative: path.split("/")[1],
          }
        : null,
    ),
  );
  vi.mocked(gitDiffIndex).mockImplementation(
    async (cwd) =>
      ({
        branch: "feature",
        head: "abc",
        additions: 2,
        deletions: 0,
        remote: "origin",
        upstream: "origin/feature",
        defaultBranch: "main",
        ahead: 0,
        behind: 0,
        aheadOfDefault: 0,
        headPushed: true,
        files: (cwd.endsWith("/a")
          ? ["same.txt", "extra.txt", "other.txt"]
          : ["same.txt", "other.txt"]
        ).map((relative) => ({
          path: `${cwd}/${relative}`,
          relative,
          status: "modified",
          additions: 1,
          deletions: 0,
          staged: relative === "other.txt" || (cwd.endsWith("/b") && relative === "same.txt"),
          unstaged: !(relative === "other.txt" || (cwd.endsWith("/b") && relative === "same.txt")),
        })),
      }) satisfies GitDiffIndex,
  );
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function render(attachedProjects: string[] = []) {
  await act(async () =>
    root.render(
      createElement(MonoChangesView, {
        sessionId: session,
        cwd: "/home",
        request: {},
        agentName: "Mono",
        attachedProjects,
      }),
    ),
  );
}

function active() {
  return container.querySelector<HTMLDivElement>(
    '[role="tabpanel"][aria-label^="Commit"]:not([hidden])',
  )!;
}

async function click(button: HTMLButtonElement | null) {
  expect(button).not.toBeNull();
  expect(button!.disabled).toBe(false);
  await act(async () => button!.click());
}

async function switchRepo(name: "a" | "b") {
  await click(
    container.querySelector(`[role="tab"][data-mono-project="/home/${name}"]`),
  );
}

async function message(value: string) {
  await act(async () => {
    const field = active().querySelector("textarea")!;
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("preserves repo drafts and stages, commits and pushes only in the chosen repo", async () => {
  await render();
  await message("Draft A");
  const extra = active().querySelector('[title="extra.txt"]')!.closest("li")!;
  await click(extra.querySelector('[title="Stage Changes"]'));
  expect(gitStageFile).toHaveBeenCalledExactlyOnceWith("/home/a", "extra.txt");
  await switchRepo("b");
  await message("Draft B");
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("Draft A");
  await switchRepo("b");
  expect(active().querySelector("textarea")!.value).toBe("Draft B");
  await click(active().querySelector('[aria-label="Commit options"]'));
  await click(
    Array.from(
      active().querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((button) => button.textContent === "Commit & Push")!,
  );
  expect(gitCommit).toHaveBeenCalledExactlyOnceWith("/home/b", "Draft B", false);
  expect(keepSessionChanges).toHaveBeenCalledExactlyOnceWith(
    session,
    "/home",
    "b/same.txt",
  );
  expect(gitPush).toHaveBeenCalledExactlyOnceWith("/home/b");
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("Draft A");
});

it("lists the Git index's staged and unstaged files", async () => {
  await render();
  const sections = [...active().querySelectorAll("li")].map((item) =>
    item.querySelector("[title]")?.getAttribute("title"),
  );
  expect(sections).toEqual(
    expect.arrayContaining(["other.txt", "same.txt", "extra.txt"]),
  );
  expect(active().textContent).toContain("Staged Changes");
  expect(
    active()
      .querySelector('[title="other.txt"]')!
      .closest("li")!
      .querySelector('[title="Unstage Changes"]'),
  ).not.toBeNull();
  expect(
    active()
      .querySelector('[title="same.txt"]')!
      .closest("li")!
      .querySelector('[title="Stage Changes"]'),
  ).not.toBeNull();
});

it("generates from the active repo's staged changes", async () => {
  await render();
  await switchRepo("b");
  await click(active().querySelector('[aria-label="Generate commit message"]'));
  expect(generateCommitMessage).toHaveBeenCalledExactlyOnceWith(
    "/home/b",
    undefined,
    expect.any(AbortSignal),
  );
  expect(gitCommit).not.toHaveBeenCalled();
  expect(active().querySelector("textarea")!.value).toBe("Generated message");
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("");
});

it("lists every project as a tab, the open one selected", async () => {
  await render();
  const tabs = [
    ...container.querySelectorAll<HTMLButtonElement>(
      '[role="tablist"][aria-label="Projects"] [role="tab"]',
    ),
  ];
  expect(tabs.map((tab) => tab.getAttribute("data-mono-project"))).toEqual([
    "/home/a",
    "/home/b",
  ]);
  expect(tabs.map((tab) => tab.getAttribute("aria-selected"))).toEqual([
    "true",
    "false",
  ]);
  await switchRepo("b");
  expect(tabs[1].getAttribute("aria-selected")).toBe("true");
  expect(
    active().querySelector("[data-mono-commit]")!.getAttribute("data-mono-commit"),
  ).toBe("/home/b");
});

it("shows repository discovery failures", async () => {
  vi.mocked(gitLocateFiles).mockRejectedValueOnce(
    new Error("Discovery failed"),
  );
  await render();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Discovery failed",
  );
  expect(gitCommit).not.toHaveBeenCalled();
});

it("groups a worktree under its project and switches checkouts from the chevron", async () => {
  const paths = ["a/one.txt", "a-wt/two.txt", "a-wt/three.txt"];
  vi.mocked(sessionCheckpointStatus).mockResolvedValue({
    files: paths.map((relative) => ({
      path: `/home/${relative}`,
      relative,
      status: "modified",
      additions: 1,
      deletions: 0,
      exact: true,
      undoable: true,
    })),
  });
  locateAs(
    paths.map((path) => ({
      root: `/home/${path.split("/")[0]}`,
      relative: path.split("/")[1],
    })),
  );
  await render();
  const tabs = [
    ...container.querySelectorAll<HTMLButtonElement>(
      '[role="tablist"][aria-label="Projects"] [role="tab"]',
    ),
  ];
  expect(tabs.map((tab) => tab.getAttribute("data-mono-project"))).toEqual([
    "/home/a",
  ]);
  expect(tabs[0].textContent).toContain("3");
  await click(container.querySelector('[role="tablist"] [aria-haspopup="listbox"]'));
  const options = [
    ...document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ];
  expect(options.map((option) => option.title)).toEqual([
    "/home/a",
    "/home/a-wt",
  ]);
  await click(options[1]);
  expect(
    active().querySelector("[data-mono-commit]")!.getAttribute("data-mono-commit"),
  ).toBe("/home/a-wt");
});

it("reopens a session on its cached changes while they refresh", async () => {
  await render();
  await act(async () => root.unmount());
  root = createRoot(container);
  vi.mocked(sessionCheckpointStatus).mockReturnValue(new Promise(() => {}));
  vi.mocked(gitDiffIndex).mockReturnValue(new Promise(() => {}));
  await render();
  expect(
    [...container.querySelectorAll('[role="tab"]')].map((tab) =>
      tab.getAttribute("data-mono-project"),
    ),
  ).toEqual(["/home/a", "/home/b"]);
  expect(active().querySelector('[title="extra.txt"]')).not.toBeNull();
});

it("offers the Mono's projects before the work changes anything", async () => {
  vi.mocked(sessionCheckpointStatus).mockResolvedValue({ files: [] });
  locateAs(
    [],
    [{ root: "/home/a", relative: ".monocode-probe" }, null],
  );
  await render(["/home/a", "/home/not-git"]);
  expect(
    [...container.querySelectorAll('[role="tab"]')].map((tab) =>
      tab.getAttribute("data-mono-project"),
    ),
  ).toEqual(["/home/a"]);
  expect(
    active().querySelector("[data-mono-commit]")!.getAttribute("data-mono-commit"),
  ).toBe("/home/a");
  expect(active().querySelector("textarea")).not.toBeNull();
});

it("shows an empty state when the Mono has no projects", async () => {
  vi.mocked(sessionCheckpointStatus).mockResolvedValue({ files: [] });
  locateAs([]);
  await render();
  expect(container.querySelector("textarea")).toBeNull();
  expect(container.textContent).toContain(
    "Give Mono a project to work on",
  );
  expect(gitDiffIndex).not.toHaveBeenCalled();
});
