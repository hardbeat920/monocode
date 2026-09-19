// Soak / stress integration test: drives the real `kimi acp` at LOW
// effort (kimi-for-coding) through a fleet of concurrent threads, rapid
// queued sends, cancel/stop/forget cycles, process kills, and cwd drift —
// simulating heavy real-world usage at ~4x the case count of the smoke suite.
// Gated on KIMI_SOAK=1 so CI and machines without the binary are unaffected.
import { existsSync } from "node:fs";
import { afterAll, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "./types";

const BIN = "/Users/elijah/.kimi-code/bin/kimi";
const REAL = process.env.KIMI_SOAK === "1" && existsSync(BIN);
const MODEL = "kimi:kimi-code/kimi-for-coding";
const LOW = { effort: "low" } as const;

const live = vi.hoisted(() => ({
  children: new Map<string, import("node:child_process").ChildProcess>(),
  listeners: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
  sent: [] as { key: string; method?: string; id?: number; raw: string }[],
  spawnCount: 0,
}));

vi.mock("./child", async () => {
  const { spawn } = await import("node:child_process");
  const readline = await import("node:readline");
  return {
    resolveKimiBinary: async () => ({ path: BIN }),
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
      live.sent.push({ key: id, method: message.method, id: message.id, raw: line });
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
vi.mock("../fs", () => ({ homeDir: async () => "/Users/elijah" }));

const kimi = await import("./kimi");

const sentMethods = () => live.sent.map((entry) => entry.method);
const waitFor = (predicate: () => boolean, ms = 120_000) =>
  vi.waitFor(() => expect(predicate()).toBe(true), { timeout: ms, interval: 250 });

describe.skipIf(!REAL)("kimi soak — low-effort fleet", () => {
  const usedThreads = new Set<string>();
  afterAll(async () => {
    for (const thread of usedThreads) {
      await kimi.forgetKimiSession(thread).catch(() => undefined);
    }
    for (const child of live.children.values()) child.kill("SIGKILL");
    live.children.clear();
  });

  const turn = (
    thread: string,
    text: string,
    events: HarnessEvent[],
    cwd = "/tmp",
    model: string = MODEL,
    settings: Record<string, string> | undefined = LOW,
    parkApprovals = false,
  ): Promise<void> => {
    usedThreads.add(thread);
    const input: SendTurnInput = {
      sessionId: thread,
      cwd,
      model,
      modelSettings: settings ? { ...settings } : undefined,
      text,
      runtimeMode: "supervised",
      onEvent: (event) => {
        events.push(event);
        // Unless a test deliberately parks on a permission request (to
        // prove a turn is in flight), auto-approve so nothing can wedge.
        if (!parkApprovals && event.type === "approval.requested") {
          kimi.respondKimiApproval(thread, event.requestId, "allow");
        }
      },
    };
    return kimi.sendKimiTurn(input);
  };

  const completed = (events: HarnessEvent[]) =>
    events.filter((e) => e.type === "message.completed").length;
  const errors = (events: HarnessEvent[]) =>
    events.filter((e) => e.type === "session.error");
  const promptsSent = () =>
    live.sent.filter((s) => s.method === "session/prompt").length;
  // Wire entries are keyed `thread#generation` — scope per thread so a
  // concurrent fleet can't trip another thread's predicate.
  const threadPromptSent = (thread: string) =>
    live.sent.some(
      (s) => s.method === "session/prompt" && s.key.startsWith(`${thread}#`),
    );
  const threadChild = (thread: string) =>
    [...live.children.keys()].filter((k) => k.startsWith(`${thread}#`)).at(-1);
  const expectCompleted = (events: HarnessEvent[], count: number) => {
    expect(errors(events)).toEqual([]);
    expect(completed(events)).toBe(count);
  };

  it(
    "handles a 6-thread fleet of 3 sequential turns each",
    async () => {
      const fleets = Array.from({ length: 6 }, (_, i) => `fleet-${i}`);
      const perThread = new Map<string, HarnessEvent[]>(
        fleets.map((t) => [t, []]),
      );
      await Promise.all(
        fleets.map(async (t, i) => {
          const events = perThread.get(t)!;
          for (let turnNo = 0; turnNo < 3; turnNo++) {
            await turn(t, `Reply with the single word OK${i}${turnNo}.`, events);
          }
        }),
      );
      for (const t of fleets) expectCompleted(perThread.get(t)!, 3);
      // Each thread spawned exactly one child — no spawn storm.
      expect(live.spawnCount).toBe(6);
      expect(promptsSent()).toBe(18);
    },
    420_000,
  );

  it(
    "serializes 4 rapid-fire sends on one thread without a lost turn",
    async () => {
      const spawnBase = live.spawnCount;
      const promptBase = promptsSent();
      const events: HarnessEvent[] = [];
      const sends = ["one", "two", "three", "four"].map((w) =>
        turn("rapid", `Reply with the single word ${w}.`, events),
      );
      await Promise.all(sends);
      expectCompleted(events, 4);
      expect(live.spawnCount - spawnBase).toBe(1);
      expect(promptsSent() - promptBase).toBe(4);
      await kimi.forgetKimiSession("rapid");
    },
    300_000,
  );

  it(
    "survives 5 cancel cycles — every resend recycles and resumes",
    async () => {
      const events: HarnessEvent[] = [];
      let midTurnCancels = 0;
      for (let cycle = 0; cycle < 5; cycle++) {
        const approvalsAtStart = events.filter(
          (e) => e.type === "approval.requested",
        ).length;
        let turnDone = false;
        // `ls` + the default effort reliably triggers a permission request,
        // which parks the turn server-side until we answer — the prompt is
        // provably in flight when we cancel. (LOW effort often just answers
        // in text, which would make every cancel a no-op.)
        const pending = turn(
          "cancel-cycle",
          "Use your tools to run the shell command `ls -la /tmp` and report the output.",
          events,
          "/tmp",
          MODEL,
          undefined,
          true, // park on the permission request: the turn cannot end while
          // our response is outstanding, so the cancel provably lands
          // mid-turn.
        ).finally(() => {
          turnDone = true;
        });
        void pending.catch(() => undefined);
        await waitFor(
          () =>
            events.filter((e) => e.type === "approval.requested").length >
              approvalsAtStart || turnDone,
        );
        const inFlight = !turnDone;
        const at = Date.now();
        await kimi.cancelKimiTurn("cancel-cycle");
        await pending;
        expect(Date.now() - at).toBeLessThan(60_000);
        const spawns = live.spawnCount;
        const done: HarnessEvent[] = [];
        await turn("cancel-cycle", "Reply with the single word OK.", done);
        expect(done).toContainEqual({ type: "message.completed" });
        // A cancel that landed mid-turn marked the transport stale → the
        // resend must run on a fresh process. If the turn already finished
        // first, reuse is correct too — only assert in the mid-turn case.
        if (inFlight) {
          midTurnCancels += 1;
          expect(live.spawnCount).toBeGreaterThan(spawns);
        }
      }
      // At least one cycle must have exercised the real mid-turn path.
      expect(midTurnCancels).toBeGreaterThanOrEqual(1);
      expect(sentMethods()).toContain("session/resume");
      await kimi.forgetKimiSession("cancel-cycle");
    },
    480_000,
  );

  it(
    "survives 3 stop cycles — next send starts a fresh process with resume",
    async () => {
      const spawnBase = live.spawnCount;
      const events: HarnessEvent[] = [];
      for (let cycle = 0; cycle < 3; cycle++) {
        const pending = turn(
          "stop-cycle",
          "Write a poem about the ocean, at least twenty lines.",
          events,
        );
        void pending.catch(() => undefined);
        await waitFor(() => threadPromptSent("stop-cycle"));
        await kimi.stopKimiSession("stop-cycle");
        await pending;
        const done: HarnessEvent[] = [];
        await turn("stop-cycle", "Reply with the single word OK.", done);
        expect(done).toContainEqual({ type: "message.completed" });
      }
      expect(live.spawnCount - spawnBase).toBe(4); // initial + 3 respawns
      await kimi.forgetKimiSession("stop-cycle");
    },
    420_000,
  );

  it(
    "survives 3 forget cycles — next send starts a brand-new session",
    async () => {
      const events: HarnessEvent[] = [];
      for (let cycle = 0; cycle < 3; cycle++) {
        const pending = turn(
          "forget-cycle",
          "Write a poem about mountains, at least twenty lines.",
          events,
        );
        void pending.catch(() => undefined);
        await waitFor(() => threadPromptSent("forget-cycle"));
        await kimi.forgetKimiSession("forget-cycle");
        await pending;
        live.sent.length = 0;
        const done: HarnessEvent[] = [];
        await turn("forget-cycle", "Reply with the single word OK.", done);
        expect(done).toContainEqual({ type: "message.completed" });
        // Forget drops the binding: a fresh session, never a resume.
        expect(sentMethods()).toContain("session/new");
        expect(sentMethods()).not.toContain("session/resume");
      }
    },
    420_000,
  );

  it(
    "survives 3 mid-turn process kills — each resend resumes cleanly",
    async () => {
      for (let cycle = 0; cycle < 3; cycle++) {
        const events: HarnessEvent[] = [];
        const pending = turn(
          "kill-cycle",
          "Write a poem about rivers, at least twenty lines.",
          events,
        );
        void pending.catch(() => undefined);
        await waitFor(
          () => threadPromptSent("kill-cycle") && threadChild("kill-cycle") != null,
        );
        const key = threadChild("kill-cycle")!;
        live.children.get(key)!.kill("SIGKILL");
        await pending;
        expect(events.some((e) => e.type === "session.ended")).toBe(true);
        const done: HarnessEvent[] = [];
        await turn("kill-cycle", "Reply with the single word OK.", done);
        expect(done).toContainEqual({ type: "message.completed" });
      }
      expect(sentMethods()).toContain("session/resume");
      await kimi.forgetKimiSession("kill-cycle");
    },
    420_000,
  );

  it(
    "moves a thread to a different cwd without leaking the old session",
    async () => {
      const spawnBase = live.spawnCount;
      const first: HarnessEvent[] = [];
      await turn("drift", "Reply with the single word OK.", first, "/tmp");
      expectCompleted(first, 1);
      live.sent.length = 0;
      const second: HarnessEvent[] = [];
      await turn("drift", "Reply with the single word OK.", second, "/private/tmp");
      expectCompleted(second, 1);
      // Moved cwd drops the resume binding → fresh session on a fresh process.
      expect(sentMethods()).toContain("session/new");
      expect(sentMethods()).not.toContain("session/resume");
      expect(live.spawnCount - spawnBase).toBe(2);
      await kimi.forgetKimiSession("drift");
    },
    300_000,
  );

  it(
    "suppresses a send that was queued when the running turn was cancelled",
    async () => {
      const eventsA: HarnessEvent[] = [];
      const a = turn(
        "suppress",
        "Use your tools to run the shell command `sleep 5 && echo done` and report the output.",
        eventsA,
      );
      void a.catch(() => undefined);
      await waitFor(
        () =>
          eventsA.some((e) => e.type === "approval.requested") ||
          threadPromptSent("suppress"),
      );
      const eventsB: HarnessEvent[] = [];
      const b = turn("suppress", "PINEAPPLE-UNIQUE-MARKER", eventsB);
      void b.catch(() => undefined);
      await kimi.cancelKimiTurn("suppress");
      await a;
      await b;
      // B was queued at cancel time: its prompt must never reach the wire.
      expect(
        live.sent.some((s) => s.raw.includes("PINEAPPLE-UNIQUE-MARKER")),
      ).toBe(false);
      const eventsC: HarnessEvent[] = [];
      await turn("suppress", "Reply with the single word OK.", eventsC);
      expectCompleted(eventsC, 1);
      await kimi.forgetKimiSession("suppress");
    },
    300_000,
  );

  it(
    "kitchen sink: 4 concurrent threads, each a different abuse pattern",
    async () => {
      const patterns: Record<string, HarnessEvent[]> = {
        steady: [],
        cancels: [],
        stops: [],
        kills: [],
      };
      await Promise.all([
        // T1: three quiet turns back to back.
        (async () => {
          const ev = patterns.steady;
          for (let i = 0; i < 3; i++) {
            await turn("steady", "Reply with the single word OK.", ev);
          }
        })(),
        // T2: send → cancel mid-flight → resend → complete.
        (async () => {
          const ev = patterns.cancels;
          const pending = turn(
            "cancels",
            "Use your tools to run the shell command `sleep 4 && echo done` and report the output.",
            ev,
          );
          void pending.catch(() => undefined);
          await waitFor(
            () =>
              ev.some((e) => e.type === "approval.requested") ||
              threadPromptSent("cancels"),
          );
          await kimi.cancelKimiTurn("cancels");
          await pending;
          await turn("cancels", "Reply with the single word OK.", ev);
        })(),
        // T3: send → stop mid-flight → resend → complete.
        (async () => {
          const ev = patterns.stops;
          const pending = turn(
            "stops",
            "Write a poem about forests, at least twenty lines.",
            ev,
          );
          void pending.catch(() => undefined);
          await waitFor(() => threadPromptSent("stops"));
          await kimi.stopKimiSession("stops");
          await pending;
          await turn("stops", "Reply with the single word OK.", ev);
        })(),
        // T4: send → SIGKILL mid-flight → resend → complete.
        (async () => {
          const ev = patterns.kills;
          const pending = turn(
            "kills",
            "Write a poem about deserts, at least twenty lines.",
            ev,
          );
          void pending.catch(() => undefined);
          await waitFor(
            () => threadPromptSent("kills") && threadChild("kills") != null,
          );
          live.children.get(threadChild("kills")!)!.kill("SIGKILL");
          await pending;
          await turn("kills", "Reply with the single word OK.", ev);
        })(),
      ]);
      expectCompleted(patterns.steady, 3);
      expect(completed(patterns.cancels)).toBe(1);
      expect(completed(patterns.stops)).toBe(1);
      expect(completed(patterns.kills)).toBe(1);
      for (const t of Object.keys(patterns)) {
        await kimi.forgetKimiSession(t);
      }
    },
    600_000,
  );

  it(
    "long task interrupt + continue on the same thread",
    async () => {
      const events: HarnessEvent[] = [];
      const pending = turn(
        "marathon",
        "Count from 1 to 60, one number per line.",
        events,
      );
      void pending.catch(() => undefined);
      await waitFor(
        () =>
          events.some((e) => e.type === "message.delta") ||
          threadPromptSent("marathon"),
      );
      await kimi.cancelKimiTurn("marathon");
      await pending;
      const done: HarnessEvent[] = [];
      await turn(
        "marathon",
        "Now reply with the single word OK.",
        done,
      );
      expectCompleted(done, 1);
      expect(sentMethods()).toContain("session/resume");
      await kimi.forgetKimiSession("marathon");
    },
    420_000,
  );
});
