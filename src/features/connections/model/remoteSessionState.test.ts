import { expect, it } from "vitest";
import { newSession } from "../../sessions/model/session";
import { shouldPersistSession } from "../../sessions/data/sessionStore";
import type { HostSession } from "./protocol";
import { remoteSessionState, withRemoteChoice } from "./remoteSessionState";

it("keeps a host transcript in normal session state under its local tab ID", () => {
  const shell = newSession("codex", "remote://env/home/me/repo");
  const snapshot = {
    projectId: "project",
    revision: 2,
    status: "idle",
    updatedAt: 1,
    session: {
      ...shell,
      id: "host-session",
      cwd: "/home/me/repo-worktrees/dev",
      title: "Fix the build",
      blocks: [
        { id: "turn", role: "user", text: "Fix it" },
        { id: "plan", role: "plan", text: "Build the fix" },
      ],
    },
  } as HostSession;
  const session = remoteSessionState(shell, snapshot, {
    key: shell.cwd,
    environmentId: "env",
    projectId: "project",
    cwd: "/home/me/repo",
  });
  expect(session).toMatchObject({
    id: shell.id,
    cwd: shell.cwd,
    worktreeCwd: "remote://env/home/me/repo-worktrees/dev",
    title: "Fix the build",
    blocks: snapshot.session.blocks,
  });
  expect(shouldPersistSession(session)).toBe(false);
});

it("gives a remote tab's shell the host's unsent choice", () => {
  const shell = newSession("codex", "remote://env/home/me/repo");
  const other = newSession("codex", "/Users/me/repo");
  const sessions = [shell, other];
  const next = withRemoteChoice(sessions, shell.id, {
    harness: "claude",
    model: "claude:opus",
    modelSettings: { reasoningEffort: "high" },
    runtimeMode: shell.runtimeMode,
  });
  expect(next[0]).toMatchObject({
    id: shell.id,
    harness: "claude",
    model: "claude:opus",
    modelSettings: { reasoningEffort: "high" },
  });
  expect(next[1]).toBe(other);
});

it("leaves the session list untouched when the choice is unchanged", () => {
  const shell = newSession(
    "claude",
    "remote://env/home/me/repo",
    "claude:opus",
  );
  const sessions = [{ ...shell, modelSettings: { reasoningEffort: "high" } }];
  const draft = {
    harness: shell.harness,
    model: shell.model,
    modelSettings: { reasoningEffort: "high" },
    runtimeMode: shell.runtimeMode,
  };
  expect(withRemoteChoice(sessions, shell.id, draft)).toBe(sessions);
  expect(withRemoteChoice(sessions, "closed-tab", draft)).toBe(sessions);
});
