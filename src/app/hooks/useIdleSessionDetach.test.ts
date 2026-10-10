// @vitest-environment happy-dom
import {
  act,
  createElement,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { OrchestrationRun } from "../../features/orchestration/model/orchestration";
import { prepareOrchestrationWorkerDetails } from "../../features/orchestration/model/orchestrationWorkspace";
import {
  flushSessionWrites,
  getSession,
  upsertSession,
  type SessionRecord,
} from "../../features/sessions/data/sessionStore";
import { useUnseenFinishedSessions } from "../../features/sessions/hooks/useUnseenFinishedSessions";
import { liveAgentsFromSessions } from "../../features/sessions/model/liveAgents";
import {
  newSession,
  type Session,
} from "../../features/sessions/model/session";
import {
  newTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import { useIdleSessionDetach } from "./useIdleSessionDetach";
import {
  createMono,
  saveMonoSessionId,
} from "../../features/monos/model/mono";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  forgetHarnessSession: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("../../integrations/harness/core/registry", () => ({
  forgetHarnessSession: mocks.forgetHarnessSession,
}));

let root: Root | undefined;
let container: HTMLDivElement;
let stored: Map<string, SessionRecord>;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  mocks.forgetHarnessSession.mockReset().mockResolvedValue(undefined);
  stored = new Map();
  mocks.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === "session_upsert") {
      const record = structuredClone({
        ...args.session,
        createdAt: 1,
        updatedAt: 2,
      });
      stored.set(record.id, record);
      return record;
    }
    if (command === "session_get") {
      return structuredClone(stored.get(args.sessionId) ?? null);
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  act(() => root?.unmount());
  container.remove();
  await flushSessionWrites();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function chat(id: string, patch: Partial<Session> = {}): Session {
  return {
    ...newSession("claude", "/repo"),
    id,
    blocks: [
      { id: `${id}-user`, role: "user", text: "Review the assigned files" },
      {
        id: `${id}-answer`,
        role: "assistant",
        text: "The assignment is complete.",
      },
    ],
    ...patch,
  };
}

function run(status: OrchestrationRun["status"]): OrchestrationRun {
  return {
    version: 1,
    leadId: "lead",
    cwd: "/repo",
    status,
    allowedHarnesses: ["claude"],
    maxWorkers: 1,
    cli: "monocode",
    tasks: [],
    continuations: 0,
    requests: {},
  };
}

type WorkspaceState = {
  sessions: Session[];
  tabs: WorkspaceTab[];
  orchestrationRuns: OrchestrationRun[];
  busySessionIds: ReadonlySet<string>;
  activeSessionId: string;
  liveAgentsEnabled: boolean;
};

function mountWorkspace(initial: Partial<WorkspaceState> = {}) {
  let props: WorkspaceState = {
    sessions: [
      chat("lead", { busy: true }),
      chat("worker", {
        busy: true,
        orchestrationLeadId: "lead",
        providerSessionId: "worker-thread",
        worktreeCwd: "/repo-workers/worker",
      }),
      chat("other"),
    ],
    tabs: [newTab("lead"), newTab("other")],
    orchestrationRuns: [run("active")],
    busySessionIds: new Set(["lead", "worker"]),
    activeSessionId: "other",
    liveAgentsEnabled: true,
    ...initial,
  };
  const sessionsRef = { current: props.sessions };
  const tabsRef = { current: props.tabs };
  const openingSessionIds = { current: new Set<string>() };
  const loadedSessionCache = { current: new Map<string, Session>() };
  const skipForgetSessionIds = { current: new Set<string>() };
  const persistSession = vi.fn((session: Session | undefined) => {
    if (session) void upsertSession(session);
  });
  let snapshot!: {
    sessions: Session[];
    unseenFinishedIds: Set<string>;
    setSessions: Dispatch<SetStateAction<Session[]>>;
  };

  function Workspace(state: WorkspaceState) {
    const [sessions, setSessions] = useState(state.sessions);
    sessionsRef.current = sessions;
    tabsRef.current = state.tabs;
    const unseenFinishedIds = useUnseenFinishedSessions(
      sessions,
      state.busySessionIds,
      state.activeSessionId,
    );
    useIdleSessionDetach({
      ...state,
      sessions,
      sessionsRef,
      tabsRef,
      unseenFinishedIds,
      openingSessionIds,
      loadedSessionCache,
      skipForgetSessionIds,
      persistSession,
      setSessions,
    });
    snapshot = { sessions, unseenFinishedIds, setSessions };
    return null;
  }

  act(() => root!.render(createElement(Workspace, props)));
  const update = (patch: Partial<WorkspaceState>) => {
    props = { ...props, ...patch };
    act(() => {
      if (patch.sessions) snapshot.setSessions(patch.sessions);
      root!.render(createElement(Workspace, props));
    });
  };
  return {
    snapshot: () => snapshot,
    update,
    finish: (status: OrchestrationRun["status"] = "finished") =>
      update({
        sessions: snapshot.sessions.map((session) => ({
          ...session,
          busy: false,
        })),
        busySessionIds: new Set(),
        orchestrationRuns: [run(status)],
      }),
    persistSession,
    openingSessionIds: openingSessionIds.current,
    cache: loadedSessionCache.current,
    skipForgetSessionIds: skipForgetSessionIds.current,
  };
}

async function advance(milliseconds = 250) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
  await flushSessionWrites();
}

it("keeps the resident conversation mounted independently of ordinary tabs", async () => {
  const agent = chat("resident");
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  });
  saveMonoSessionId(createMono([agent.cwd]).id, agent.id);
  const workspace = mountWorkspace({
    sessions: [agent, chat("other")],
    tabs: [newTab("other")],
    orchestrationRuns: [],
    liveAgentsEnabled: false,
    busySessionIds: new Set(),
    activeSessionId: "other",
  });
  await advance();
  expect(workspace.snapshot().sessions.map((session) => session.id)).toEqual([
    "resident",
    "other",
  ]);
  expect(workspace.persistSession).not.toHaveBeenCalled();
  expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
});

describe("orchestration worker detachment", () => {
  it("retains finished workers until their lead closes, then persists and forgets them", async () => {
    const workspace = mountWorkspace();
    await advance();
    workspace.finish();
    await advance();
    expect(
      workspace.snapshot().sessions.map((session) => session.id),
    ).toContain("worker");
    expect(workspace.persistSession).not.toHaveBeenCalled();
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();

    workspace.update({ tabs: [newTab("other")] });
    await advance(249);
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
    await advance(1);

    expect(workspace.snapshot().sessions.map((session) => session.id)).toEqual([
      "lead",
      "other",
    ]);
    expect(workspace.snapshot().unseenFinishedIds).toEqual(new Set(["lead"]));
    expect(
      liveAgentsFromSessions(
        workspace.snapshot().sessions,
        workspace.snapshot().unseenFinishedIds,
      ),
    ).toMatchObject([{ id: "lead", done: true }]);
    expect(workspace.persistSession).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ id: "worker", busy: false }),
    );
    expect(mocks.forgetHarnessSession).toHaveBeenCalledExactlyOnceWith(
      "claude",
      "worker",
    );
    expect(stored.get("worker")?.blocks.at(-1)?.text).toBe(
      "The assignment is complete.",
    );
  });

  it.each(["active", "paused"] as const)(
    "keeps idle workers of a closed lead attached while the run is %s",
    async (status) => {
      const workspace = mountWorkspace();
      workspace.finish(status);
      workspace.update({ tabs: [newTab("other")] });
      await advance();
      expect(
        workspace.snapshot().sessions.map((session) => session.id),
      ).toContain("worker");
      expect(workspace.persistSession).not.toHaveBeenCalled();
      expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();

      workspace.update({ orchestrationRuns: [run("finished")] });
      await advance();
      expect(
        workspace.snapshot().sessions.map((session) => session.id),
      ).not.toContain("worker");
      expect(mocks.forgetHarnessSession).toHaveBeenCalledExactlyOnceWith(
        "claude",
        "worker",
      );
    },
  );

  it.each(["finished", "stopped"] as const)(
    "uses the latest %s run and closed tabs when a detach timer is already pending",
    async (status) => {
      const workspace = mountWorkspace();
      await advance(100);
      workspace.finish(status);
      workspace.update({ tabs: [newTab("other")] });
      await advance(150);
      expect(
        workspace.snapshot().sessions.map((session) => session.id),
      ).not.toContain("worker");
      expect(mocks.forgetHarnessSession).toHaveBeenCalledExactlyOnceWith(
        "claude",
        "worker",
      );
    },
  );

  it.each(["busy", "opening", "transferring"] as const)(
    "does not detach a %s worker even after the run finishes",
    async (reason) => {
      const workspace = mountWorkspace({
        sessions: [
          chat("worker", {
            orchestrationLeadId: "lead",
            busy: reason === "busy",
          }),
          chat("other"),
        ],
        tabs: [newTab("other")],
        orchestrationRuns: [run("finished")],
        busySessionIds: new Set(reason === "busy" ? ["lead", "worker"] : []),
      });
      if (reason === "opening") workspace.openingSessionIds.add("worker");
      if (reason === "transferring")
        workspace.skipForgetSessionIds.add("worker");
      await advance();
      expect(
        workspace.snapshot().sessions.map((session) => session.id),
      ).toContain("worker");
      expect(workspace.persistSession).not.toHaveBeenCalled();
      expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
    },
  );

  it("keeps ordinary unseen finished chats until they are focused", async () => {
    const workspace = mountWorkspace({
      sessions: [chat("ordinary", { busy: true }), chat("other")],
      tabs: [newTab("other")],
      orchestrationRuns: [],
      busySessionIds: new Set(["ordinary"]),
    });
    workspace.finish();
    await advance();
    expect(workspace.snapshot().unseenFinishedIds).toEqual(
      new Set(["ordinary"]),
    );
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();

    workspace.update({
      tabs: [newTab("ordinary")],
      activeSessionId: "ordinary",
    });
    await advance();
    expect(workspace.snapshot().unseenFinishedIds.size).toBe(0);
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalledWith(
      "claude",
      "ordinary",
    );
    workspace.update({ tabs: [], activeSessionId: "other" });
    await advance();
    expect(
      workspace.snapshot().sessions.map((session) => session.id),
    ).not.toContain("ordinary");
    expect(mocks.forgetHarnessSession).toHaveBeenCalledWith(
      "claude",
      "ordinary",
    );
  });

  it("does not retain unseen finished chats when Live Agents is disabled", async () => {
    const workspace = mountWorkspace({
      sessions: [chat("ordinary", { busy: true }), chat("other")],
      tabs: [newTab("other")],
      orchestrationRuns: [],
      busySessionIds: new Set(["ordinary"]),
    });
    workspace.finish();
    await advance();
    workspace.update({ liveAgentsEnabled: false });
    await advance();
    expect(workspace.snapshot().sessions.map((session) => session.id)).toEqual([
      "other",
    ]);
    expect(mocks.forgetHarnessSession).toHaveBeenCalledExactlyOnceWith(
      "claude",
      "ordinary",
    );
  });

  it("keeps inbox discussions mounted without marking them unseen", async () => {
    const workspace = mountWorkspace({
      sessions: [chat("inbox", { inboxAsk: true, busy: true }), chat("other")],
      tabs: [newTab("other")],
      orchestrationRuns: [],
      busySessionIds: new Set(["inbox"]),
    });
    workspace.finish();
    await advance();
    expect(workspace.snapshot().unseenFinishedIds.size).toBe(0);
    expect(
      workspace.snapshot().sessions.map((session) => session.id),
    ).toContain("inbox");
    expect(workspace.persistSession).not.toHaveBeenCalled();
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
  });

  it("reloads a detached worker's transcript from storage after the cache is evicted", async () => {
    const workspace = mountWorkspace();
    workspace.finish();
    workspace.update({ activeSessionId: "lead" });
    const finishedWorker = workspace
      .snapshot()
      .sessions.find((session) => session.id === "worker")!;
    workspace.update({ tabs: [newTab("other")], activeSessionId: "other" });
    await advance();
    expect(workspace.snapshot().sessions.map((session) => session.id)).toEqual([
      "other",
    ]);
    expect(mocks.forgetHarnessSession).toHaveBeenCalledWith("claude", "worker");
    workspace.cache.clear();

    const details = await prepareOrchestrationWorkerDetails(
      [{ leadId: "lead", sessionId: "worker" }],
      {
        openLead: async (id) => {
          const lead = await getSession(id);
          expect(lead).not.toBeNull();
          workspace.update({
            sessions: [...workspace.snapshot().sessions, lead!],
            tabs: [newTab("other"), newTab(id)],
            activeSessionId: id,
          });
        },
        openWorker: async (id) => {
          const worker = await getSession(id);
          if (worker)
            workspace.update({
              sessions: [...workspace.snapshot().sessions, worker],
            });
          return worker;
        },
        hasSession: (id) =>
          workspace.snapshot().sessions.some((session) => session.id === id),
      },
    );
    expect(details).toEqual({
      leadId: "lead",
      workers: [{ leadId: "lead", sessionId: "worker" }],
    });
    expect(mocks.invoke).toHaveBeenCalledWith("session_get", {
      sessionId: "worker",
    });
    expect(
      workspace.snapshot().sessions.find((session) => session.id === "worker"),
    ).toMatchObject({
      busy: false,
      orchestrationLeadId: "lead",
      providerSessionId: finishedWorker.providerSessionId,
      worktreeCwd: finishedWorker.worktreeCwd,
      blocks: finishedWorker.blocks,
    });
    mocks.forgetHarnessSession.mockClear();
    await advance();
    expect(
      workspace.snapshot().sessions.map((session) => session.id),
    ).toContain("worker");
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
  });

  it("cancels pending detachment when the workspace unmounts", async () => {
    const workspace = mountWorkspace();
    workspace.finish();
    workspace.update({ tabs: [newTab("other")] });
    act(() => root!.unmount());
    root = undefined;
    await advance();
    expect(workspace.persistSession).not.toHaveBeenCalled();
    expect(mocks.forgetHarnessSession).not.toHaveBeenCalled();
  });
});


it("detaches and reopens a copy-on-write owner with its execution directory intact", async () => {
  const owner = chat("copy-owner", { cowId: "copy-a", worktreeCwd: "/repo-copies/copy-a", providerSessionId: "copy-thread" });
  const workspace = mountWorkspace({ sessions: [owner, chat("other")], tabs: [newTab("other")], orchestrationRuns: [], busySessionIds: new Set(), liveAgentsEnabled: false });
  await advance();
  expect(workspace.snapshot().sessions.map((session) => session.id)).toEqual(["other"]);
  expect(workspace.cache.get(owner.id)).toMatchObject({ cowId: owner.cowId, worktreeCwd: owner.worktreeCwd });
  workspace.cache.clear();
  const reopened = await getSession(owner.id);
  expect(reopened).toMatchObject({ id: owner.id, cowId: owner.cowId, worktreeCwd: owner.worktreeCwd, providerSessionId: owner.providerSessionId, blocks: owner.blocks });
  workspace.update({ sessions: [...workspace.snapshot().sessions, reopened!], tabs: [newTab("other"), newTab(owner.id)], activeSessionId: owner.id });
  await advance();
  expect(workspace.snapshot().sessions.find((session) => session.id === owner.id)?.worktreeCwd).toBe(owner.worktreeCwd);
  expect(mocks.invoke.mock.calls.some(([command]) => command === "cow_remove")).toBe(false);
});
