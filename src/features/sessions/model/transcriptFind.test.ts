import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import { findTranscriptBlocks } from "./transcriptFind";

describe("findTranscriptBlocks", () => {
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Find the sidebar chip" },
    {
      id: "tool",
      role: "tool",
      text: "",
      tool: {
        kind: "search",
        status: "completed",
        preview: { kind: "search", query: "sidebar chip" },
      },
    },
    { id: "assistant", role: "assistant", text: "The sidebar chip is fixed." },
    { id: "reasoning", role: "reasoning", text: "sidebar chip" },
  ];

  it("finds matching transcript blocks in reading order, including tool previews", () => {
    expect(findTranscriptBlocks(blocks, "SIDEBAR chip")).toEqual([
      "user",
      "tool",
      "assistant",
    ]);
  });

  it("ignores empty queries and hidden reasoning", () => {
    expect(findTranscriptBlocks(blocks, "  ")).toEqual([]);
    expect(findTranscriptBlocks(blocks, "reasoning only")).toEqual([]);
  });
});
