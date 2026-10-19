import { describe, expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  monoReaction,
  monoReactionBlocks,
  monoReplyText,
} from "./monoReaction";

const user = (id: string, extra: Partial<Block> = {}): Block => ({
  id,
  role: "user",
  text: id,
  ...extra,
});
const reply = (id: string, text: string, extra: Partial<Block> = {}): Block => ({
  id,
  role: "assistant",
  text,
  ...extra,
});

describe("Mono reactions", () => {
  it("reads a reply that is only one emoji in the tag", () => {
    expect(monoReaction("<reaction>👍</reaction>")).toBe("👍");
    expect(monoReaction(" <reaction> 👍🏽 </reaction>\n")).toBe("👍🏽");
    expect(monoReaction("<reaction>👍👍</reaction>")).toBeUndefined();
    expect(monoReaction("<reaction>ok</reaction>")).toBeUndefined();
    expect(monoReaction("Sure! <reaction>👍</reaction>")).toBeUndefined();
    expect(monoReplyText("<reaction>🎉</reaction>")).toBe(
      "Reacted 🎉 to your message",
    );
    expect(monoReplyText("Done.")).toBe("Done.");
  });

  it("moves the reaction onto the message it answers", () => {
    const blocks = [
      user("a"),
      reply("r1", "Here you go."),
      user("b"),
      reply("r2", "<reaction>👍</reaction>"),
    ];
    const shown = monoReactionBlocks(blocks);
    expect(shown.map((block) => block.id)).toEqual(["a", "r1", "b"]);
    expect(shown[2]).toMatchObject({ id: "b", monoReaction: "👍" });
    expect(shown[0]).toBe(blocks[0]);
    // The same input gives the same blocks, so turn caches hold.
    expect(monoReactionBlocks(blocks)).toBe(shown);
    expect(monoReactionBlocks([...blocks])[2]).toBe(shown[2]);
  });

  it("drops the work behind a reaction but keeps errors", () => {
    const shown = monoReactionBlocks([
      user("a"),
      { id: "think", role: "reasoning", text: "They are wrapping up." },
      { id: "tool", role: "tool", text: "", tool: { title: "memory.add" } },
      { id: "err", role: "system", text: "Hook failed", notice: "error" },
      reply("r", "<reaction>👍</reaction>"),
    ]);
    expect(shown.map((block) => block.id)).toEqual(["a", "err"]);
    expect(shown[0].monoReaction).toBe("👍");
  });

  it("leaves chats without reactions untouched", () => {
    const blocks = [user("a"), reply("r", "<reaction>hi</reaction>")];
    expect(monoReactionBlocks(blocks)).toBe(blocks);
  });

  it("hides a reply while it may still become a reaction", () => {
    for (const text of ["<", "<react", "<reaction>", "<reaction>👍"])
      expect(
        monoReactionBlocks([user("a"), reply("r", text, { streaming: true })]),
      ).toHaveLength(1);
    expect(
      monoReactionBlocks([user("a"), reply("r", "Sure", { streaming: true })]),
    ).toHaveLength(2);
    // Once finished, a malformed tag is shown as written.
    expect(
      monoReactionBlocks([user("a"), reply("r", "<reaction>👍")]),
    ).toHaveLength(2);
  });

  it("shows the emoji on its own when there is no message to sit on", () => {
    const shown = monoReactionBlocks([
      user("report", { internal: true }),
      reply("r", "<reaction>🎉</reaction>"),
    ]);
    expect(shown[1]).toMatchObject({ role: "assistant", text: "🎉" });
  });
});
