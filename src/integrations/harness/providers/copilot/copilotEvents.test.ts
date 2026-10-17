import { describe, expect, it } from "vitest";
import { CopilotEvents } from "./copilotEvents";
import type { HarnessEvent } from "../../core/types";

function update(router: CopilotEvents, value: unknown): HarnessEvent[] {
  return router.route({ sessionId: "parent-session", update: value });
}
function search(router: CopilotEvents) {
  return update(router, {
    sessionUpdate: "tool_call",
    toolCallId: "search-1",
    title: "search_code_subagent",
    kind: "search",
    status: "pending",
    rawInput: { query: "architecture" },
  });
}
function text(router: CopilotEvents, value: string) {
  return update(router, {
    sessionUpdate: "agent_message_chunk",
    content: { type: "text", text: value },
  });
}
function messageText(events: HarnessEvent[]): string {
  return events
    .flatMap((event) => (event.type === "message.delta" ? [event.text] : []))
    .join("");
}

describe("Copilot search subagent output", () => {
  it("keeps search activity and Copilot-attributed child tools under the agent row", () => {
    const router = new CopilotEvents();
    expect(search(router)[0]).toMatchObject({
      type: "tool.updated",
      callId: "search-1",
      kind: "agent",
      title: "Search code",
    });
    const events = update(router, {
      sessionUpdate: "tool_call",
      toolCallId: "read-1",
      kind: "read",
      title: "Read file",
      status: "pending",
      _meta: { "github.com/copilot": { agentId: "search-1" } },
    });
    expect(events[0]).toMatchObject({
      type: "agent.step",
      callId: "search-1",
      stepId: "tool:read-1",
      kind: "tool",
      toolKind: "read",
    });
    expect(events.some((event) => event.type === "tool.updated")).toBe(false);
  });

  it.each([1, 2, 7, 32, 1000])(
    "removes internal citations across chunks of size %i while preserving the answer",
    (size) => {
      const router = new CopilotEvents();
      search(router);
      const output =
        "I’ll inspect the code.\n<final_answer>\n/AGENTS.md:1-260\n/src/Host/Endpoint.cs:1-120\n</final_answer>Keystone is an identity orchestration service.";
      const events: HarnessEvent[] = [];
      for (let offset = 0; offset < output.length; offset += size)
        events.push(...text(router, output.slice(offset, offset + size)));
      events.push(...router.flush());
      expect(messageText(events)).toBe(
        "I’ll inspect the code.\nKeystone is an identity orchestration service.",
      );
    },
  );

  it("preserves XML in ordinary answers and XML that is not a search citation list", () => {
    const router = new CopilotEvents();
    const example = "<final_answer>Hello world</final_answer>";
    expect(messageText(text(router, example))).toBe(example);
    search(router);
    expect(messageText([...text(router, example), ...router.flush()])).toBe(
      example,
    );
  });

  it("does not filter citation examples after the search has finished", () => {
    const router = new CopilotEvents();
    search(router);
    update(router, {
      sessionUpdate: "tool_call_update",
      toolCallId: "search-1",
      status: "completed",
    });
    const example = "<final_answer>\n/src/example.ts:1-20\n</final_answer>";
    expect(messageText(text(router, example))).toBe(example);
  });

  it("flushes partial tags at the end of the turn and bounds unclosed payloads", () => {
    const router = new CopilotEvents();
    search(router);
    expect(text(router, "<final_ans")).toEqual([]);
    expect(messageText(router.flush())).toBe("<final_ans");
    search(router);
    const unclosed = "<final_answer>" + "x".repeat(64 * 1024);
    expect(messageText(text(router, unclosed))).toBe(unclosed);
    expect(router.flush()).toEqual([]);
  });

  it.each([
    ["<final_", "answer>/src/a.ts:1-2</final_answer>answer"],
    ["<final_answer>/src/a.ts:1-2</final_", "answer>answer"],
  ])("holds split opening and closing tags: %s", (first, second) => {
    const router = new CopilotEvents();
    search(router);
    expect(text(router, first)).toEqual([]);
    expect(messageText([...text(router, second), ...router.flush()])).toBe(
      "answer",
    );
  });

  it.each(
    Array.from(
      { length: "<final_answer>".length - 1 },
      (_, index) => index + 1,
    ),
  )("handles an opening tag split at position %i", (position) => {
    const router = new CopilotEvents();
    search(router);
    const opening = "<final_answer>";
    expect(text(router, opening.slice(0, position))).toEqual([]);
    expect(
      messageText([
        ...text(
          router,
          opening.slice(position) + "/src/a.ts:1-2</final_answer>answer",
        ),
        ...router.flush(),
      ]),
    ).toBe("answer");
  });

  it("filters multiple citation blocks but preserves nested XML it cannot classify", () => {
    const router = new CopilotEvents();
    search(router);
    const citation = "<final_answer>/src/a.ts:1-2</final_answer>";
    expect(
      messageText(text(router, `before${citation}between${citation}after`)),
    ).toBe("beforebetweenafter");
    const nested = `<final_answer>${citation}</final_answer>`;
    expect(messageText([...text(router, nested), ...router.flush()])).toBe(
      nested,
    );
  });

  it("retains the authoritative result in the completed search tool", () => {
    const router = new CopilotEvents();
    search(router);
    const citations = "<final_answer>\n/src/example.ts:1-20\n</final_answer>";
    expect(text(router, citations)).toEqual([]);
    const events = update(router, {
      sessionUpdate: "tool_call_update",
      toolCallId: "search-1",
      status: "completed",
      rawOutput: { content: citations },
    });
    expect(events[0]).toMatchObject({
      type: "tool.updated",
      callId: "search-1",
      status: "completed",
    });
    expect(JSON.stringify(events)).toContain("/src/example.ts:1-20");
  });
});
