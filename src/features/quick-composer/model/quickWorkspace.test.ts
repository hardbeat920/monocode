import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { newSession } from "../../sessions/model/session";
import type { Worktree } from "../../source-control/model/worktrees";
import { parseQuickLaunch } from "./quickComposer";
import {
  applyQuickWorkspace,
  quickWorkspaceLaunch,
  workspaceForProject,
} from "./quickWorkspace";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const tree: Worktree = {
  path: "/tmp/project-feature",
  branch: "feature",
  head: "abc",
  isMain: false,
  locked: false,
  missing: false,
  prunable: false,
  dirty: false,
  unpushed: 0,
  sessionIds: [],
};
const base = {
  prompt: "fix it",
  cwd: "/tmp/project",
  harness: "codex" as const,
  reveal: false,
};
beforeEach(() => vi.mocked(invoke).mockReset());

it("defers new worktree creation and carries the chosen base through launch parsing into the session", async () => {
  const fields = await quickWorkspaceLaunch({
    cwd: base.cwd,
    mode: "worktree",
    base: "origin/develop",
  });
  expect(invoke).not.toHaveBeenCalled();
  const launch = parseQuickLaunch({ ...base, ...fields })!;
  const session = applyQuickWorkspace(newSession("codex", base.cwd), launch);
  expect(session).toMatchObject({
    workspaceMode: "worktree",
    worktreeBase: "origin/develop",
    cwd: base.cwd,
  });
  expect(session.worktreeCwd).toBeUndefined();
});

it("revalidates an existing worktree and starts in it while keeping the project identity", async () => {
  vi.mocked(invoke).mockResolvedValue({ worktrees: [tree] });
  const fields = await quickWorkspaceLaunch({
    cwd: base.cwd,
    mode: "current",
    tree,
  });
  expect(invoke).toHaveBeenCalledWith("git_worktrees", { cwd: base.cwd });
  const launch = parseQuickLaunch({ ...base, ...fields })!;
  expect(
    applyQuickWorkspace(newSession("codex", base.cwd), launch),
  ).toMatchObject({ cwd: base.cwd, worktreeCwd: tree.path });
});

it("rejects worktrees that were removed after selecting them", async () => {
  vi.mocked(invoke).mockResolvedValue({
    worktrees: [{ ...tree, missing: true }],
  });
  await expect(
    quickWorkspaceLaunch({ cwd: base.cwd, mode: "current", tree }),
  ).rejects.toThrow("no longer available");
});

it("resets the base and existing worktree when the project changes", () => {
  expect(
    workspaceForProject(
      { cwd: base.cwd, mode: "worktree", base: "feature", tree },
      "/tmp/other",
    ),
  ).toEqual({ cwd: "/tmp/other", mode: "current" });
});

it("keeps ordinary launches compatible and rejects conflicting workspace fields", () => {
  expect(parseQuickLaunch(base)).toEqual(base);
  for (const fields of [
    { workspaceMode: "unknown" },
    { workspaceMode: "worktree", worktreeCwd: tree.path },
    { worktreeBase: "main" },
    { workspaceMode: "worktree", worktreeBase: "" },
    { worktreeCwd: 42 },
  ])
    expect(parseQuickLaunch({ ...base, ...fields })).toBeNull();
});
