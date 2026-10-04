import { afterEach, describe, expect, it, vi } from "vitest";
import type { UnlistenFn } from "@tauri-apps/api/event";

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: never }) => void>(),
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: Error) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function installResolvedListeners() {
  mocks.listen.mockImplementation(
    async (name: string, handler: (event: { payload: never }) => void) => {
      mocks.handlers.set(name, handler);
      return vi.fn();
    },
  );
}

async function loadChild() {
  vi.resetModules();
  return import("./child");
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  mocks.handlers.clear();
  mocks.invoke.mockReset();
  mocks.listen.mockReset();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("isCurrentChildExit", () => {
  it("matches only the live child's pid", async () => {
    installResolvedListeners();
    const { isCurrentChildExit } = await loadChild();
    expect(isCurrentChildExit(undefined, 41)).toBe(false);
    expect(isCurrentChildExit(42, 41)).toBe(false);
    expect(isCurrentChildExit(42, 42)).toBe(true);
  });
});

describe("child bridge", () => {
  it("waits until every listener is installed", async () => {
    const pending = deferred<UnlistenFn>();
    mocks.listen.mockImplementation(
      (name: string, handler: (event: { payload: never }) => void) => {
        mocks.handlers.set(name, handler);
        return name === "harness-stdout" ? pending.promise : Promise.resolve(vi.fn());
      },
    );
    const child = await loadChild();
    let acquired = false;
    const lease = child.acquireHarnessBridge().then((release) => {
      acquired = true;
      return release;
    });

    await flush();
    expect(acquired).toBe(false);

    pending.resolve(vi.fn());
    const release = await lease;
    expect(acquired).toBe(true);
    release();
  });

  it("cleans a failed installation and allows retry", async () => {
    vi.useFakeTimers();
    const late = deferred<UnlistenFn>();
    const firstUnlisten = vi.fn();
    const lateUnlisten = vi.fn();
    mocks.listen
      .mockResolvedValueOnce(firstUnlisten)
      .mockRejectedValueOnce(new Error("listen failed"))
      .mockReturnValueOnce(late.promise)
      .mockResolvedValueOnce(vi.fn())
      .mockResolvedValueOnce(vi.fn());
    const child = await loadChild();
    const releaseApp = child.startHarnessBridge();

    await expect(child.acquireHarnessBridge()).rejects.toThrow("listen failed");
    expect(firstUnlisten).toHaveBeenCalledOnce();

    late.resolve(lateUnlisten);
    await flush();
    expect(lateUnlisten).toHaveBeenCalledOnce();

    installResolvedListeners();
    const releaseProbe = await child.acquireHarnessBridge();
    releaseProbe();
    releaseApp();
    await vi.runAllTimersAsync();
  });

  it("reconciles an exit that arrives before spawn returns its pid", async () => {
    installResolvedListeners();
    const spawned = deferred<number>();
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "harness_spawn") return spawned.promise;
      return Promise.resolve();
    });
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const onExit = vi.fn();
    child.watchChild("probe", vi.fn(), onExit);

    const spawning = child.spawnChild("probe", "pi", ["--mode", "rpc"], "/repo");
    mocks.handlers.get("harness-exit")?.({
      payload: { sessionId: "probe", code: 1, pid: 42 } as never,
    });
    expect(onExit).not.toHaveBeenCalled();

    spawned.resolve(42);
    await spawning;
    expect(onExit).toHaveBeenCalledWith(1);
    release();
  });

  it("passes stored overrides through resolution and command validation", async () => {
    vi.stubGlobal("localStorage", {
      getItem: (key: string) =>
        key === "monocode.providerBinaryPaths.v1"
          ? JSON.stringify({
              claude: "/opt/claude/bin/claude",
              codex: "/opt/codex/bin/codex",
              cursor: "/opt/cursor/bin/cursor-agent",
              grok: "/opt/grok/bin/grok",
              opencode: "/opt/opencode/bin/opencode",
              pi: "/opt/pi/bin/pi",
              omp: "/opt/omp/bin/omp",
              fx: "/opt/fx/bin/fx",
              hermes: "/opt/hermes/bin/hermes",
              antigravity: "/opt/antigravity/bin/agy_acp_server.par",
            })
          : null,
    });
    mocks.invoke.mockResolvedValue({ path: "/resolved" });
    const child = await loadChild();

    await child.resolveCodexBinary();
    expect(mocks.invoke).toHaveBeenCalledWith("harness_resolve_configured", {
      provider: "codex",
      binaryPath: "/opt/codex/bin/codex",
    });

    for (const [provider, binaryPath, resolve] of [
      ["claude", "/opt/claude/bin/claude", child.resolveClaudeBinary],
      ["cursor", "/opt/cursor/bin/cursor-agent", child.resolveCursorBinary],
      ["grok", "/opt/grok/bin/grok", child.resolveGrokBinary],
      ["pi", "/opt/pi/bin/pi", child.resolvePiBinary],
      ["omp", "/opt/omp/bin/omp", child.resolveOmpBinary],
      ["fx", "/opt/fx/bin/fx", child.resolveFxBinary],
      ["hermes", "/opt/hermes/bin/hermes", child.resolveHermesBinary],
      [
        "antigravity",
        "/opt/antigravity/bin/agy_acp_server.par",
        child.resolveAntigravityBinary,
      ],
    ] as const) {
      await resolve();
      expect(mocks.invoke).toHaveBeenLastCalledWith("harness_resolve_configured", {
        provider,
        binaryPath,
      });
    }

    await child.execChild("/resolved", ["--version"], undefined, "opencode");
    expect(mocks.invoke).toHaveBeenCalledWith("harness_exec", {
      command: "/resolved",
      args: ["--version"],
      cwd: undefined,
      binaryProvider: "opencode",
      binaryPath: "/opt/opencode/bin/opencode",
    });

    await child.execChild("/resolved", ["--version"], undefined, "codex", null);
    expect(mocks.invoke).toHaveBeenLastCalledWith("harness_exec", {
      command: "/resolved",
      args: ["--version"],
      cwd: undefined,
      binaryProvider: "codex",
      binaryPath: null,
    });
  });

  it("never routes a retired generation's stdout or exit to its replacement", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });
    const oldLines: string[] = [];
    const oldExit = vi.fn();
    child.watchChild("thread#0", (line) => oldLines.push(line), oldExit);
    emit("harness-stdout", { sessionId: "thread#0", line: "gen0" });
    expect(oldLines).toEqual(["gen0"]);

    // Retire the generation, then register its replacement under a new key.
    child.unwatchChild("thread#0");
    const newLines: string[] = [];
    const newExit = vi.fn();
    child.watchChild("thread#1", (line) => newLines.push(line), newExit);

    // Late output from the killed process arrives under its own key and is
    // dropped — it can never reach the replacement's handlers.
    emit("harness-stdout", { sessionId: "thread#0", line: "gen0-late" });
    emit("harness-exit", { sessionId: "thread#0", code: 1, pid: 42 });
    expect(oldLines).toEqual(["gen0"]);
    expect(oldExit).not.toHaveBeenCalled();
    expect(newLines).toEqual([]);
    expect(newExit).not.toHaveBeenCalled();

    // The replacement still receives its own traffic.
    emit("harness-stdout", { sessionId: "thread#1", line: "gen1" });
    expect(newLines).toEqual(["gen1"]);
    release();
  });

  it("drops late output from a child replaced under the same session id", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });
    const pids = [41, 42];
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "harness_spawn" ? pids.shift() : undefined,
    );
    child.watchChild("thread", () => undefined, vi.fn());
    await child.spawnChild("thread", "/bin/claude", [], "/repo");
    await child.killChild("thread");

    const lines: string[] = [];
    const errors: string[] = [];
    child.watchChild(
      "thread",
      (line) => lines.push(line),
      vi.fn(),
      (line) => errors.push(line),
    );
    const spawning = child.spawnChild("thread", "/bin/claude", [], "/repo");
    // The old child's output can still be in flight while the new one starts.
    emit("harness-stdout", { sessionId: "thread", line: "old", pid: 41 });
    emit("harness-stderr", { sessionId: "thread", line: "old", pid: 41 });
    await spawning;
    emit("harness-stdout", { sessionId: "thread", line: "old-late", pid: 41 });
    emit("harness-stdout", { sessionId: "thread", line: "new", pid: 42 });
    emit("harness-stdout", { sessionId: "thread", line: "no pid" });

    expect(lines).toEqual(["new", "no pid"]);
    expect(errors).toEqual([]);
    release();
  });

  it("delivers output that arrives after its child's exit", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });
    const pids = [41, 42];
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "harness_spawn" ? pids.shift() : undefined,
    );
    const lines: string[] = [];
    const exit = vi.fn();
    child.watchChild("thread", (line) => lines.push(line), exit);
    await child.spawnChild("thread", "/bin/claude", [], "/repo");
    emit("harness-exit", { sessionId: "thread", code: 0, pid: 41 });
    emit("harness-stdout", { sessionId: "thread", line: "last", pid: 41 });
    expect(exit).toHaveBeenCalledWith(0);
    expect(lines).toEqual(["last"]);

    // Once the session starts another child, the exited one is retired.
    await child.spawnChild("thread", "/bin/claude", [], "/repo");
    emit("harness-stdout", { sessionId: "thread", line: "stale", pid: 41 });
    emit("harness-stdout", { sessionId: "thread", line: "new", pid: 42 });
    expect(lines).toEqual(["last", "new"]);
    release();
  });

  it("does not hold output for children another window owns", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });

    // Events are broadcast to every window; this one never spawned "other".
    for (let i = 0; i < 5; i += 1) {
      emit("harness-stdout", { sessionId: "other", line: `line ${i}` });
      emit("harness-sse", { sessionId: "other", data: `event ${i}` });
    }

    const lines: string[] = [];
    const events: string[] = [];
    child.watchChild("other", (line) => lines.push(line), vi.fn());
    child.watchSse("other", (data) => events.push(data));
    expect(lines).toEqual([]);
    expect(events).toEqual([]);
    release();
  });

  it("still replays output a spawned child printed before it was watched", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });
    mocks.invoke.mockResolvedValue(42);

    await child.spawnChild("mine", "agent", [], "/tmp");
    emit("harness-stdout", { sessionId: "mine", line: "early" });
    await child.openHarnessSse("mine", "http://127.0.0.1:1/event");
    emit("harness-sse", { sessionId: "mine", data: "early-event" });

    const lines: string[] = [];
    const events: string[] = [];
    child.watchChild("mine", (line) => lines.push(line), vi.fn());
    child.watchSse("mine", (data) => events.push(data));
    expect(lines).toEqual(["early"]);
    expect(events).toEqual(["early-event"]);
    release();
  });

  it("drops output a killed child prints after it was stopped", async () => {
    installResolvedListeners();
    const child = await loadChild();
    const release = await child.acquireHarnessBridge();
    const emit = (name: string, payload: unknown) =>
      mocks.handlers.get(name)?.({ payload: payload as never });
    mocks.invoke.mockResolvedValue(42);

    child.watchChild("probe", vi.fn(), vi.fn());
    await child.spawnChild("probe", "agent", [], "/tmp");
    await child.killChild("probe");
    emit("harness-stdout", { sessionId: "probe", line: "late" });

    const lines: string[] = [];
    child.watchChild("probe", (line) => lines.push(line), vi.fn());
    expect(lines).toEqual([]);
    release();
  });
});
