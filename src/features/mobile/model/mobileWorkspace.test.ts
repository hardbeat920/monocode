// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listCowWorkspaces } from "../../source-control/model/cow";
import { listWorktrees } from "../../source-control/model/worktrees";
import {
  mobileProjectChanges,
  mobileProjectFiles,
  mobileProjectId,
  mobileWorkspaceChoice,
} from "./mobileWorkspace";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../source-control/model/cow", () => ({
  listCowWorkspaces: vi.fn(),
}));
vi.mock("../../source-control/model/worktrees", () => ({
  listWorktrees: vi.fn(),
}));
vi.mock("../../projects/model/recents", async (original) => ({
  ...(await original<typeof import("../../projects/model/recents")>()),
  loadRecents: () => [{ path: "/repo" }],
  loadPinnedProjects: () => [],
}));
vi.mock("../../monos/model/mono", async (original) => ({
  ...(await original<typeof import("../../monos/model/mono")>()),
  listMonos: () => [],
}));

const copy = {
  id: "copy",
  path: "/copies/owned",
  sourceCwd: "/repo",
  projectCwd: "/repo",
  sessionId: "owner",
  head: "abc",
};
const project = mobileProjectId("/repo");
beforeEach(() => {
  vi.mocked(listWorktrees)
    .mockReset()
    .mockResolvedValue({ worktrees: [], defaultRoot: "/trees" });
  vi.mocked(listCowWorkspaces).mockReset().mockResolvedValue([copy]);
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command) =>
      command === "git_diff_files" ? { files: [] } : [],
    );
});

it("opens files and Changes for a registered copy in the named project", async () => {
  await mobileProjectFiles(project, "src", copy.path);
  expect(invoke).toHaveBeenCalledWith("mobile_list_dir", {
    root: copy.path,
    relative: "src",
  });
  await mobileProjectChanges(project, copy.path);
  expect(invoke).toHaveBeenCalledWith("git_diff_files", { cwd: copy.path });
});

it.each(["/arbitrary", "/copies/owned/child"])(
  "rejects unregistered path %s",
  async (path) => {
    await expect(mobileProjectFiles(project, "", path)).rejects.toThrow(
      "no longer available",
    );
    expect(invoke).not.toHaveBeenCalled();
  },
);

it("rejects a copy belonging to another project", async () => {
  vi.mocked(listCowWorkspaces).mockResolvedValue([
    { ...copy, projectCwd: "/other" },
  ]);
  await expect(mobileProjectFiles(project, "", copy.path)).rejects.toThrow(
    "no longer available",
  );
  expect(invoke).not.toHaveBeenCalled();
});

it("never authorizes file access after a failed CoW lookup", async () => {
  vi.mocked(listCowWorkspaces).mockRejectedValue(
    new Error("Ownership unavailable"),
  );
  await expect(mobileProjectFiles(project, "", copy.path)).rejects.toThrow();
  expect(invoke).not.toHaveBeenCalled();
});

it("does not let a new session attach to another session's copy", async () => {
  await expect(
    mobileWorkspaceChoice("/repo", { worktreePath: copy.path }),
  ).rejects.toThrow("no longer available");
});

it("explicit current checkout clears pending isolation defaults", async () => {
  const choice = await mobileWorkspaceChoice("/repo", {
    workspaceMode: "current",
  });
  expect({
    workspaceMode: "cow",
    worktreeBase: "HEAD",
    cowSourceCwd: "/donor",
    ...choice,
  }).toMatchObject({
    workspaceMode: undefined,
    worktreeBase: undefined,
    cowSourceCwd: undefined,
  });
});

it("explicit existing worktree clears pending isolation defaults", async () => {
  vi.mocked(listWorktrees).mockResolvedValue({
    defaultRoot: "/trees",
    worktrees: [
      {
        path: "/tree",
        branch: "feature",
        head: "abc",
        isMain: false,
        locked: false,
        prunable: false,
        missing: false,
        dirty: false,
        unpushed: 0,
        sessionIds: [],
      },
    ],
  });
  const choice = await mobileWorkspaceChoice("/repo", {
    worktreePath: "/tree",
  });
  expect({
    workspaceMode: "cow",
    worktreeBase: "HEAD",
    cowSourceCwd: "/donor",
    ...choice,
  }).toMatchObject({
    workspaceMode: undefined,
    worktreeBase: undefined,
    cowSourceCwd: undefined,
    worktreeCwd: "/tree",
  });
});
