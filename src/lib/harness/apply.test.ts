import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../session";
import { appendUser, applyHarnessEvent, appendSteerUser, stopStreaming } from "./apply";

let now = 0;

beforeEach(() => {
  now = 0;
  vi.spyOn(Date, "now").mockImplementation(() => now);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("turn duration", () => {
  it("stamps how long the agent worked when the turn ends", () => {
    now = 1_000;
    let session = appendUser(newSession("cursor", "/tmp"), "hi");
    expect(session.busy).toBe(true);
    expect(session.blocks[0]?.startedAt).toBe(1_000);
    expect(session.blocks[0]?.durationMs).toBeUndefined();

    now = 26_000;
    session = stopStreaming(session);
    expect(session.busy).toBe(false);
    expect(session.blocks[0]?.durationMs).toBe(25_000);
  });

  it("does not overwrite a duration already recorded", () => {
    now = 1_000;
    let session = appendUser(newSession("cursor", "/tmp"), "hi");
    now = 5_000;
    session = stopStreaming(session);
    now = 90_000;
    session = stopStreaming(session);
    expect(session.blocks[0]?.durationMs).toBe(4_000);
  });

  it("records duration when the turn errors", () => {
    now = 1_000;
    let session = appendUser(newSession("cursor", "/tmp"), "hi");
    now = 8_000;
    session = applyHarnessEvent(session, {
      type: "session.error",
      message: "boom",
    });
    expect(session.busy).toBe(false);
    expect(session.blocks[0]?.durationMs).toBe(7_000);
  });
});

describe("streamed markdown", () => {
  it("keeps heading breaks, tables, and doubled letters", () => {
    const chunks = [
      "# Result",
      "\n",
      "\n",
      "book",
      "keeper..\n",
      "\n",
      "| a | b |\n",
      "| --- | --- |\n",
      "| 1 | 2 |",
    ];
    const session = chunks.reduce(
      (current, text) =>
        applyHarnessEvent(current, { type: "message.delta", text }),
      newSession("pi", "/tmp"),
    );
    expect(session.blocks[0]?.text).toBe(chunks.join(""));
  });

  it("does not double an assistant block when a completed snapshot repeats it", () => {
    let session = newSession("claude", "/tmp");
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "I'll read the file",
    });
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "I'll read the file",
    });
    expect(session.blocks).toHaveLength(1);
    expect(session.blocks[0]?.text).toBe("I'll read the file");
  });
});

describe("appendSteerUser", () => {
  it("appends a user message without sealing an in-flight assistant block", () => {
    let session = appendUser(newSession("cursor", "/tmp"), "build it");
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "Working on it",
    });
    expect(session.blocks[1]?.streaming).toBe(true);

    session = appendSteerUser(session, "focus on tests");
    expect(session.blocks).toHaveLength(3);
    expect(session.blocks[1]?.streaming).toBe(true);
    expect(session.blocks[2]).toMatchObject({
      role: "user",
      text: "focus on tests",
    });
    expect(session.blocks[2]?.startedAt).toBeUndefined();
    expect(session.busy).toBe(true);
  });

  it("keeps a note card on a steered user turn", () => {
    let session = appendUser(newSession("cursor", "/tmp"), "build it");
    session = appendSteerUser(session, "hi", [], {
      noteCard: { id: "n1", slug: "overview", title: "Overview" },
    });
    expect(session.blocks[1]).toMatchObject({
      role: "user",
      text: "hi",
      noteCard: { id: "n1", slug: "overview", title: "Overview" },
    });
  });
});

describe("status blocks", () => {
  it("keeps one row when the same status repeats", () => {
    let session = appendUser(newSession("claude", "/tmp"), "go");
    session = applyHarnessEvent(session, {
      type: "status",
      text: "Retrying in 3s",
    });
    session = applyHarnessEvent(session, {
      type: "status",
      text: "Retrying in 3s",
    });
    const system = session.blocks.filter((block) => block.role === "system");
    expect(system).toHaveLength(1);
    expect(system[0]?.text).toBe("Retrying in 3s");
  });

  it("still appends a status that differs from the last one", () => {
    let session = appendUser(newSession("claude", "/tmp"), "go");
    session = applyHarnessEvent(session, { type: "status", text: "Retrying" });
    session = applyHarnessEvent(session, { type: "status", text: "Compacting" });
    expect(
      session.blocks.filter((block) => block.role === "system"),
    ).toHaveLength(2);
  });

  it("ignores blank status text", () => {
    let session = appendUser(newSession("claude", "/tmp"), "go");
    session = applyHarnessEvent(session, { type: "status", text: "  " });
    expect(session.blocks.some((block) => block.role === "system")).toBe(false);
  });
});

describe("error blocks", () => {
  it("does not append the same turn error twice", () => {
    let session = appendUser(newSession("antigravity", "/tmp"), "go");
    session = applyHarnessEvent(session, {
      type: "session.error",
      message: "timeout waiting for response",
    });
    session = applyHarnessEvent(session, {
      type: "session.error",
      message: "timeout waiting for response",
    });

    const errors = session.blocks.filter(
      (block) =>
        block.role === "system" &&
        block.text === "timeout waiting for response",
    );
    expect(errors).toHaveLength(1);
  });
});

describe("applyHarnessEvent context", () => {
  it("tracks the newest level instead of summing turns", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, {
      type: "context",
      used: 30_000,
      window: 200_000,
    });
    session = applyHarnessEvent(session, { type: "context", used: 55_000 });
    expect(session.context).toEqual({ used: 55_000, window: 200_000 });
  });

  it("keeps the level when only a window arrives", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, { type: "context", used: 12_000 });
    session = applyHarnessEvent(session, { type: "context", window: 400_000 });
    expect(session.context).toEqual({ used: 12_000, window: 400_000 });
  });

  it("leaves blocks alone", () => {
    const session = applyHarnessEvent(newSession("codex", "/repo"), {
      type: "context",
      used: 1_000,
      window: 200_000,
    });
    expect(session.blocks).toEqual([]);
  });
});

describe("tool enrichment", () => {
  it("fills in a bare Read row when approval carries the path", () => {
    let session = newSession("cursor", "/repo");
    session = applyHarnessEvent(session, {
      type: "tool.updated",
      callId: "call_1",
      title: "Read",
      kind: "read",
      status: "pending",
    });
    session = applyHarnessEvent(session, {
      type: "approval.requested",
      requestId: 1,
      title: "Read src/App.tsx",
      kind: "read",
      callId: "call_1",
      preview: { kind: "read", path: "src/App.tsx", fileName: "App.tsx" },
    });
    const tool = session.blocks.find((block) => block.tool?.callId === "call_1");
    expect(tool?.text).toBe("Read src/App.tsx");
    expect(tool?.tool?.preview?.path).toBe("src/App.tsx");
  });

  it("replaces a bare Bash label with the command once input arrives", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, {
      type: "tool.started",
      callId: "call_1",
      title: "Bash",
      kind: "execute",
      status: "pending",
    });
    session = applyHarnessEvent(session, {
      type: "tool.updated",
      callId: "call_1",
      title: "ls",
      kind: "execute",
      status: "pending",
    });
    const tool = session.blocks.find((block) => block.tool?.callId === "call_1");
    expect(tool?.text).toBe("ls");
  });
});

describe("clarifying questions", () => {
  const questions = [
    {
      id: "Which file?",
      prompt: "Which file?",
      multiSelect: false,
      allowCustom: true,
      options: [
        { id: "a.ts", label: "a.ts" },
        { id: "b.ts", label: "b.ts" },
      ],
    },
  ];

  it("parks the prompt on the session instead of an Allow/Deny row", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, {
      type: "question.asked",
      requestId: 3,
      title: "Which file?",
      questions,
    });
    expect(session.pendingQuestion).toEqual({
      requestId: 3,
      title: "Which file?",
      questions,
    });
    expect(session.blocks).toEqual([]);
  });

  it("clears the prompt when the user answers or skips", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, {
      type: "question.asked",
      requestId: 3,
      questions,
    });
    session = applyHarnessEvent(session, {
      type: "question.resolved",
      requestId: 3,
      decision: "answered",
    });
    expect(session.pendingQuestion).toBeUndefined();
  });

  it("drops a parked prompt when the turn stops", () => {
    let session = newSession("claude", "/repo");
    session = applyHarnessEvent(session, {
      type: "question.asked",
      requestId: 3,
      questions,
    });
    session = stopStreaming(session);
    expect(session.pendingQuestion).toBeUndefined();
  });
});
