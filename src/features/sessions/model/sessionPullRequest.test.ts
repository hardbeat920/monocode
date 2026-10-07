import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "./session";
import {
  createdPullRequest,
  findSessionPullRequest,
  pushedBranches,
} from "./sessionPullRequest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

let nextId = 0;
function shell(command: string, detail = "", status = "completed"): Block {
  nextId += 1;
  return {
    id: `tool-${nextId}`,
    role: "tool",
    text: command,
    tool: { kind: "execute", title: command, detail, status },
  };
}

const PR_775 = {
  kind: "pr",
  repo: "hardbeat920/monocode",
  number: 775,
  url: "https://github.com/hardbeat920/monocode/pull/775",
};

describe("session pull requests", () => {
  it("takes the URL a gh pr create printed, even through a pipe", () => {
    const blocks = [
      shell(
        'gh pr create -R hardbeat920/monocode --draft --head SHLE1:fix/a --title "fix" 2>&1 | tail -3',
        "https://github.com/hardbeat920/monocode/pull/775",
      ),
    ];
    expect(createdPullRequest(blocks)).toEqual(PR_775);
  });

  it("ignores failed creates and URLs other commands printed", () => {
    expect(
      createdPullRequest([
        shell(
          "gh pr create --title x",
          "https://github.com/a/b/pull/1 already exists",
          "failed",
        ),
        shell("gh pr view 2", "https://github.com/a/b/pull/2"),
      ]),
    ).toBeNull();
  });

  it("reads the pushed remote and branch from git's report", () => {
    const blocks = [
      shell(
        "cd /repo && for i in 1 2 3; do git push -u fork fix/a 2>&1 | tail -3 && break; done",
        "To https://github.com/SHLE1/monocode.git\n * [new branch]      fix/a -> fix/a\nbranch 'fix/a' set up to track 'fork/fix/a'.",
      ),
    ];
    expect(pushedBranches(blocks)).toEqual([
      { branch: "fix/a", remote: "fork" },
    ]);
  });

  it("falls back to refspecs, then to the checked-out branch", () => {
    expect(
      pushedBranches([
        shell("git push origin HEAD:feat/x", "Everything up-to-date"),
      ]),
    ).toEqual([{ branch: "feat/x", remote: "origin" }]);
    expect(
      pushedBranches([shell("git push", "Everything up-to-date")]),
    ).toEqual([{}]);
  });

  it("skips deletions, dry runs, failed pushes and mentions of push", () => {
    expect(
      pushedBranches([
        shell("git push origin --delete old"),
        shell("git push --dry-run origin feat/x"),
        shell("git push origin feat/y", "rejected", "failed"),
        shell("git log --grep push"),
      ]),
    ).toEqual([]);
    expect(pushedBranches([shell("git -C /repo push origin feat/z")])).toEqual([
      { branch: "feat/z", remote: "origin" },
    ]);
    expect(pushedBranches([shell("git -C /repo log --grep push")])).toEqual([]);
  });

  it("lists the latest push first without repeats", () => {
    expect(
      pushedBranches([
        shell("git push origin a"),
        shell("git push origin b"),
        shell("git push origin a"),
      ]),
    ).toEqual([
      { branch: "a", remote: "origin" },
      { branch: "b", remote: "origin" },
    ]);
  });

  it("looks up a pushed branch's PR opened after the push", async () => {
    vi.mocked(invoke).mockResolvedValue({
      number: 775,
      title: "fix",
      url: PR_775.url,
      state: "open",
    });
    const blocks = [shell("git push -u fork fix/a")];
    await expect(
      findSessionPullRequest({ blocks }, "/repo", { dedicatedWorktree: false }),
    ).resolves.toEqual(PR_775);
    expect(invoke).toHaveBeenCalledWith("git_branch_pr", {
      cwd: "/repo",
      branch: "fix/a",
      remote: "fork",
    });
  });

  it("checks only sessions that pushed or own their worktree", async () => {
    await expect(
      findSessionPullRequest({ blocks: [] }, "/repo", {
        dedicatedWorktree: false,
      }),
    ).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();

    vi.mocked(invoke).mockResolvedValue(null);
    await findSessionPullRequest({ blocks: [] }, "/tree", {
      dedicatedWorktree: true,
    });
    expect(invoke).toHaveBeenCalledWith("git_branch_pr", {
      cwd: "/tree",
      branch: null,
      remote: null,
    });
  });

  it("never replaces an existing link", async () => {
    const blocks = [
      shell("gh pr create", "https://github.com/hardbeat920/monocode/pull/9"),
    ];
    await expect(
      findSessionPullRequest(
        { blocks, linkedWorkItem: PR_775 as never },
        "/repo",
        { dedicatedWorktree: true },
      ),
    ).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
