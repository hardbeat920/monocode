import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubPrDiff,
  githubWorkItemDetails,
  githubWorkItemThread,
} from "./githubTasks";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mocked = vi.mocked(invoke);

describe("github work item freshness", () => {
  beforeEach(() => {
    clearInboxCache();
    mocked.mockReset();
    mocked.mockImplementation(async (command: string) =>
      command === "git_github_pr_diff"
        ? { additions: 0, deletions: 0, files: [], patch: "", truncated: false }
        : command === "git_github_work_item_thread"
          ? { comments: [], commits: [], truncated: false }
          : { body: "", author: "" },
    );
  });

  it("shares an in-flight details request", async () => {
    await Promise.all([
      githubWorkItemDetails("/repo", "o/r", "pr", 1),
      githubWorkItemDetails("/repo", "o/r", "pr", 1),
    ]);
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it("reuses recent data only when the caller allows it", async () => {
    await githubWorkItemDetails("/repo", "o/r", "pr", 1);
    await githubWorkItemThread("/repo", "o/r", "pr", 1);
    await githubPrDiff("/repo", "o/r", 1);
    expect(mocked).toHaveBeenCalledTimes(3);

    await githubWorkItemDetails("/repo", "o/r", "pr", 1, { maxAgeMs: 30_000 });
    await githubWorkItemThread("/repo", "o/r", "pr", 1, { maxAgeMs: 30_000 });
    await githubPrDiff("/repo", "o/r", 1, { maxAgeMs: 30_000 });
    expect(mocked).toHaveBeenCalledTimes(3);

    await githubWorkItemDetails("/repo", "o/r", "pr", 1);
    expect(mocked).toHaveBeenCalledTimes(4);
  });
});
