// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it } from "vitest";
import { setProjectDefaultProvider } from "../../features/sessions/model/projectProviders";
import { newSession } from "../../features/sessions/model/session";
import { setWorktreeFocus } from "../../features/source-control/model/worktreeFocus";
import { newWorkspaceSession } from "./newWorkspaceSession";

beforeEach(() => localStorage.clear());
afterEach(() => {
  setWorktreeFocus("/current", undefined);
  setWorktreeFocus("/other", undefined);
});

it("creates the first session in the visible project despite defaults from another project", () => {
  const defaults = newSession("claude", "/other", undefined, "auto");
  setProjectDefaultProvider("/current", "codex", "codex:gpt-5.4");
  setWorktreeFocus("/other", { path: "/other-tree", branch: "other" });

  const session = newWorkspaceSession("/current", defaults.runtimeMode);

  expect(session.cwd).toBe("/current");
  expect(session.harness).toBe("codex");
  expect(session.runtimeMode).toBe("auto");
  expect(session.worktreeCwd).toBeUndefined();
  expect(session.blocks).toEqual([]);
});

it("uses the visible project's selected worktree for its first session", () => {
  setWorktreeFocus("/current", { path: "/current-tree", branch: "feature" });

  const session = newWorkspaceSession("/current");

  expect(session.cwd).toBe("/current");
  expect(session.worktreeCwd).toBe("/current-tree");
  expect(session.branch).toBe("feature");
});

it("starts in the project root without session defaults or a separate worktree", () => {
  setWorktreeFocus("/current", { path: "/current", branch: "main" });

  const session = newWorkspaceSession("/current");

  expect(session.cwd).toBe("/current");
  expect(session.runtimeMode).toBe("supervised");
  expect(session.worktreeCwd).toBeUndefined();
});

it("creates a separate CoW session from the visible project's selected copy", () => {
  setWorktreeFocus("/current", { path: "/current-cow/copy", branch: "feature", cowId: "copy", sessionId: "owner" });

  const session = newWorkspaceSession("/current");

  expect(session.cwd).toBe("/current");
  expect(session.workspaceMode).toBe("cow");
  expect(session.cowSourceCwd).toBe("/current-cow/copy");
  expect(session.cowId).toBeUndefined();
  expect(session.worktreeCwd).toBeUndefined();
  expect(session.worktreeBase).toBeUndefined();
});
