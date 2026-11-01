import { describe, expect, it } from "vitest";
import type { HarnessEvent } from "../../../integrations/harness/core/types";
import type { Session } from "../../sessions/model/session";
import { ModEventTap } from "./events";

function session(patch: Partial<Session> = {}): Session {
  return {
    id: "s1",
    harness: "claude",
    model: "m",
    modelSettings: {},
    runtimeMode: "default",
    title: "",
    cwd: "/repo",
    blocks: [],
    ...patch,
  } as Session;
}

const batch = (...events: HarnessEvent[]) => new Map([["s1", events]]);

describe("ModEventTap", () => {
  it("derives turns from busy flips outside the flush, with duration and metrics", () => {
    const tap = new ModEventTap();
    const idle = [session()];
    const busy = [session({ busy: true })];

    // The composer sets busy before the harness flushes anything.
    const start = tap.turns(idle, busy, 1000);
    expect(start.map((e) => e.type === "event" && e.name)).toEqual([
      "session.start",
      "turn.start",
    ]);
    expect(start[0]).toMatchObject({
      event: {
        id: "s1",
        cwd: "/repo",
        projectCwd: "/repo",
        provider: "claude",
        isInteractive: true,
      },
    });

    expect(
      tap.derive(
        batch(
          { type: "turn.started", providerTurnId: "t" },
          { type: "context", used: 10, window: 100 },
          { type: "turn.metrics", inputTokens: 5 },
          { type: "turn.metrics", outputTokens: 7 },
        ),
        busy,
      ),
    ).toEqual([
      {
        type: "event",
        name: "context.update",
        event: { sessionId: "s1", used: 10, window: 100 },
      },
    ]);
    // turn.ready leaves busy alone; the turn's finally block clears it.
    expect(tap.derive(batch({ type: "turn.ready" }), busy)).toEqual([]);
    expect(tap.turns(busy, busy, 2000)).toEqual([]);

    expect(tap.turns(busy, idle, 4000)).toEqual([
      {
        type: "event",
        name: "turn.complete",
        event: {
          sessionId: "s1",
          durationMs: 3000,
          metrics: {
            inputTokens: 5,
            outputTokens: 7,
            cacheReadTokens: undefined,
            cacheWriteTokens: undefined,
          },
        },
      },
    ]);
  });

  it("emits tool.call once and resolves it on a terminal status", () => {
    const tap = new ModEventTap();
    const busy = [session({ busy: true })];
    const started = tap.derive(
      batch({
        type: "tool.started",
        callId: "c1",
        title: "git status",
        kind: "execute",
      }),
      busy,
    );
    expect(started.at(-1)).toEqual({
      type: "event",
      name: "tool.call",
      event: {
        sessionId: "s1",
        callId: "c1",
        tool: "Bash",
        command: "git status",
        file_path: undefined,
        paths: undefined,
      },
    });
    expect(
      tap.derive(
        batch({ type: "tool.started", callId: "c1", title: "again" }),
        busy,
      ),
    ).toEqual([]);

    const done = tap.derive(
      batch(
        {
          type: "tool.updated",
          callId: "c1",
          status: "in_progress",
          preview: { kind: "shell", output: "ok" },
        },
        { type: "tool.updated", callId: "c1", status: "completed" },
      ),
      busy,
    );
    expect(done).toEqual([
      {
        type: "tool.done",
        callId: "c1",
        result: { isError: false, result: { stdout: "ok" } },
      },
    ]);
  });

  it("maps file tools and settles calls the turn left open", () => {
    const tap = new ModEventTap();
    const busy = [session({ busy: true })];
    const [, call] = tap.derive(
      batch({
        type: "tool.started",
        callId: "w1",
        title: "Write a.ts",
        kind: "edit",
        preview: { kind: "write", path: "/repo/a.ts", contentOnly: true },
      }),
      busy,
    );
    expect(call).toMatchObject({
      event: { tool: "Write", file_path: "/repo/a.ts", paths: ["/repo/a.ts"] },
    });

    const idle = [
      session({
        blocks: [
          {
            id: "b",
            role: "tool",
            text: "",
            tool: { callId: "w1", status: "failed" },
          },
        ] as Session["blocks"],
      }),
    ];
    const end = tap.turns(busy, idle);
    expect(end[0]).toEqual({
      type: "tool.done",
      callId: "w1",
      result: { isError: true, result: undefined },
    });
    expect(end[1]).toMatchObject({ name: "turn.complete" });
  });

  it("emits a subagent's tool steps as tool.call with the parent call as agentId", () => {
    const tap = new ModEventTap();
    const s = [session()];
    tap.derive(batch({ type: "session.started" }), s);
    const step = {
      type: "agent.step" as const,
      callId: "agent1",
      stepId: "sub_read",
      kind: "tool" as const,
      text: "Read /repo/a.ts",
      toolKind: "read",
      preview: { kind: "read" as const, path: "/repo/a.ts" },
    };
    expect(
      tap.derive(
        batch(
          { ...step, status: "in_progress" },
          { ...step, stepId: "msg", kind: "message", text: "hi" },
        ),
        s,
      ),
    ).toEqual([
      {
        type: "event",
        name: "tool.call",
        event: expect.objectContaining({
          callId: "sub_read",
          tool: "Read",
          agentId: "agent1",
          file_path: "/repo/a.ts",
        }),
      },
    ]);
    expect(
      tap.derive(batch({ ...step, text: "", status: "completed" }), s),
    ).toEqual([
      {
        type: "tool.done",
        callId: "sub_read",
        result: { isError: false, result: undefined },
      },
    ]);
  });

  it("emits session.end and starts again for a reused id", () => {
    const tap = new ModEventTap();
    const s = [session()];
    tap.derive(batch({ type: "session.started" }), s);
    expect(tap.derive(batch({ type: "session.ended" }), s)).toEqual([
      { type: "event", name: "session.end", event: { sessionId: "s1" } },
    ]);
    expect(
      tap.derive(batch({ type: "session.started" }), s)[0],
    ).toMatchObject({
      name: "session.start",
    });
  });
});
