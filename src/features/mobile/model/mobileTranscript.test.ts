import { describe, expect, it } from "vitest";
import { newSession, type Block } from "../../sessions/model/session";
import { mobileTranscript } from "./mobileTranscript";

const at = Date.UTC(2026, 9, 13, 9);

function session(blocks: Block[]) {
  return { ...newSession("claude", "/repo"), blocks };
}

const prompt = (id: string, text: string, extra: Partial<Block> = {}) =>
  ({
    id,
    role: "user",
    text,
    startedAt: at,
    durationMs: 64_000,
    ...extra,
  }) as Block;
const tool = (id: string, title: string) =>
  ({
    id,
    role: "tool",
    text: title,
    tool: { kind: "read", title, status: "completed" },
  }) as Block;
const reply = (id: string, text: string, extra: Partial<Block> = {}) =>
  ({ id, role: "assistant", text, ...extra }) as Block;

describe("mobile transcript", () => {
  it("folds a settled session turn's work behind its status line", () => {
    const [turn] = mobileTranscript(
      session([
        prompt("u", "Fix the bug"),
        reply("p", "Looking into it."),
        tool("t", "Read src/app.ts"),
        reply("a", "Fixed it."),
      ]),
      { busy: false },
    );
    expect(turn.items.map((item) => item.type)).toEqual([
      "user",
      "header",
      "text",
    ]);
    const header = turn.items[1];
    expect(header).toMatchObject({ live: false, durationMs: 64_000 });
    // Opened, the fold reads as on desktop: the note, then the work after it.
    expect(header.type === "header" && header.entries).toEqual([
      { type: "prose", id: "p", text: "Looking into it." },
      expect.objectContaining({
        type: "phase",
        kind: "research",
        title: "Read src/app.ts",
      }),
    ]);
    expect(turn.items[2]).toMatchObject({ text: "Fixed it." });
    expect(turn).toMatchObject({
      footer: true,
      completedAt: at + 64_000,
      copyText: "Looking into it.\n\nFixed it.",
    });
    expect(turn.stampAt).toBeUndefined();
  });

  it("keeps a running turn live, with its work open", () => {
    const [turn] = mobileTranscript(
      session([
        prompt("u", "Fix the bug", { durationMs: undefined }),
        tool("t", "Read src/app.ts"),
      ]),
      { busy: true },
    );
    expect(turn.items[1]).toMatchObject({
      type: "header",
      live: true,
      startedAt: at,
    });
    expect(turn.items.some((item) => item.type === "work")).toBe(true);
    expect(turn.footer).toBeUndefined();
  });

  it("signs a Mono's turn, hides its work and stamps the day", () => {
    const [turn] = mobileTranscript(
      session([
        prompt("u", "Check the build"),
        tool("t", "Read src/app.ts"),
        reply("a", "All green."),
      ]),
      { busy: false, mono: { name: "Pip" } },
    );
    expect(turn.stampAt).toBe(at);
    expect(turn.items.map((item) => item.type)).toEqual([
      "user",
      "header",
      "text",
    ]);
    expect(turn.items[1]).toMatchObject({ label: "Pip", entries: [] });
  });

  it("pins a Mono's reaction to the message it answers", () => {
    const [turn] = mobileTranscript(
      session([prompt("u", "thanks!"), reply("a", "<reaction>👍</reaction>")]),
      { busy: false, mono: { name: "Pip" } },
    );
    expect(turn.items).toEqual([
      expect.objectContaining({ type: "user", reaction: "👍" }),
    ]);
    expect(turn.footer).toBeUndefined();
  });
});
