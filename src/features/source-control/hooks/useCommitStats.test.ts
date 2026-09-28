// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitFiles: vi.fn(),
}));

import {
  gitCommitFiles,
  type GitChangedFile,
} from "../../../platform/tauri/fs";
import type { CommitStats } from "../model/commitStats";
import { useCommitStats } from "./useCommitStats";

const mockFiles = vi.mocked(gitCommitFiles);

let container: HTMLDivElement;
let root: Root;
let alive = false;
let observed: CommitStats | null | undefined;

function file(additions: number, deletions: number): GitChangedFile {
  return {
    path: "/repo/a.ts",
    relative: "a.ts",
    status: "modified",
    additions,
    deletions,
    staged: false,
    unstaged: false,
  };
}

function Probe({ cwd, sha }: { cwd: string; sha: string }) {
  observed = useCommitStats(cwd, sha);
  return null;
}

function render(cwd: string, sha: string) {
  act(() => root.render(createElement(Probe, { cwd, sha })));
}

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockFiles.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  alive = true;
  observed = undefined;
});

afterEach(() => {
  if (alive) act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("useCommitStats", () => {
  it("loads and reports the summary", async () => {
    mockFiles.mockResolvedValue([file(1, 2), file(3, 4)]);
    render("/repo-hook-load", "sha");
    expect(observed).toBeUndefined();

    await flush();
    expect(observed).toEqual({ filesChanged: 2, additions: 4, deletions: 6 });
  });

  it("reports null when Git cannot report the commit", async () => {
    mockFiles.mockRejectedValue(new Error("git exploded"));
    render("/repo-hook-error", "sha");

    await flush();
    expect(observed).toBeNull();
  });

  it("does not call Git without a project folder", async () => {
    render("~", "sha");
    await flush();
    expect(mockFiles).not.toHaveBeenCalled();
    expect(observed).toBeUndefined();
  });
});
