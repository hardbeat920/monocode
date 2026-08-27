import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../session";
import {
  appendUser,
  applyHarnessEvent,
  appendSteerUser,
  stopStreaming,
} from "./apply";
import type { HarnessEvent } from "./types";

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

describe("streamed text fidelity", () => {
  it("appends repeated assistant boundaries verbatim", () => {
    let session = newSession("cursor", "/tmp");
    for (const text of ["hel", "lo", "\n", "\n", "Wait.", ". Next"]) {
      session = applyHarnessEvent(session, { type: "message.delta", text });
    }

    expect(session.blocks).toMatchObject([
      {
        role: "assistant",
        text: "hello\n\nWait.. Next",
        streaming: true,
      },
    ]);
  });

  it("appends whitespace-only reasoning boundaries verbatim", () => {
    let session = newSession("codex", "/tmp");
    for (const text of ["thinking", " ", " ", "\n", "\n"]) {
      session = applyHarnessEvent(session, { type: "reasoning.delta", text });
    }

    expect(session.blocks).toMatchObject([
      {
        role: "reasoning",
        text: "thinking  \n\n",
        streaming: true,
      },
    ]);
  });
});

describe("transcript ordering", () => {
  it("keeps assistant text on both sides of a tool in separate blocks", () => {
    const events: HarnessEvent[] = [
      { type: "message.delta", text: "before" },
      {
        type: "tool.started",
        callId: "read-1",
        title: "Read file",
        kind: "read",
      },
      { type: "message.delta", text: "after" },
    ];

    const session = events.reduce(applyHarnessEvent, newSession("pi", "/tmp"));

    expect(session.blocks).toMatchObject([
      { role: "assistant", text: "before", streaming: false },
      { role: "tool", tool: { callId: "read-1" } },
      { role: "assistant", text: "after", streaming: true },
    ]);
  });

  it("keeps reasoning text on both sides of a tool in separate blocks", () => {
    const events: HarnessEvent[] = [
      { type: "reasoning.delta", text: "before" },
      {
        type: "tool.started",
        callId: "read-1",
        title: "Read file",
        kind: "read",
      },
      { type: "reasoning.delta", text: "after" },
    ];

    const session = events.reduce(
      applyHarnessEvent,
      newSession("codex", "/tmp"),
    );

    expect(session.blocks).toMatchObject([
      { role: "reasoning", text: "before", streaming: false },
      { role: "tool", tool: { callId: "read-1" } },
      { role: "reasoning", text: "after", streaming: true },
    ]);
  });

  it("preserves a complete Markdown fixture", () => {
    const chunks = [
      "# Result\n",
      "\n",
      "- book",
      "keeper\n",
      "\nfirst line ",
      " \nsecond line\n",
      "\n```ts\n",
      'const value = "bookkeeper";\n',
      "```",
    ];
    const session = chunks.reduce(
      (current, text) =>
        applyHarnessEvent(current, { type: "message.delta", text }),
      newSession("cursor", "/tmp"),
    );

    expect(session.blocks[0]?.text).toBe(chunks.join(""));
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

  it("completion seals text streams from both sides of an in-turn steer", () => {
    let session = appendUser(newSession("claude", "/tmp"), "build it");
    session = applyHarnessEvent(session, {
      type: "reasoning.delta",
      text: "before reasoning",
    });
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "before answer",
    });
    session = appendSteerUser(session, "focus on tests");
    session = applyHarnessEvent(session, {
      type: "reasoning.delta",
      text: "after reasoning",
    });
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "after answer",
    });

    session = applyHarnessEvent(session, { type: "reasoning.completed" });
    session = applyHarnessEvent(session, { type: "message.completed" });

    const textBlocks = session.blocks.filter(
      (block) => block.role === "assistant" || block.role === "reasoning",
    );
    expect(textBlocks.map((block) => block.text)).toEqual([
      "before reasoning",
      "before answer",
      "after reasoning",
      "after answer",
    ]);
    expect(textBlocks.every((block) => block.streaming === false)).toBe(true);
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
    session = applyHarnessEvent(session, {
      type: "status",
      text: "Compacting",
    });
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
    const tool = session.blocks.find(
      (block) => block.tool?.callId === "call_1",
    );
    expect(tool?.text).toBe("Read src/App.tsx");
    expect(tool?.tool?.preview?.path).toBe("src/App.tsx");
  });
});
