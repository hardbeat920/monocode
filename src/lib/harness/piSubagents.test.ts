import { describe, expect, it } from "vitest";
import { piSubagentEvents } from "./piSubagents";
import { applyHarnessEvent } from "./apply";
import { newSession } from "../session";
import type { HarnessEvent } from "./types";

function apply(events: HarnessEvent[]) {
  return events.reduce(applyHarnessEvent, newSession("pi", "/repo"));
}

describe("Pi subagent snapshots", () => {
  it("merges reasoning, prose and tool results without duplicating repeated snapshots", () => {
    const result = {
      details: {
        results: [
          {
            agent: "scout",
            task: "Check auth",
            exitCode: 0,
            messages: [
              {
                role: "user",
                content: [{ type: "text", text: "Private task input" }],
              },
              {
                role: "assistant",
                content: [
                  { type: "thinking", thinking: "Follow the imports" },
                  { type: "text", text: "Reading auth" },
                  {
                    type: "toolCall",
                    id: "read-1",
                    name: "read",
                    arguments: { path: "auth.ts" },
                  },
                ],
              },
            ],
          },
        ],
      },
    };
    const started = piSubagentEvents("spawn", {}, result, false);
    const completed = structuredClone(result);
    (completed.details.results[0].messages as unknown[]).push({
      role: "toolResult",
      toolCallId: "read-1",
      isError: true,
      content: [{ type: "text", text: "Missing file" }],
    });
    const session = apply([
      ...started,
      ...started,
      ...piSubagentEvents("spawn", {}, completed, true),
    ]);
    expect(session.blocks).toHaveLength(1);
    expect(session.blocks[0].agentRun?.steps).toEqual([
      expect.objectContaining({
        kind: "reasoning",
        text: "Follow the imports",
      }),
      expect.objectContaining({ kind: "message", text: "Reading auth" }),
      expect.objectContaining({
        kind: "tool",
        status: "failed",
        text: "Read auth.ts",
      }),
    ]);
  });

  it("gives parallel agents distinct rows even when their local tool ids match", () => {
    const result = {
      details: {
        results: ["first", "second"].map((task, index) => ({
          agent: "scout",
          task,
          exitCode: index,
          stderr: index ? "Provider failed" : "",
          messages: [
            {
              role: "assistant",
              content: [
                {
                  type: "toolCall",
                  id: "same",
                  name: "read",
                  arguments: { path: `${task}.ts` },
                },
              ],
            },
          ],
        })),
      },
    };
    const session = apply(
      piSubagentEvents("batch", { tasks: [{}, {}] }, result, true, true),
    );
    const agents = session.blocks.filter(
      (block) => block.tool?.kind === "agent",
    );
    expect(agents).toHaveLength(2);
    expect(agents.map((block) => block.text)).toEqual(["first", "second"]);
    expect(agents[0].agentRun?.steps[0].text).toBe("Read first.ts");
    expect(agents[0].tool?.status).toBe("completed");
    expect(agents[1].tool?.status).toBe("failed");
    expect(agents[1].tool?.detail).toBe("Provider failed");
    expect(session.blocks[0].tool?.kind).toBe("other");
  });

  it("leaves unknown extension detail formats alone", () => {
    expect(
      piSubagentEvents("call", {}, { details: { arbitrary: [] } }, false),
    ).toEqual([]);
  });
});

describe("omp task progress", () => {
  it("settles the current tool in place as it moves into the recent-tools tail", () => {
    const progress = {
      index: 0,
      id: "scout-1",
      agent: "scout",
      task: "Inspect auth",
      status: "running",
      toolCount: 1,
      currentTool: "read",
      currentToolArgs: "auth.ts",
      recentTools: [],
      recentOutput: ["Second line", "First line"],
    };
    const start = piSubagentEvents(
      "task",
      {},
      { details: { progress: [progress], results: [] } },
      false,
    );
    const end = piSubagentEvents(
      "task",
      {},
      {
        details: {
          progress: [
            {
              ...progress,
              status: "completed",
              currentTool: undefined,
              recentTools: [{ tool: "read", args: "auth.ts", endMs: 123 }],
            },
          ],
          results: [
            {
              index: 0,
              id: "scout-1",
              exitCode: 0,
              output: "Found the handler",
            },
          ],
        },
      },
      true,
    );
    const session = apply([...start, ...end]);
    const row = session.blocks[0];
    expect(row.tool?.status).toBe("completed");
    expect(row.tool?.detail).toBe("Found the handler");
    expect(row.agentRun?.steps.filter((step) => step.kind === "tool")).toEqual([
      expect.objectContaining({ status: "completed", text: "read auth.ts" }),
    ]);
    expect(
      row.agentRun?.steps.find((step) => step.kind === "message")?.text,
    ).toBe("First line\nSecond line");
  });

  it("keeps background runs active after their launch tool returns", () => {
    const events = piSubagentEvents(
      "task",
      {},
      {
        details: {
          async: { state: "running" },
          results: [],
          progress: [
            {
              index: 0,
              agent: "scout",
              task: "Explore",
              status: "running",
              toolCount: 0,
            },
          ],
        },
      },
      true,
    );
    expect(events[0]).toMatchObject({
      type: "tool.updated",
      status: "in_progress",
    });
  });
});
