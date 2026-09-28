import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitCommitFiles: vi.fn(),
}));

import {
  gitCommitFiles,
  type GitChangedFile,
} from "../../../platform/tauri/fs";
import {
  loadCommitStats,
  MAX_CACHED_COMMIT_STATS,
  peekCommitStats,
  summarizeCommitFiles,
} from "./commitStats";

const mockFiles = vi.mocked(gitCommitFiles);

function file(
  additions: number,
  deletions: number,
  relative = "a.ts",
): GitChangedFile {
  return {
    path: `/repo/${relative}`,
    relative,
    status: "modified",
    additions,
    deletions,
    staged: false,
    unstaged: false,
  };
}

beforeEach(() => {
  mockFiles.mockReset();
});

describe("summarizeCommitFiles", () => {
  it("counts files and totals the line changes", () => {
    expect(summarizeCommitFiles([file(1, 2), file(3, 4, "b.ts")])).toEqual({
      filesChanged: 2,
      additions: 4,
      deletions: 6,
    });
  });

  it("reports zeros for an empty commit", () => {
    expect(summarizeCommitFiles([])).toEqual({
      filesChanged: 0,
      additions: 0,
      deletions: 0,
    });
  });

  it("clamps a negative count from a binary file", () => {
    expect(summarizeCommitFiles([file(-1, -1)])).toEqual({
      filesChanged: 1,
      additions: 0,
      deletions: 0,
    });
  });
});

describe("loadCommitStats", () => {
  it("caches a successful lookup for later peeks", async () => {
    mockFiles.mockResolvedValue([file(2, 1)]);
    expect(peekCommitStats("/repo-cache", "sha")).toBeUndefined();

    const first = await loadCommitStats("/repo-cache", "sha");
    expect(first).toEqual({ filesChanged: 1, additions: 2, deletions: 1 });
    expect(peekCommitStats("/repo-cache", "sha")).toEqual(first);

    const second = await loadCommitStats("/repo-cache", "sha");
    expect(second).toEqual(first);
    expect(mockFiles).toHaveBeenCalledTimes(1);
  });

  it("shares one request between concurrent callers", async () => {
    let resolve!: (files: GitChangedFile[]) => void;
    mockFiles.mockReturnValue(
      new Promise<GitChangedFile[]>((r) => {
        resolve = r;
      }),
    );

    const first = loadCommitStats("/repo-concurrent", "sha");
    const second = loadCommitStats("/repo-concurrent", "sha");
    expect(mockFiles).toHaveBeenCalledTimes(1);

    resolve([file(5, 5)]);
    expect(await first).toEqual(await second);
    expect(await first).toEqual({
      filesChanged: 1,
      additions: 5,
      deletions: 5,
    });
  });

  it("returns null on failure and retries on the next call", async () => {
    mockFiles.mockRejectedValueOnce(new Error("git exploded"));
    expect(await loadCommitStats("/repo-retry", "sha")).toBeNull();
    expect(peekCommitStats("/repo-retry", "sha")).toBeUndefined();

    mockFiles.mockResolvedValueOnce([file(1, 1)]);
    expect(await loadCommitStats("/repo-retry", "sha")).toEqual({
      filesChanged: 1,
      additions: 1,
      deletions: 1,
    });
    expect(mockFiles).toHaveBeenCalledTimes(2);
  });

  it("drops the oldest entries once the cache is full", async () => {
    mockFiles.mockResolvedValue([file(1, 1)]);

    // Two commits past the cap, so the very first one is evicted. Eviction is
    // what stops a long session across many projects growing without bound.
    const first = "sha-0";
    await loadCommitStats("/repo-cap", first);
    for (let i = 1; i <= MAX_CACHED_COMMIT_STATS; i++) {
      await loadCommitStats("/repo-cap", `sha-${i}`);
    }

    expect(peekCommitStats("/repo-cap", first)).toBeUndefined();
    // The most recent entry survives, and re-reading an evicted commit is a
    // fresh Git call rather than a permanent miss.
    expect(
      peekCommitStats("/repo-cap", `sha-${MAX_CACHED_COMMIT_STATS}`),
    ).toBeDefined();
    const callsBefore = mockFiles.mock.calls.length;
    await loadCommitStats("/repo-cap", first);
    expect(mockFiles).toHaveBeenCalledTimes(callsBefore + 1);
  });

  it("does not evict on a failed lookup", async () => {
    mockFiles.mockResolvedValue([file(1, 1)]);
    await loadCommitStats("/repo-cap-fail", "keep-me");
    const callsBefore = mockFiles.mock.calls.length;

    mockFiles.mockRejectedValue(new Error("git exploded"));
    expect(await loadCommitStats("/repo-cap-fail", "boom")).toBeNull();
    // The failure is not cached, so it costs one call and no cache slot.
    expect(mockFiles).toHaveBeenCalledTimes(callsBefore + 1);

    // The good entry is still there and still cached: no extra Git call.
    expect(peekCommitStats("/repo-cap-fail", "keep-me")).toBeDefined();
    await loadCommitStats("/repo-cap-fail", "keep-me");
    expect(mockFiles).toHaveBeenCalledTimes(callsBefore + 1);
  });
});
