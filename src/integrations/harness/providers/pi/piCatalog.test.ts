import { afterEach, beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  killChild: vi.fn(async (_id: string) => {}),
  resolveBinary: vi.fn(async () => ({ path: "/fake/pi" })),
  setHarnessModels: vi.fn(),
  spawnChild: vi.fn(async () => {}),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  writeChild: vi.fn(async (_id: string, _line: string) => {}),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: vi.fn(async () => "/home/test"),
}));
vi.mock("../../../../features/sessions/model/models", () => ({
  setHarnessModels: mocks.setHarnessModels,
}));
vi.mock("../../core/child", () => ({
  killChild: mocks.killChild,
  resolveOmpBinary: mocks.resolveBinary,
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: mocks.unwatchChild,
  watchChild: mocks.watchChild,
  writeChild: mocks.writeChild,
}));
import {
  discoverOmpModels,
  discoverPiModels,
  refreshPiCatalog,
} from "./piCatalog";

const builtin = { provider: "meta", id: "shared-id", name: "Built-in" };
const extension = {
  provider: "antigravity",
  id: "shared-id",
  name: "Extension",
  contextWindow: 200_000,
};

// Keep the real PiRpc multiplexer: the fixture answers the same JSONL requests
// as a child process, including the request id and command fields.
function reply(probeId: string, line: string, models: unknown[]) {
  const request = JSON.parse(line);
  const onLine = mocks.watchChild.mock.calls.find(([id]) => id === probeId)![1];
  onLine(
    JSON.stringify({
      type: "response",
      id: request.id,
      command: request.type,
      success: true,
      data: { models },
    }),
  );
}

function expectStopped() {
  const probeId = mocks.watchChild.mock.calls[0][0];
  expect(mocks.unwatchChild).toHaveBeenCalledExactlyOnceWith(probeId);
  expect(mocks.killChild).toHaveBeenCalledExactlyOnceWith(probeId);
  expect(vi.getTimerCount()).toBe(0);
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(0);
  vi.clearAllMocks();
  mocks.resolveBinary.mockResolvedValue({ path: "/fake/pi" });
  mocks.spawnChild.mockResolvedValue();
  mocks.killChild.mockResolvedValue();
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, [builtin]);
  });
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

it("loads Pi extensions when discovering package-provided models", async () => {
  const result = discoverPiModels("/workspace");
  await vi.runAllTimersAsync();
  await result;
  expect(mocks.spawnChild).toHaveBeenCalledWith(
    expect.any(String),
    "/fake/pi",
    ["--mode", "rpc", "--no-session"],
    "/workspace",
    undefined,
    "pi",
  );
  expect(JSON.parse(mocks.writeChild.mock.calls[0][1]).type).toBe(
    "get_available_models",
  );
  expectStopped();
});

it("includes models registered three seconds after RPC starts answering", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, Date.now() < 3_000 ? [builtin] : [builtin, extension]);
  });
  let settled = false;
  const result = discoverPiModels("/workspace").then((models) => {
    settled = true;
    return models;
  });
  await vi.advanceTimersByTimeAsync(2_999);
  expect(settled).toBe(false);
  await vi.runAllTimersAsync();
  expect((await result).map((model) => model.nativeId)).toEqual([
    "meta/shared-id",
    "antigravity/shared-id",
  ]);
  expectStopped();
});

it("preserves extension isolation and immediate discovery for omp", async () => {
  const models = await discoverOmpModels("/workspace");
  expect(models.map((model) => model.id)).toEqual(["omp:meta/shared-id"]);
  expect(mocks.spawnChild).toHaveBeenCalledWith(
    expect.any(String),
    "/fake/pi",
    expect.arrayContaining(["--no-extensions"]),
    "/workspace",
    undefined,
    "omp",
  );
  expect(mocks.writeChild).toHaveBeenCalledTimes(1);
  expect(Date.now()).toBe(0);
  expectStopped();
});

it("keeps polling unchanged early snapshots through the startup window", async () => {
  let settled = false;
  const result = discoverPiModels("/workspace").then((models) => {
    settled = true;
    return models;
  });
  await vi.advanceTimersByTimeAsync(4_999);
  expect(settled).toBe(false);
  expect(mocks.writeChild).toHaveBeenCalledTimes(10);
  await vi.advanceTimersByTimeAsync(1);
  expect((await result).map((model) => model.nativeId)).toEqual([
    "meta/shared-id",
  ]);
  expect(mocks.writeChild).toHaveBeenCalledTimes(11);
  expectStopped();
});

it("waits for a quiet period after a late catalog change", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, Date.now() < 4_500 ? [builtin] : [builtin, extension]);
  });
  let settled = false;
  const result = discoverPiModels("/workspace").then((models) => {
    settled = true;
    return models;
  });
  await vi.advanceTimersByTimeAsync(5_000);
  expect(settled).toBe(false);
  await vi.advanceTimersByTimeAsync(500);
  expect(await result).toHaveLength(2);
  expect(Date.now()).toBe(5_500);
  expectStopped();
});

it("merges snapshots by provider/model id and keeps the latest metadata", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(
      id,
      line,
      Date.now() < 500
        ? [builtin, extension]
        : [{ ...extension, contextWindow: 1_000_000 }],
    );
  });
  const result = discoverPiModels("/workspace");
  await vi.runAllTimersAsync();
  const models = await result;
  expect(models.map((model) => model.nativeId)).toEqual([
    "meta/shared-id",
    "antigravity/shared-id",
  ]);
  expect(models[1].contextWindow).toBe(1_000_000);
  expectStopped();
});

it("discovers extension-only catalogs after initially empty responses", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, Date.now() < 3_000 ? [] : [extension]);
  });
  const result = discoverPiModels("/workspace");
  await vi.runAllTimersAsync();
  expect((await result).map((model) => model.nativeId)).toEqual([
    "antigravity/shared-id",
  ]);
  expectStopped();
});

it("finishes an empty catalog without replacing existing models", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => reply(id, line, []));
  const result = refreshPiCatalog();
  await vi.runAllTimersAsync();
  await result;
  expect(Date.now()).toBe(5_000);
  expect(mocks.setHarnessModels).not.toHaveBeenCalled();
  expectStopped();
});

it("deduplicates concurrent refreshes and publishes the settled catalog", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, Date.now() < 3_000 ? [builtin] : [builtin, extension]);
  });
  const first = refreshPiCatalog();
  const second = refreshPiCatalog();
  expect(first).toBe(second);
  await vi.runAllTimersAsync();
  await first;
  expect(mocks.spawnChild).toHaveBeenCalledTimes(1);
  expect(mocks.setHarnessModels).toHaveBeenCalledExactlyOnceWith(
    "pi",
    expect.arrayContaining([
      expect.objectContaining({ nativeId: "antigravity/shared-id" }),
    ]),
  );
  expectStopped();
});

it("times out and cleans up a probe that never answers", async () => {
  mocks.writeChild.mockImplementation(async () => {});
  const result = expect(discoverPiModels("/workspace")).rejects.toThrow(
    "Pi model discovery timed out",
  );
  await vi.advanceTimersByTimeAsync(45_000);
  await result;
  expect(mocks.writeChild).toHaveBeenCalledTimes(1);
  expectStopped();
});

it("returns collected models at the deadline if a later request stalls", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    if (Date.now() === 0) reply(id, line, [builtin]);
  });
  const result = discoverPiModels("/workspace");
  await vi.advanceTimersByTimeAsync(45_000);
  expect((await result).map((model) => model.nativeId)).toEqual([
    "meta/shared-id",
  ]);
  expect(mocks.writeChild).toHaveBeenCalledTimes(2);
  expectStopped();
  // A reply arriving after cleanup cannot restart discovery or leave RPC timers.
  const [probeId, line] = mocks.writeChild.mock.calls[1];
  reply(probeId, line, [builtin, extension]);
  await vi.advanceTimersByTimeAsync(5_000);
  expect(mocks.writeChild).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(0);
});

it("bounds discovery even when the catalog changes on every response", async () => {
  mocks.writeChild.mockImplementation(async (id, line) => {
    reply(id, line, [{ ...extension, contextWindow: 200_000 + Date.now() }]);
  });
  const result = discoverPiModels("/workspace");
  await vi.runAllTimersAsync();
  expect(await result).toHaveLength(1);
  expect(Date.now()).toBe(45_000);
  expect(mocks.writeChild).toHaveBeenCalledTimes(90);
  expectStopped();
});

it("cancels polling immediately when the process exits between requests", async () => {
  const result = expect(discoverPiModels("/workspace")).rejects.toThrow(
    "Pi catalog probe exited",
  );
  await vi.advanceTimersByTimeAsync(250);
  mocks.watchChild.mock.calls[0][2]();
  await result;
  expect(Date.now()).toBe(250);
  expectStopped();
  await vi.advanceTimersByTimeAsync(45_000);
  expect(mocks.writeChild).toHaveBeenCalledTimes(1);
});

it("cancels an outstanding request when the process exits", async () => {
  mocks.writeChild.mockImplementation(async () => {});
  const result = expect(discoverPiModels("/workspace")).rejects.toThrow(
    "Pi catalog probe exited",
  );
  await vi.advanceTimersByTimeAsync(250);
  mocks.watchChild.mock.calls[0][2]();
  await result;
  expectStopped();
});

it("handles process exit before spawn resolves", async () => {
  mocks.spawnChild.mockImplementationOnce(async () => {
    mocks.watchChild.mock.calls[0][2]();
  });
  await expect(discoverPiModels("/workspace")).rejects.toThrow(
    "Pi catalog probe exited",
  );
  expect(mocks.writeChild).not.toHaveBeenCalled();
  expectStopped();
});

it("cleans up when spawning or writing to the process fails", async () => {
  mocks.spawnChild.mockRejectedValueOnce(new Error("spawn failed"));
  await expect(discoverPiModels("/workspace")).rejects.toThrow("spawn failed");
  expectStopped();
  vi.clearAllMocks();
  mocks.writeChild.mockRejectedValueOnce(new Error("write failed"));
  await expect(discoverPiModels("/workspace")).rejects.toThrow("write failed");
  expectStopped();
});
