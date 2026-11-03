// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { buildUnifiedFile } from "../model/unifiedDiff";
import type { UnifiedDiffFileModel } from "./UnifiedDiffView";

const native = vi.hoisted(() => ({
  commits: vi.fn(),
  commitFile: vi.fn(),
  status: vi.fn(),
  sessionFile: vi.fn(),
  listeners: new Set<(id: string) => void>(),
  models: [] as UnifiedDiffFileModel[],
}));
vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitFiles: native.commits,
  gitCommitFileDiff: native.commitFile,
}));
vi.mock("../../sessions/model/checkpoint", () => ({
  sessionCheckpointStatus: native.status,
  sessionCheckpointFileDiff: native.sessionFile,
  subscribeReviewChanged: (listener: (id: string) => void) => {
    native.listeners.add(listener);
    return () => native.listeners.delete(listener);
  },
}));
vi.mock("../model/diffWorker", () => ({ prepareUnifiedDiff: vi.fn() }));
vi.mock("./UnifiedDiffView", () => ({
  UnifiedDiffView: ({ files }: { files: UnifiedDiffFileModel[] }) => {
    native.models = files;
    return null;
  },
}));
import { prepareUnifiedDiff } from "../model/diffWorker";
import { CommitDiff } from "./CommitDiff";
import { SessionChangesDiff } from "./SessionChangesDiff";

let root: Root;
let host: HTMLDivElement;
const file = {
  path: "/repo/a.ts",
  relative: "a.ts",
  status: "modified",
  additions: 1,
  deletions: 1,
  staged: true,
  unstaged: false,
  exact: true,
  undoable: true,
};
const snapshot = {
  original: "old\n",
  current: "new\n",
  binary: false,
  tooLarge: false,
};
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.resetAllMocks();
  native.listeners.clear();
  native.models = [];
  native.commits.mockResolvedValue([file]);
  native.commitFile.mockResolvedValue(snapshot);
  native.status.mockResolvedValue({ files: [file] });
  native.sessionFile.mockResolvedValue(snapshot);
  vi.mocked(prepareUnifiedDiff).mockImplementation(async (old, next) =>
    buildUnifiedFile(old, next),
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
const commit = async (visible = true, sha = "abc", cwd = "/repo") => {
  await act(async () =>
    root.render(createElement(CommitDiff, { cwd, sha, visible })),
  );
};
const session = async (visible = true) => {
  await act(async () =>
    root.render(
      createElement(SessionChangesDiff, {
        cwd: "/repo",
        sessionId: "s1",
        visible,
      }),
    ),
  );
};

it("loads a commit on first activation and retains the prepared snapshot on return", async () => {
  await commit(false);
  expect(native.commits).not.toHaveBeenCalled();
  await commit();
  expect(native.commitFile).toHaveBeenCalledOnce();
  const blocks = native.models[0].blocks;
  await commit(false);
  await commit();
  expect(native.commits).toHaveBeenCalledOnce();
  expect(native.commitFile).toHaveBeenCalledOnce();
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  expect(native.models[0].blocks).toBe(blocks);
  await commit(true, "different");
  expect(native.commits).toHaveBeenCalledTimes(2);
  expect(native.commitFile).toHaveBeenCalledTimes(2);
  await commit(true, "different", "/another-repo");
  expect(native.commits).toHaveBeenCalledTimes(3);
});

it("resumes a partially loaded commit without rereading completed files and retries errors", async () => {
  native.commits.mockResolvedValue([
    file,
    { ...file, path: "/repo/b.ts", relative: "b.ts" },
  ]);
  let attempts = 0;
  native.commitFile.mockImplementation(async (_cwd, _sha, path) => {
    if (path === "b.ts" && attempts++ === 0)
      throw new Error("temporarily unavailable");
    return snapshot;
  });
  await commit();
  expect(native.models[1].emptyMessage).toContain("temporarily unavailable");
  const first = native.models[0].blocks;
  await commit(false);
  await commit();
  expect(
    native.commitFile.mock.calls.filter((call) => call[2] === "a.ts"),
  ).toHaveLength(1);
  expect(
    native.commitFile.mock.calls.filter((call) => call[2] === "b.ts"),
  ).toHaveLength(2);
  expect(native.models[0].blocks).toBe(first);
  expect(native.models[1].emptyMessage).toBeUndefined();
  await commit(false);
  await commit();
  expect(native.commits).toHaveBeenCalledTimes(2);
});

it("revalidates session snapshots on return without rebuilding unchanged blocks", async () => {
  await session(false);
  expect(native.status).not.toHaveBeenCalled();
  await session();
  const first = native.models[0].blocks;
  await session(false);
  await session();
  expect(native.status).toHaveBeenCalledTimes(2);
  expect(native.sessionFile).toHaveBeenCalledTimes(2);
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  expect(native.models[0].blocks).toBe(first);
  await session(false);
  native.sessionFile.mockResolvedValue({
    ...snapshot,
    current: "changed while away\n",
  });
  await session();
  expect(prepareUnifiedDiff).toHaveBeenCalledTimes(2);
  expect(native.models[0].blocks).not.toBe(first);
});

it("still updates the session review when a new checkpoint is published", async () => {
  await session();
  await act(async () => {
    for (const listener of native.listeners) listener("s1");
  });
  expect(prepareUnifiedDiff).toHaveBeenCalledOnce();
  native.sessionFile.mockResolvedValue({ ...snapshot, current: "next turn\n" });
  await act(async () => {
    for (const listener of native.listeners) listener("s1");
  });
  expect(prepareUnifiedDiff).toHaveBeenCalledTimes(2);
  expect(
    native.models[0].blocks
      .flatMap((block) => block.lines)
      .some((line) => line.text === "next turn"),
  ).toBe(true);
});
