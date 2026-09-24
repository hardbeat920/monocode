// Real-endpoint integration test: drives the actual muse adapter against the
// installed `muse` CLI over stdio. Skipped unless MUSE_REAL=1 and the binary
// exists — CI and machines without the CLI are unaffected. Running it makes
// real Model API calls, so it is opt-in only.
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const BIN = process.env.MUSE_BIN ?? join(homedir(), ".local/bin/muse");
const REAL = process.env.MUSE_REAL === "1" && existsSync(BIN);

const live = vi.hoisted(() => ({
  children: new Map<string, import("node:child_process").ChildProcess>(),
  listeners: new Map<string, (line: string) => void>(),
  stderrListeners: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
  pids: new Map<string, number>(),
  spawns: [] as { id: string; command: string; args: string[]; cwd: string }[],
}));

vi.mock("../../core/child", async () => {
  const { spawn } = await import("node:child_process");
  const readline = await import("node:readline");
  return {
    resolveMuseBinary: async () => ({ path: BIN }),
    spawnChild: async (
      id: string,
      command: string,
      args: string[],
      cwd: string,
    ) => {
      live.spawns.push({ id, command, args, cwd });
      const child = spawn(command, args, { cwd });
      live.children.set(id, child);
      live.pids.set(id, child.pid ?? 0);
      readline
        .createInterface({ input: child.stdout })
        .on("line", (line) => live.listeners.get(id)?.(line));
      readline
        .createInterface({ input: child.stderr })
        .on("line", (line) => live.stderrListeners.get(id)?.(line));
      // Mirror the real bridge's pid check: a stale child's late exit must
      // not be delivered to the handler of the child that replaced it.
      child.on("exit", (code) => {
        if (live.pids.get(id) !== child.pid) return;
        live.pids.delete(id);
        live.exits.get(id)?.(code);
      });
    },
    killChild: async (id: string) => {
      live.listeners.delete(id);
      live.exits.delete(id);
      live.stderrListeners.delete(id);
      live.pids.delete(id);
      live.children.get(id)?.kill("SIGKILL");
      live.children.delete(id);
    },
    watchChild: (
      id: string,
      onLine: (line: string) => void,
      onExit?: (code: number | null) => void,
      onStderr?: (line: string) => void,
    ) => {
      live.listeners.set(id, onLine);
      if (onExit) live.exits.set(id, onExit);
      if (onStderr) live.stderrListeners.set(id, onStderr);
    },
    unwatchChild: (id: string) => {
      live.listeners.delete(id);
      live.exits.delete(id);
      live.stderrListeners.delete(id);
    },
  };
});

const muse = await import("./muse");

const waitFor = (predicate: () => boolean, ms = 120_000) =>
  vi.waitFor(() => expect(predicate()).toBe(true), {
    timeout: ms,
    interval: 200,
  });

function turnInput(events: HarnessEvent[]): SendTurnInput {
  return {
    sessionId: "muse-real-thread",
    cwd: "/tmp",
    model: "muse:muse-spark-1.2-contributor",
    modelSettings: {},
    runtimeMode: "auto",
    text: "Reply with the single word OK.",
    onEvent: (event) => events.push(event),
  };
}

describe.skipIf(!REAL)("muse real exec endpoint", () => {
  it(
    "runs a turn end-to-end and resumes the session on the next send",
    async () => {
      live.spawns.length = 0;
      const events: HarnessEvent[] = [];
      await muse.sendMuseTurn(turnInput(events));
      expect(events).toContainEqual({ type: "message.completed" });
      expect(
        events.some((event) => event.type === "session.providerBound"),
      ).toBe(true);
      expect(live.spawns).toHaveLength(1);
      expect(live.spawns[0]!.args.slice(0, 5)).toEqual([
        "exec",
        "--json",
        "--workspace",
        "/tmp",
        "--trust-workspace",
      ]);
      expect(live.spawns[0]!.args).not.toContain("--session-id");

      const events2: HarnessEvent[] = [];
      await muse.sendMuseTurn(turnInput(events2));
      expect(events2).toContainEqual({ type: "message.completed" });
      expect(live.spawns).toHaveLength(2);
      const sessionFlag = live.spawns[1]!.args.indexOf("--session-id");
      expect(sessionFlag).toBeGreaterThan(-1);
      const bound = events.filter(
        (event): event is Extract<HarnessEvent, { type: "session.providerBound" }> =>
          event.type === "session.providerBound",
      );
      expect(bound).toHaveLength(1);
      expect(live.spawns[1]!.args[sessionFlag + 1]).toBe(
        bound[0]!.providerSessionId,
      );
      await muse.forgetMuseSession("muse-real-thread");
    },
    300_000,
  );

  it(
    "cancels a mid-prompt turn fast and recovers on the next send",
    async () => {
      live.spawns.length = 0;
      const events: HarnessEvent[] = [];
      const pending = muse.sendMuseTurn({
        ...turnInput(events),
        text: "Use your tools to run the shell command `sleep 10; echo done` and report the output.",
      });
      await waitFor(() => live.spawns.length === 1);
      await new Promise((resolve) => setTimeout(resolve, 1500));
      const cancelledAt = Date.now();
      await muse.cancelMuseTurn("muse-real-thread");
      await pending; // must resolve promptly, not after the 30-minute timeout
      expect(Date.now() - cancelledAt).toBeLessThan(60_000);
      expect(live.children.size).toBe(0);

      const events2: HarnessEvent[] = [];
      await muse.sendMuseTurn(turnInput(events2));
      expect(events2).toContainEqual({ type: "message.completed" });
      await muse.forgetMuseSession("muse-real-thread");
    },
    300_000,
  );
});
