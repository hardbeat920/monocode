import { describe, expect, it } from "vitest";
import { AcpSubagents } from "./acpSubagents";
import { eventsFromAcpUpdate as fxEvents } from "./fxProtocol";
import { eventsFromAcpUpdate as grokEvents } from "./grokProtocol";
import { applyHarnessEvent } from "./apply";
import { newSession } from "../session";
import type { HarnessEvent } from "./types";

describe.each([
  ["fx", fxEvents],
  ["grok", grokEvents],
] as const)("%s subagents", (provider, parse) => {
  it("names delegated work and merges child tool updates without leaking prose", () => {
    const router = new AcpSubagents();
    let session = newSession(provider, "/repo");
    const push = (update: Record<string, unknown>) => {
      const params = { sessionId: "parent", update };
      for (const event of router.route(params, parse(params)))
        session = applyHarnessEvent(session, event);
    };
    push({
      sessionUpdate: "tool_call",
      toolCallId: "spawn",
      kind: "other",
      title: "Task",
      status: "in_progress",
      rawInput: { _toolName: "task", description: "Check auth" },
    });
    const meta = { parentToolCallId: "spawn" };
    push({
      sessionUpdate: "agent_message_chunk",
      _meta: meta,
      content: { type: "text", text: "Checking " },
    });
    push({
      sessionUpdate: "agent_message_chunk",
      _meta: meta,
      content: { type: "text", text: "auth." },
    });
    push({
      sessionUpdate: "tool_call",
      _meta: meta,
      toolCallId: "read",
      kind: "read",
      title: "Read auth.ts",
      status: "in_progress",
    });
    // Sparse completions may omit the parent metadata entirely.
    push({
      sessionUpdate: "tool_call_update",
      toolCallId: "read",
      status: "completed",
    });
    push({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "Parent answer" },
    });
    const block = session.blocks.find(
      (entry) => entry.tool?.callId === "spawn",
    )!;
    expect(block.tool?.kind).toBe("agent");
    expect(block.text).toBe("Check auth");
    expect(block.agentRun?.steps).toHaveLength(2);
    expect(block.agentRun?.steps[0].text).toBe("Checking auth.");
    expect(block.agentRun?.steps[1].status).toBe("completed");
    expect(
      session.blocks
        .filter((entry) => entry.role === "assistant")
        .map((entry) => entry.text),
    ).toEqual(["Parent answer"]);
    expect(session.blocks.some((entry) => entry.tool?.callId === "read")).toBe(
      false,
    );
  });
});

describe("ACP child routing", () => {
  it("buffers children until the matching row exists, even with simultaneous spawns", () => {
    const router = new AcpSubagents();
    const child = (id: string, text: string): HarnessEvent[] =>
      router.route(
        { update: { _meta: { cursor: { parentToolCallId: id } } } },
        [{ type: "message.delta", text }],
      );
    expect(child("b", "Second child")).toEqual([]);
    expect(child("a", "First child")).toEqual([]);
    const start = (callId: string) =>
      router.route({}, [
        { type: "tool.updated", callId, kind: "agent", title: callId },
      ]);
    expect(start("a")).toEqual([
      expect.objectContaining({ callId: "a", type: "tool.updated" }),
      expect.objectContaining({
        callId: "a",
        type: "agent.step",
        text: "First child",
      }),
    ]);
    expect(start("b")[1]).toMatchObject({ callId: "b", text: "Second child" });
  });

  it("keeps nested work on its ancestor and suppresses child context and completion", () => {
    const router = new AcpSubagents();
    router.route({}, [
      { type: "tool.started", callId: "root", title: "Explore", kind: "agent" },
    ]);
    router.route({ parentToolCallId: "root" }, [
      { type: "tool.started", callId: "nested", title: "Task", kind: "agent" },
    ]);
    const params = { parentToolCallId: "nested" };
    expect(
      router.route(params, [
        { type: "message.delta", text: "Nested answer" },
      ])[0],
    ).toMatchObject({ type: "agent.step", callId: "root" });
    expect(
      router.route(params, [
        { type: "context", used: 999 },
        { type: "message.completed" },
        { type: "session.ended" },
      ]),
    ).toEqual([]);
  });

  it("replaces whole prose snapshots and starts a new step after tools", () => {
    const router = new AcpSubagents();
    router.route({}, [{ type: "tool.started", callId: "a", title: "Explore" }]);
    const params = {
      update: { sessionUpdate: "agent_message", parentToolCallId: "a" },
    };
    const first = router.route(params, [
      { type: "message.delta", text: "Hello" },
    ])[0];
    const repeated = router.route(params, [
      { type: "message.delta", text: "Hello again" },
    ])[0];
    expect(repeated).toMatchObject({ ...first, text: "Hello again" });
    router.route(params, [
      { type: "tool.started", callId: "t", title: "Read" },
    ]);
    expect(
      router.route(params, [{ type: "message.delta", text: "Done" }])[0],
    ).not.toMatchObject({ stepId: (first as { stepId: string }).stepId });
  });
});
