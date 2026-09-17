import { describe, expect, it } from "vitest";
import {
  formatLiveElapsed,
  liveAgentsFromSessions,
} from "./liveAgents";
import { newSession, type Block, type Session } from "./session";

function chat(cwd: string, patch: Partial<Session> = {}): Session {
  const session = newSession("claude", cwd);
  session.title = "claude · Fix the sidebar";
  session.blocks = [{ id: "u1", role: "user", text: "hello", startedAt: 1_000 }];
  return { ...session, ...patch, blocks: patch.blocks ?? session.blocks };
}

function edit(id: string, path = "src/App.tsx", status = "in_progress"): Block {
  const fileName = path.split("/").pop() ?? path;
  return {
    id,
    role: "tool",
    text: `Edited ${path}`,
    tool: {
      kind: "edit",
      title: `Edited ${path}`,
      status,
      preview: { kind: "write", path, fileName },
    },
  };
}

describe("liveAgentsFromSessions", () => {
  it("keeps internal workers in their lead's agent panel", () => {
    const lead = chat("/repo", { id: "lead", busy: true });
    const worker = chat("/repo", { id: "worker", busy: true, orchestrationLeadId: "lead" });
    expect(liveAgentsFromSessions([lead, worker]).map((agent) => agent.id)).toEqual(["lead"]);
  });
  it("skips idle sessions", () => {
    expect(liveAgentsFromSessions([chat("/tmp/a")])).toEqual([]);
  });

  it("maps a busy turn into a live agent", () => {
    const session = chat("/tmp/agent-terminal", {
      busy: true,
      blocks: [
        { id: "u1", role: "user", text: "hello", startedAt: 1_000 },
        edit("t1"),
      ],
    });
    expect(liveAgentsFromSessions([session])).toEqual([
      {
        id: session.id,
        cwd: "/tmp/agent-terminal",
        title: "Fix the sidebar",
        harness: "claude",
        activity: "Edited src/App.tsx",
        startedAt: 1_000,
        durationMs: undefined,
        needsApproval: false,
        done: false,
      },
    ]);
  });

  it("puts sessions waiting on approval first", () => {
    const working = chat("/tmp/a", {
      busy: true,
      blocks: [
        { id: "u1", role: "user", text: "go", startedAt: 1_000 },
        edit("t1"),
      ],
    });
    const waiting = chat("/tmp/b", {
      busy: true,
      blocks: [
        { id: "u1", role: "user", text: "go", startedAt: 2_000 },
        {
          id: "a1",
          role: "approval",
          text: "run rm",
          approval: { requestId: 1 },
        },
      ],
    });
    expect(liveAgentsFromSessions([working, waiting]).map((row) => row.id)).toEqual(
      [waiting.id, working.id],
    );
    expect(liveAgentsFromSessions([working, waiting])[0]?.needsApproval).toBe(
      true,
    );
  });

  it("treats a parked clarifying question as needing approval", () => {
    const waiting = chat("/tmp/ask", {
      busy: true,
      pendingQuestion: {
        requestId: 4,
        title: "Which file?",
        questions: [
          {
            id: "q1",
            prompt: "Which file?",
            multiSelect: false,
            allowCustom: true,
            options: [{ id: "a.ts", label: "a.ts" }],
          },
        ],
      },
    });
    expect(liveAgentsFromSessions([waiting])[0]).toMatchObject({
      id: waiting.id,
      activity: "Which file?",
      needsApproval: true,
      done: false,
    });
  });

  it("sorts working agents by longest-running turn first", () => {
    const newer = chat("/tmp/new", {
      busy: true,
      blocks: [{ id: "u1", role: "user", text: "go", startedAt: 5_000 }],
    });
    const older = chat("/tmp/old", {
      busy: true,
      blocks: [{ id: "u1", role: "user", text: "go", startedAt: 1_000 }],
    });
    expect(liveAgentsFromSessions([newer, older]).map((row) => row.cwd)).toEqual(
      ["/tmp/old", "/tmp/new"],
    );
  });

  it("keeps an unfocused finished session until it is seen", () => {
    const finished = chat("/tmp/done", {
      blocks: [
        {
          id: "u1",
          role: "user",
          text: "go",
          startedAt: 1_000,
          durationMs: 12_000,
        },
        edit("t1", "src/App.tsx", "completed"),
      ],
    });
    expect(liveAgentsFromSessions([finished])).toEqual([]);
    expect(
      liveAgentsFromSessions([finished], new Set([finished.id])),
    ).toEqual([
      {
        id: finished.id,
        cwd: "/tmp/done",
        title: "Fix the sidebar",
        harness: "claude",
        activity: "Done",
        startedAt: 1_000,
        durationMs: 12_000,
        needsApproval: false,
        done: true,
      },
    ]);
  });

  it("keeps a working session above a finished one", () => {
    const working = chat("/tmp/a", {
      busy: true,
      blocks: [{ id: "u1", role: "user", text: "go", startedAt: 5_000 }],
    });
    const finished = chat("/tmp/b", {
      blocks: [
        {
          id: "u1",
          role: "user",
          text: "go",
          startedAt: 1_000,
          durationMs: 8_000,
        },
      ],
    });
    expect(
      liveAgentsFromSessions(
        [finished, working],
        new Set([finished.id]),
      ).map((row) => row.cwd),
    ).toEqual(["/tmp/a", "/tmp/b"]);
  });
});

describe("formatLiveElapsed", () => {
  it("formats seconds, minutes, and hours", () => {
    expect(formatLiveElapsed(0, 1_000)).toBe("1s");
    expect(formatLiveElapsed(0, 38_000)).toBe("38s");
    expect(formatLiveElapsed(0, 72_000)).toBe("1m 12s");
    expect(formatLiveElapsed(0, 120_000)).toBe("2m");
    expect(formatLiveElapsed(0, 3_600_000)).toBe("1h");
    expect(formatLiveElapsed(0, 3_720_000)).toBe("1h 2m");
  });
});
