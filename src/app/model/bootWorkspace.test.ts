import { beforeEach, expect, it, vi } from "vitest";
import { newSession } from "../../features/sessions/model/session";
import { newTab } from "../../features/workspace/model/layout";
import { collectWorkspaceSnapshot } from "../../features/workspace/model/workspaceSnapshot";

const store = vi.hoisted(() => ({
  loadWorkspaceSnapshot: vi.fn(),
  listInFlightSessions: vi.fn(),
  getSession: vi.fn(),
  upsertSession: vi.fn(),
}));
vi.mock("../../features/sessions/data/sessionStore", async (original) => ({
  ...(await original<
    typeof import("../../features/sessions/data/sessionStore")
  >()),
  ...store,
}));
vi.mock("../../integrations/harness/core/registry", () => ({}));
vi.mock("../../integrations/harness/core/child", () => ({}));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  store.listInFlightSessions.mockResolvedValue([]);
  store.upsertSession.mockResolvedValue(null);
});

function savedWorkspace() {
  const sessions = Array.from({ length: 10 }, (_, index) => ({
    ...newSession("codex", "/repo"),
    id: `session-${index}`,
    blocks: [{ id: `user-${index}`, role: "user" as const, text: "Hello" }],
  }));
  const tabs = sessions.map((session) => newTab(session.id));
  store.loadWorkspaceSnapshot.mockResolvedValue(
    collectWorkspaceSnapshot(tabs, sessions, tabs[0].id, "/repo", new Map()),
  );
  store.getSession.mockImplementation(
    async (id: string) => sessions.find((session) => session.id === id) ?? null,
  );
  return sessions;
}

it("restores ten idle tabs without rewriting their transcripts before first paint", async () => {
  const sessions = savedWorkspace();
  const { loadResumedWorkspace } = await import("./appLifecycle");
  const restored = await loadResumedWorkspace();
  expect(restored?.sessions.map((session) => session.id)).toEqual(
    sessions.map((session) => session.id),
  );
  expect(restored?.tabs).toHaveLength(10);
  expect(store.upsertSession).not.toHaveBeenCalled();
});

it("still persists the interruption marker for a recovered running turn", async () => {
  const sessions = savedWorkspace();
  store.listInFlightSessions.mockResolvedValue([
    { sessionId: sessions[0].id, cwd: "/repo" },
  ]);
  const { loadResumedWorkspace } = await import("./appLifecycle");
  const restored = await loadResumedWorkspace();
  expect(store.upsertSession).toHaveBeenCalledTimes(1);
  expect(store.upsertSession).toHaveBeenCalledWith(restored?.sessions[0]);
  expect(restored?.sessions[0].blocks.at(-1)?.notice).toBe("interrupt");
});
