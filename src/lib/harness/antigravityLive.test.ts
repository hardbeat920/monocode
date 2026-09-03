import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let spawnedArgs: string[] = [];

vi.mock("./child", () => ({
  resolveAntigravityBinary: async () => ({ path: "/fake/agy" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawnedArgs = args;
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void, exit: (code: number | null) => void) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

import {
  cancelAntigravityTurn,
  forgetAntigravitySession,
  sendAntigravityTurn,
  stopAntigravitySession,
} from "./antigravity";
import type { HarnessEvent } from "./types";

const waitFor = async (pred: () => boolean, label = "condition") => {
  for (let i = 0; i < 100; i++) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

describe("antigravity live turn sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    spawnedArgs = [];
    onLine = undefined;
    onExit = undefined;
  });

  afterEach(async () => {
    await forgetAntigravitySession("sess-1");
  });

  it("spawns agy and streams back message deltas and completion", async () => {
    const events: HarnessEvent[] = [];
    const turnPromise = sendAntigravityTurn({
      sessionId: "sess-1",
      cwd: "/repo",
      model: "antigravity:gemini-3.8-flash-high",
      runtimeMode: "supervised",
      text: "hello flash 3.8",
      onEvent: (e) => events.push(e),
    });

    await waitFor(() => sent.length > 0, "first message sent");

    expect(spawnedArgs).toContain("--model");
    expect(spawnedArgs).toContain("gemini-3.8-flash-high");

    // Check sent message
    expect(sent.length).toBe(1);
    const sentMsg = JSON.parse(sent[0]);
    expect(sentMsg).toEqual({
      event: "user",
      message: {
        role: "user",
        content: "hello flash 3.8",
      },
    });

    // Simulate CLI events
    onLine!(
      JSON.stringify({
        event: "init",
        conversation_id: "conv-agy-1",
        init: { model: "gemini-3.8-flash-high" },
      }),
    );
    onLine!(
      JSON.stringify({
        event: "step_update",
        step_update: {
          step_index: 1,
          state: "ACTIVE",
          step_type: "agent_response",
          text_delta: "Hello from Gemini 3.8 Flash!",
        },
      }),
    );
    onLine!(
      JSON.stringify({
        event: "result",
        result: {
          conversation_id: "conv-agy-1",
          status: "SUCCESS",
          response: "Hello from Gemini 3.8 Flash!",
          usage: { total_tokens: 150 },
        },
      }),
    );

    await turnPromise;

    expect(events).toEqual([
      { type: "session.started" },
      { type: "session.providerBound", providerSessionId: "conv-agy-1" },
      { type: "message.delta", text: "Hello from Gemini 3.8 Flash!" },
      { type: "message.completed" },
      { type: "context", used: 150 },
    ]);
  });

  it("resumes with conversation id across turns and scopes tool callIds", async () => {
    const events1: HarnessEvent[] = [];
    const turn1 = sendAntigravityTurn({
      sessionId: "sess-1",
      cwd: "/repo",
      model: "antigravity:gemini-3.8-flash-high",
      runtimeMode: "supervised",
      text: "remember 42",
      onEvent: (e) => events1.push(e),
    });

    await waitFor(() => onLine != null, "first turn watched");

    onLine!(
      JSON.stringify({
        event: "init",
        conversation_id: "conv-agy-42",
        init: { model: "gemini-3.8-flash-high" },
      }),
    );
    onLine!(
      JSON.stringify({
        event: "step_update",
        step_update: {
          step_index: 1,
          state: "ACTIVE",
          step_type: "tool",
          tool_name: "test_tool",
        },
      }),
    );
    onLine!(
      JSON.stringify({
        event: "result",
        result: { status: "SUCCESS" },
      }),
    );
    await turn1;

    // Verify turn 1's tool callId is scoped by turn 1
    const toolEvent = events1.find((e) => e.type === "tool.started");
    expect(toolEvent).toBeDefined();
    if (toolEvent && toolEvent.type === "tool.started") {
      expect(toolEvent.callId).toBe("tool-sess-1-1-1");
    }

    // Now stop session (simulating restart or park)
    await stopAntigravitySession("sess-1");

    // Turn 2 should spawn with --conversation conv-agy-42
    sent.length = 0;
    const events2: HarnessEvent[] = [];
    const turn2 = sendAntigravityTurn({
      sessionId: "sess-1",
      cwd: "/repo",
      model: "antigravity:gemini-3.8-flash-high",
      runtimeMode: "supervised",
      text: "what was the number?",
      onEvent: (e) => events2.push(e),
    });

    await waitFor(() => sent.length > 0, "second turn spawned");

    expect(spawnedArgs).toContain("--conversation");
    expect(spawnedArgs).toContain("conv-agy-42");

    onLine!(
      JSON.stringify({
        event: "result",
        result: { status: "SUCCESS" },
      }),
    );
    await turn2;
  });

  it("handles turn cancellation cleanly", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendAntigravityTurn({
      sessionId: "sess-1",
      cwd: "/repo",
      model: "antigravity:gemini-3.8-flash-high",
      runtimeMode: "supervised",
      text: "long running prompt",
      onEvent: (e) => events.push(e),
    });

    await cancelAntigravityTurn("sess-1");
    await turn;
  });
});
