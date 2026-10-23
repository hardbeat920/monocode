import { beforeEach, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { sessionConversationPage } from "../../agent-app/model/sessionConversation";
import type { HostSession } from "./protocol";

const connections = vi.hoisted(() => ({
  hostSessions: {} as Record<string, string>,
  loadRemoteSession: vi.fn(),
  remoteRequest: vi.fn(),
  cachedRemoteSessionSummary: vi.fn(),
  remoteMachineFor: vi.fn(async (environmentId: string) =>
    environmentId === "env"
      ? { id: "machine", name: "Dev", endpoint: "", environmentId }
      : undefined,
  ),
}));
vi.mock("./connections", () => ({
  ...connections,
  remoteSessionFor: (shellId: string) => connections.hostSessions[shellId],
}));
vi.mock("./remoteProjects", () => ({
  remoteProjectFor: (path: string) =>
    path.startsWith("remote://env/")
      ? { key: path, environmentId: "env", projectId: "project", cwd: "/home/me/repo" }
      : undefined,
  remotePath: (environmentId: string, path: string) =>
    `remote://${environmentId}${path}`,
}));

import { readRemoteShell, remoteShellListings } from "./remoteShells";

const cwd = "remote://env/home/me/repo";
const shell = (id: string) => ({ ...newSession("claude", cwd), id });

beforeEach(() => {
  connections.hostSessions = { tab: "host-session" };
  connections.loadRemoteSession.mockReset();
  connections.remoteRequest.mockReset();
  connections.cachedRemoteSessionSummary.mockReset();
});

it("reads a background tab's transcript from its host", async () => {
  connections.loadRemoteSession.mockResolvedValue({
    projectId: "project",
    revision: 3,
    status: "idle",
    updatedAt: 1,
    session: {
      ...newSession("codex", "/home/me/repo"),
      id: "host-session",
      title: "Fix the build",
      blocks: [
        { id: "u", role: "user", text: "Fix it" },
        { id: "a", role: "assistant", text: "Fixed" },
      ],
    },
  } satisfies HostSession);
  const session = await readRemoteShell(shell("tab"));
  expect(connections.loadRemoteSession).toHaveBeenCalledWith(
    "machine",
    "host-session",
  );
  expect(session).toMatchObject({ id: "tab", cwd, title: "Fix the build" });
  expect(sessionConversationPage(session)).toMatchObject({
    sessionId: "tab",
    title: "Fix the build",
    busy: false,
    turns: [
      { turnId: "u", user: { text: "Fix it" }, assistant: { text: "Fixed" } },
    ],
  });
});

it("returns a tab without a host conversation as it is", async () => {
  const blank = shell("blank");
  expect(await readRemoteShell(blank)).toBe(blank);
  expect(connections.loadRemoteSession).not.toHaveBeenCalled();
});

it("refuses to read when the machine is not connected", async () => {
  connections.remoteMachineFor.mockResolvedValueOnce(undefined);
  await expect(readRemoteShell(shell("tab"))).rejects.toThrow(
    "The machine for this remote project isn't connected on this computer",
  );
  expect(connections.loadRemoteSession).not.toHaveBeenCalled();
});

it("lists remote tabs with the host's titles and run state", async () => {
  connections.hostSessions = { tab: "host-session" };
  connections.remoteRequest.mockResolvedValue([
    {
      id: "host-session",
      title: "Fix the build",
      harness: "codex",
      model: "codex:gpt",
      status: "running",
      draft: false,
      projectId: "project",
      revision: 1,
      updatedAt: 1,
    },
  ]);
  expect(
    await remoteShellListings(cwd, [shell("tab"), shell("blank")]),
  ).toEqual([
    {
      id: "tab",
      title: "Fix the build",
      harness: "codex",
      model: "codex:gpt",
      busy: true,
      hasDraft: false,
    },
    expect.objectContaining({ id: "blank", busy: false }),
  ]);
  expect(connections.remoteRequest).toHaveBeenCalledWith(
    "machine",
    "sessions.list",
    { projectId: "project" },
  );
});

it("falls back to the last host list while the machine is unreachable", async () => {
  connections.remoteRequest.mockRejectedValue(new Error("SSH dropped"));
  connections.cachedRemoteSessionSummary.mockReturnValue({
    id: "host-session",
    title: "Cached title",
    harness: "codex",
    status: "idle",
  });
  expect(await remoteShellListings(cwd, [shell("tab")])).toMatchObject([
    { id: "tab", title: "Cached title", busy: false },
  ]);
  expect(connections.cachedRemoteSessionSummary).toHaveBeenCalledWith(
    cwd,
    "host-session",
  );
});

it("keeps the tab's own draft when the host has no summary for it", async () => {
  connections.remoteRequest.mockRejectedValue(new Error("SSH dropped"));
  connections.cachedRemoteSessionSummary.mockReturnValue(undefined);
  const drafted = {
    ...shell("tab"),
    blocks: [{ id: "d", role: "user" as const, text: "Later", draft: true }],
  };
  expect(await remoteShellListings(cwd, [drafted])).toMatchObject([
    { id: "tab", hasDraft: true },
  ]);
});
