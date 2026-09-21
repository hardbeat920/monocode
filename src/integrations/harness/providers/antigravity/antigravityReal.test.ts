// Real-endpoint integration test: drives the actual adapter against the
// installed `agy_acp_server.par` over stdio. Skipped unless AGY_REAL=1 and the
// endpoint exists — CI and machines without the binary are unaffected.
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const BIN =
  process.env.AGY_BIN ??
  join(homedir(), ".local/bin/agy_acp_server.par");
const REAL = process.env.AGY_REAL === "1" && existsSync(BIN);
const ARGS = process.platform === "linux" ? ["--uid="] : [];

const live = vi.hoisted(() => ({
  children: new Map<string, import("node:child_process").ChildProcess>(),
  listeners: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
  sent: [] as { key: string; method?: string; id?: number }[],
  spawnCount: 0,
}));

vi.mock("../../core/child", async () => {
  const { spawn } = await import("node:child_process");
  const readline = await import("node:readline");
  return {
    resolveAntigravityBinary: async () => ({ path: BIN, args: ARGS }),
    spawnChild: async (
      id: string,
      command: string,
      args: string[],
      cwd: string,
    ) => {
      const child = spawn(command, args, { cwd });
      live.children.set(id, child);
      live.spawnCount += 1;
      readline
        .createInterface({ input: child.stdout })
        .on("line", (line) => live.listeners.get(id)?.(line));
      child.on("exit", (code) => live.exits.get(id)?.(code));
    },
    killChild: async (id: string) => {
      live.children.get(id)?.kill("SIGKILL");
      live.children.delete(id);
    },
    writeChild: async (id: string, line: string) => {
      const child = live.children.get(id);
      if (!child) throw new Error(`no child for ${id}`);
      const message = JSON.parse(line) as { method?: string; id?: number };
      live.sent.push({ key: id, method: message.method, id: message.id });
      await new Promise<void>((resolve, reject) =>
        child.stdin.write(line + "\n", (error) =>
          error ? reject(error) : resolve(),
        ),
      );
    },
    watchChild: (
      id: string,
      onLine: (line: string) => void,
      onExit?: (code: number | null) => void,
    ) => {
      live.listeners.set(id, onLine);
      if (onExit) live.exits.set(id, onExit);
    },
    unwatchChild: (id: string) => {
      live.listeners.delete(id);
      live.exits.delete(id);
    },
  };
});
vi.mock("../../../../platform/tauri/fs", () => ({ homeDir: async () => homedir() }));

const agy = await import("./antigravity");

const sentMethods = () => live.sent.map((entry) => entry.method);
// vi.waitFor retries until the callback stops throwing — a falsy return
// counts as success, so wrap predicates in an assertion.
const waitFor = (predicate: () => boolean, ms = 120_000) =>
  vi.waitFor(
    () => expect(predicate()).toBe(true),
    { timeout: ms, interval: 200 },
  );

describe.skipIf(!REAL)("antigravity real ACP endpoint", () => {
  const resetWire = () => {
    live.sent.length = 0;
  };
  const turn = (text: string, events: HarnessEvent[]): Promise<void> => {
    const input: SendTurnInput = {
      sessionId: "real-thread",
      cwd: process.platform === "win32" ? process.cwd() : "/tmp",
      model: "antigravity:gemini-3.8-flash-high",
      text,
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    };
    return agy.sendAntigravityTurn(input);
  };

  it(
    "runs a turn end-to-end and reuses the transport",
    async () => {
      resetWire();
      const events: HarnessEvent[] = [];
      await turn("Reply with the single word OK.", events);
      expect(events).toContainEqual({ type: "message.completed" });
      expect(live.spawnCount).toBe(1);
      expect(sentMethods()).toContain("session/new");

      const events2: HarnessEvent[] = [];
      await turn("Reply with the single word OK.", events2);
      expect(events2).toContainEqual({ type: "message.completed" });
      // Second turn reuses the same child — no extra spawn.
      expect(live.spawnCount).toBe(1);
      await agy.forgetAntigravitySession("real-thread");
    },
    240_000,
  );

  it(
    "cancels a mid-prompt turn fast and recovers on the next send",
    async () => {
      resetWire();
      const events: HarnessEvent[] = [];
      const pending = turn(
        "Use your tools to run the shell command `ls -la /tmp` and report the output.",
        events,
      );
      // A permission request parks the turn server-side until we answer — the
      // prompt is guaranteed in flight when we cancel.
      await waitFor(() => events.some((e) => e.type === "approval.requested"));
      const cancelledAt = Date.now();
      await agy.cancelAntigravityTurn("real-thread");
      await pending; // must resolve promptly, not after the 30-minute timeout
      expect(Date.now() - cancelledAt).toBeLessThan(60_000);
      expect(sentMethods()).toContain("session/cancel");

      // The cancelled transport is stale: the next send recycles and resumes.
      const before = live.spawnCount;
      const events2: HarnessEvent[] = [];
      await turn("Reply with the single word OK.", events2);
      expect(live.spawnCount).toBe(before + 1);
      expect(sentMethods()).toContain("session/resume");
      expect(events2).toContainEqual({ type: "message.completed" });
      await agy.forgetAntigravitySession("real-thread");
    },
    300_000,
  );

  it(
    "survives the provider process exiting mid-turn and resumes after respawn",
    async () => {
      resetWire();
      const events: HarnessEvent[] = [];
      const pending = turn("Write a long poem about the sea.", events);
      await waitFor(
        () =>
          sentMethods().includes("session/prompt") &&
          live.children.size > 0,
      );
      const key = [...live.children.keys()].at(-1)!;
      live.children.get(key)!.kill("SIGKILL");
      await pending; // exit settles the turn instead of wedging it
      expect(events.some((e) => e.type === "session.ended")).toBe(true);

      const events2: HarnessEvent[] = [];
      await turn("Reply with the single word OK.", events2);
      expect(sentMethods()).toContain("session/resume");
      expect(events2).toContainEqual({ type: "message.completed" });
      await agy.forgetAntigravitySession("real-thread");
    },
    300_000,
  );
});
