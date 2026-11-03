// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { buildUnifiedFile } from "../model/unifiedDiff";
import type { UnifiedDiffFileModel } from "./UnifiedDiffView";

const state = vi.hoisted(() => ({
  refresh: (() => {}) as () => void,
  models: [] as UnifiedDiffFileModel[],
}));
vi.mock("../../../platform/tauri/fs", () => ({
  gitDiffFiles: vi.fn(),
  gitFileDiff: vi.fn(),
  gitStageFile: vi.fn(),
  gitDiscardFile: vi.fn(),
  gitStageContents: vi.fn(),
  notifyGitChanged: vi.fn(),
  subscribeGitChanged: (listener: () => void) => {
    state.refresh = listener;
    return () => {};
  },
}));
vi.mock("../model/diffWorker", () => ({ prepareUnifiedDiff: vi.fn() }));
vi.mock("../../files/editor/editorGit", () => ({ stageChunkText: vi.fn() }));
vi.mock("./UnifiedDiffView", () => ({
  UnifiedDiffView: ({ files }: { files: UnifiedDiffFileModel[] }) => {
    state.models = files;
    return null;
  },
}));
import { gitDiffFiles, gitFileDiff } from "../../../platform/tauri/fs";
import { prepareUnifiedDiff } from "../model/diffWorker";
import { WorkingTreeDiff } from "./WorkingTreeDiff";
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(gitDiffFiles).mockResolvedValue({
    files: [
      {
        path: "/repo/a.ts",
        relative: "a.ts",
        status: "modified",
        additions: 1,
        deletions: 1,
        staged: false,
        unstaged: true,
      },
    ],
  } as Awaited<ReturnType<typeof gitDiffFiles>>);
  vi.mocked(gitFileDiff).mockResolvedValue({
    original: "old\n",
    current: "new\n",
    binary: false,
    tooLarge: false,
  } as Awaited<ReturnType<typeof gitFileDiff>>);
  vi.mocked(prepareUnifiedDiff).mockImplementation(async (old, next) =>
    buildUnifiedFile(old, next),
  );
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function render(focusPath = "/repo/a.ts", visible = true) {
  await act(async () =>
    root.render(
      createElement(WorkingTreeDiff, { cwd: "/repo", focusPath, visible }),
    ),
  );
}
async function refresh() {
  await act(async () => {
    state.refresh();
    await vi.advanceTimersByTimeAsync(20);
  });
}

it("reuses prepared blocks on unchanged refreshes and recomputes changed content", async () => {
  await render();
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  const previous = state.models[0].blocks;
  await refresh();
  expect(gitFileDiff).toHaveBeenCalledTimes(2);
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  expect(state.models[0].blocks).toBe(previous);
  vi.mocked(gitFileDiff).mockResolvedValueOnce({
    original: "old\n",
    current: "different\n",
    binary: false,
    tooLarge: false,
  } as Awaited<ReturnType<typeof gitFileDiff>>);
  await refresh();
  expect(prepareUnifiedDiff).toHaveBeenCalledTimes(2);
  expect(state.models[0].blocks).not.toBe(previous);
});

it("does not reload Git data when only the focused file changes", async () => {
  await render();
  await render("/repo/b.ts");
  expect(gitDiffFiles).toHaveBeenCalledOnce();
  expect(gitFileDiff).toHaveBeenCalledOnce();
});

it("keeps the cached review while checking for changes after returning to its tab", async () => {
  await render();
  const blocks = state.models[0].blocks;
  await render("/repo/a.ts", false);
  let resolveIndex!: (value: Awaited<ReturnType<typeof gitDiffFiles>>) => void;
  vi.mocked(gitDiffFiles).mockReturnValueOnce(
    new Promise((resolve) => {
      resolveIndex = resolve;
    }),
  );
  await render();
  expect(state.models[0].blocks).toBe(blocks);
  expect(state.models[0].emptyMessage).toBeUndefined();
  await act(async () =>
    resolveIndex({
      files: [
        {
          path: "/repo/a.ts",
          relative: "a.ts",
          status: "modified",
          additions: 1,
          deletions: 1,
          staged: false,
          unstaged: true,
        },
      ],
    }),
  );
  expect(gitFileDiff).toHaveBeenCalledTimes(2);
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  expect(state.models[0].blocks).toBe(blocks);
});
