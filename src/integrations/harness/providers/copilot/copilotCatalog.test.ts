import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  modelsFor,
  resetHarnessModelOverlays,
} from "../../../../features/sessions/model/models";

const io = vi.hoisted(() => ({
  lines: new Map<string, (line: string) => void>(),
  spawn: vi.fn(async () => undefined),
  kill: vi.fn(async () => undefined),
  authError: false,
  holdSetup: false,
  delayInitialize: 0,
  commandTiming: "before" as "before" | "after" | "none",
  emptyCommands: false,
}));
vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));
vi.mock("../../core/child", () => ({
  resolveCopilotBinary: async () => ({ path: "/fake/copilot" }),
  spawnChild: io.spawn,
  killChild: io.kill,
  unwatchChild: (id: string) => io.lines.delete(id),
  watchChild: (id: string, onLine: (line: string) => void) =>
    io.lines.set(id, onLine),
  writeChild: async (id: string, line: string) => {
    const request = JSON.parse(line);
    if (request.method === "initialize") {
      const reply = () =>
        io.lines.get(id)?.(
          JSON.stringify({ id: request.id, result: { protocolVersion: 1 } }),
        );
      if (io.delayInitialize) setTimeout(reply, io.delayInitialize);
      else reply();
    } else if (request.method === "session/new") {
      if (io.holdSetup) return;
      if (io.authError) {
        io.lines.get(id)?.(
          JSON.stringify({
            id: request.id,
            error: { message: "Authentication required" },
          }),
        );
        return;
      }
      const notifyCommands = () =>
        io.lines.get(id)?.(
          JSON.stringify({
            method: "session/update",
            params: {
              update: {
                sessionUpdate: "available_commands_update",
                availableCommands: io.emptyCommands
                  ? []
                  : [{ name: "context", description: "Show context" }],
              },
            },
          }),
        );
      if (io.commandTiming === "before") notifyCommands();
      else if (io.commandTiming === "after") setTimeout(notifyCommands, 100);
      io.lines.get(id)?.(
        JSON.stringify({
          id: request.id,
          result: {
            sessionId: "probe-session",
            models: {
              currentModelId: "auto",
              availableModels: [{ modelId: "auto", name: "Auto" }],
            },
          },
        }),
      );
    }
  },
}));
import {
  discoverCopilotCommands,
  discoverCopilotModels,
  refreshCopilotCatalog,
} from "./copilotCatalog";

beforeEach(() => {
  io.authError = false;
  io.holdSetup = false;
  io.delayInitialize = 0;
  io.commandTiming = "before";
  io.emptyCommands = false;
  io.lines.clear();
  vi.clearAllMocks();
  resetHarnessModelOverlays();
});

it("discovers models and commands with isolated probes and cleans up both children", async () => {
  const [models, commands] = await Promise.all([
    discoverCopilotModels("/repo"),
    discoverCopilotCommands("/repo"),
  ]);
  expect(models).toEqual([
    { id: "copilot:auto", harness: "copilot", nativeId: "auto", name: "Auto" },
  ]);
  expect(commands[0]).toMatchObject({
    name: "context",
    invocation: "context",
    source: "copilot",
  });
  expect(io.spawn).toHaveBeenCalledTimes(2);
  expect(io.spawn.mock.calls[0]?.[0]).not.toBe(io.spawn.mock.calls[1]?.[0]);
  expect(io.kill).toHaveBeenCalledTimes(2);
  expect(io.lines.size).toBe(0);
});

it("deduplicates refreshes and retains the last catalog when login is required", async () => {
  const first = refreshCopilotCatalog();
  expect(refreshCopilotCatalog()).toBe(first);
  await first;
  const previous = modelsFor("copilot");
  io.authError = true;
  const debug = vi.spyOn(console, "debug").mockImplementation(() => undefined);
  try {
    await refreshCopilotCatalog();
    expect(modelsFor("copilot")).toEqual(previous);
    expect(io.kill).toHaveBeenCalledTimes(2);
    expect(io.lines.size).toBe(0);
  } finally {
    debug.mockRestore();
  }
});

afterEach(() => vi.useRealTimers());

it("times out discovery, surfaces the error and kills the probe child", async () => {
  vi.useFakeTimers();
  io.delayInitialize = 15_000;
  io.holdSetup = true;
  const pending = discoverCopilotModels("/repo");
  const rejected = expect(pending).rejects.toThrow(
    "Copilot model discovery timed out",
  );
  await vi.advanceTimersByTimeAsync(30_000);
  await rejected;
  expect(io.kill).toHaveBeenCalledOnce();
  expect(io.lines.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("surfaces actionable login help when discovery is unauthenticated", async () => {
  io.authError = true;
  await expect(discoverCopilotModels("/repo")).rejects.toThrow("copilot login");
  expect(io.kill).toHaveBeenCalledOnce();
  expect(io.lines.size).toBe(0);
});

it("waits for command metadata arriving after the session response", async () => {
  vi.useFakeTimers();
  io.commandTiming = "after";
  let settled = false;
  const pending = discoverCopilotCommands("/repo").then((commands) => {
    settled = true;
    return commands;
  });
  await vi.advanceTimersByTimeAsync(99);
  expect(settled).toBe(false);
  expect(io.kill).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  await expect(pending).resolves.toEqual([
    expect.objectContaining({ name: "context", source: "copilot" }),
  ]);
  expect(io.kill).toHaveBeenCalledOnce();
  expect(io.lines.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("returns models within a bounded wait when command metadata is absent", async () => {
  vi.useFakeTimers();
  io.commandTiming = "none";
  const pending = discoverCopilotModels("/repo");
  await vi.advanceTimersByTimeAsync(0);
  expect(io.kill).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1_000);
  await expect(pending).resolves.toEqual([
    expect.objectContaining({ id: "copilot:auto" }),
  ]);
  expect(io.kill).toHaveBeenCalledOnce();
  expect(io.lines.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it("accepts an explicitly empty command update without waiting", async () => {
  vi.useFakeTimers();
  io.emptyCommands = true;
  const pending = discoverCopilotCommands("/repo");
  await vi.advanceTimersByTimeAsync(0);
  await expect(pending).resolves.toEqual([]);
  expect(io.kill).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});
