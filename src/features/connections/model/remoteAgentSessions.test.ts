// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession, type Block, type Session } from "../../sessions/model/session";
import type { HostSession, RemoteMachine } from "./protocol";
import type { RemoteProject } from "./remoteProjects";
import {
  cancelRemoteSessionWatchers,
  draftRemoteAgentSession,
  loadRemoteAgentSession,
  remoteCompletionOutcome,
  removeRemoteAgentSession,
  resolveRemoteAgentSessionTarget,
  sendRemoteAgentSession,
  stopRemoteAgentSession,
  type RemoteAgentSessionDeps,
  type RemoteAgentSessionTarget,
} from "./remoteAgentSessions";

const project: RemoteProject = {
  key: "remote://env-1/srv/repo",
  environmentId: "env-1",
  projectId: "proj-1",
  cwd: "/srv/repo",
};

const machine: RemoteMachine = {
  id: "machine-1",
  name: "Linux Box",
  endpoint: "http://127.0.0.1:4321",
  environmentId: "env-1",
};

function hostSession(overrides: {
  id?: string;
  title?: string;
  cwd?: string;
  status?: HostSession["status"];
  runId?: string;
  blocks?: Block[];
  busy?: boolean;
} = {}): HostSession {
  const base = newSession(
    "codex",
    overrides.cwd ?? project.cwd,
    "codex:gpt-5",
  );
  base.id = overrides.id ?? "host-sess-1";
  base.title = overrides.title ?? "Remote fix";
  base.busy = overrides.busy ?? overrides.status === "running";
  base.blocks = overrides.blocks ?? [
    { id: "u1", role: "user", text: "Explain the failure" },
    { id: "a1", role: "assistant", text: "Root cause is X." },
  ];
  return {
    session: base,
    projectId: project.projectId,
    revision: 3,
    runId: overrides.runId,
    status: overrides.status ?? "idle",
    updatedAt: 100,
  };
}

function makeDeps(
  overrides: Partial<RemoteAgentSessionDeps> = {},
): RemoteAgentSessionDeps {
  return {
    remoteMachineFor: vi.fn(async () => machine),
    loadRemoteSession: vi.fn(async () => hostSession()),
    remoteRequest: vi.fn(async () => ({
      commandId: "cmd-1",
      sessionId: "host-sess-1",
      revision: 4,
    })) as RemoteAgentSessionDeps["remoteRequest"],
    updateCachedRemoteProjectSessions: vi.fn(),
    refreshRemoteProjectSessions: vi.fn(),
    onRemoteSessionDeleted: vi.fn(),
    randomId: () => "rand-1",
    pollIntervalMs: 10,
    ...overrides,
  };
}

const storedValues = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  setItem: (key: string, value: string) => storedValues.set(key, value),
  removeItem: (key: string) => storedValues.delete(key),
});

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  cancelRemoteSessionWatchers("host-sess-1");
  vi.useRealTimers();
});

describe("resolveRemoteAgentSessionTarget", () => {
  it("ignores local sessions and unsent blank remote shells", () => {
    const localShell = newSession("codex", "/tmp/local");
    localShell.id = "local-1";
    expect(
      resolveRemoteAgentSessionTarget("local-1", {
        openShell: localShell,
        cwd: "/tmp/local",
      }),
    ).toBeUndefined();

    const blankRemoteShell = newSession("codex", project.key);
    blankRemoteShell.id = "shell-blank";
    expect(
      resolveRemoteAgentSessionTarget("shell-blank", {
        openShell: blankRemoteShell,
        remoteSessionFor: () => undefined,
        remoteProjectFor: () => project,
      }),
    ).toBeUndefined();

    expect(
      resolveRemoteAgentSessionTarget("other", {
        cwd: "/tmp/local",
        remoteProjectFor: () => project,
      }),
    ).toBeUndefined();
  });

  it("resolves mapped open shells by either host ID or shell ID", () => {
    const openShell = newSession("codex", project.key);
    openShell.id = "shell-1";

    for (const id of ["host-sess-1", "shell-1"]) {
      const target = resolveRemoteAgentSessionTarget(id, {
        openShell,
        remoteSessionFor: (shellId) =>
          shellId === "shell-1" ? "host-sess-1" : undefined,
        remoteProjectFor: (cwd) =>
          cwd === project.key ? project : undefined,
      });
      expect(target).toEqual({
        project,
        hostId: "host-sess-1",
        openShell,
      });
    }
  });

  it("resolves closed remote-only sessions via cwd, hint, or cached projects", () => {
    expect(
      resolveRemoteAgentSessionTarget("host-sess-1", {
        cwd: project.key,
        remoteProjectFor: (cwd) =>
          cwd === project.key ? project : undefined,
      }),
    ).toEqual({ project, hostId: "host-sess-1" });

    expect(
      resolveRemoteAgentSessionTarget("host-sess-1", {
        hintedProjectCwd: project.key,
        remoteProjectFor: (cwd) =>
          cwd === project.key ? project : undefined,
      }),
    ).toEqual({ project, hostId: "host-sess-1" });

    expect(
      resolveRemoteAgentSessionTarget("host-sess-1", {
        allRemoteProjects: () => [project],
        cachedRemoteSessionSummary: (cwd, id) =>
          cwd === project.key && id === "host-sess-1"
            ? {
                id: "host-sess-1",
                projectId: project.projectId,
                title: "Remote fix",
                harness: "codex",
                status: "idle",
                revision: 1,
                updatedAt: 1,
              }
            : undefined,
      }),
    ).toEqual({ project, hostId: "host-sess-1" });
  });
});

describe("loadRemoteAgentSession", () => {
  it("loads a closed remote session with host ID, remote project cwd, and live transcript", async () => {
    const deps = makeDeps({
      loadRemoteSession: vi.fn(async () =>
        hostSession({ cwd: "/srv/repo-wt/feat" }),
      ),
    });
    const session = await loadRemoteAgentSession(
      { project, hostId: "host-sess-1" },
      deps,
    );
    expect(session).not.toBeNull();
    expect(session?.id).toBe("host-sess-1");
    expect(session?.cwd).toBe(project.key);
    expect(session?.worktreeCwd).toBe("remote://env-1/srv/repo-wt/feat");
    expect(session?.title).toBe("Remote fix");
    expect(session?.blocks).toHaveLength(2);
  });

  it("syncs snapshot and retains host ID when an open mapped shell exists", async () => {
    const openShell: Session = {
      ...newSession("codex", project.key),
      id: "shell-1",
      title: "New remote session",
      blocks: [],
    };
    const onSnapshot = vi.fn();
    const deps = makeDeps();
    const session = await loadRemoteAgentSession(
      { project, hostId: "host-sess-1", openShell },
      deps,
      { onSnapshot },
    );
    expect(session?.id).toBe("host-sess-1");
    expect(session?.title).toBe("Remote fix");
    expect(session?.blocks).toHaveLength(2);
    expect(onSnapshot).toHaveBeenCalledWith(
      "shell-1",
      expect.objectContaining({ revision: 3 }),
    );
  });

  it("returns null when the host session does not exist and throws when offline", async () => {
    const missingDeps = makeDeps({
      loadRemoteSession: vi.fn(async () => {
        throw new Error("Host rejected request: Session not found on this machine");
      }),
    });
    await expect(
      loadRemoteAgentSession({ project, hostId: "missing" }, missingDeps),
    ).resolves.toBeNull();

    const offlineDeps = makeDeps({
      remoteMachineFor: vi.fn(async () => undefined),
    });
    await expect(
      loadRemoteAgentSession({ project, hostId: "host-sess-1" }, offlineDeps),
    ).rejects.toThrow("Connect this project's remote machine");
  });
});

describe("sendRemoteAgentSession & draftRemoteAgentSession", () => {
  const target: RemoteAgentSessionTarget = { project, hostId: "host-sess-1" };

  it("dispatches send via commands.dispatch, remembers follow-up, and settles Mono completion", async () => {
    vi.useFakeTimers();
    const initial = hostSession();
    const completed = hostSession({
      status: "idle",
      blocks: [
        ...initial.session.blocks,
        { id: "app-mono-r1", role: "user", text: "Fix it now" },
        { id: "a2", role: "assistant", text: "Patched and verified." },
      ],
    });
    const loadRemoteSession = vi
      .fn()
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce(
        hostSession({ status: "running", runId: "run-1" }),
      )
      .mockResolvedValueOnce(completed);
    const deps = makeDeps({ loadRemoteSession });
    const rememberFollowUp = vi.fn();
    const settled = vi.fn();
    const accept = vi.fn();
    const discard = vi.fn();

    const result = await sendRemoteAgentSession(
      target,
      "Fix it now",
      "app-mono-r1",
      {
        rememberFollowUp,
        createSettler: () => Object.assign(settled, { accept, discard }),
      },
      deps,
    );

    expect(result).toEqual({ alreadySubmitted: false });
    expect(deps.remoteRequest).toHaveBeenCalledWith(
      machine.id,
      "commands.dispatch",
      {
        type: "send",
        commandId: "app-mono-r1",
        sessionId: "host-sess-1",
        text: "Fix it now",
        attachments: [],
        intent: "default",
      },
    );
    expect(rememberFollowUp).toHaveBeenCalledTimes(1);
    expect(accept).toHaveBeenCalledTimes(1);
    expect(discard).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(15);
    await vi.advanceTimersByTimeAsync(15);

    expect(settled).toHaveBeenCalledWith(
      { status: "completed", text: "Patched and verified." },
      expect.objectContaining({ id: "host-sess-1", title: "Remote fix" }),
    );
  });

  it("treats duplicate send requestId idempotently and rejects conflicts/busy/drafts", async () => {
    const withSent = hostSession({
      blocks: [{ id: "app-mono-r1", role: "user", text: "Fix it now" }],
    });
    const deps = makeDeps({ loadRemoteSession: vi.fn(async () => withSent) });
    const rememberFollowUp = vi.fn();

    await expect(
      sendRemoteAgentSession(
        target,
        "Fix it now",
        "app-mono-r1",
        { rememberFollowUp },
        deps,
      ),
    ).resolves.toEqual({ alreadySubmitted: true });
    expect(deps.remoteRequest).not.toHaveBeenCalled();
    expect(rememberFollowUp).toHaveBeenCalledTimes(1);

    await expect(
      sendRemoteAgentSession(target, "Different", "app-mono-r1", {}, deps),
    ).rejects.toThrow("Request ID was already used with another prompt");

    const busyDeps = makeDeps({
      loadRemoteSession: vi.fn(async () =>
        hostSession({ status: "running", runId: "run-1" }),
      ),
    });
    await expect(
      sendRemoteAgentSession(target, "Next", "app-mono-r2", {}, busyDeps),
    ).rejects.toThrow("Session is busy");

    const draftDeps = makeDeps({
      loadRemoteSession: vi.fn(async () =>
        hostSession({
          blocks: [{ id: "app-mono-d1", role: "user", text: "Draft", draft: true }],
        }),
      ),
    });
    await expect(
      sendRemoteAgentSession(target, "Draft", "app-mono-d1", {}, draftDeps),
    ).rejects.toThrow("Request ID belongs to an unsent draft");
    await expect(
      sendRemoteAgentSession(target, "Other", "app-mono-r3", {}, draftDeps),
    ).rejects.toThrow("Session already has a draft");
  });

  it("discards completion settler if commands.dispatch rejects", async () => {
    const deps = makeDeps({
      remoteRequest: vi.fn(async () => {
        throw new Error("Host unavailable");
      }) as RemoteAgentSessionDeps["remoteRequest"],
    });
    const discard = vi.fn();
    await expect(
      sendRemoteAgentSession(
        target,
        "Run check",
        "app-mono-r4",
        { createSettler: () => Object.assign(vi.fn(), { accept: vi.fn(), discard }) },
        deps,
      ),
    ).rejects.toThrow("Host unavailable");
    expect(discard).toHaveBeenCalledTimes(1);
  });

  it("restores or settles completion monitoring on retry without re-dispatching", async () => {
    vi.useFakeTimers();
    const runningWithReq = hostSession({
      status: "running",
      runId: "run-2",
      blocks: [{ id: "u-retry", appRequestId: "req-retry", role: "user", text: "Fix it" }],
    });
    const completedWithReq = hostSession({
      status: "idle",
      blocks: [
        { id: "u-retry", appRequestId: "req-retry", role: "user", text: "Fix it" },
        { id: "a-retry", role: "assistant", text: "Done." },
      ],
    });
    const loadRemoteSession = vi
      .fn()
      .mockResolvedValueOnce(runningWithReq)
      .mockResolvedValueOnce(runningWithReq)
      .mockResolvedValueOnce(completedWithReq);
    const deps = makeDeps({ loadRemoteSession });
    const createSettler = vi.fn(() =>
      Object.assign(vi.fn(), { accept: vi.fn(), discard: vi.fn() }),
    );

    await expect(
      sendRemoteAgentSession(target, "Fix it", "req-retry", { createSettler }, deps),
    ).resolves.toEqual({ alreadySubmitted: true });
    expect(deps.remoteRequest).not.toHaveBeenCalled();
    expect(createSettler).toHaveBeenCalledTimes(1);
    const settler = createSettler.mock.results[0].value;
    expect(settler.accept).toHaveBeenCalledTimes(1);

    // Replaying while active watcher is running reuses it without a duplicate settler.
    await expect(
      sendRemoteAgentSession(target, "Fix it", "req-retry", { createSettler }, deps),
    ).resolves.toEqual({ alreadySubmitted: true });
    expect(createSettler).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(15);
    expect(settler).toHaveBeenCalledWith(
      { status: "completed", text: "Done." },
      expect.objectContaining({ id: "host-sess-1" }),
    );

    // Replaying when host already finished settles directly.
    const settledImmediate = vi.fn();
    const acceptImmediate = vi.fn();
    await expect(
      sendRemoteAgentSession(
        target,
        "Fix it",
        "req-done",
        {
          createSettler: () =>
            Object.assign(settledImmediate, {
              accept: acceptImmediate,
              discard: vi.fn(),
            }),
        },
        makeDeps({
          loadRemoteSession: vi.fn(async () =>
            hostSession({
              status: "idle",
              blocks: [
                { id: "req-done", role: "user", text: "Fix it" },
                { id: "a-done", role: "assistant", text: "Already done." },
              ],
            }),
          ),
        }),
      ),
    ).resolves.toEqual({ alreadySubmitted: true });
    expect(acceptImmediate).toHaveBeenCalledTimes(1);
    expect(settledImmediate).toHaveBeenCalledWith(
      { status: "completed", text: "Already done." },
      expect.objectContaining({ id: "host-sess-1" }),
    );
  });

  it("saves drafts remotely and handles duplicate draft requestId idempotently", async () => {
    const deps = makeDeps();
    await expect(
      draftRemoteAgentSession(target, "Prepare PR", "app-mono-d2", {}, deps),
    ).resolves.toEqual({ alreadySaved: false, draft: true });
    expect(deps.remoteRequest).toHaveBeenCalledWith(
      machine.id,
      "commands.dispatch",
      {
        type: "draft",
        commandId: "app-mono-d2",
        sessionId: "host-sess-1",
        text: "Prepare PR",
        attachments: [],
      },
    );

    const existingDraftDeps = makeDeps({
      loadRemoteSession: vi.fn(async () =>
        hostSession({
          blocks: [{ id: "app-mono-d2", role: "user", text: "Prepare PR", draft: true }],
        }),
      ),
    });
    await expect(
      draftRemoteAgentSession(
        target,
        "Prepare PR",
        "app-mono-d2",
        {},
        existingDraftDeps,
      ),
    ).resolves.toEqual({ alreadySaved: true, draft: true });
    expect(existingDraftDeps.remoteRequest).not.toHaveBeenCalled();
  });
});

describe("remoteCompletionOutcome", () => {
  it("extracts cancelled and failed outcomes from settled host blocks", () => {
    const cancelled = hostSession({
      status: "idle",
      blocks: [
        { id: "req-1", role: "user", text: "Do work" },
        { id: "a1", role: "assistant", text: "Partial" },
        { id: "s1", role: "system", text: "Stopped by you." },
      ],
    });
    expect(remoteCompletionOutcome(cancelled, "req-1")).toEqual({
      status: "cancelled",
      text: "Partial",
      error: "Stopped by you.",
    });

    const failed = hostSession({
      status: "idle",
      blocks: [
        { id: "req-2", role: "user", text: "Do work" },
        { id: "s2", role: "system", text: "Provider exited with status 1." },
      ],
    });
    expect(remoteCompletionOutcome(failed, "req-2")).toEqual({
      status: "failed",
      text: "",
      error: "Provider exited with status 1.",
    });

    const interjected = hostSession({
      status: "idle",
      blocks: [
        { id: "req-3", role: "user", text: "Do work" },
        {
          id: "s3",
          role: "system",
          text: "Advisor check-in note",
          interjection: { customType: "advisor" },
        },
        { id: "a3", role: "assistant", text: "Finished cleanly." },
      ],
    });
    expect(remoteCompletionOutcome(interjected, "req-3")).toEqual({
      status: "completed",
      text: "Finished cleanly.",
    });
  });
});

describe("stopRemoteAgentSession & removeRemoteAgentSession", () => {
  const openShell = newSession("codex", project.key);
  openShell.id = "shell-1";
  const target: RemoteAgentSessionTarget = {
    project,
    hostId: "host-sess-1",
    openShell,
  };

  it("cancels running host sessions on stop and is a no-op when idle", async () => {
    const runningDeps = makeDeps({
      loadRemoteSession: vi.fn(async () =>
        hostSession({ status: "running", runId: "run-9" }),
      ),
    });
    await stopRemoteAgentSession(target, runningDeps);
    expect(runningDeps.remoteRequest).toHaveBeenCalledWith(
      machine.id,
      "commands.dispatch",
      {
        type: "cancel",
        commandId: "rand-1",
        sessionId: "host-sess-1",
        runId: "run-9",
      },
    );

    const idleDeps = makeDeps();
    await stopRemoteAgentSession(target, idleDeps);
    expect(idleDeps.remoteRequest).not.toHaveBeenCalled();
  });

  it("archives and deletes on the host before cleaning up the mapped local tab", async () => {
    const archiveDeps = makeDeps();
    await removeRemoteAgentSession(target, "archive", archiveDeps);
    expect(archiveDeps.remoteRequest).toHaveBeenCalledWith(
      machine.id,
      "sessions.update",
      {
        projectId: project.projectId,
        sessionId: "host-sess-1",
        archived: true,
      },
    );
    expect(archiveDeps.updateCachedRemoteProjectSessions).toHaveBeenCalledTimes(1);
    expect(archiveDeps.refreshRemoteProjectSessions).toHaveBeenCalledTimes(1);
    expect(archiveDeps.onRemoteSessionDeleted).toHaveBeenCalledWith("host-sess-1");

    const deleteDeps = makeDeps();
    await removeRemoteAgentSession(target, "delete", deleteDeps);
    expect(deleteDeps.remoteRequest).toHaveBeenCalledWith(
      machine.id,
      "sessions.delete",
      {
        projectId: project.projectId,
        sessionId: "host-sess-1",
      },
    );
    expect(deleteDeps.onRemoteSessionDeleted).toHaveBeenCalledWith("host-sess-1");
  });

  it("fails instead of returning local-only success when machine is offline or host rejects delete", async () => {
    const offlineDeps = makeDeps({
      remoteMachineFor: vi.fn(async () => undefined),
    });
    await expect(
      removeRemoteAgentSession(target, "delete", offlineDeps),
    ).rejects.toThrow("Connect this project's remote machine to delete this session");
    expect(offlineDeps.onRemoteSessionDeleted).not.toHaveBeenCalled();

    const runningDeleteDeps = makeDeps({
      remoteRequest: vi.fn(async () => {
        throw new Error("Stop this session before deleting it");
      }) as RemoteAgentSessionDeps["remoteRequest"],
    });
    await expect(
      removeRemoteAgentSession(target, "delete", runningDeleteDeps),
    ).rejects.toThrow("Stop this session before deleting it");
    expect(runningDeleteDeps.onRemoteSessionDeleted).not.toHaveBeenCalled();
  });
});
