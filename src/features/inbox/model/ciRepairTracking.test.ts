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

it("keeps an active repair linked to a project after its folder moves", async () => {
  const store = await import("./ciRepairTracking");
  let finish!: (outcome: "completed") => void;
  store.trackCiRepair("/old-project", request, "chat1", (settle) => {
    finish = settle;
    return true;
  });
  store.rebaseCiRepairs("/old-project", "/new-project");
  expect(store.getCiRepairs()[0].cwd).toBe("/new-project");
  finish("completed");
  vi.resetModules();
  const reopened = await import("./ciRepairTracking");
  expect(reopened.getCiRepairs()[0]).toEqual(
    expect.objectContaining({ cwd: "/new-project", phase: "completed" }),
  );
});

it("does not retain a repair that the chat could not start", async () => {
  const { trackCiRepair, getCiRepairs } = await import("./ciRepairTracking");
  expect(() =>
    trackCiRepair("/web", request, "busy-chat", () => false),
  ).toThrow("Could not start this fix");
  expect(getCiRepairs()).toEqual([]);
});

it("keeps completed repairs after reopening and marks unfinished work as interrupted", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(1_900_000_000_000);
  const store = await import("./ciRepairTracking");
  store.trackCiRepair("/web", request, "finished-chat", (settle) => {
    settle("completed");
    return true;
  });
  store.trackCiRepair("/web", request, "unfinished-chat", () => true);
  now.mockRestore();
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

it("does not overwrite repairs saved by another window after this window loaded", async () => {
  const first = await import("./ciRepairTracking");
  expect(first.getCiRepairs()).toEqual([]);
  vi.resetModules();
  const second = await import("./ciRepairTracking");
  second.trackCiRepair("/web", request, "other-window", (settle) => {
    settle("completed");
    return true;
  });
  first.trackCiRepair("/web", request, "this-window", (settle) => {
    settle("completed");
    return true;
  });
  vi.resetModules();
  const reopened = await import("./ciRepairTracking");
  expect(
    reopened
      .getCiRepairs()
      .map((repair) => repair.sessionId)
      .sort(),
  ).toEqual(["other-window", "this-window"]);
});

it("updates subscribers when a repair finishes in another window", async () => {
  const first = await import("./ciRepairTracking");
  let finish!: (outcome: "completed") => void;
  first.trackCiRepair("/web", request, "owner", (settle) => {
    finish = settle;
    return true;
  });
  vi.resetModules();
  const second = await import("./ciRepairTracking");
  second.getCiRepairs();
  const changed = vi.fn();
  const unsubscribe = second.subscribeCiRepairs(changed);
  try {
    finish("completed");
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: `monocode.ciRepairs.v1.${first.getCiRepairs()[0].id}`,
      }),
    );
    expect(second.getCiRepairs()[0].phase).toBe("completed");
    expect(changed).toHaveBeenCalled();
  } finally {
    unsubscribe();
  }
});

it("keeps a completed repair in memory when storage fills up", async () => {
  const store = await import("./ciRepairTracking");
  let finish!: (outcome: "completed") => void;
  store.trackCiRepair("/web", request, "completed-chat", (settle) => {
    finish = settle;
    return true;
  });
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Quota exceeded");
  });
  try {
    finish("completed");
    store.trackCiRepair("/web", request, "new-chat", () => true);
    expect(
      store
        .getCiRepairs()
        .find((repair) => repair.sessionId === "completed-chat")?.phase,
    ).toBe("completed");
  } finally {
    write.mockRestore();
  }
});
