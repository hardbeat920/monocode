import { describe, expect, it } from "vitest";
import { buildImportedSession } from "./claudeSessionImport";
import { newSession } from "./session";

function jsonl(records: Array<Record<string, unknown>>): string {
  return records.map((record) => JSON.stringify(record)).join("\n");
}

function importOf(records: Array<Record<string, unknown>>) {
  return buildImportedSession({
    base: newSession("claude", "/repo", "claude:claude-sonnet-5"),
    transcript: jsonl(records),
    providerSessionId: "conv-1",
  });
}

describe("importing a stored Claude conversation", () => {
  it("rebuilds the exchange as alternating blocks", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "neden bozuldu" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "sebebi su" }],
        },
      },
      { type: "user", message: { role: "user", content: "peki ya bu" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "o da soyle" }],
        },
      },
    ]);

    expect(session.blocks.map((block) => [block.role, block.text])).toEqual([
      ["user", "neden bozuldu"],
      ["assistant", "sebebi su"],
      ["user", "peki ya bu"],
      ["assistant", "o da soyle"],
    ]);
    // Nothing may still look live once the file has been read to the end.
    expect(session.blocks.some((block) => block.streaming)).toBe(false);
  });

  it("binds the conversation so the next turn continues it", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "merhaba" } },
    ]);
    expect(session.providerSessionId).toBe("conv-1");
  });

  it("brings tool work across as its own block", () => {
    const session = importOf([
      { type: "user", message: { role: "user", content: "dosyayi oku" } },
      {
        type: "assistant",
        message: {
          role: "assistant",
          content: [
            {
              type: "tool_use",
              id: "t1",
              name: "Read",
              input: { file_path: "/repo/a.ts" },
            },
          ],
        },
      },
      {
        type: "user",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "t1", content: "icerik" },
          ],
        },
      },
    ]);

    const tool = session.blocks.find((block) => block.role === "tool");
    expect(tool).toBeDefined();
    expect(tool?.text).toContain("Read");
  });

  it("keeps an empty conversation empty", () => {
    const session = importOf([{ type: "mode", mode: "normal" }]);
    expect(session.blocks).toEqual([]);
  });
});
