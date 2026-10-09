// @vitest-environment happy-dom
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));
vi.mock("./MonoSidebar", () => ({
  MonoSidebar: ({ children }: { children: ReactNode }) => children,
  MonoSidebarHeader: ({ heading }: { heading: ReactNode }) => heading,
}));
vi.mock("../../source-control/ui/SessionChangesDiff", () => ({
  SessionChangesDiff: () => null,
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
  gitStageFile: vi.fn(),
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

import { MonoChangesPanel } from "./MonoChangesPanel";
import {
  gitCommit,
  gitDiffIndex,
  gitLocateFiles,
  gitPush,
  gitStageFile,
  type GitDiffIndex,
} from "../../../platform/tauri/fs";
import { generateCommitMessage } from "../../../integrations/harness";
import {
  keepSessionChanges,
  sessionCheckpointStatus,
} from "../../sessions/model/checkpoint";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.clearAllMocks();
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
  vi.mocked(gitLocateFiles).mockResolvedValue(
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
          staged: relative === "other.txt",
          unstaged: relative !== "other.txt",
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

async function render() {
  await act(async () =>
    root.render(
      createElement(MonoChangesPanel, {
        sessionId: "mono",
        cwd: "/home",
        request: { tab: "commit" },
        color: "blue",
        onClose: () => {},
      }),
    ),
  );
}

function active() {
  return container.querySelector<HTMLDivElement>(
    '[role="tabpanel"][aria-label="Commit"]:not([hidden])',
  )!;
}

async function click(button: HTMLButtonElement | null) {
  expect(button).not.toBeNull();
  expect(button!.disabled).toBe(false);
  await act(async () => button!.click());
}

async function switchRepo(name: "a" | "b") {
  await click(container.querySelector('button[aria-haspopup="menu"]'));
  await click(
    container.querySelector(`[role="menuitemradio"][title="/home/${name}"]`),
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

it("preserves repo drafts and selections and commits and pushes only the chosen repo", async () => {
  await render();
  await message("Draft A");
  const extra = active().querySelector('[title="extra.txt"]')!.closest("li")!;
  await click(extra.querySelector('[title="Unstage Changes"]'));
  await switchRepo("b");
  await message("Draft B");
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("Draft A");
  expect(
    active()
      .querySelector('[title="extra.txt"]')!
      .closest("li")!
      .querySelector('[title="Stage Changes"]'),
  ).not.toBeNull();
  await switchRepo("b");
  expect(active().querySelector("textarea")!.value).toBe("Draft B");
  await click(active().querySelector('[aria-label="Commit options"]'));
  await click(
    Array.from(
      active().querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((button) => button.textContent === "Commit & Push")!,
  );
  expect(gitCommit).toHaveBeenCalledExactlyOnceWith(
    "/home/b",
    "Draft B",
    false,
    ["same.txt"],
  );
  expect(keepSessionChanges).toHaveBeenCalledExactlyOnceWith(
    "mono",
    "/home",
    "b/same.txt",
  );
  expect(gitPush).toHaveBeenCalledExactlyOnceWith("/home/b");
  expect(gitStageFile).not.toHaveBeenCalled();
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("Draft A");
});

it("generates from only the active repo selection without staging files", async () => {
  await render();
  await switchRepo("b");
  await click(active().querySelector('[aria-label="Generate commit message"]'));
  expect(generateCommitMessage).toHaveBeenCalledExactlyOnceWith(
    "/home/b",
    undefined,
    expect.any(AbortSignal),
    ["same.txt"],
  );
  expect(gitStageFile).not.toHaveBeenCalled();
  expect(gitCommit).not.toHaveBeenCalled();
  expect(active().querySelector("textarea")!.value).toBe("Generated message");
  await switchRepo("a");
  expect(active().querySelector("textarea")!.value).toBe("");
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
