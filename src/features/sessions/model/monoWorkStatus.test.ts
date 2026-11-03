import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import { monoWorkStatus } from "./monoWorkStatus";

const tool = (kind: string): Block => ({
  id: kind,
  role: "tool",
  text: "Private activity details",
  tool: { kind, status: "in_progress" },
});

describe("Mono work status", () => {
  it.each([
    ["shell", "Running command…"],
    ["edit", "Editing file…"],
    ["search", "Exploring project…"],
    ["read", "Reading file…"],
    ["agent", "Running agent…"],
    ["other", "Using tool…"],
  ])("uses a short label for %s work", (kind, label) => {
    expect(monoWorkStatus([tool(kind)], true).label).toBe(label);
  });

  it("follows the latest narration and varies labels between messages, not chunks", () => {
    const first: Block = {
      id: "first",
      role: "assistant",
      text: "Private progress note",
      streaming: true,
    };
    const firstStatus = monoWorkStatus([tool("shell"), first], true);
    expect(firstStatus.label).toBe("Thinking…");
    expect(
      monoWorkStatus(
        [tool("shell"), { ...first, text: `${first.text} with another chunk` }],
        true,
      ),
    ).toEqual(firstStatus);
    expect(
      monoWorkStatus(
        [
          first,
          tool("shell"),
          { id: "second", role: "reasoning", text: "Private reasoning" },
        ],
        true,
      ).label,
    ).toBe("Pondering…");
  });

  it("prioritizes an unresolved approval over later narration", () => {
    expect(
      monoWorkStatus(
        [
          { ...tool("edit"), approval: { requestId: 1 } },
          { id: "note", role: "assistant", text: "Waiting" },
        ],
        true,
      ).label,
    ).toBe("Waiting for approval…");
  });

  it("keeps the finished summary and failed delegation visible", () => {
    expect(
      monoWorkStatus([tool("shell"), { ...tool("shell"), id: "second" }], false)
        .label,
    ).toBe("Ran 2 commands");
    const failed = {
      ...tool("agent"),
      tool: { kind: "agent", status: "failed" },
    };
    expect(monoWorkStatus([failed], false).label).toBe("Subagent failed");
  });
});
