import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
const spawns: Array<{ args: string[]; cwd: string }> = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string) => {
    if (command === "write_attachment")
      return "/tmp/monocode-attachments/pasted.png";
    throw new Error(`unexpected invoke: ${command}`);
  },
}));

vi.mock("./child", () => ({
  resolveAntigravityBinary: async () => ({ path: "/fake/agy" }),
  spawnChild: async (
    _id: string,
    _path: string,
    args: string[],
    cwd: string,
  ) => {
    spawns.push({ args, cwd });
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (line: string) => void,
    exit: (code: number | null) => void,
  ) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const {
  sendAntigravityTurn,
  steerAntigravityTurn,
  cancelAntigravityTurn,
  respondAntigravityQuestion,
  stopAntigravitySession,
  resetAntigravityLiveForTests,
} = await import("./antigravity");
import type { HarnessEvent } from "./types";

const waitFor = async (predicate: () => boolean) => {
  for (let i = 0; i < 100; i += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error("timed out");
};

function emit(event: Record<string, unknown>) {
  onLine?.(JSON.stringify(event));
}

function input(events: HarnessEvent[], text = "hello") {
  return {
    sessionId: "a1",
    cwd: "/repo",
    model: "antigravity:gemini-3.7-flash-high",
    modelSettings: {},
    runtimeMode: "supervised" as const,
    text,
    attachments: [],
    onEvent: (event: HarnessEvent) => events.push(event),
  };
}

beforeEach(() => {
  sent.length = 0;
  spawns.length = 0;
  onLine = undefined;
  onExit = undefined;
});

afterEach(async () => {
  await resetAntigravityLiveForTests();
});

describe("Antigravity live adapter", () => {
  it("waits for init before sending a text-only user event", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    expect(spawns[0].cwd).toBe("/repo");
    expect(sent).toEqual([]);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    expect(JSON.parse(sent[0])).toEqual({
      event: "user",
      message: { content: "hello" },
    });
    emit({
      event: "step_update",
      step_update: {
        conversation_id: "conversation-1",
        step_type: "agent_response",
        state: "DONE",
        text_delta: "hi",
      },
    });
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "hi",
      },
    });
    await turn;
    expect(events).toEqual(
      expect.arrayContaining([
        { type: "session.providerBound", providerSessionId: "conversation-1" },
        { type: "session.started" },
        { type: "message.delta", text: "hi" },
        { type: "message.completed" },
      ]),
    );
  });

  it("keeps a child warm for follow-up turns", async () => {
    const events: HarnessEvent[] = [];
    const first = sendAntigravityTurn(input(events, "one"));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "one",
      },
    });
    await first;
    const second = sendAntigravityTurn(input(events, "two"));
    await waitFor(() => sent.length === 2);
    expect(spawns).toHaveLength(1);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "two",
      },
    });
    await second;
  });

  it("cancels immediately and resumes on a later turn", async () => {
    const events: HarnessEvent[] = [];
    const first = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    await cancelAntigravityTurn("a1");
    await first;
    const second = sendAntigravityTurn(input(events, "again"));
    await waitFor(() => spawns.length === 2);
    expect(spawns[1].args).toEqual(
      expect.arrayContaining(["--conversation", "conversation-1"]),
    );
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 2);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "done",
      },
    });
    await second;
  });

  it("materializes pasted image data then sends a text @ reference", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn({
      ...input(events),
      attachments: [
        {
          id: "pasted",
          name: "pasted.png",
          mimeType: "image/png",
          kind: "image",
          size: 3,
          data: "AQID",
        },
      ],
    });
    await waitFor(() => spawns.length === 1);
    expect(spawns[0].args).toEqual(
      expect.arrayContaining(["--add-dir", "/tmp/monocode-attachments"]),
    );
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    expect(JSON.parse(sent[0])).toEqual({
      event: "user",
      message: {
        content:
          "Attachments to inspect:\n@[/tmp/monocode-attachments/pasted.png]\n\nhello",
      },
    });
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "done",
      },
    });
    await turn;
  });

  it("steers an in-flight turn by writing a user message to child stdin", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);

    await steerAntigravityTurn({
      sessionId: "a1",
      cwd: "/repo",
      text: "please focus on tests",
    });

    await waitFor(() => sent.length === 2);
    expect(JSON.parse(sent[1])).toEqual({
      event: "user",
      message: { content: "please focus on tests" },
    });

    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "focused on tests",
      },
    });
    await turn;
  });

  it("fails an in-flight turn when the child exits", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    onExit?.(1);
    await expect(turn).rejects.toThrow("exited before completing");
  });

  it("handles clarifying questions and responds with user answer via stdin", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);

    emit({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "ask_question",
        step_index: 3,
        state: "ACTIVE",
        tool_info: {
          parameters: {
            questions: [
              {
                id: "q1",
                question: "Do you want tests?",
                options: ["Yes", "No"],
              },
            ],
          },
        },
      },
    });

    const asked = events.find((e) => e.type === "question.asked");
    expect(asked).toBeDefined();

    respondAntigravityQuestion("a1", 3, {
      kind: "answered",
      answers: { q1: ["Yes"] },
    });

    await waitFor(() => sent.length === 2);
    expect(JSON.parse(sent[1])).toEqual({
      event: "user",
      message: {
        content: "User response to clarifying questions:\n- Do you want tests?: Yes",
      },
    });

    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "done with tests",
      },
    });
    await turn;
  });

  it("handles user skipping clarifying question by sending skipped notification over stdin", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn(input(events));
    await waitFor(() => spawns.length === 1);
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);

    emit({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "ask_question",
        step_index: 4,
        state: "ACTIVE",
        tool_info: {
          parameters: {
            questions: [
              {
                id: "q1",
                question: "Which database?",
                options: ["PostgreSQL", "SQLite"],
              },
            ],
          },
        },
      },
    });

    const asked = events.find((e) => e.type === "question.asked");
    expect(asked).toBeDefined();

    respondAntigravityQuestion("a1", 4, {
      kind: "skipped",
    });

    await waitFor(() => sent.length === 2);
    expect(JSON.parse(sent[1])).toEqual({
      event: "user",
      message: {
        content:
          "The user skipped answering the clarifying question(s). Please proceed using your best judgment.",
      },
    });

    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "decided autonomously",
      },
    });
    await turn;
  });

  it("re-spawns with --mode plan when follow-up turn includes /plan", async () => {
    const events: HarnessEvent[] = [];
    const first = sendAntigravityTurn(input(events, "turn one"));
    await waitFor(() => spawns.length === 1);
    expect(spawns[0].args).not.toEqual(expect.arrayContaining(["--mode", "plan"]));
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "turn one done",
      },
    });
    await first;

    const second = sendAntigravityTurn(input(events, "turn two /plan"));
    await waitFor(() => spawns.length === 2);
    expect(spawns[1].args).toEqual(
      expect.arrayContaining([
        "--conversation",
        "conversation-1",
        "--mode",
        "plan",
      ]),
    );
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 2);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "plan ready",
      },
    });
    await second;
  });

  it("re-spawns with cumulative --add-dir arguments when follow-up turn introduces a new attachment directory", async () => {
    const events: HarnessEvent[] = [];
    const first = sendAntigravityTurn({
      ...input(events, "first"),
      attachments: [
        {
          id: "att1",
          name: "one.png",
          path: "/tmp/dir1/one.png",
          kind: "image",
          mimeType: "image/png",
          size: 10,
        },
      ],
    });
    await waitFor(() => spawns.length === 1);
    expect(spawns[0].args).toEqual(
      expect.arrayContaining(["--add-dir", "/tmp/dir1"]),
    );
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 1);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "got first",
      },
    });
    await first;

    // Follow-up turn with an attachment from a new directory
    const second = sendAntigravityTurn({
      ...input(events, "second"),
      attachments: [
        {
          id: "att2",
          name: "two.png",
          path: "/tmp/dir2/two.png",
          kind: "image",
          mimeType: "image/png",
          size: 10,
        },
      ],
    });
    await waitFor(() => spawns.length === 2);
    expect(spawns[1].args).toEqual(
      expect.arrayContaining([
        "--conversation",
        "conversation-1",
        "--add-dir",
        "/tmp/dir1",
        "--add-dir",
        "/tmp/dir2",
      ]),
    );
    emit({ event: "init", conversation_id: "conversation-1", init: {} });
    await waitFor(() => sent.length === 2);
    emit({
      event: "result",
      result: {
        conversation_id: "conversation-1",
        status: "SUCCESS",
        response: "got second",
      },
    });
    await second;
  });
});

