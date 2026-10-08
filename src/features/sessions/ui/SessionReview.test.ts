// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../model/checkpoint", () => ({
  sessionCheckpointStatus: vi.fn(),
  subscribeReviewChanged: () => () => {},
  keepSessionChanges: vi.fn(),
  undoSessionChanges: vi.fn(),
}));
vi.mock("../../../platform/tauri/fs", () => ({
  basename: (path: string) => path.split("/").pop() ?? path,
  notifyGitChanged: vi.fn(),
  subscribeGitChanged: () => () => {},
}));
vi.mock("../../files/model/fileIndex", () => ({
  invalidateProjectFiles: vi.fn(),
}));
vi.mock("../../files/model/fileWatch", () => ({
  invalidateWatchedFiles: vi.fn(),
}));
vi.mock("../../files/ui/FileTypeIcon", () => ({ FileTypeIcon: () => null }));

import {
  sessionCheckpointStatus,
  type CheckpointFile,
} from "../model/checkpoint";
import { SessionReview } from "./SessionReview";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(sessionCheckpointStatus).mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function file(overrides: Partial<CheckpointFile> = {}): CheckpointFile {
  return {
    path: "/repo/src-tauri/src/gitlab.rs",
    relative: "src-tauri/src/gitlab.rs",
    status: "modified",
    additions: 0,
    deletions: 0,
    exact: false,
    undoable: false,
    ...overrides,
  };
}

async function render(files: CheckpointFile[]) {
  vi.mocked(sessionCheckpointStatus).mockResolvedValue({ files });
  await act(async () => {
    root.render(
      createElement(SessionReview, {
        sessionId: "session",
        cwd: "/repo",
        onOpenDiff: vi.fn(),
      }),
    );
  });
}

describe("SessionReview line counts", () => {
  it("does not present unavailable mixed-change counts as zero", async () => {
    await render([file()]);
    expect(container.textContent).toContain("Changed 1 file");
    expect(container.textContent).toContain("Line counts unavailable");
    expect(container.textContent).not.toContain("+0");
    expect(container.textContent).not.toContain("-0");
    const undo = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Undo",
    );
    expect(undo?.disabled).toBe(true);
  });

  it("does not present a partial sum as the total when a file is mixed", async () => {
    await render([
      file({ exact: true, undoable: true, additions: 8, deletions: 1 }),
      file({ path: "/repo/mixed.ts", relative: "mixed.ts" }),
    ]);
    expect(container.textContent).toContain("Changed 2 files");
    expect(container.textContent).toContain("Line counts unavailable");
    expect(container.querySelectorAll(".text-diff-add-fg")).toHaveLength(1);
    expect(container.textContent).toContain("+8");
    expect(container.textContent).toContain("-1");
  });

  it("sums exact session counts even when undo is unsafe", async () => {
    await render([
      file({ exact: true, additions: 8, deletions: 1 }),
      file({
        path: "/repo/other.ts",
        relative: "other.ts",
        exact: true,
        additions: 2,
        deletions: 3,
      }),
    ]);
    expect(container.textContent).toContain("+10");
    expect(container.textContent).toContain("-4");
    expect(container.textContent).not.toContain("Line counts unavailable");
  });
});
