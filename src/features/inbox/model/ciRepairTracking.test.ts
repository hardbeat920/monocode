// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { buildCiRepairRequest } from "./ciRepair";

const request = buildCiRepairRequest({
  repo: "acme/web",
  number: 42,
  headOid: "old-sha",
  evidence: [
    {
      name: "tests",
      workflow: "CI",
      state: "fail",
      url: "https://github.com/acme/web/actions/runs/1/job/2",
      startedAt: null,
      completedAt: null,
    },
  ],
});

beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});

it("tracks a submitted repair until its own agent turn finishes", async () => {
  const { trackCiRepair, getCiRepairs } = await import("./ciRepairTracking");
  let finish!: (outcome: "completed") => void;
  trackCiRepair("/web", request, "chat1", (settle) => {
    finish = settle;
    return true;
  });
  expect(getCiRepairs()).toEqual([
    expect.objectContaining({
      repo: "acme/web",
      number: 42,
      headOid: "old-sha",
      cwd: "/web",
      sessionId: "chat1",
      phase: "running",
    }),
  ]);
  finish("completed");
  expect(getCiRepairs()[0].phase).toBe("completed");
});

it("does not retain a repair that the chat could not start", async () => {
  const { trackCiRepair, getCiRepairs } = await import("./ciRepairTracking");
  expect(() =>
    trackCiRepair("/web", request, "busy-chat", () => false),
  ).toThrow("Could not start this fix");
  expect(getCiRepairs()).toEqual([]);
});

it("keeps completed repairs after reopening and marks unfinished work as interrupted", async () => {
  const store = await import("./ciRepairTracking");
  store.trackCiRepair("/web", request, "finished-chat", (settle) => {
    settle("completed");
    return true;
  });
  store.trackCiRepair("/web", request, "unfinished-chat", () => true);
  vi.resetModules();
  const restored = await import("./ciRepairTracking");
  expect(
    restored
      .getCiRepairs()
      .map(({ sessionId, phase }) => ({ sessionId, phase })),
  ).toEqual([
    { sessionId: "unfinished-chat", phase: "interrupted" },
    { sessionId: "finished-chat", phase: "completed" },
  ]);
});
