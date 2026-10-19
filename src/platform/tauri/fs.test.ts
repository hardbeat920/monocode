import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  gitCommit,
  gitDiffIndex,
  gitDiffStats,
  gitDiffFiles,
  notifyGitChanged,
  type GitDiffIndex,
  gitHeadMessage,
  isCheckoutBlockedByChanges,
  listSkills,
  pickFolders,
  resolveProjectLocation,
} from "./fs";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("pickFolders", () => {
  it("returns every folder chosen in one pass", async () => {
    vi.mocked(open).mockResolvedValueOnce(["/a/one", "/a/two"]);
    await expect(pickFolders()).resolves.toEqual(["/a/one", "/a/two"]);
    expect(vi.mocked(open).mock.calls[0]?.[0]).toMatchObject({
      directory: true,
      multiple: true,
    });
  });

  it("handles the dialog returning a bare string for a single folder", async () => {
    vi.mocked(open).mockResolvedValueOnce("/a/only");
    await expect(pickFolders()).resolves.toEqual(["/a/only"]);
  });

  it("returns nothing when the dialog is dismissed", async () => {
    vi.mocked(open).mockResolvedValueOnce(null);
    await expect(pickFolders()).resolves.toEqual([]);
  });
});

describe("isCheckoutBlockedByChanges", () => {
  it("detects git's tracked-file checkout error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt\nPlease commit your changes or stash them before you switch branches.",
      ),
    ).toBe(true);
  });

  it("detects git's untracked-file checkout error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "error: The following untracked working tree files would be overwritten by checkout:\n\tnew.txt\nPlease move or remove them before you switch branches.",
      ),
    ).toBe(true);
  });

  it("detects the mapped app error", () => {
    expect(
      isCheckoutBlockedByChanges(
        "Your local changes would be overwritten. Commit or stash them first.",
      ),
    ).toBe(true);
  });

  it("ignores unrelated git errors", () => {
    expect(isCheckoutBlockedByChanges("Branch missing not found")).toBe(false);
    expect(isCheckoutBlockedByChanges("Not a git repository")).toBe(false);
  });
});

describe("listSkills", () => {
  it("invokes list_skills with cwd and disabledPaths", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await listSkills("/repo", ["/repo/.agents/skills/review/SKILL.md"]);
    expect(invoke).toHaveBeenCalledWith("list_skills", {
      cwd: "/repo",
      disabledPaths: ["/repo/.agents/skills/review/SKILL.md"],
    });
  });

  it("passes null when disabledPaths is omitted", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await listSkills("/repo");
    expect(invoke).toHaveBeenCalledWith("list_skills", {
      cwd: "/repo",
      disabledPaths: null,
    });
  });
});

describe("resolveProjectLocation", () => {
  it("passes the saved filesystem identity to the backend", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      path: "/repo-renamed",
      identity: "unix:1:2",
    });

    await expect(resolveProjectLocation("/repo", "unix:1:2")).resolves.toEqual({
      path: "/repo-renamed",
      identity: "unix:1:2",
    });
    expect(invoke).toHaveBeenCalledWith("resolve_project_location", {
      path: "/repo",
      identity: "unix:1:2",
    });
  });

  it("uses null until the project has a saved identity", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await resolveProjectLocation("/repo");
    expect(invoke).toHaveBeenCalledWith("resolve_project_location", {
      path: "/repo",
      identity: null,
    });
  });
});

describe("gitCommit", () => {
  it("invokes git_commit without amend by default", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await gitCommit("/repo", "Add feature");
    expect(invoke).toHaveBeenCalledWith("git_commit", {
      cwd: "/repo",
      message: "Add feature",
      amend: false,
    });
  });

  it("passes amend when requested", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await gitCommit("/repo", "Fix feature", true);
    expect(invoke).toHaveBeenCalledWith("git_commit", {
      cwd: "/repo",
      message: "Fix feature",
      amend: true,
    });
  });
});

describe("gitHeadMessage", () => {
  it("invokes git_head_message with cwd", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("Subject\n\nBody");
    await expect(gitHeadMessage("/repo")).resolves.toBe("Subject\n\nBody");
    expect(invoke).toHaveBeenCalledWith("git_head_message", { cwd: "/repo" });
  });
});

describe("concurrent Git index reads", () => {
  it("shares one request per checkout and releases it after completion", async () => {
    vi.mocked(invoke).mockReset();
    let finish!: (value: GitDiffIndex) => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const first = gitDiffIndex("/coalesced");
    const second = gitDiffIndex("/coalesced");
    expect(second).toBe(first);
    expect(invoke).toHaveBeenCalledTimes(1);
    const result = { files: [] } as unknown as GitDiffIndex;
    finish(result);
    await expect(second).resolves.toBe(result);
    vi.mocked(invoke).mockResolvedValueOnce(result);
    expect(gitDiffIndex("/coalesced")).not.toBe(first);
    await gitDiffIndex("/coalesced");
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("does not mix worktrees and allows retries after failure", async () => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Disk unavailable"));
    await expect(gitDiffIndex("/failed-index")).rejects.toThrow(
      "Disk unavailable",
    );
    vi.mocked(invoke).mockResolvedValue({ files: [] });
    const a = gitDiffIndex("/failed-index");
    const b = gitDiffIndex("/another-index");
    expect(a).not.toBe(b);
    await Promise.all([a, b]);
    expect(invoke).toHaveBeenCalledTimes(3);
  });
});

it("refreshes pre-mutation indexes for both original and new readers", async () => {
  vi.mocked(invoke).mockReset();
  vi.stubGlobal("window", { dispatchEvent: vi.fn() });
  try {
    let finish!: (index: unknown) => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const old = gitDiffIndex("/mutating");
    notifyGitChanged();
    vi.mocked(invoke).mockResolvedValue({ files: ["new"] });
    const fresh = gitDiffIndex("/mutating");
    expect(fresh).not.toBe(old);
    finish({ files: ["old"] });
    await expect(fresh).resolves.toEqual({ files: ["new"] });
    await expect(old).resolves.toEqual({ files: ["new"] });
  } finally {
    vi.unstubAllGlobals();
  }
});

it("coalesces stats and file reads separately without mixing checkouts", async () => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue({ files: [] });
  const stats = gitDiffStats("/stats-a");
  const files = gitDiffFiles("/stats-a");
  expect(gitDiffStats("/stats-a")).toBe(stats);
  expect(gitDiffFiles("/stats-a")).toBe(files);
  expect(stats).not.toBe(files);
  const other = gitDiffStats("/stats-b");
  expect(other).not.toBe(stats);
  await Promise.all([stats, files, other]);
  expect(invoke).toHaveBeenCalledTimes(3);
});

it("does not publish a stale failure after a mutation", async () => {
  vi.mocked(invoke).mockReset();
  vi.stubGlobal("window", { dispatchEvent: vi.fn() });
  try {
    let fail!: (error: Error) => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          fail = reject;
        }),
    );
    const old = gitDiffStats("/failed-mutation");
    notifyGitChanged();
    vi.mocked(invoke).mockResolvedValue({
      files: 1,
      additions: 2,
      deletions: 3,
    });
    fail(new Error("old checkout unavailable"));
    await expect(old).resolves.toEqual({
      files: 1,
      additions: 2,
      deletions: 3,
    });
  } finally {
    vi.unstubAllGlobals();
  }
});
